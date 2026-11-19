//! Native, block-bounded ZXC v9 compression and decompression.
#![forbid(unsafe_code)]

#[cfg(feature = "ledger")]
pub mod ledger;

/// Include a diagnostic block only in a ledger build (including downstream probes).
#[cfg(feature = "ledger")]
#[macro_export]
macro_rules! probe {
    ($body:block) => {{
        use $crate::ledger::add;
        $body
    }};
}
#[cfg(not(feature = "ledger"))]
#[macro_export]
macro_rules! probe {
    ($body:block) => {};
}

macro_rules! zone {
    ($name:literal) => {
        #[cfg(feature = "tracy")]
        let _zone = tracy_client::span!($name);
    };
}

mod block;
/// Diagnostic wrapper for the block input/body buffers, including positioned reads.
#[macro_export]
#[cfg(feature = "ledger")]
macro_rules! resize_zero {
    ($buf:ident, $len:expr) => {{
        let len = $len;
        let old = $buf.capacity();
        $crate::probe!({
            add("zero:read buffer", len.saturating_sub($buf.len()));
            if len <= $buf.capacity() {
                add("capacity:request fits", len);
            }
        });
        $buf.resize(len, 0);
        $crate::ledger::add("capacity:growth delta", $buf.capacity() - old);
    }};
}
#[cfg(not(feature = "ledger"))]
#[macro_export]
macro_rules! resize_zero {
    ($buf:ident, $len:expr) => {
        $buf.resize($len, 0)
    };
}
mod dict;
mod encode;
mod entropy;
mod frame;
pub mod wire;
pub use block::{BlockDecoder, OwnedVerifiedBlock, VerifiedBlock};
pub use dict::Dictionary;
pub use encode::{BlockEncoder, EncodedBlock};
pub use frame::{DecodeOptions, Decoder, EncodeOptions, Encoder, Mode, decode, encode};

/// Frame I/O shared by the serial codec and the parallel codec driver.
pub mod stream {
    pub use crate::frame::{DecodeFrame, EncodeFrame, read_data_block, read_source_block};
}

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
