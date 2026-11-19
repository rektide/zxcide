//! Deliberately retain a batch barrier as the Rayon comparison. The caller
//! reads and writes, while a private pool computes up to K jobs in parallel.
use crate::{Config, Scheduler, compute};
use rayon::{ThreadPool, ThreadPoolBuilder, prelude::*};
use std::{io, sync::Mutex};

pub(crate) struct Driver<'a> {
    config: Config,
    pool: &'a ThreadPool,
}

impl Driver<'_> {
    pub(crate) fn with<T>(config: Config, operation: impl FnOnce(Driver<'_>) -> io::Result<T>) -> io::Result<T> {
        ThreadPoolBuilder::new()
            .num_threads(config.workers)
            .thread_name(|i| format!("zxc rayon {i}"))
            .build_scoped(|thread| thread.run(), |pool| operation(Driver { config, pool }))
            .map_err(io::Error::other)?
    }
}

impl Scheduler for Driver<'_> {
    fn run<S: Default + Send, J: Send + 'static, O: Send + 'static>(
        &self,
        mut next: impl FnMut(Vec<u8>) -> io::Result<Option<J>>,
        work: impl Fn(&mut S, u64, J) -> io::Result<O> + Sync,
        mut commit: impl FnMut(O) -> io::Result<Vec<u8>>,
    ) -> io::Result<()> {
        let mut index = 0;
        let slots = self.config.slots;
        let mut free: Vec<_> = (0..slots).map(|_| Vec::new()).collect();
        let mut batch = Vec::with_capacity(slots);
        let mut done = Vec::with_capacity(slots);
        // Each pool thread is the sole user of its scratch. The mutex makes
        // that ownership safe without unsafe access or allocation per task.
        let scratch: Vec<_> = (0..self.config.workers).map(|_| Mutex::new(None)).collect();
        loop {
            let mut end = None;
            while let Some(buf) = free.pop() {
                match next(buf) {
                    Ok(Some(item)) => {
                        batch.push((index, item));
                        index += 1;
                    }
                    Ok(None) => {
                        end = Some(Ok(()));
                        break;
                    }
                    Err(error) => {
                        end = Some(Err(error));
                        break;
                    }
                }
            }
            if !batch.is_empty() {
                {
                    zone!("parallel rayon batch barrier");
                    self.pool.install(|| {
                        batch
                            .par_drain(..)
                            .map(|(index, item)| {
                                let thread = rayon::current_thread_index().expect("local pool worker");
                                let mut scratch = scratch[thread].lock().unwrap();
                                compute(&mut scratch, index, item, &work)
                            })
                            .collect_into_vec(&mut done);
                    });
                }
                for result in done.drain(..) {
                    zone!("parallel ordered commit");
                    free.push(commit(result?)?);
                }
            }
            if let Some(end) = end {
                return end;
            }
        }
    }
}
