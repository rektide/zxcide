//! Entropy candidates for the density-oriented encoder. Price the actual
//! byte-aligned runs (including descriptor/table costs) before emitting bits.
use super::{MAX_CODE_LEN, Tree};
use crate::Dictionary;
use std::{
    borrow::Cow,
    cmp::Reverse,
    collections::{BinaryHeap, VecDeque},
};

fn lengths(freq: &[usize; 256]) -> [u8; 128] {
    let mut heap = BinaryHeap::new();
    for (i, &count) in freq.iter().enumerate() {
        if count != 0 {
            heap.push(Reverse((count, i)));
        }
    }
    let mut parents = [None; 511];
    let mut next = 256;
    while heap.len() > 1 {
        let Reverse((ca, a)) = heap.pop().unwrap();
        let Reverse((cb, b)) = heap.pop().unwrap();
        parents[a] = Some(next);
        parents[b] = Some(next);
        heap.push(Reverse((ca + cb, next)));
        next += 1;
    }
    let mut table = [0; 128];
    for (s, &count) in freq.iter().enumerate() {
        if count == 0 {
            continue;
        }
        let (mut i, mut d) = (s, 0u8);
        while let Some(p) = parents[i] {
            i = p;
            d += 1;
        }
        // Preserve the usual heap result when already representable.
        if d > MAX_CODE_LEN {
            return limited_lengths(freq);
        }
        table[s / 2] |= d.max(1) << (4 * (s % 2));
    }
    table
}

/// Package-merge, minimizing weighted code lengths subject to the wire cap.
/// We only get here with multiple leaves, at most 256 (well within 2^11).
fn limited_lengths(freq: &[usize; 256]) -> [u8; 128] {
    let mut leaves: Vec<_> = freq
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(sym, weight)| (weight > 0).then_some((weight, sym)))
        .collect();
    leaves.sort_unstable(); // ascending weight, ties by symbol
    let n = leaves.len();

    // Each level merges fresh leaves and adjacent pairs from the level below.
    // Keep only the cheapest 2*n-2 items: the final selection needs no more,
    // and expanding any selected packages cannot need a longer prefix below.
    const MAX_ITEMS: usize = 2 * 256 - 2;
    let take = 2 * n - 2;
    let mut weights = [[0; MAX_ITEMS]; 2];
    let [mut prev, mut next] = weights.each_mut();
    // For each prefix length, record how many items in it are leaves. The
    // rest are packages, whose children form another prefix one level down.
    let mut leaf_counts = [[0u16; MAX_ITEMS + 1]; MAX_CODE_LEN as usize - 1];
    for (w, &(weight, _)) in prev.iter_mut().zip(&leaves) {
        *w = weight;
    }
    let mut len = n;
    for counts in &mut leaf_counts {
        let packages = len / 2;
        let (mut leaf, mut pair) = (0, 0);
        len = (n + packages).min(take);
        for (w, count) in next[..len].iter_mut().zip(&mut counts[1..]) {
            let lw = leaves.get(leaf).map_or(usize::MAX, |x| x.0);
            let pw = if pair < packages {
                prev[2 * pair] + prev[2 * pair + 1]
            } else {
                usize::MAX
            };
            if leaf < n && lw <= pw {
                *w = lw;
                leaf += 1;
            } else {
                *w = pw;
                pair += 1;
            }
            *count = leaf as u16;
        }
        std::mem::swap(&mut prev, &mut next);
    }

    // Select the cheapest 2*n-2 items at the top level. Count each leaf's
    // appearances across those items and the packages they expand into.
    debug_assert_eq!(len, take);
    let mut table = [0; 128];
    let mut selected = take;
    for counts in leaf_counts.iter().rev() {
        let leaf = usize::from(counts[selected]);
        for &(_, sym) in &leaves[..leaf] {
            table[sym / 2] += 1 << (4 * (sym % 2));
        }
        selected = 2 * (selected - leaf);
    }
    for &(_, sym) in &leaves[..selected] {
        table[sym / 2] += 1 << (4 * (sym % 2));
    }
    table
}

