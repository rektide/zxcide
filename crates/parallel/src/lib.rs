//! Bounded, ordered streaming ZXC encoding and decoding. Stream I/O remains
//! on the caller's thread; only independent data blocks are sent to workers.
#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use zxc_codec::{
    BlockDecoder, BlockEncoder, DecodeOptions, Dictionary, EncodeOptions,
    stream::{DecodeFrame, EncodeFrame, read_data_block, read_source_block},
    wire::FrameHeader,
};

macro_rules! zone {
    ($name:literal) => {
        #[cfg(feature = "tracy")]
        let _zone = tracy_client::span!($name);
    };
}

mod channel;
#[cfg(feature = "rayon")]
mod rayon;
#[cfg(test)]
mod scheduler_tests;

/// The transport used for multi-worker operations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Backend {
    #[default]
    Crossbeam,
    Flume,
    Kanal,
    Crossfire,
    Rayon,
}

impl Backend {
    /// Backends built into this binary.
    pub const fn available() -> &'static [Self] {
        &[
            Self::Crossbeam,
            #[cfg(feature = "flume")]
            Self::Flume,
            #[cfg(feature = "kanal")]
            Self::Kanal,
            #[cfg(feature = "crossfire")]
            Self::Crossfire,
            #[cfg(feature = "rayon")]
            Self::Rayon,
        ]
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Crossbeam => "crossbeam",
            Self::Flume => "flume",
            Self::Kanal => "kanal",
            Self::Crossfire => "crossfire",
            Self::Rayon => "rayon",
        }
    }
}

/// Worker count and bound on blocks read but not yet written. A slot remains
/// occupied even after out-of-order completion. The seek index, if requested,
/// is outside this bound, as is the private scratch of each worker.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub workers: usize,
    pub slots: usize,
    pub backend: Backend,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            workers: 1,
            slots: 2,
            backend: Backend::default(),
        }
    }
}

impl Config {
    pub const MAX_WORKERS: usize = 256;
    pub const MAX_SLOTS: usize = 256;

    /// Validate bounds and feature availability without starting any workers.
    pub fn validate(self) -> io::Result<()> {
        for (name, value, max) in [
            ("workers", self.workers, Self::MAX_WORKERS),
            ("slots", self.slots, Self::MAX_SLOTS),
        ] {
            if !(1..=max).contains(&value) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{name} must be 1..={max}"),
                ));
            }
        }
        if !Backend::available().contains(&self.backend) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} backend is not enabled", self.backend.name()),
            ));
        }
        Ok(())
    }
}

trait Scheduler: Sized {
    fn run<S: Default + Send, J: Send + 'static, O: Send + 'static>(
        &self,
        next: impl FnMut(&mut Vec<u8>) -> io::Result<Option<J>>,
        work: impl Fn(&mut S, u64, J, Vec<u8>) -> io::Result<O> + Sync,
        commit: impl FnMut(O) -> io::Result<Vec<u8>>,
    ) -> io::Result<()>;
}

fn compute<S: Default, J, O>(
    scratch: &mut Option<S>,
    index: u64,
    job: J,
    work: &impl Fn(&mut S, u64, J) -> io::Result<O>,
) -> io::Result<O> {
    // Put worker panics in the result stream too: otherwise a missing sequence
    // could leave the coordinator waiting while the remaining workers idle.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        zone!("parallel worker compute");
        work(scratch.get_or_insert_with(S::default), index, job)
    }));
    result.unwrap_or_else(|_| {
        *scratch = None;
        Err(io::Error::other(format!("block worker panicked at index {index}")))
    })
}

macro_rules! dispatch {
    ($config:ident, $serial:path, $crossbeam:ident, $parallel:ident, $r:ident, $w:ident, $options:ident) => {{
        $config.validate()?;
        if $config.workers == 1 {
            return $serial($r, $w, $options);
        }
        match $config.backend {
            Backend::Crossbeam => $crossbeam(channel::Driver::<channel::Crossbeam>::new($config), $r, $w, $options),
            #[cfg(feature = "flume")]
            Backend::Flume => $parallel(channel::Driver::<channel::Flume>::new($config), $r, $w, $options),
            #[cfg(feature = "kanal")]
            Backend::Kanal => $parallel(channel::Driver::<channel::Kanal>::new($config), $r, $w, $options),
            #[cfg(feature = "crossfire")]
            Backend::Crossfire => $parallel(channel::Driver::<channel::Crossfire>::new($config), $r, $w, $options),
            #[cfg(feature = "rayon")]
            Backend::Rayon => rayon::Driver::with($config, |driver| $parallel(driver, $r, $w, $options)),
            #[allow(unreachable_patterns)]
            _ => unreachable!("backend availability was validated"),
        }
    }};
}

