//! Shared bounded sliding window; only queue construction and endpoint calls
//! vary by transport. All producers and consumers use blocking endpoints.
use crate::{Config, Scheduler, compute};
use std::{
    io,
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

pub(crate) trait Transport {
    type JobTx<T: Send + 'static>;
    type JobRx<T: Send + 'static>: Send + Clone;
    type ResultTx<T: Send + 'static>: Send + Clone;
    type ResultRx<T: Send + 'static>;

    fn jobs<T: Send + 'static>(slots: usize) -> (Self::JobTx<T>, Self::JobRx<T>);
    fn results<T: Send + 'static>(slots: usize) -> (Self::ResultTx<T>, Self::ResultRx<T>);
    fn send_job<T: Send + 'static>(tx: &Self::JobTx<T>, item: T) -> bool;
    fn recv_job<T: Send + 'static>(rx: &Self::JobRx<T>) -> Option<T>;
    fn send_result<T: Send + 'static>(tx: &Self::ResultTx<T>, item: T) -> bool;
    fn recv_result<T: Send + 'static>(rx: &Self::ResultRx<T>) -> Option<T>;
}

macro_rules! adapter {
    ($name:ident, $jobs:path, $results:path, $job_tx:ty, $job_rx:ty, $res_tx:ty, $res_rx:ty) => {
        pub(crate) struct $name;
        impl Transport for $name {
            type JobTx<T: Send + 'static> = $job_tx;
            type JobRx<T: Send + 'static> = $job_rx;
            type ResultTx<T: Send + 'static> = $res_tx;
            type ResultRx<T: Send + 'static> = $res_rx;

            fn jobs<T: Send + 'static>(slots: usize) -> (Self::JobTx<T>, Self::JobRx<T>) {
                $jobs(slots)
            }
            fn results<T: Send + 'static>(slots: usize) -> (Self::ResultTx<T>, Self::ResultRx<T>) {
                $results(slots)
            }
            fn send_job<T: Send + 'static>(tx: &Self::JobTx<T>, item: T) -> bool {
                tx.send(item).is_ok()
            }
            fn recv_job<T: Send + 'static>(rx: &Self::JobRx<T>) -> Option<T> {
                rx.recv().ok()
            }
            fn send_result<T: Send + 'static>(tx: &Self::ResultTx<T>, item: T) -> bool {
                tx.send(item).is_ok()
            }
            fn recv_result<T: Send + 'static>(rx: &Self::ResultRx<T>) -> Option<T> {
                rx.recv().ok()
            }
        }
    };
}

adapter!(
    Crossbeam,
    crossbeam_channel::bounded,
    crossbeam_channel::bounded,
    crossbeam_channel::Sender<T>,
    crossbeam_channel::Receiver<T>,
    crossbeam_channel::Sender<T>,
    crossbeam_channel::Receiver<T>
);
#[cfg(feature = "flume")]
adapter!(
    Flume,
    flume::bounded,
    flume::bounded,
    flume::Sender<T>,
    flume::Receiver<T>,
    flume::Sender<T>,
    flume::Receiver<T>
);
#[cfg(feature = "kanal")]
adapter!(
    Kanal,
    kanal::bounded,
    kanal::bounded,
    kanal::Sender<T>,
    kanal::Receiver<T>,
    kanal::Sender<T>,
    kanal::Receiver<T>
);
#[cfg(feature = "crossfire")]
use crossfire::{MRx, MTx, Rx, mpmc, mpsc};
#[cfg(feature = "crossfire")]
adapter!(
    Crossfire,
    mpmc::bounded_blocking,
    mpsc::bounded_blocking,
    MTx<mpmc::Array<T>>,
    MRx<mpmc::Array<T>>,
    MTx<mpsc::Array<T>>,
    Rx<mpsc::Array<T>>
);

pub(crate) struct Driver<Q>(Config, PhantomData<Q>);