impl Tree {
    pub(super) fn emit(&self, data: &[u8], freq: &[usize; 256], header: &[u8], limit: usize) -> Option<Vec<u8>> {
        // Only the terminal run can pack more than one bit per occurrence.
        // An alphabet of 256 has at most 255 internal nodes (one for the
        // single-symbol exception), so a wire run ID fits in one byte.
        #[derive(Clone, Copy, Default)]
        struct Prefix {
            runs: [u8; MAX_CODE_LEN as usize - 1],
            bits: u16,
            len: u8,
        }
        #[derive(Clone, Copy, Default)]
        struct Route {
            prefix: Prefix,
            last: u8,
            code: u8,
            width: u8, // zero for symbols absent from this table
        }
        let mut paths = [Route::default(); 256];
        let mut queue = VecDeque::from([(0, Prefix::default())]);
        let mut bits = [0usize; 256];
        let mut nrun = 0;
        while let Some((i, prefix)) = queue.pop_front() {
            let node = &self.0[i];
            let run = nrun as u8;
            if node.flat > 0 {
                for (code, &s) in node.lookup.iter().enumerate() {
                    paths[s as usize] = Route {
                        prefix,
                        last: run,
                        code: code as u8,
                        width: node.flat,
                    };
                }
            } else {
                for (side, child) in node.child.into_iter().enumerate() {
                    if let Some(c) = child {
                        if let Some(s) = self.0[c].symbol {
                            paths[s as usize] = Route {
                                prefix,
                                last: run,
                                code: side as u8,
                                width: 1,
                            };
                        } else {
                            let mut sub = prefix;
                            sub.runs[sub.len as usize] = run;
                            sub.bits |= (side as u16) << sub.len;
                            sub.len += 1;
                            queue.push_back((c, sub));
                        }
                    }
                }
            }
            nrun += 1;
        }
        for (path, &count) in paths.iter().zip(freq) {
            if count > 0 && path.width == 0 {
                return None;
            }
            for &run in &path.prefix.runs[..usize::from(path.prefix.len)] {
                bits[run as usize] += count;
            }
            bits[path.last as usize] += count * usize::from(path.width);
        }
        let mut size = header.len();
        for b in &mut bits[..nrun] {
            let length = (*b).div_ceil(8);
            *b = size * 8;
            size += length;
        }
        if size >= limit {
            return None;
        }
        // Emit directly behind the inline table (empty for dictionary runs).
        let mut packed = Vec::with_capacity(size + 1);
        packed.extend_from_slice(header);
        packed.resize(size + 1, 0);
        crate::probe!({
            add("copy:Huffman candidate", header.len());
            add("zero:Huffman bitstream", size + 1 - header.len());
        });
        for &s in data {
            let path = &paths[s as usize];
            let mut code = path.prefix.bits;
            for &run in &path.prefix.runs[..usize::from(path.prefix.len)] {
                let cur = &mut bits[run as usize];
                let bit = *cur;
                *cur += 1;
                packed[bit / 8] |= (code as u8 & 1) << (bit % 8);
                code >>= 1;
            }
            let cur = &mut bits[path.last as usize];
            let bit = *cur;
            *cur += usize::from(path.width);
            let v = u16::from(path.code) << (bit % 8);
            packed[bit / 8] |= v as u8;
            packed[bit / 8 + 1] |= (v >> 8) as u8;
        }
        packed.pop();
        Some(packed)
    }
}

fn encode_rle(src: &[u8]) -> Vec<u8> {
    fn run(s: &[u8]) -> bool {
        s.len() >= 4 && s[0] == s[1] && s[0] == s[2] && s[0] == s[3]
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let begin = i;
        if run(&src[i..]) {
            i += 4;
            while i < src.len() && src[i] == src[begin] && i - begin < 131 {
                i += 1;
            }
            out.extend([128 + (i - begin - 4) as u8, src[begin]]);
        } else {
            i += 1;
            while i < src.len() && i - begin < 128 && !run(&src[i..]) {
                i += 1;
            }
            out.push((i - begin - 1) as u8);
            out.extend_from_slice(&src[begin..i]);
            crate::probe!({ add("copy:RLE candidate literals", i - begin) });
        }
    }
    out
}

pub(crate) fn best<'a>(src: &'a [u8], literal: bool, dict: Option<&Dictionary>) -> (u8, Cow<'a, [u8]>) {
    zone!("price entropy");
    let mut best = (0, Cow::Borrowed(src));
    let mut budget = src.len().saturating_sub(4); // descriptor when not raw
    if budget == 0 {
        return best;
    }
    if literal {
        let rle = encode_rle(src);
        if rle.len() < budget {
            budget = rle.len();
            best = (1, Cow::Owned(rle));
        }
    }
    let mut freq = [0; 256];
    for &b in src {
        freq[usize::from(b)] += 1;
    }
    if let Some(tree) = dict.and_then(|d| d.tree.as_ref())
        && let Some(bytes) = tree.emit(src, &freq, &[], budget)
    {
        budget = bytes.len();
        best = (3, Cow::Owned(bytes));
    }
    if budget > 128 {
        let table = lengths(&freq);
        let tree = Tree::new(&table).expect("generated a complete Huffman tree");
        if let Some(bytes) = tree.emit(src, &freq, &table, budget) {
            best = (2, Cow::Owned(bytes));
        }
    }
    best
}
