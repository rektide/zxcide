use super::*;

#[test]
fn packed_subtrees_decode_all_widths_and_tail_lengths() {
    for width in 1..=8 {
        let symbols = 1 << width;
        let mut lengths = [0; 128];
        lengths[..symbols / 2].fill((width << 4) | width);
        let tree = Tree::new(&lengths).unwrap();
        // Complete canonical trees use bit-reversed symbol IDs on the wire.
        // Cover every code and every possible final partial group. Start
        // with set padding so a read of unused bits is observable.
        let source: Vec<_> = (0..symbols + 8).map(|i| (i * 109 % symbols) as u8).collect();
        for n in symbols..symbols + 8 {
            let mut run = vec![0xff; (n * usize::from(width)).div_ceil(8)];
            for (i, &s) in source[..n].iter().enumerate() {
                for b in 0..width {
                    let bit = i * usize::from(width) + usize::from(b);
                    if s >> (width - b - 1) & 1 == 0 {
                        run[bit / 8] &= !(1 << (bit % 8));
                    }
                }
            }
            assert_eq!(tree.decode(&run, n).unwrap(), source[..n]);
            assert!(tree.decode(&run[..run.len() - 1], n).is_err());
        }
    }
}

#[test]
fn emit_prices_and_packs_mixed_branch_and_flat_runs() {
    // Symbol 0 goes left. Symbols 1..=8 share the three-bit flat run on
    // the right. Its 3rd and 6th codes straddle a byte boundary.
    let mut table = [0; 128];
    table[..5].copy_from_slice(&[0x41, 0x44, 0x44, 0x44, 0x04]);
    let tree = Tree::new(&table).unwrap();
    let src = [0, 2, 8, 5, 0, 1, 3, 6, 4, 7];
    let mut freq = [0; 256];
    for &s in &src {
        freq[s as usize] += 1;
    }
    for header in [&[][..], &table[..]] {
        let expected = [header, &[0xee, 0x03, 0x7c, 0xa0, 0x7a]].concat();
        assert_eq!(tree.emit(&src, &freq, header, expected.len()), None);
        assert_eq!(tree.emit(&src, &freq, header, expected.len() + 1), Some(expected));
    }
    freq[9] = 1;
    assert_eq!(tree.emit(&[9], &freq, &[], usize::MAX), None);
}

#[test]
fn skewed_histogram_uses_length_limited_huffman_for_either_section() {
    // The heap tree for Fibonacci weights reaches 15 bits. Reorder the
    // 2583 bytes with a coprime stride so RLE does not hide the regression.
    let weights = [1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610, 987];
    let grouped: Vec<_> = weights
        .iter()
        .enumerate()
        .flat_map(|(sym, &n)| std::iter::repeat_n(sym as u8, n))
        .collect();
    let src: Vec<_> = (0..grouped.len()).map(|i| grouped[i * 1009 % grouped.len()]).collect();
    for literal in [false, true] {
        let (enc, bytes) = best(&src, literal, None);
        assert_eq!(enc, 2);
        let (table, bits) = bytes.split_at(128);
        let tree = Tree::new(table).unwrap(); // wire depth + complete Kraft sum
        let depth = |sym: usize| table[sym / 2] >> (4 * (sym % 2)) & 15;
        assert!((0..weights.len()).all(|sym| depth(sym) != 0));
        assert!((weights.len()..256).all(|sym| depth(sym) == 0));
        // Optimal 11-bit weighted cost (computed by enumerating leaf counts
        // per depth in a full tree, assigning heavier leaves first).
        assert_eq!(
            weights
                .iter()
                .enumerate()
                .map(|(s, n)| n * usize::from(depth(s)))
                .sum::<usize>(),
            6749
        );
        assert_eq!(tree.decode(bits, src.len()).unwrap(), src);
    }
}

#[test]
fn entropy_still_falls_back_when_wire_cost_does_not_win() {
    let flat: Vec<_> = (0..=255).cycle().take(768).collect();
    for literal in [false, true] {
        let (enc, bytes) = best(&flat, literal, None);
        assert_eq!(enc, 0);
        assert_eq!(&*bytes, flat);
    }
    assert_eq!(best(&[1; 12], true, None).0, 1); // RLE beats raw; no room for a table
}

#[test]
fn bitmap_kernels_match_the_cursor_at_exact_child_and_output_ends() {
    for first in 0..=255u8 {
        // Balanced padding on each side makes every byte value enter the
        // full-group path; no-padding and skewed variants exercise fallback.
        for middle in [&[][..], &[0x55, 0xaa, 0x55, 0xaa], &[0, 0, 0, 1], &[255, 255, 254, 255]] {
            for size in 0..=8 {
                let mut bits = vec![first];
                bits.extend_from_slice(middle);
                bits.push(!first);
                let n = if middle.is_empty() {
                    size
                } else {
                    8 * (1 + middle.len()) + size
                };
                bits.truncate(n.div_ceil(8));
                let left = (0..n).filter(|&j| bits[j / 8] >> (j % 8) & 1 == 0).count();
                let region: Box<[u8]> = (0..n).map(|j| (j as u8).wrapping_mul(19)).collect();
                let mut expected = vec![0; n];
                merge::scalar(&region, &mut expected, &bits, left);
                let mut actual: Box<[u8]> = vec![0; n].into_boxed_slice();
                merge::table(&region, &mut actual, &bits, left);
                assert_eq!(*actual, expected, "table: {first:#04x}, n={n}, left={left}");
                #[cfg(feature = "merge-simd")]
                {
                    actual.fill(0);
                    merge::simd(&region, &mut actual, &bits, left);
                    assert_eq!(*actual, expected, "simd: {first:#04x}, n={n}, left={left}");
                }
                actual.fill(0);
                merge::gather(&region, &mut actual, &bits, left);
                assert_eq!(*actual, expected, "selected: {first:#04x}, n={n}, left={left}");
            }
        }
    }
}

#[test]
fn mixed_merges_under_uniform_ancestor_preserve_bitmap_tails() {
    // Canonical codes A=0 B=10 C=110 D=111. All root selections go
    // right; its child and grandchild both actually merge. Each bitmap
    // has a partial byte, with deliberately set unused high bits.
    let mut table = [0; 128];
    table[b'A' as usize / 2] = 1 << 4;
    table[b'B' as usize / 2] = 2 | (3 << 4);
    table[b'D' as usize / 2] = 3;
    let tree = Tree::new(&table).unwrap();
    // B C D B D -> root 11111, right child 01101, grandchild 011.
    let wire = [0xff, 0xf6, 0xfe];
    assert_eq!(tree.decode(&wire, 5).unwrap(), b"BCDBD");
    for n in 0..wire.len() {
        assert!(tree.decode(&wire[..n], 5).is_err());
    }
    assert!(tree.decode(&[0xff, 0xf6, 0xfe, 0], 5).is_err());
}

#[test]
fn merge_with_one_side_empty_accepts_only_the_existing_branch() {
    let mut table = [0; 128];
    table[0] = 1;
    let tree = Tree::new(&table).unwrap();
    assert_eq!(tree.decode(&[0, 0xfe], 9).unwrap(), [0; 9]);
    assert!(tree.decode(&[0, 1], 9).is_err());
    assert!(tree.decode(&[0], 9).is_err());
    assert!(tree.decode(&[0], 0).is_err());
    assert!(tree.decode(&[], 0).unwrap().is_empty());
}
