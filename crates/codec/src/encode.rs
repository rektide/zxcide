//! Greedy 64 KiB-window parser. The same parse can be priced in either layout.
use crate::{Dictionary, Mode};

#[derive(Clone, Copy)]
struct Match {
    at: usize,
    len: usize,
    off: u16,
}

#[derive(Default)]
pub struct Workspace {
    table: Vec<u32>,
    matches: Vec<Match>,
    history: Vec<u8>,
    payload: Vec<u8>,
    dense: Vec<u8>,
}

fn key(data: &[u8], i: usize) -> usize {
    (u32::from_le_bytes(data[i..i + 4].try_into().unwrap()).wrapping_mul(0x9e3779b1) >> 16) as usize
}
fn parse(data: &[u8], prefix: usize, table: &mut Vec<u32>, matches: &mut Vec<Match>) {
    zone!("match finder");
    table.clear();
    table.resize(65536, u32::MAX);
    matches.clear();
    for i in 0..prefix.min(data.len().saturating_sub(4)) {
        table[key(data, i)] = i as u32;
    }
    let mut i = prefix;
    while i + 5 <= data.len() {
        let slot = &mut table[key(data, i)];
        let p = std::mem::replace(slot, i as u32) as usize;
        if p >= i || i - p > 65536 || data[p..p + 5] != data[i..i + 5] {
            i += 1;
            continue;
        }
        let mut len = 5;
        while i + len + 8 <= data.len() && data[p + len..p + len + 8] == data[i + len..i + len + 8] {
            len += 8;
        }
        while i + len < data.len() && data[p + len] == data[i + len] {
            len += 1;
        }
        matches.push(Match {
            at: i - prefix,
            len,
            off: (i - p - 1) as u16,
        });
        i += len;
    }
}
fn varint(out: &mut Vec<u8>, n: usize) {
    if n < 128 {
        out.push(n as u8)
    } else if n < 16384 {
        out.extend([(n as u8 & 63) | 128, (n >> 6) as u8])
    } else {
        out.extend([(n as u8 & 31) | 192, (n >> 5) as u8, (n >> 13) as u8])
    }
}

impl Workspace {
    pub fn block<'a>(&'a mut self, plain: &'a [u8], mode: Mode, dict: Option<&Dictionary>) -> (u8, &'a [u8]) {
        if plain.len() <= 44 {
            return (0, plain);
        }
        let (input, prefix) = if let Some(d) = dict {
            self.history.clear();
            self.history.extend_from_slice(&d.content);
            self.history.extend_from_slice(plain);
            (&self.history[..], d.content.len())
        } else {
            (plain, 0)
        };
        parse(input, prefix, &mut self.table, &mut self.matches);
        if self.matches.is_empty() && !matches!(mode, Mode::Dense) {
            return (0, plain);
        }
        zone!("pack sequences");
        let packed = matches!(mode, Mode::Packed);
        let narrow = !packed && self.matches.iter().all(|m| m.off <= 255);
        let mask = if packed { 255 } else { 15 };
        let lit_count = plain.len() - self.matches.iter().map(|m| m.len).sum::<usize>();
        let out = &mut self.payload;
        out.clear();
        out.extend((self.matches.len() as u32).to_le_bytes());
        out.extend((lit_count as u32).to_le_bytes());
        out.extend([0, 0, 0, narrow as u8]);
        let mut last = 0;
        for m in &self.matches {
            out.extend_from_slice(&plain[last..m.at]);
            last = m.at + m.len;
        }
        out.extend_from_slice(&plain[last..]);
        let mut end_lit = out.len();
        last = 0;
        for m in &self.matches {
            let ll = (m.at - last).min(mask) as u8;
            let ml = (m.len - 5).min(mask) as u8;
            if packed {
                out.extend(m.off.to_le_bytes());
                out.extend([ml, ll]);
            } else {
                out.push(ll << 4 | ml);
            }
            last = m.at + m.len;
        }
        if !packed {
            for m in &self.matches {
                if narrow {
                    out.push(m.off as u8);
                } else {
                    out.extend(m.off.to_le_bytes());
                }
            }
        }
        last = 0;
        for m in &self.matches {
            if m.at - last >= mask {
                varint(out, m.at - last - mask);
            }
            if m.len - 5 >= mask {
                varint(out, m.len - 5 - mask);
            }
            last = m.at + m.len;
        }
        if matches!(mode, Mode::Dense) {
            let ns = self.matches.len();
            let (le, lit) = crate::entropy::best(&out[12..end_lit], true, dict);
            let (te, tok) = crate::entropy::best(&out[end_lit..end_lit + ns], false, None);
            if le != 0 || te != 0 {
                let dense = &mut self.dense;
                dense.clear();
                dense.extend_from_slice(&out[..12]);
                dense[8] = le;
                dense[9] = te;
                if le != 0 {
                    dense.extend((lit.len() as u32).to_le_bytes());
                }
                if te != 0 {
                    dense.extend((tok.len() as u32).to_le_bytes());
                }
                dense.extend_from_slice(&lit);
                let new_end = dense.len();
                dense.extend_from_slice(&tok);
                dense.extend_from_slice(&out[end_lit + ns..]);
                end_lit = new_end;
                std::mem::swap(dense, out);
            }
        }
        if out.len() < end_lit + 32 {
            out.resize(end_lit + 32, 0);
        }
        if out.len() >= plain.len() {
            (0, plain)
        } else {
            (if packed { 2 } else { 1 }, out)
        }
    }
}
