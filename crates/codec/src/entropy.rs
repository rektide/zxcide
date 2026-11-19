//! Scalar PivCo: discover breadth-first wire runs, then merge sibling
//! symbol lists bottom-up. Each subtree owns a disjoint output interval.
use crate::{require, take};
use std::{collections::VecDeque, io};
mod pack;
pub(crate) use pack::best;

#[derive(Default, Debug)]
struct Node {
    child: [Option<usize>; 2],
    symbol: Option<u8>,
    flat: u8,
    lookup: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct Tree(Vec<Node>);

impl Tree {
    pub fn new(table: &[u8]) -> io::Result<Self> {
        require(table.len() == 128, "invalid Huffman table size")?;
        let mut symbols = Vec::new();
        let mut kraft = 0;
        for i in 0..256 {
            let depth = table[i / 2] >> (4 * (i % 2)) & 15;
            require(depth <= 11, "Huffman length exceeds 11")?;
            if depth > 0 {
                symbols.push((depth, i as u8));
                kraft += 1 << (11 - depth);
            }
        }
        require(
            kraft == 2048 || (symbols.len() == 1 && symbols[0].0 == 1),
            "incomplete Huffman tree",
        )?;
        symbols.sort_unstable();
        let mut nodes = vec![Node::default()];
        let (mut code, mut prev) = (0u16, 0);
        for (depth, sym) in symbols {
            code <<= depth - prev;
            let mut pos = 0;
            for bit in (0..depth).rev() {
                let side = usize::from((code >> bit) & 1);
                pos = match nodes[pos].child[side] {
                    Some(child) => child,
                    None => {
                        let next = nodes.len();
                        nodes[pos].child[side] = Some(next);
                        nodes.push(Node::default());
                        next
                    }
                };
            }
            nodes[pos].symbol = Some(sym);
            code += 1;
            prev = depth;
        }
        // A perfect subtree of height >=2 has one packed run, not one
        // bitmap per internal node. Packed codes take the root bit first.
        let mut height = vec![None; nodes.len()];
        for i in (0..nodes.len()).rev() {
            if nodes[i].symbol.is_some() {
                height[i] = Some(0);
            } else if let [Some(l), Some(r)] = nodes[i].child
                && height[l] == height[r]
            {
                height[i] = height[l].map(|x| x + 1);
            }
        }
        let mut queue = VecDeque::from([0]);
        while let Some(i) = queue.pop_front() {
            match height[i] {
                Some(d) if d >= 2 => {
                    nodes[i].flat = d;
                    for value in 0..1usize << d {
                        let mut n = i;
                        for j in 0..d {
                            n = nodes[n].child[(value >> j) & 1].unwrap();
                        }
                        let sym = nodes[n].symbol.unwrap();
                        nodes[i].lookup.push(sym);
                    }
                }
                _ => queue.extend(nodes[i].child.into_iter().flatten()),
            }
        }
        Ok(Self(nodes))
    }

    pub fn decode(&self, mut src: &[u8], count: usize) -> io::Result<Vec<u8>> {
        zone!("PivCo decode");
        let mut order = Vec::with_capacity(self.0.len());
        let mut queue = VecDeque::from([(0, 0, count)]);
        let mut output = vec![0; count];
        while let Some((i, start, n)) = queue.pop_front() {
            if n == 0 {
                continue;
            }
            let node = &self.0[i];
            if let Some(sym) = node.symbol {
                output[start..start + n].fill(sym);
                continue;
            }
            let run = take(&mut src, (n * usize::from(node.flat.max(1))).div_ceil(8))?;
            if node.flat == 0 {
                let whole = n / 8;
                let ones = run[..whole].iter().map(|b| b.count_ones() as usize).sum::<usize>()
                    + if n % 8 == 0 {
                        0
                    } else {
                        (run[whole] & ((1 << (n % 8)) - 1)).count_ones() as usize
                    };
                order.push((start, n, ones, run));
                for (child, (base, cnt)) in node
                    .child
                    .into_iter()
                    .zip([(start, n - ones), (start + n - ones, ones)])
                {
                    if let Some(c) = child {
                        queue.push_back((c, base, cnt));
                    } else {
                        require(cnt == 0, "Huffman run selects missing child")?;
                    }
                }
            } else {
                for (k, dst) in output[start..start + n].iter_mut().enumerate() {
                    let bit = k * usize::from(node.flat);
                    let byte = bit / 8;
                    let pair = u16::from(run[byte]) | (u16::from(*run.get(byte + 1).unwrap_or(&0)) << 8);
                    *dst = node.lookup[usize::from((pair >> (bit % 8)) & ((1 << node.flat) - 1))];
                }
            }
        }
        require(src.is_empty(), "trailing Huffman data")?;
        let mut scratch = vec![0; count];
        for (start, n, ones, run) in order.into_iter().rev() {
            if ones == 0 || ones == n {
                continue;
            }
            let region = &mut output[start..start + n];
            let mut cursor = [0, n - ones];
            for (chunk, &bits) in scratch[..n].chunks_mut(8).zip(run) {
                for (j, dst) in chunk.iter_mut().enumerate() {
                    let side = usize::from((bits >> j) & 1);
                    *dst = region[cursor[side]];
                    cursor[side] += 1;
                }
            }
            region.copy_from_slice(&scratch[..n]);
        }
        Ok(output)
    }
}

pub fn rle(mut data: &[u8], count: usize) -> io::Result<Vec<u8>> {
    zone!("RLE decode");
    let mut out = Vec::with_capacity(count);
    while !data.is_empty() {
        let t = take(&mut data, 1)?[0];
        let n = usize::from(t & 127) + if t >= 128 { 4 } else { 1 };
        require(n <= count - out.len(), "RLE output too long")?;
        if t >= 128 {
            out.resize(out.len() + n, take(&mut data, 1)?[0]);
        } else {
            out.extend_from_slice(take(&mut data, n)?);
        }
    }
    require(out.len() == count, "short RLE output")?;
    Ok(out)
}