/// Encode one frame. The caller owns flushing and output-file publication.
pub fn encode(r: impl Read, w: impl Write, options: EncodeOptions<'_>, config: Config) -> io::Result<()> {
    dispatch!(config, zxc_codec::encode, encode_with, encode_with, r, w, options)
}

fn encode_with(
    driver: impl Scheduler,
    mut r: impl Read,
    mut w: impl Write,
    options: EncodeOptions<'_>,
) -> io::Result<()> {
    zone!("parallel encode frame");
    let mut sizes = Vec::new();
    let mut frame = EncodeFrame::start(&mut w, options, &mut sizes)?;
    let block_size = frame.block_size();
    let mut eof = false;
    driver.run(
        |buf| {
            if eof {
                return Ok(None);
            }
            read_source_block(&mut r, buf, block_size)?;
            eof = buf.len() < block_size;
            Ok((!buf.is_empty()).then_some(()))
        },
        |work: &mut BlockEncoder, index, (), buf| Ok(work.encode_owned(buf, options, index)),
        |block| {
            frame.commit(&mut w, &block)?;
            Ok(block.into_payload())
        },
    )?;
    frame.finish(&mut w)
}

/// Decode all concatenated frames. A failure can leave earlier verified
/// blocks in the writer, never a corrupt block or a later one. A synchronous
/// read or write that blocks externally cannot be interrupted by the driver.
/// The caller owns flushing and output-file publication.
pub fn decode(r: impl Read, w: impl Write, options: DecodeOptions<'_>, config: Config) -> io::Result<()> {
    dispatch!(config, zxc_codec::decode, decode_channel, decode_with, r, w, options)
}

fn decode_channel<Q: channel::Transport>(
    driver: channel::Driver<Q>,
    mut r: impl Read,
    mut w: impl Write,
    options: DecodeOptions<'_>,
) -> io::Result<()> {
    let dicts = options.dictionaries;
    let mut total = 0;
    let mut next = DecodeFrame::start(&mut r, true)?;
    while let Some(mut frame) = next {
        let size = frame.header().block_size();
        let slot = dictionary_slot(frame.header(), dicts)?;
        next = driver.with_workers(
            |work: &mut BlockDecoder, index, (header, slot): (_, Option<usize>), buf| {
                Ok((header, work.decode_owned(header, buf, slot.map(|i| &dicts[i]), index)?))
            },
            |session| {
                let mut slot = slot;
                loop {
                    {
                        zone!("parallel decode frame");
                        let settings = frame.header();
                        session.run(
                            |buf| Ok(read_data_block(&mut r, settings, buf)?.map(|header| (header, slot))),
                            |(header, block)| {
                                frame.commit(&mut w, header, block.as_verified(), &mut total, options.max_output)?;
                                Ok(block.into_buffer())
                            },
                        )?;
                        // All jobs and writes precede the footer, and no input
                        // from the next frame is read until finish succeeds.
                        frame.finish(&mut r)?;
                    }
                    match DecodeFrame::start(&mut r, false)? {
                        Some(new) if new.header().block_size() == size => {
                            slot = dictionary_slot(new.header(), dicts)?;
                            frame = new;
                        }
                        // End this resource epoch. Joining drops even idle
                        // worker scratch and every slot before the size change.
                        next => return Ok(next),
                    }
                }
            },
        )?;
    }
    Ok(())
}

fn dictionary_slot(header: FrameHeader, dicts: &[Dictionary]) -> io::Result<Option<usize>> {
    // Resolve once, including on empty frames. Like FrameHeader::dictionary,
    // select the first match (ID zero is valid); jobs carry no borrowed data.
    header
        .dictionary_id()
        .map(|id| {
            dicts
                .iter()
                .position(|d| d.id() == id)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("dictionary {id:08x} required")))
        })
        .transpose()
}

#[cfg(any(feature = "flume", feature = "kanal", feature = "crossfire", feature = "rayon"))]
fn decode_with(
    driver: impl Scheduler,
    mut r: impl Read,
    mut w: impl Write,
    options: DecodeOptions<'_>,
) -> io::Result<()> {
    let mut first = true;
    let mut total = 0;
    while let Some(mut frame) = DecodeFrame::start(&mut r, first)? {
        zone!("parallel decode frame");
        first = false;
        let settings = frame.header();
        let dict = settings.dictionary(options.dictionaries)?;
        driver.run(
            |buf| read_data_block(&mut r, settings, buf),
            |work: &mut BlockDecoder, index, header, buf| Ok((header, work.decode_owned(header, buf, dict, index)?)),
            |(header, block)| {
                frame.commit(&mut w, header, block.as_verified(), &mut total, options.max_output)?;
                Ok(block.into_buffer())
            },
        )?;
        frame.finish(&mut r)?;
    }
    Ok(())
}
