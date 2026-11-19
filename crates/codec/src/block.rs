use crate::{
    Dictionary, bad,
    entropy::{self, Tree},
    require, take,
    wire::{DataBlockHeader, checksum},
    word,
};
use std::{borrow::Cow, io, ops::Range};

/// Reusable reconstruction scratch for decoding independent data blocks.
#[derive(Default)]
pub struct BlockDecoder {
    out: Vec<u8>,
}

/// Plaintext and the verified checksum (if any) of one block.
#[derive(Debug)]
pub struct VerifiedBlock<'a> {
    pub bytes: &'a [u8],
    pub checksum: Option<u32>,
}

/// A verified block whose backing buffer can move between threads.
pub struct OwnedVerifiedBlock {
    buffer: Vec<u8>,
    plain: Range<usize>,
    checksum: Option<u32>,
}

impl OwnedVerifiedBlock {
    pub fn as_verified(&self) -> VerifiedBlock<'_> {
        VerifiedBlock {
            bytes: &self.buffer[self.plain.clone()],
            checksum: self.checksum,
        }
    }

    /// Reclaim the backing buffer after an ordered write.
    pub fn into_buffer(self) -> Vec<u8> {
        self.buffer
    }
}

impl BlockDecoder {
    /// Decode a parsed header's body (payload followed by any checksum).
    /// `index` is the zero-based block index within the frame. A positioned
    /// reader should supply the expected plaintext size from the frame layout.
    pub fn decode<'a>(
        &'a mut self,
        header: DataBlockHeader,
        body: &'a [u8],
        dictionary: Option<&Dictionary>,
        index: u64,
        expected_size: Option<usize>,
    ) -> io::Result<VerifiedBlock<'a>> {
        require(body.len() == header.body_size(), "incorrect data block body size")?;
        require(
            dictionary.map(Dictionary::id) == header.frame.dictionary_id(),
            "data block dictionary does not match frame",
        )?;
        let (payload, trailer) = body.split_at(header.payload_size);
        let bytes = if header.kind == 0 {
            payload
        } else {
            unpack(
                header.kind,
                payload,
                header.frame.block_size(),
                dictionary,
                &mut self.out,
            )?
        };
        require(
            expected_size.is_none_or(|size| size == bytes.len()),
            "unexpected decoded block size",
        )?;
        let checksum = if header.frame.checksum() {
            let c = u32::from_le_bytes(trailer.try_into().unwrap());
            require(checksum(bytes, index) == c, "block checksum mismatch")?;
            Some(c)
        } else {
            None
        };
        Ok(VerifiedBlock { bytes, checksum })
    }

    /// Like [`Self::decode`], transferring a buffer without copying verified
    /// plaintext. A RAW block's backing buffer can also hold checksum bytes.
    pub fn decode_owned(
        &mut self,
        header: DataBlockHeader,
        mut body: Vec<u8>,
        dictionary: Option<&Dictionary>,
        index: u64,
    ) -> io::Result<OwnedVerifiedBlock> {
        let block = self.decode(header, &body, dictionary, index, None)?;
        let (len, checksum) = (block.bytes.len(), block.checksum);
        let start = if header.kind == 0 {
            0
        } else {
            let start = self.out.len() - len;
            std::mem::swap(&mut self.out, &mut body);
            start
        };
        Ok(OwnedVerifiedBlock {
            buffer: body,
            plain: start..start + len,
            checksum,
        })
    }
}

fn extra(src: &mut &[u8]) -> io::Result<usize> {
    let b = take(src, 1)?[0] as usize;
    Ok(match b {
        0..128 => b,
        128..192 => (b & 63) | ((take(src, 1)?[0] as usize) << 6),
        192..224 => {
            let hi = take(src, 2)?;
            (b & 31) | ((hi[0] as usize) << 5) | ((hi[1] as usize) << 13)
        }
        _ => return Err(bad("invalid length prefix")),
    })
}

