use crate::{Config, Scheduler, channel};
use std::{
    cell::Cell,
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

pub(crate) const TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn bounded(test: impl FnOnce() + Send + 'static) {
    let (done, wait) = mpsc::channel();
    let thread = thread::spawn(move || {
        test();
        done.send(()).unwrap();
    });
    // Don't join on timeout: a broken channel adapter must fail the test, not
    // strand the entire runner waiting for a deadlocked thread.
    assert_ne!(
        wait.recv_timeout(TIMEOUT),
        Err(mpsc::RecvTimeoutError::Timeout),
        "driver hung"
    );
    thread.join().unwrap();
}

macro_rules! each_channel {
    ($test:ident, $workers:expr, $slots:expr) => {{
        let config = |backend| crate::Config {
            workers: $workers,
            slots: $slots,
            backend,
        };
        $test::<crate::channel::Crossbeam>(config(crate::Backend::Crossbeam));
        #[cfg(feature = "flume")]
        $test::<crate::channel::Flume>(config(crate::Backend::Flume));
        #[cfg(feature = "kanal")]
        $test::<crate::channel::Kanal>(config(crate::Backend::Kanal));
        #[cfg(feature = "crossfire")]
        $test::<crate::channel::Crossfire>(config(crate::Backend::Crossfire));
    }};
}

pub(crate) use each_channel;

#[test]
fn stress_windows_and_shutdown() {
    for (workers, slots) in [(4, 1), (6, 2), (2, 3), (3, 7), (2, 16)] {
        each_channel!(exercise_channel, workers, slots);
        #[cfg(feature = "rayon")]
        bounded(move || {
            crate::rayon::Driver::with(
                Config {
                    workers,
                    slots,
                    backend: crate::Backend::Rayon,
                },
                |driver| {
                    exercise(&driver, slots);
                    Ok(())
                },
            )
            .unwrap();
        });
    }
}

fn exercise_channel<Q: channel::Transport + 'static>(config: Config) {
    bounded(move || exercise(&channel::Driver::<Q>::new(config), config.slots));
}

#[derive(Default)]
struct TrackedScratch(Option<Arc<AtomicUsize>>);

impl TrackedScratch {
    fn track(&mut self, live: &Arc<AtomicUsize>) {
        self.0.get_or_insert_with(|| {
            live.fetch_add(1, Ordering::Relaxed);
            live.clone()
        });
    }
}

impl Drop for TrackedScratch {
    fn drop(&mut self) {
        if let Some(live) = &self.0 {
            live.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

fn exercise(driver: &impl Scheduler, slots: usize) {
    #[derive(Clone, Copy, Debug)]
    enum Stop {
        Eof(usize),
        Source,
        Sink,
        Panic,
    }
    let live = Arc::new(AtomicUsize::new(0));
    for stop in [
        Stop::Eof(0),
        Stop::Eof(1),
        Stop::Eof(257),
        Stop::Source,
        Stop::Sink,
        Stop::Panic,
    ] {
        let (read, written) = (Cell::new(0), Cell::new(0));
        let result = driver.run(
            |buf| {
                let n = read.get();
                assert!(n - written.get() < slots, "admission exceeded {slots} slots");
                match stop {
                    Stop::Eof(end) if n == end => return Ok(None),
                    Stop::Source if n == 33 => return Err(io::Error::other("source failed")),
                    _ => {}
                }
                // Cap every test input even if error handling regresses.
                assert!(n <= 257);
                read.set(n + 1);
                buf.clear();
                buf.extend_from_slice(&(n as u64).to_le_bytes());
                Ok(Some(()))
            },
            |scratch: &mut TrackedScratch, index, (), buf| {
                scratch.track(&live);
                assert_eq!(buf, index.to_le_bytes());
                if index % 3 == 0 {
                    thread::yield_now();
                }
                if matches!(stop, Stop::Panic) && index == 3 {
                    panic!("injected worker panic");
                }
                Ok(buf)
            },
            |buf| {
                let n = written.get();
                assert_eq!(buf, (n as u64).to_le_bytes());
                if matches!(stop, Stop::Sink) && n == 2 {
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "sink failed"));
                }
                written.set(n + 1);
                Ok(buf)
            },
        );
        match stop {
            Stop::Eof(end) => {
                result.unwrap();
                assert_eq!(written.get(), end);
            }
            Stop::Source => {
                assert_eq!(result.unwrap_err().to_string(), "source failed");
                assert_eq!(written.get(), 33);
            }
            Stop::Sink => {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
                assert_eq!(written.get(), 2);
            }
            Stop::Panic => {
                assert_eq!(result.unwrap_err().to_string(), "block worker panicked at index 3");
                assert_eq!(written.get(), 3);
            }
        }
        assert_eq!(
            live.load(Ordering::Relaxed),
            0,
            "worker left scratch live after {stop:?}"
        );
    }
}

#[test]
fn fails_cleanly_with_partial_sink_writes() {
    let plain = Arc::new(vec![b'x'; 4096 * 32]);
    let options = zxcide::EncodeOptions {
        block_log: 12,
        ..Default::default()
    };
    let mut archive = Vec::new();
    zxcide::encode(&plain[..], &mut archive, options).unwrap();
    let archive = Arc::new(archive);

    struct Sink(usize);

    impl io::Write for Sink {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            let len = data.len().min(self.0);
            if len == 0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "injected sink failure"));
            }
            self.0 -= len;
            Ok(len)
        }

        fn flush(&mut self) -> io::Result<()> {
            panic!("the driver does not flush the sink")
        }
    }

    for &backend in crate::Backend::available() {
        let (plain, archive) = (plain.clone(), archive.clone());
        bounded(move || {
            let cfg = Config {
                workers: 3,
                slots: 7,
                backend,
            };
            // Header + one byte of the first block header, ensuring encode
            // fails inside ordered commit, not before workers are started.
            let sink = Sink(zxcide::wire::FRAME_HEADER_SIZE + 1);
            let error = crate::encode(&plain[..], sink, options, cfg).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            let error = crate::decode(&archive[..], Sink(4097), Default::default(), cfg).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        });
    }
}
