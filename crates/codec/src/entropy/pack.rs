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
    fn emit(&self, data: &[u8], freq: &[usize; 256], limit: usize) -> Option<Vec<u8>> {
        // For each symbol build its sequence of (wire run, bits, width).
        let mut paths: [Vec<(usize, u8, u8)>; 256] = std::array::from_fn(|_| Vec::new());
        let mut queue = VecDeque::from([(0, Vec::new())]);
        let mut nrun = 0;
        while let Some((i, path)) = queue.pop_front() {
            let node = &self.0[i];
            if let Some(s) = node.symbol {
                paths[s as usize] = path;
                continue;
            }
            if node.flat > 0 {
                for (code, &s) in node.lookup.iter().enumerate() {
                    paths[s as usize] = path.clone();
                    paths[s as usize].push((nrun, code as u8, node.flat));
                }
            } else {
                for (side, child) in node.child.iter().enumerate() {
                    if let Some(c) = child {
                        let mut sub = path.clone();
                        sub.push((nrun, side as u8, 1));
                        queue.push_back((*c, sub));
                    }
                }
            }
            nrun += 1;
        }
        let mut bits = vec![0; nrun];
        for (path, &count) in paths.iter().zip(freq) {
            if count > 0 && path.is_empty() {
                return None;
            }
            for &(run, _, width) in path {
                bits[run] += count * usize::from(width);
            }
        }
        let mut size = 0;
        for b in &mut bits {
            let length = (*b).div_ceil(8);
            *b = size * 8;
            size += length;
        }
        if size >= limit {
            return None;
        }
        let mut packed = vec![0; size + 1];
        crate::probe!({ add("zero:Huffman bitstream", size + 1) });
        for &s in data {
            for &(run, code, width) in &paths[s as usize] {
                let bit = bits[run];
                let v = u16::from(code) << (bit % 8);
                packed[bit / 8] |= v as u8;
                packed[bit / 8 + 1] |= (v >> 8) as u8;
                bits[run] += usize::from(width);
            }
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
        && let Some(bytes) = tree.emit(src, &freq, budget)
    {
        budget = bytes.len();
        best = (3, Cow::Owned(bytes));
    }
    if budget > 128 {
        let table = lengths(&freq);
        let tree = Tree::new(&table).expect("generated a complete Huffman tree");
        if let Some(bits) = tree.emit(src, &freq, budget - 128) {
            let mut buf = table.to_vec();
            crate::probe!({ add("copy:Huffman candidate", table.len() + bits.len()) });
            buf.extend(bits);
            best = (2, Cow::Owned(buf));
        }
    }
    best
}
