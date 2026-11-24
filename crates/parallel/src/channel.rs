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
        next: impl FnMut(&mut Vec<u8>) -> io::Result<Option<J>>,
        work: impl Fn(&mut S, u64, J, Vec<u8>) -> io::Result<O> + Sync,
        commit: impl FnMut(O) -> io::Result<Vec<u8>>,
    ) -> io::Result<()> {
        self.with_workers(work, |session| session.run(next, commit))
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

    /// Keep the window and worker scratch for a sequence of drained frames.
    pub(crate) fn with_workers<S: Default + Send, J: Send + 'static, O: Send + 'static, T>(
        &self,
        work: impl Fn(&mut S, u64, J, Vec<u8>) -> io::Result<O> + Sync,
        operation: impl FnOnce(&mut Session<Q, J, O>) -> io::Result<T>,
    ) -> io::Result<T> {
        self.with_workers_using(work, operation, |_| {
            Ok(thread::Builder::new().name("zxcide block".into()))
        })
    }

    fn with_workers_using<S: Default + Send, J: Send + 'static, O: Send + 'static, T>(
        &self,
        work: impl Fn(&mut S, u64, J, Vec<u8>) -> io::Result<O> + Sync,
        operation: impl FnOnce(&mut Session<Q, J, O>) -> io::Result<T>,
        mut builder: impl FnMut(usize) -> io::Result<thread::Builder>,
    ) -> io::Result<T> {
        let slots = self.0.slots;
        let work = |scratch: &mut S, index, (job, buf)| work(scratch, index, job, buf);
        let (work, cancelled) = (&work, &AtomicBool::new(false));
        thread::scope(|scope| {
            // Both coordinator endpoints must be local to the scope body, so
            // they close before scope joins workers even during unwinding.
            let (job_tx, job_rx) = Q::jobs(slots);
            let (result_tx, result_rx) = Q::results(slots);
            let mut session = Session {
                job_tx,
                result_rx,
                free: (0..slots).map(|_| Vec::new()).collect(),
                pending: (0..slots).map(|_| None).collect(),
            };
            let _cancel = Cancel(cancelled);
            for i in 0..self.0.workers {
                let rx = job_rx.clone();
                let tx = result_tx.clone();
                builder(i)?.spawn_scoped(scope, move || {
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
            operation(&mut session)
        })
    }
}

type Job<J> = (u64, (J, Vec<u8>));
type Completed<O> = (u64, io::Result<O>);

pub(crate) struct Session<Q: Transport, J: Send + 'static, O: Send + 'static> {
    job_tx: Q::JobTx<Job<J>>,
    result_rx: Q::ResultRx<Completed<O>>,
    free: Vec<Vec<u8>>,
    pending: Vec<Option<io::Result<O>>>,
}

impl<Q: Transport, J: Send + 'static, O: Send + 'static> Session<Q, J, O> {
    /// Run one frame. Success returns every credit and leaves no queued work;
    /// on failure callers must exit the scope rather than starting a new frame.
    pub(crate) fn run(
        &mut self,
        next: impl FnMut(&mut Vec<u8>) -> io::Result<Option<J>>,
        commit: impl FnMut(O) -> io::Result<Vec<u8>>,
    ) -> io::Result<()> {
        self.run_observed(next, commit, |_| {})
    }

    fn run_observed(
        &mut self,
        mut next: impl FnMut(&mut Vec<u8>) -> io::Result<Option<J>>,
        mut commit: impl FnMut(O) -> io::Result<Vec<u8>>,
        mut observe_wait: impl FnMut(u64),
    ) -> io::Result<()> {
        let slots = self.pending.len();
        debug_assert_eq!(self.free.len(), slots);
        let (mut admitted, mut committed) = (0_u64, 0_u64);
        let ring = |index| (index % slots as u64) as usize;
        let mut end = None;
        loop {
            // Borrow a free buffer to read; only a data block spends its
            // credit. A valid EOF leaves the same buffer in the window.
            while end.is_none() {
                let Some(buf) = self.free.last_mut() else { break };
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
                let Some(after) = admitted.checked_add(1) else {
                    end = Some(Err(io::Error::new(io::ErrorKind::InvalidData, "block index overflow")));
                    break;
                };
                let buf = self.free.pop().unwrap();
                zone!("parallel send job");
                if !Q::send_job(&self.job_tx, (admitted, (item, buf))) {
                    end = Some(Err(io::Error::other("block job queue disconnected")));
                    break;
                }
                admitted = after;
            }
            if admitted == committed {
                return end.expect("only a terminal read leaves the window empty");
            }

            {
                zone!("parallel wait ordered head");
                while self.pending[ring(committed)].is_none() {
                    observe_wait(committed);
                    zone!("parallel receive result");
                    let (index, result) = Q::recv_result(&self.result_rx)
                        .ok_or_else(|| io::Error::other("block result queue disconnected"))?;
                    debug_assert!((committed..admitted).contains(&index));
                    debug_assert!(self.pending[ring(index)].is_none());
                    if result.is_err() {
                        // Stop admission immediately, but drain through the
                        // first bad block in *sequence* order.
                        end.get_or_insert(Ok(()));
                    }
                    self.pending[ring(index)] = Some(result);
                }
            }

            while let Some(result) = self.pending[ring(committed)].take() {
                zone!("parallel ordered commit");
                self.free.push(commit(result?)?);
                // A pending result is always below checked `admitted`.
                committed += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler_tests::{TIMEOUT, bounded, each_channel};
    use std::{
        cell::Cell,
        collections::HashMap,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::{Mutex, atomic::AtomicUsize, mpsc},
    };

    fn blocked_head<Q: Transport + 'static>(config: Config) {
        bounded(move || {
            let (wait_tx, wait_rx) = mpsc::channel();
            let (head_tx, head_rx) = mpsc::channel();
            let head_rx = Mutex::new(head_rx);
            thread::scope(|scope| {
                let coordinator = scope.spawn(move || {
                    let read = Cell::new(0);
                    let written = Cell::new(0);
                    Driver::<Q>::new(config)
                        .with_workers(
                            |(): &mut (), i, (), buf| {
                                if i == 0 {
                                    head_rx.lock().unwrap().recv_timeout(TIMEOUT).unwrap();
                                }
                                Ok((i, buf))
                            },
                            |session| {
                                session.run_observed(
                                    |_| {
                                        let n = read.get();
                                        if n == 3 {
                                            return Ok(None);
                                        }
                                        assert!(n < 2 || written.get() > 0, "admitted job 2 before committing job 0");
                                        read.set(n + 1);
                                        Ok(Some(()))
                                    },
                                    |(i, buf)| {
                                        assert_eq!(written.get(), i);
                                        written.set(i + 1);
                                        Ok(buf)
                                    },
                                    |head| wait_tx.send((head, read.get(), written.get())).unwrap(),
                                )
                            },
                        )
                        .unwrap();
                    assert_eq!(written.get(), 3);
                });
                assert_eq!(wait_rx.recv_timeout(TIMEOUT).unwrap(), (0, 2, 0));
                // The only result that could have arrived is job 1. The
                // coordinator must still wait for 0 without admitting job 2.
                assert_eq!(wait_rx.recv_timeout(TIMEOUT).unwrap(), (0, 2, 0));
                head_tx.send(()).unwrap();
                coordinator.join().unwrap();
            });
        });
    }

    #[test]
    fn out_of_order_completion_does_not_return_credit() {
        each_channel!(blocked_head, 2, 2);
    }

    #[test]
    fn session_reuses_worker_scratch_and_preserves_eof_credit() {
        bounded(|| {
            let initialized = AtomicUsize::new(0);
            let identities = Mutex::new(HashMap::new());
            let cfg = Config {
                workers: 4,
                slots: 1,
                ..Config::default()
            };
            Driver::<Crossbeam>::new(cfg)
                .with_workers(
                    |id: &mut Option<usize>, index, expected, buf| {
                        assert_eq!(index, expected);
                        let id = *id.get_or_insert_with(|| initialized.fetch_add(1, Ordering::Relaxed));
                        let prev = identities.lock().unwrap().insert(thread::current().id(), id);
                        assert!(prev.is_none_or(|prev| prev == id));
                        Ok(buf)
                    },
                    |session| {
                        let mut allocation = None;
                        for len in [0, 3, 0, 0, 1, 0, 9].into_iter().cycle().take(70) {
                            let mut read = 0;
                            session.run(
                                |buf| {
                                    if let Some((ptr, cap)) = allocation {
                                        assert_eq!((buf.as_ptr(), buf.capacity()), (ptr, cap));
                                    } else if read < len {
                                        buf.reserve(8192);
                                        allocation = Some((buf.as_ptr(), buf.capacity()));
                                    }
                                    if read == len {
                                        return Ok(None);
                                    }
                                    read += 1;
                                    Ok(Some(read - 1))
                                },
                                Ok,
                            )?;
                            assert_eq!(session.free.len(), 1);
                            assert_eq!(read, len);
                        }
                        Ok(())
                    },
                )
                .unwrap();
            assert!((1..=cfg.workers).contains(&initialized.load(Ordering::Relaxed)));
        });
    }

    #[test]
    fn decode_epochs_split_only_on_block_size_changes() {
        thread_local! { static EPOCHS: Cell<usize> = const { Cell::new(0) }; }
        fn jobs<T>(slots: usize) -> (crossbeam_channel::Sender<T>, crossbeam_channel::Receiver<T>) {
            EPOCHS.set(EPOCHS.get() + 1);
            crossbeam_channel::bounded(slots)
        }
        adapter!(
            Counted,
            jobs,
            crossbeam_channel::bounded,
            crossbeam_channel::Sender<T>,
            crossbeam_channel::Receiver<T>,
            crossbeam_channel::Sender<T>,
            crossbeam_channel::Receiver<T>
        );
        bounded(|| {
            let mut archive = Vec::new();
            let mut expected = Vec::new();
            for (block_log, seekable, checksum, size) in [
                (21, true, true, 1 << 21), // only one worker gets large scratch
                (21, false, false, 0),
                (12, false, true, 0), // even an empty downshift drops the epoch
                (12, true, false, 1 << 13),
                (12, false, true, 33),
                (21, false, false, 77),
            ] {
                let plain = vec![size as u8; size];
                expected.extend_from_slice(&plain);
                zxcide::encode(
                    &plain[..],
                    &mut archive,
                    zxcide::EncodeOptions {
                        block_log,
                        seekable,
                        checksum,
                        ..Default::default()
                    },
                )
                .unwrap();
            }
            let cfg = Config {
                workers: 5,
                slots: 2,
                ..Config::default()
            };
            let mut out = Vec::new();
            crate::decode_channel(Driver::<Counted>::new(cfg), &archive[..], &mut out, Default::default()).unwrap();
            assert_eq!(EPOCHS.get(), 3);
            assert!(out == expected);
        });
    }

    #[test]
    fn later_worker_panic_stops_admission_but_earlier_write_error_wins() {
        for fail_write in [false, true] {
            bounded(move || {
                let cfg = Config {
                    workers: 2,
                    slots: 2,
                    ..Config::default()
                };
                let (release, gate) = mpsc::channel();
                let gate = Mutex::new(gate);
                let read = Cell::new(0);
                let waits = Cell::new(0);
                let result = Driver::<Crossbeam>::new(cfg).with_workers(
                    |(): &mut (), i, fail: bool, buf| {
                        if fail && i == 0 {
                            gate.lock().unwrap().recv_timeout(TIMEOUT).unwrap();
                        }
                        assert!(!(fail && i == 1), "injected later worker panic");
                        Ok(buf)
                    },
                    |session| {
                        let mut first = Some(false);
                        session.run(|_| Ok(first.take()), Ok)?;
                        session.run_observed(
                            |_| {
                                read.set(read.get() + 1);
                                assert!(read.get() <= cfg.slots, "admitted after observing worker error");
                                Ok(Some(true))
                            },
                            |buf| {
                                if fail_write {
                                    return Err(io::ErrorKind::BrokenPipe.into());
                                }
                                Ok(buf)
                            },
                            |index| {
                                assert_eq!(index, 0);
                                waits.set(waits.get() + 1);
                                if waits.get() == 2 {
                                    // Index 0 cannot finish yet, so the first result was index 1.
                                    release.send(()).unwrap();
                                }
                            },
                        )
                    },
                );
                let error = result.unwrap_err();
                if fail_write {
                    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
                } else {
                    assert_eq!(error.to_string(), "block worker panicked at index 1");
                }
                assert_eq!(waits.get(), 2);
            });
        }
    }

    #[test]
    fn coordinator_unwind_joins_with_a_full_window() {
        bounded(|| {
            let live = AtomicUsize::new(0);
            struct Scratch<'a>(Option<&'a AtomicUsize>);
            impl Drop for Scratch<'_> {
                fn drop(&mut self) {
                    if let Some(live) = self.0 {
                        live.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
            let config = Config {
                workers: 2,
                slots: 3,
                ..Config::default()
            };
            let (worked, wait) = mpsc::channel();
            let result = catch_unwind(AssertUnwindSafe(|| {
                Driver::<Crossbeam>::new(config).with_workers(
                    |scratch: &mut Option<Scratch<'_>>, _, (), buf| {
                        scratch.get_or_insert_with(|| {
                            live.fetch_add(1, Ordering::Relaxed);
                            Scratch(Some(&live))
                        });
                        worked.send(()).unwrap();
                        Ok(buf)
                    },
                    |session| {
                        session.run(
                            |_| Ok(Some(())),
                            |_| {
                                for _ in 0..config.slots {
                                    wait.recv_timeout(TIMEOUT).unwrap();
                                }
                                assert!(live.load(Ordering::Relaxed) > 0);
                                panic!("injected coordinator panic");
                            },
                        )
                    },
                )
            }));
            assert_eq!(
                result.unwrap_err().downcast_ref::<&str>(),
                Some(&"injected coordinator panic")
            );
            assert_eq!(live.load(Ordering::Relaxed), 0);
        });
    }

    #[test]
    fn partial_spawn_failure_closes_idle_workers_without_running_coordinator() {
        bounded(|| {
            let config = Config {
                workers: 3,
                slots: 1,
                ..Config::default()
            };
            let error = Driver::<Crossbeam>::new(config)
                .with_workers_using(
                    |(): &mut (), _, (), buf| Ok(buf),
                    |_| -> io::Result<()> { panic!("operation ran after partial spawn") },
                    |i| {
                        if i == 1 {
                            return Err(io::Error::other("injected spawn failure"));
                        }
                        Ok(thread::Builder::new())
                    },
                )
                .unwrap_err();
            assert_eq!(error.to_string(), "injected spawn failure");
        });
    }
}
