//! Bounded hash-chain search and approximate bit-priced parsing for Dense.

use super::Match;
use std::ops::Range;

const HASH_BITS: usize = 17;
const WINDOW: usize = 1 << 16;
const SEARCH_DEPTH: usize = 24;
const SPAN: usize = 32 * 1024;
const MIN_MATCH: usize = 5;
const LONG_MATCH: usize = 128;
const LONG_MATCH_TAIL: usize = 16;

// Approximate entropy costs. Charge two bytes per SPL offset (the final
// writer can still choose narrow offsets), plus an estimated token price.
const LITERAL_BITS: u32 = 7;
const SEQUENCE_BITS: u32 = 16 + 5;

/// Extra per-worker scratch is about 896 KiB at capacity: hash heads, a
/// 64K-entry ring of predecessor deltas, and states for a 32 KiB parse span.
#[derive(Default)]
pub(super) struct Parser {
    heads: Vec<u32>,
    links: Vec<u16>,
    states: Vec<State>,
}

#[derive(Clone, Copy)]
struct State {
    bits: u32,
    len: u16, // incoming match length (bounded by SPAN), or zero for a literal
    off: u16,
}

fn word(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(data[at..at + 4].try_into().unwrap())
}

fn extend(data: &[u8], mut at: usize, mut prev: usize) -> usize {
    let start = at;
    while at + 8 <= data.len() {
        let a = u64::from_le_bytes(data[at..at + 8].try_into().unwrap());
        let b = u64::from_le_bytes(data[prev..prev + 8].try_into().unwrap());
        let diff = a ^ b;
        if diff != 0 {
            return at - start + diff.trailing_zeros() as usize / 8;
        }
        at += 8;
        prev += 8;
    }
    while at < data.len() && data[at] == data[prev] {
        at += 1;
        prev += 1;
    }
    at - start
}

impl Parser {
    fn insert(&mut self, at: usize, word: u32, fifth: u8) -> usize {
        let v = u64::from(word) | (u64::from(fifth) << 32);
        let key = (v.wrapping_mul(0x9e3779b185ebca87) >> (64 - HASH_BITS)) as usize;
        let prev = std::mem::replace(&mut self.heads[key], at as u32) as usize;
        // No link when the previous head is absent or will be out of reach
        // from any future position. An exact-window head is still returned.
        self.links[at & (WINDOW - 1)] = u16::try_from(at.saturating_sub(prev)).unwrap_or(0);
        prev
    }

    fn index(&mut self, data: &[u8], range: Range<usize>) {
        for at in range.start..range.end.min(data.len().saturating_sub(MIN_MATCH - 1)) {
            self.insert(at, word(data, at), data[at + 4]);
        }
    }

    fn longest(&self, data: &[u8], at: usize, word_at: u32, mut prev: usize) -> (usize, u16) {
        let mut best = MIN_MATCH - 1;
        let mut off = 0;
        for _ in 0..SEARCH_DEPTH {
            if prev >= at || at - prev > WINDOW {
                break;
            }
            // A longer candidate must cover the first mismatch after `best`.
            if data[at + best] == data[prev + best] && word_at == word(data, prev) && data[at + 4] == data[prev + 4] {
                let len = MIN_MATCH + extend(data, at + MIN_MATCH, prev + MIN_MATCH);
                if len > best {
                    best = len;
                    off = (at - prev - 1) as u16;
                    if best >= LONG_MATCH || at + best == data.len() {
                        break;
                    }
                }
            }
            if at - prev == WINDOW {
                // The next link would be outside the window, and the ring
                // slot for `prev` was overwritten by the current insertion.
                break;
            }
            let delta = usize::from(self.links[prev & (WINDOW - 1)]);
            if delta == 0 {
                break;
            }
            prev -= delta;
        }
        (best, off)
    }

    pub(super) fn parse(&mut self, data: &[u8], prefix: usize, matches: &mut Vec<Match>) {
        zone!("Dense match finder");
        matches.clear();
        let n = data.len() - prefix;
        if n < MIN_MATCH {
            return;
        }
        self.heads.clear();
        self.heads.resize(1 << HASH_BITS, u32::MAX);
        self.links.resize(WINDOW, 0);
        // History is [dict | block]. Include dictionary keys that straddle
        // the join: an overlapping match can keep copying from the output.
        self.index(data, 0..prefix);

        let mut start = 0;
        while start < n {
            let end = (start + SPAN).min(n);
            let ll = matches.last().map_or(start, |m| start - (m.at + m.len));
            self.parse_span(data, prefix + start, prefix + end, ll);

            let prior_matches = matches.len();
            let mut pos = end - start;
            while pos != 0 {
                let prev = self.states[pos];
                if prev.len == 0 {
                    pos -= 1;
                } else {
                    let len = usize::from(prev.len);
                    pos -= len;
                    matches.push(Match {
                        at: start + pos,
                        len,
                        off: prev.off,
                    });
                }
            }
            matches[prior_matches..].reverse();

            start = end;
            if let Some(last) = matches.last_mut().filter(|m| m.at + m.len == end) {
                // Don't force a sequence break at the scratch boundary if
                // the chosen final match can continue at the same offset.
                let at = prefix + end;
                let more = extend(data, at, at - (usize::from(last.off) + 1));
                last.len += more;
                start += more;
                self.index(data, at..at + more);
            }
        }
    }

    fn parse_span(&mut self, data: &[u8], start: usize, end: usize, mut ll: usize) {
        let n = end - start;
        self.states.clear();
        self.states.reserve_exact(n + 1);
        self.states.resize(
            n + 1,
            State {
                bits: u32::MAX,
                len: 0,
                off: 0,
            },
        );
        self.states[0].bits = 0;
        let mut skip_until = 0;
        for pos in 0..n {
            let at = start + pos;
            let current = self.states[pos];
            if pos != 0 {
                // A literal state can only have come from the previous byte.
                ll = if current.len == 0 { ll + 1 } else { 0 };
            }
            let lit_bits = current.bits + LITERAL_BITS;
            if lit_bits < self.states[pos + 1].bits {
                self.states[pos + 1] = State {
                    bits: lit_bits,
                    len: 0,
                    off: 0,
                };
            }
            if data.len() - at < MIN_MATCH {
                continue;
            }
            let w = word(data, at);
            let prev = self.insert(at, w, data[at + 4]);
            if pos < skip_until || n - pos < MIN_MATCH {
                continue;
            }
            let (max_len, off) = self.longest(&data[..end], at, w, prev);
            if max_len < MIN_MATCH {
                continue;
            }

            // Keep the least estimated cost per endpoint. Allow shortening a
            // match to reach a better next match. Literal-run prices use only
            // the winning path, not every possible run length at this byte.
            let base = current.bits + SEQUENCE_BITS + overflow_bits(ll, 15);
            for len in MIN_MATCH..=max_len {
                let bits = base + overflow_bits(len, MIN_MATCH + 15);
                let next = &mut self.states[pos + len];
                if bits < next.bits {
                    *next = State {
                        bits,
                        len: len as u16,
                        off,
                    };
                }
            }
            if max_len >= LONG_MATCH {
                // The relaxed edges already cover the long match. Avoid
                // repeatedly extending across the same long run; leave the
                // last few positions eligible to find a better continuation.
                // Skipped positions still enter the history chains above.
                skip_until = pos + max_len - LONG_MATCH_TAIL;
            }
        }
    }
}

fn overflow_bits(len: usize, start: usize) -> u32 {
    if len < start {
        0
    } else if len < start + 128 {
        8
    } else if len < start + 16384 {
        16
    } else {
        24
    }
}
