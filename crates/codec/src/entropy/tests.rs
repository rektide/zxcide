use super::*;

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
