//! Scratch-only lane capture. Binary layout is decoded in reduce.py.
use std::{
    cell::RefCell,
    fs::File,
    io::{self, BufWriter, Write},
    time::Instant,
};

pub(crate) const CAP: usize = 2048;
pub(crate) const NONE: u64 = u64::MAX;

#[derive(Clone, Copy)]
#[repr(u32)]
pub(crate) enum Op {
    Pool = 1,
    Joined,
    Pop,
    Full,
    Next,
    NextDone,
    JobSend,
    JobSent,
    ResultRecv,
    ResultGot,
    Pending,
    Commit,
    Credit,
    Terminal,
    Entry,
    JobRecv,
    JobGot,
    Compute,
    Computed,
    ResultSend,
    ResultSent,
    Exit,
}

struct Event(u64, u64, u32, u32);

pub(crate) struct Lane {
    start: Option<Instant>,
    rows: Vec<Event>,
    overflow: bool,
}

impl Lane {
    fn new(start: Option<Instant>) -> Self {
        Self {
            start,
            rows: Vec::with_capacity(if start.is_some() { CAP } else { 0 }),
            overflow: false,
        }
    }

    #[inline]
    pub(crate) fn at(&self) -> u64 {
        self.start.map_or(0, |start| start.elapsed().as_nanos() as u64)
    }

    #[inline]
    pub(crate) fn save(&mut self, t: u64, op: Op, i: u64, aux: u32) {
        if self.start.is_some() {
            if self.rows.len() == CAP {
                self.overflow = true;
            } else {
                self.rows.push(Event(t, i, op as u32, aux));
            }
        }
    }

    #[inline]
    pub(crate) fn mark(&mut self, op: Op, i: u64, aux: u32) {
        self.save(self.at(), op, i, aux);
    }
}

pub(crate) struct Blocks {
    pub(crate) input: Vec<u32>,
    pub(crate) output: Vec<u32>,
}

impl Blocks {
    pub(crate) fn new() -> Self {
        Self {
            input: Vec::with_capacity(CAP),
            output: Vec::with_capacity(CAP),
        }
    }

    pub(crate) fn push(v: &mut Vec<u32>, value: u32) {
        // Overflow is validated after decode by requiring both lists to match
        // the number of admitted jobs (and the lane overflow checks).
        if v.len() < CAP {
            v.push(value);
        }
    }
}

struct Pool {
    coord: Lane,
    workers: Vec<Lane>,
}

struct Frame {
    start: u64,
    footer: u64,
    end: u64,
    pool: Pool,
    blocks: Blocks,
}

struct Session {
    start: Instant,
    path: std::ffi::OsString,
    frames: Vec<Frame>,
    pool: Option<Pool>,
}

thread_local! { static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) }; }

/// Called around the CLI action including output flush; sidecar follows it.
pub fn session(action: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    let path = std::env::var_os("ZXC_WAIT_TRACE");
    SESSION.with_borrow_mut(|s| {
        *s = path.map(|path| Session {
            start: Instant::now(),
            path,
            frames: Vec::with_capacity(256),
            pool: None,
        });
    });
    let result = action();
    let s = SESSION.with_borrow_mut(Option::take);
    result?;
    if let Some(s) = s {
        s.finish()?;
    }
    Ok(())
}

pub(crate) fn frame_clock() -> Option<Instant> {
    SESSION.with_borrow(|s| s.as_ref().map(|s| s.start))
}

pub(crate) fn pool(workers: usize) -> (Lane, Vec<Lane>) {
    let clock = frame_clock();
    (Lane::new(clock), (0..workers).map(|_| Lane::new(clock)).collect())
}

pub(crate) fn joined(coord: Lane, workers: Vec<Lane>) {
    SESSION.with_borrow_mut(|s| {
        if let Some(s) = s {
            assert!(s.pool.replace(Pool { coord, workers }).is_none());
        }
    });
}

pub(crate) fn frame_done(start: u64, footer: u64, end: u64, blocks: Blocks) -> io::Result<()> {
    SESSION.with_borrow_mut(|s| {
        let s = s.as_mut().unwrap();
        if s.frames.len() == 256 {
            return Err(io::Error::other("too many probe frames"));
        }
        let pool = s
            .pool
            .take()
            .ok_or_else(|| io::Error::other("probe needs channel workers"))?;
        s.frames.push(Frame {
            start,
            footer,
            end,
            pool,
            blocks,
        });
        Ok(())
    })
}

impl Session {
    fn finish(self) -> io::Result<()> {
        let done = self.start.elapsed().as_nanos() as u64;
        if self.frames.is_empty()
            || self.pool.is_some()
            || self.frames.iter().any(|f| {
                f.pool.coord.overflow
                    || f.pool.workers.iter().any(|w| w.overflow)
                    || f.blocks.input.len() != f.blocks.output.len()
                    || f.pool
                        .coord
                        .rows
                        .iter()
                        .rev()
                        .nth(1)
                        .is_none_or(|e| e.2 != Op::Terminal as u32 || e.1 as usize != f.blocks.input.len())
            })
        {
            return Err(io::Error::other("missing/incomplete probe capture or overflowing lane"));
        }
        if self.path == "capture" {
            return Ok(());
        }
        let mut w = BufWriter::with_capacity(65536, File::create(self.path)?);
        macro_rules! write_le { ($($v:expr),+ $(,)?) => { $(w.write_all(&$v.to_le_bytes())?;)+ }; }
        w.write_all(b"ZXCWAIT1")?;
        write_le!(done, self.frames.len() as u32);
        for f in self.frames {
            write_le!(
                f.start,
                f.footer,
                f.end,
                f.blocks.input.len() as u32,
                f.blocks.output.len() as u32
            );
            for v in f.blocks.input.into_iter().chain(f.blocks.output) {
                write_le!(v);
            }
            write_le!(f.pool.workers.len() as u32);
            for lane in std::iter::once(f.pool.coord).chain(f.pool.workers) {
                write_le!(lane.rows.len() as u32);
                for Event(t, i, op, aux) in lane.rows {
                    write_le!(t, i, op, aux);
                }
            }
        }
        w.flush()
    }
}
