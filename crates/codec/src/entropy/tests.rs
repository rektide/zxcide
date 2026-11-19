use super::*;

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