impl<Q: Transport> Scheduler for Driver<Q> {
    fn run<S: Default + Send, J: Send + 'static, O: Send + 'static>(
        &self,
        next: impl FnMut(Vec<u8>) -> io::Result<Option<J>>,
        work: impl Fn(&mut S, u64, J) -> io::Result<O> + Sync,
        commit: impl FnMut(O) -> io::Result<Vec<u8>>,
    ) -> io::Result<()> {
        self.run_observed(next, work, commit, |_| {})
    }
}

struct Cancel<'a>(&'a AtomicBool);

impl Drop for Cancel<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

impl<Q: Transport> Driver<Q> {
    pub(crate) fn new(config: Config) -> Self {
        Self(config, PhantomData)
    }

    fn run_observed<S: Default + Send, J: Send + 'static, O: Send + 'static>(
        &self,
        mut next: impl FnMut(Vec<u8>) -> io::Result<Option<J>>,
        work: impl Fn(&mut S, u64, J) -> io::Result<O> + Sync,
        mut commit: impl FnMut(O) -> io::Result<Vec<u8>>,
        mut observe_completion: impl FnMut(u64),
    ) -> io::Result<()> {
        let slots = self.0.slots;
        let (work, cancelled) = (&work, &AtomicBool::new(false));
        thread::scope(|scope| {
            // Both coordinator endpoints must be local to the scope body, so
            // they close before scope joins workers even during unwinding.
            let (job_tx, job_rx) = Q::jobs(slots);
            let (result_tx, result_rx) = Q::results(slots);
            let _cancel = Cancel(cancelled);
            for _ in 0..self.0.workers {
                let rx = job_rx.clone();
                let tx = result_tx.clone();
                thread::Builder::new()
                    .name("zxc block".into())
                    .spawn_scoped(scope, move || {
                        let mut scratch = None;
                        loop {
                            let job = {
                                zone!("parallel wait job");
                                Q::recv_job(&rx)
                            };
                            let Some((index, job)) = job else { break };
                            if cancelled.load(Ordering::Relaxed) {
                                break;
                            }
                            let result = compute(&mut scratch, index, job, work);
                            zone!("parallel send result");
                            if !Q::send_result(&tx, (index, result)) {
                                break;
                            }
                        }
                    })?;
            }
            drop(job_rx);
            drop(result_tx);

            let mut free: Vec<_> = (0..slots).map(|_| Vec::new()).collect();
            let mut pending: Vec<_> = (0..slots).map(|_| None).collect();
            let (mut admitted, mut committed) = (0, 0);
            let ring = |index| (index % slots as u64) as usize;
            let mut end = None;
            loop {
                // A free buffer is a credit. No source I/O until we take one.
                while end.is_none() {
                    let Some(buf) = free.pop() else { break };
                    let item = match next(buf) {
                        Ok(Some(item)) => item,
                        Ok(None) => {
                            end = Some(Ok(()));
                            break;
                        }
                        Err(error) => {
                            // Source/framing errors are after all admitted
                            // blocks. An earlier block or writer error wins.
                            end = Some(Err(error));
                            break;
                        }
                    };
                    zone!("parallel send job");
                    if !Q::send_job(&job_tx, (admitted, item)) {
                        end = Some(Err(io::Error::other("block job queue disconnected")));
                        break;
                    }
                    admitted += 1;
                }
                if admitted == committed {
                    return end.expect("only a terminal read leaves the window empty");
                }

                {
                    zone!("parallel wait ordered head");
                    while pending[ring(committed)].is_none() {
                        zone!("parallel receive result");
                        let (index, result) = Q::recv_result(&result_rx)
                            .ok_or_else(|| io::Error::other("block result queue disconnected"))?;
                        debug_assert!((committed..admitted).contains(&index));
                        debug_assert!(pending[ring(index)].is_none());
                        if result.is_err() {
                            // Stop admission immediately, but drain through the
                            // first bad block in *sequence* order.
                            end.get_or_insert(Ok(()));
                        }
                        pending[ring(index)] = Some(result);
                        observe_completion(index);
                    }
                }

                while let Some(result) = pending[ring(committed)].take() {
                    zone!("parallel ordered commit");
                    free.push(commit(result?)?);
                    committed += 1;
                }
            }
        })
    }
}
