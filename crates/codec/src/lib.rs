//! Native, block-bounded ZXC v9 compression and decompression.
#![forbid(unsafe_code)]

macro_rules! zone {
    ($name:literal) => {
        #[cfg(feature = "tracy")]
        let _zone = tracy_client::span!($name);
    };
}

mod block;
mod dict;
mod encode;
mod entropy;
mod frame;
pub mod wire;
pub use block::{BlockDecoder, VerifiedBlock};
pub use dict::Dictionary;
pub use frame::{DecodeOptions, Decoder, EncodeOptions, Encoder, Mode, decode, encode};

use std::io;
fn bad(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn require(ok: bool, msg: &'static str) -> io::Result<()> {
    if ok { Ok(()) } else { Err(bad(msg)) }
}

/// Consume a checked field from a block payload.
fn take<'a>(src: &mut &'a [u8], n: usize) -> io::Result<&'a [u8]> {
    let (head, tail) = src.split_at_checked(n).ok_or_else(|| bad("short section"))?;
    *src = tail;
    Ok(head)
}
fn word(src: &mut &[u8]) -> io::Result<usize> {
    Ok(u32::from_le_bytes(take(src, 4)?.try_into().unwrap()) as usize)
}
