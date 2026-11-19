//! ZXC v9 wire validation shared by streaming and positioned readers.

use crate::{Dictionary, bad, require};
use std::io;

pub(crate) const MAGIC: [u8; 4] = 0x9cb02ef5u32.to_le_bytes();
pub const FRAME_HEADER_SIZE: usize = 16;
pub const BLOCK_HEADER_SIZE: usize = 8;
pub const CHECKSUM_SIZE: usize = 4;
pub const DIGEST_SIZE: usize = 8;
pub const SEEK_GROUP_BLOCKS: usize = 64;
pub const SEEK_ANCHOR_SIZE: usize = 8;
pub const SEEK_ENTRY_SIZE: usize = 4;

/// Validated settings of an independent frame.
#[derive(Clone, Copy, Debug)]
pub struct FrameHeader {
    block_size: usize,
    checksum: bool,
    seekable: bool,
    dictionary_id: Option<u32>,
}

impl FrameHeader {
    pub fn parse(h: [u8; FRAME_HEADER_SIZE]) -> io::Result<Self> {
        require(h[..4] == MAGIC, "bad ZXC magic or trailing data")?;
        require(h[4] == 9, "unsupported ZXC version")?;
        require(
            h[14..] == header_sum(&h).to_le_bytes()
                && (12..=21).contains(&h[5])
                && h[6] & 31 == 0
                && h[11..14] == [0; 3],
            "invalid ZXC file header",
        )?;
        let id = u32::from_le_bytes(h[7..11].try_into().unwrap());
        let has_dictionary = h[6] & 64 != 0;
        require(has_dictionary || id == 0, "reserved dictionary field")?;
        Ok(Self {
            block_size: 1 << h[5],
            checksum: h[6] & 128 != 0,
            seekable: h[6] & 32 != 0,
            dictionary_id: has_dictionary.then_some(id),
        })
    }

    pub fn block_size(self) -> usize {
        self.block_size
    }

    pub fn checksum(self) -> bool {
        self.checksum
    }

    pub fn seekable(self) -> bool {
        self.seekable
    }

    pub fn dictionary_id(self) -> Option<u32> {
        self.dictionary_id
    }

    /// The physical overhead of a data block, including any checksum.
    pub fn block_overhead(self) -> usize {
        BLOCK_HEADER_SIZE + if self.checksum { CHECKSUM_SIZE } else { 0 }
    }

    /// Resolve the frame's dictionary by ID (including zero).
    pub fn dictionary(self, dictionaries: &[Dictionary]) -> io::Result<Option<&Dictionary>> {
        self.dictionary_id
            .map(|id| {
                dictionaries
                    .iter()
                    .find(|dict| dict.id() == id)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, format!("dictionary {id:08x} required")))
            })
            .transpose()
    }
}

/// A validated RAW, SPL or PAK header bound to its frame settings.
#[derive(Clone, Copy, Debug)]
pub struct DataBlockHeader {
    pub(crate) frame: FrameHeader,
    pub(crate) kind: u8,
    pub(crate) payload_size: usize,
}

impl DataBlockHeader {
    pub fn parse(h: [u8; BLOCK_HEADER_SIZE], frame: FrameHeader) -> io::Result<Self> {
        let size = u32::from_le_bytes(h[3..7].try_into().unwrap());
        require(h == block_header(h[0], size), "invalid block header")?;
        require(
            h[0] <= 2 && size as u64 <= frame.block_size as u64,
            "invalid data block type or size",
        )?;
        Ok(Self {
            frame,
            kind: h[0],
            payload_size: size as usize,
        })
    }

    /// Required bytes after the header, including any trailing checksum.
    pub fn body_size(self) -> usize {
        self.disk_size() - BLOCK_HEADER_SIZE
    }

    pub fn disk_size(self) -> usize {
        self.frame.block_overhead() + self.payload_size
    }
}

pub fn validate_eof_header(h: [u8; BLOCK_HEADER_SIZE]) -> io::Result<()> {
    require(h == block_header(255, 0), "invalid EOF block header")
}

/// Number of payload bytes in a seek table with `blocks` data blocks.
pub fn seek_table_len(blocks: u64) -> io::Result<u64> {
    let sizes = blocks
        .checked_mul(SEEK_ENTRY_SIZE as u64)
        .ok_or_else(|| bad("seek table size overflow"))?;
    add(
        sizes,
        blocks.div_ceil(SEEK_GROUP_BLOCKS as u64) * SEEK_ANCHOR_SIZE as u64,
    )
}

pub fn validate_seek_header(h: [u8; BLOCK_HEADER_SIZE], table_len: u64) -> io::Result<()> {
    require(h == seek_header(table_len), "invalid seek table header")
}

pub(crate) fn seek_header(table_len: u64) -> [u8; BLOCK_HEADER_SIZE] {
    block_header(254, (table_len ^ (table_len >> 32)) as u32)
}

