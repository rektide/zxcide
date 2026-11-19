//! Scalar PivCo decoder. Discover the breadth-first wire runs once, then
//! consume them in symbol order; each node has its own bit cursor.
use crate::{bad, require, take};
use std::{collections::VecDeque, io};

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
        require(kraft == 2048 || (symbols.len() == 1 && symbols[0].0 == 1), "incomplete Huffman tree")?;
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
        let mut runs = vec![(&[][..], 0usize); self.0.len()];
        let mut queue = VecDeque::from([(0, count)]);
        while let Some((i, n)) = queue.pop_front() {
            let node = &self.0[i];
            if node.symbol.is_some() {
                continue;
            }
            let run = take(&mut src, (n * usize::from(node.flat.max(1))).div_ceil(8))?;
            runs[i].0 = run;
            if node.flat == 0 {
                let whole = n / 8;
                let ones = run[..whole].iter().map(|b| b.count_ones() as usize).sum::<usize>()
                    + if n % 8 == 0 {
                        0
                    } else {
                        (run[whole] & ((1 << (n % 8)) - 1)).count_ones() as usize
                    };
                for (child, cnt) in node.child.into_iter().zip([n - ones, ones]) {
                    if let Some(c) = child {
                        queue.push_back((c, cnt));
                    } else {
                        require(cnt == 0, "Huffman run selects missing child")?;
                    }
                }
            }
        }
        require(src.is_empty(), "trailing Huffman data")?;
        let mut output = Vec::with_capacity(count);
        for _ in 0..count {
            let mut i = 0;
            loop {
                let node = &self.0[i];
                if let Some(symbol) = node.symbol {
                    output.push(symbol);
                    break;
                }
                let (run, bit) = &mut runs[i];
                let shift = *bit % 8;
                let byte = *bit / 8;
                if node.flat > 0 {
                    let pair = u16::from(run[byte]) | (u16::from(*run.get(byte + 1).unwrap_or(&0)) << 8);
                    let value = (pair >> shift) & ((1 << node.flat) - 1);
                    output.push(node.lookup[usize::from(value)]);
                    *bit += usize::from(node.flat);
                    break;
                }
                i = node.child[usize::from((run[byte] >> shift) & 1)].ok_or_else(|| bad("invalid branch"))?;
                *bit += 1;
            }
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