fn section<'a>(encoding: u8, mut data: &'a [u8], count: usize, dict: Option<&Dictionary>) -> io::Result<Cow<'a, [u8]>> {
    Ok(match encoding {
        0 => Cow::Borrowed(data),
        1 => Cow::Owned(entropy::rle(data, count)?),
        2 => {
            let tree = Tree::new(take(&mut data, 128)?)?;
            Cow::Owned(tree.decode(data, count)?)
        }
        3 => Cow::Owned(
            dict.and_then(|d| d.tree.as_ref())
                .ok_or_else(|| bad("missing dictionary Huffman table"))?
                .decode(data, count)?,
        ),
        _ => return Err(bad("unknown section encoding")),
    })
}

/// Reconstruct an SPL/PAK block in reusable storage, returning only source bytes.
fn unpack<'a>(
    kind: u8,
    mut data: &[u8],
    limit: usize,
    dict: Option<&Dictionary>,
    out: &'a mut Vec<u8>,
) -> io::Result<&'a [u8]> {
    let n = word(&mut data)?;
    let nl = word(&mut data)?;
    require(nl <= limit && n <= (limit - nl) / 5, "decoded counts exceed block size")?;
    let enc = take(&mut data, 4)?;
    let packed = kind == 2;
    require(
        if packed {
            enc[0] == 0 && enc[1] == 0
        } else {
            enc[0] <= 3 && matches!(enc[1], 0 | 2) && enc[3] <= 1
        },
        "unknown sequence encoding",
    )?;
    let lit_size = if enc[0] == 0 { nl } else { word(&mut data)? };
    let seq_size = if packed {
        n * 4
    } else if enc[1] == 0 {
        n
    } else {
        word(&mut data)?
    };
    let lit_data = take(&mut data, lit_size)?;
    require(data.len() >= 32, "missing literal slack")?;
    let seq_data = take(&mut data, seq_size)?;
    let width = if enc[3] == 1 { 1 } else { 2 };
    let offsets = take(&mut data, if packed { 0 } else { n * width })?;
    let literals = section(enc[0], lit_data, nl, dict)?;
    let sequences = section(enc[1], seq_data, n, None)?;

    zone!("LZ reconstruct");
    let mut lit = &literals[..];
    out.clear();
    let history = dict.map_or(&[][..], |d| d.content.as_slice());
    let mut match_budget = limit - nl;
    for i in 0..n {
        let (mut ll, mut ml, off, mask) = if packed {
            let w = &sequences[4 * i..4 * i + 4];
            (
                usize::from(w[3]),
                usize::from(w[2]),
                u16::from_le_bytes([w[0], w[1]]) as usize + 1,
                255,
            )
        } else {
            let t = sequences[i];
            let off = if width == 1 {
                u16::from(offsets[i])
            } else {
                u16::from_le_bytes([offsets[2 * i], offsets[2 * i + 1]])
            };
            (usize::from(t >> 4), usize::from(t & 15), usize::from(off) + 1, 15)
        };
        if ll == mask {
            ll += extra(&mut data)?;
        }
        if ml == mask {
            ml += extra(&mut data)?;
        }
        ml += 5;
        require(ml <= match_budget, "match exceeds block size")?;
        match_budget -= ml;
        out.extend_from_slice(take(&mut lit, ll)?);
        if off > out.len() {
            // The dictionary precedes the output in the logical history. Copy
            // at most its suffix, then continue at output[0] if the match crosses.
            let missing = off - out.len();
            let start = history
                .len()
                .checked_sub(missing)
                .ok_or_else(|| bad("match before start of history"))?;
            let amount = ml.min(missing);
            out.extend_from_slice(&history[start..start + amount]);
            ml -= amount;
            if ml == 0 {
                continue;
            }
        }
        let start = out.len() - off;
        // Copy the already available pattern, doubling it on each overlap.
        while ml != 0 {
            let amount = ml.min(out.len() - start);
            out.extend_from_within(start..start + amount);
            ml -= amount;
        }
    }
    out.extend_from_slice(lit);
    crate::probe!({
        add("reconstruct:literals", nl);
        add("reconstruct:matches", out.len() - nl);
    });
    Ok(out)
}

#[cfg(test)]
mod tests;
