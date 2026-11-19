use crate::{
    Dictionary, bad,
    entropy::{self, Tree},
    require, take, word,
};
use std::{borrow::Cow, io};

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
pub fn unpack<'a>(
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
    if let Some(dict) = dict {
        out.extend_from_slice(&dict.content);
    }
    let prefix = out.len();
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
        require(off <= out.len(), "match before start of history")?;
        let start = out.len() - off;
        // Copy the already available pattern, doubling it on each overlap.
        while ml != 0 {
            let amount = ml.min(out.len() - start);
            out.extend_from_within(start..start + amount);
            ml -= amount;
        }
    }
    out.extend_from_slice(lit);
    Ok(&out[prefix..])
}