const GOLD: u64 = 0x9e3779b97f4a7c15;
pub(crate) fn header_sum(h: &[u8; FRAME_HEADER_SIZE]) -> u16 {
    let a = u64::from_le_bytes(h[..8].try_into().unwrap());
    let b = u64::from_le_bytes(h[8..].try_into().unwrap()) & 0xffff_ffff_ffff;
    let c = 0xd2d84a61d2d84a61;
    ((a ^ c)
        .wrapping_mul(c)
        .wrapping_add(b)
        .wrapping_add(GOLD)
        .wrapping_mul(GOLD)
        >> 48) as u16
}
pub(crate) fn block_header(kind: u8, size: u32) -> [u8; BLOCK_HEADER_SIZE] {
    let mut h = [0; 8];
    h[0] = kind;
    h[3..7].copy_from_slice(&size.to_le_bytes());
    h[7] = ((u64::from_le_bytes(h) ^ GOLD).wrapping_mul(GOLD) >> 56) as u8;
    h
}
pub(crate) fn checksum(data: &[u8], seed: u64) -> u32 {
    zone!("checksum");
    use rapidhash::v3::{RapidSecrets, rapidhash_v3_seeded};
    let h = rapidhash_v3_seeded(data, &RapidSecrets::seed_cpp(seed));
    (h ^ (h >> 32)) as u32
}
pub(crate) fn fold(previous: u64, sum: u32) -> u64 {
    let x = (u64::from(sum) + 1).wrapping_mul(GOLD);
    let p = u128::from(previous ^ x) * 0x2545f4914f6cdd1d;
    (p ^ (p >> 64)) as u64
}
pub(crate) fn add(x: u64, y: u64) -> io::Result<u64> {
    x.checked_add(y).ok_or_else(|| bad("frame size overflow"))
}
fn width(x: u64) -> usize {
    (64 - x.leading_zeros()).div_ceil(8).max(1) as usize
}
pub(crate) fn footer(plain: u64, prefix: u64, digest: Option<u64>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(25);
    if let Some(d) = digest {
        bytes.extend(d.to_le_bytes());
    }
    let nd = width(plain);
    bytes.extend(&plain.to_le_bytes()[..nd]);
    let base = add(prefix, bytes.len() as u64 + 1)?;
    let mut nf = 1;
    loop {
        let size = add(base, nf as u64)?;
        if width(size) <= nf {
            bytes.extend(&size.to_le_bytes()[..nf]);
            bytes.push(((nf as u8 - 1) << 4) | (nd as u8 - 1));
            return Ok(bytes);
        }
        nf += 1;
    }
}

/// Widths encoded in the last byte of a frame, independent of the digest flag.
#[derive(Clone, Copy, Debug)]
pub struct FooterWidths {
    plain: usize,
    compressed: usize,
}

impl FooterWidths {
    pub fn parse(last: u8) -> io::Result<Self> {
        require(last & 0x88 == 0, "reserved footer length bits")?;
        Ok(Self {
            plain: usize::from(last & 7) + 1,
            compressed: usize::from(last >> 4) + 1,
        })
    }

    /// Bytes occupied by the two size fields (not including the last byte or digest).
    pub fn size_bytes(self) -> usize {
        self.plain + self.compressed
    }

    /// Parse the size fields, rejecting nonminimal encodings. Returns the plain
    /// size and the total compressed size, including the entire footer.
    pub fn sizes(self, bytes: &[u8]) -> io::Result<(u64, u64)> {
        require(bytes.len() == self.size_bytes(), "invalid footer sizes")?;
        let read = |bytes: &[u8]| {
            let mut buf = [0; 8];
            buf[..bytes.len()].copy_from_slice(bytes);
            u64::from_le_bytes(buf)
        };
        let plain = read(&bytes[..self.plain]);
        let compressed = read(&bytes[self.plain..]);
        // A shorter compressed-size field would reduce the total frame size by
        // a byte. Check the value that shorter field would actually have to hold.
        require(
            width(plain) == self.plain
                && (self.compressed == 1 || width(compressed.saturating_sub(1)) == self.compressed),
            "nonminimal footer sizes",
        )?;
        Ok((plain, compressed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_widths_handle_self_inclusive_size_boundaries() {
        for bytes in 1..=7 {
            let boundary = 1u64 << (8 * bytes);
            let plain = boundary - 1;
            // At each byte-width boundary the smallest encoded frame size that
            // needs an extra byte is boundary+1, not boundary.
            for (prefix, compressed) in [
                (boundary - 2 * bytes - 2, boundary - 1),
                (boundary - 2 * bytes - 1, boundary + 1),
            ] {
                let encoded = footer(plain, prefix, None).unwrap();
                let (&last, sizes) = encoded.split_last().unwrap();
                assert_eq!(
                    FooterWidths::parse(last).unwrap().sizes(sizes).unwrap(),
                    (plain, compressed)
                );
            }
            let mut nonminimal = plain.to_le_bytes()[..bytes as usize].to_vec();
            nonminimal.extend_from_slice(&boundary.to_le_bytes()[..bytes as usize + 1]);
            let last = (bytes as u8) << 4 | (bytes as u8 - 1);
            assert!(FooterWidths::parse(last).unwrap().sizes(&nonminimal).is_err());
        }
        assert!(FooterWidths::parse(1).unwrap().sizes(&[0, 0, 33]).is_err());
    }
}
