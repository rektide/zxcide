use crate::{
    Dictionary, bad,
    entropy::{self, Tree},
    require, take,
    wire::{DataBlockHeader, checksum},
    word,
};
use std::{borrow::Cow, io};

/// Reusable reconstruction scratch for decoding independent data blocks.
#[derive(Default)]
pub struct BlockDecoder {
    // Keep initialized scratch between blocks. Only the reconstructed prefix
    // is plaintext; the rest is never used as history or published.
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
    plain_len: usize,
    checksum: Option<u32>,
}

impl OwnedVerifiedBlock {
    pub fn as_verified(&self) -> VerifiedBlock<'_> {
        VerifiedBlock {
            bytes: &self.buffer[..self.plain_len],
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
        if header.kind != 0 {
            self.out.truncate(len);
            std::mem::swap(&mut self.out, &mut body);
        }
        Ok(OwnedVerifiedBlock {
            buffer: body,
            plain_len: len,
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
    let history = dict.map_or(&[][..], |d| d.content.as_slice());
    // Every token adds at least five bytes beyond the already bounded literals.
    let min_output = nl + n * 5;
    if out.len() < min_output {
        out.resize(min_output, 0);
    }
    // Select the token representation once. The sliced sections and exact
    // entropy counts give one offset per token, or n complete packed words.
    let len = if packed {
        let words = sequences.as_chunks::<4>().0;
        reconstruct::<255, _>(
            Scalar(
                words
                    .iter()
                    .map(|&[lo, hi, ml, ll]| (ll, ml, u16::from_le_bytes([lo, hi]))),
            ),
            &literals,
            data,
            limit,
            history,
            out,
        )?
    } else if width == 1 {
        reconstruct::<15, _>(
            Scalar(
                sequences
                    .iter()
                    .zip(offsets)
                    .map(|(&t, &off)| (t >> 4, t & 15, u16::from(off))),
            ),
            &literals,
            data,
            limit,
            history,
            out,
        )?
    } else {
        reconstruct::<15, _>(
            Spl16 {
                tokens: &sequences,
                offsets: offsets.as_chunks::<2>().0,
            },
            &literals,
            data,
            limit,
            history,
            out,
        )?
    };
    crate::probe!({
        add("reconstruct:literals", nl);
        add("reconstruct:matches", len - nl);
    });
    Ok(&out[..len])
}

// Other token layouts monomorphize to the checked loop without a batch branch.
trait Sequences {
    const SPL16: bool = false;
    fn next(&mut self) -> Option<(u8, u8, u16)>;

    // Return the number of literals consumed and total bytes produced, or
    // leave the token cursor and logical output prefix untouched on a miss.
    #[inline(always)]
    fn batch(
        &mut self,
        _lit: &[u8],
        _out: &mut [u8],
        _pos: usize,
        _end: usize,
        _limit: usize,
    ) -> Option<(usize, usize)> {
        None
    }
}

struct Scalar<I>(I);

impl<I: Iterator<Item = (u8, u8, u16)>> Sequences for Scalar<I> {
    #[inline(always)]
    fn next(&mut self) -> Option<(u8, u8, u16)> {
        self.0.next()
    }
}

struct Spl16<'a> {
    tokens: &'a [u8],
    offsets: &'a [[u8; 2]],
}

impl Sequences for Spl16<'_> {
    const SPL16: bool = true;

    #[inline(always)]
    fn next(&mut self) -> Option<(u8, u8, u16)> {
        let (&t, rest) = self.tokens.split_first()?;
        let (&off, tail) = self.offsets.split_first().unwrap();
        self.tokens = rest;
        self.offsets = tail;
        Some((t >> 4, t & 15, u16::from_le_bytes(off)))
    }

    #[inline(always)]
    fn batch(&mut self, lit: &[u8], out: &mut [u8], pos: usize, end: usize, limit: usize) -> Option<(usize, usize)> {
        const BATCH: usize = 2;
        let t = u32::from(u16::from_le_bytes(*self.tokens.first_chunk()?));
        // `end` counts all literals, even those still to be copied. Reserve
        // each preceding match (at most 19 bytes) plus the final 24-byte store.
        // Each preceding literal run consumes at most 14 bytes of its stream.
        if end + (BATCH - 1) * 19 + 24 > out.len().min(limit) || lit.len() < (BATCH - 1) * 14 + 16 {
            return None;
        }
        // Adding 1 to each isolated nibble sets bit 4 exactly for 0xF
        // (a length extension). The lanes are independent bytes.
        let nibble = 0x0f0f_0f0f;
        if ((((t & nibble) + 0x0101_0101) | (((t >> 4) & nibble) + 0x0101_0101)) & 0x1010_1010) != 0 {
            return None;
        }
        let (offs, tail) = self.offsets.split_first_chunk::<BATCH>().unwrap();
        let mut l = 0;
        let mut d = pos;
        for (i, &off) in offs.iter().enumerate() {
            let t = (t >> (8 * i)) as u8;
            let ll = usize::from(t >> 4);
            let ml = usize::from(t & 15) + 5;
            let off = usize::from(u16::from_le_bytes(off)) + 1;
            let m = d + ll;
            if off > m || off < ml {
                // Earlier stores only touched the uncommitted suffix. Resume
                // at the original token with the checked decoder.
                return None;
            }
            out[d..][..16].copy_from_slice(&lit[l..][..16]);
            // The needed source ends at or before `m`. Snapshot the wider
            // range before writing so even the padding may safely overlap.
            let src = *out[m - off..].first_chunk::<24>().unwrap();
            out[m..][..24].copy_from_slice(&src);
            l += ll;
            d = m + ml;
        }
        self.tokens = &self.tokens[BATCH..];
        self.offsets = tail;
        Some((l, d - pos))
    }
}

fn reconstruct<const MASK: u8, S: Sequences>(
    mut sequences: S,
    mut lit: &[u8],
    mut data: &[u8],
    limit: usize,
    history: &[u8],
    out: &mut Vec<u8>,
) -> io::Result<usize> {
    let mut match_budget = limit - lit.len();
    let mut pos = 0;
    loop {
        if let Some((l, d)) = sequences.batch(lit, out, pos, limit - match_budget, limit) {
            lit = &lit[l..];
            pos += d;
            match_budget -= d - l;
            continue;
        }
        let Some((ll, ml, off)) = sequences.next() else { break };
        let mut ll = usize::from(ll);
        if ll == usize::from(MASK) {
            ll += extra(&mut data)?;
        }
        let mut ml = usize::from(ml) + 5;
        if ml == usize::from(MASK) + 5 {
            ml += extra(&mut data)?;
        }
        let off = usize::from(off) + 1;
        require(ml <= match_budget, "match exceeds block size")?;
        match_budget -= ml;
        // The bound includes all literals, even those not yet written. Grow
        // geometrically only when needed, not to the frame limit on entry.
        let need = limit - match_budget;
        if need > out.len() {
            out.resize(need.max(out.len() * 2).min(limit), 0);
        }
        let match_at = pos + ll;
        // Fixed-width copies avoid a variable-size copy for short runs. Extra
        // bytes are outside the logical prefix and will be overwritten before
        // they can be used as history or returned as plaintext.
        // The wider SPL16 copy also handles short extended literal runs.
        if S::SPL16
            && ll <= 32
            && pos + 32 <= out.len()
            && let Some(bytes) = lit.first_chunk::<32>()
        {
            out[pos..pos + 32].copy_from_slice(bytes);
            lit = &lit[ll..];
        } else if ll <= 8
            && pos + 8 <= out.len()
            && let Some(bytes) = lit.first_chunk::<8>()
        {
            out[pos..pos + 8].copy_from_slice(bytes);
            lit = &lit[ll..];
        } else if ll != 0 {
            let src = take(&mut lit, ll)?;
            out[pos..match_at].copy_from_slice(src);
        }
        pos = match_at;
        if off <= pos && ml <= off {
            let start = pos - off;
            // The logical source is all in previously written output. The
            // wider copy may overlap only in its unneeded bytes; copy_within
            // preserves the original source bytes in that case too.
            // Cover all inline SPL lengths without a copy-width ladder.
            if S::SPL16 && ml <= 24 && pos + 24 <= out.len() {
                out.copy_within(start..start + 24, pos);
            } else if ml <= 8 && pos + 8 <= out.len() {
                out.copy_within(start..start + 8, pos);
            } else if ml <= 16 && pos + 16 <= out.len() {
                out.copy_within(start..start + 16, pos);
            } else {
                out.copy_within(start..start + ml, pos);
            }
            pos += ml;
            continue;
        }
        if off > pos {
            // The dictionary precedes the output in the logical history. Copy
            // at most its suffix, then continue at output[0] if the match crosses.
            let missing = off - pos;
            let start = history
                .len()
                .checked_sub(missing)
                .ok_or_else(|| bad("match before start of history"))?;
            let amount = ml.min(missing);
            out[pos..pos + amount].copy_from_slice(&history[start..start + amount]);
            pos += amount;
            ml -= amount;
            if ml == 0 {
                continue;
            }
        }
        let start = pos - off;
        // Copy the already available pattern, doubling it on each overlap.
        while ml != 0 {
            let amount = ml.min(pos - start);
            out.copy_within(start..start + amount, pos);
            pos += amount;
            ml -= amount;
        }
    }
    out[pos..pos + lit.len()].copy_from_slice(lit);
    Ok(pos + lit.len())
}

#[cfg(test)]
mod tests;
