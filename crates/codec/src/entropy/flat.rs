//! A group of eight fixed-width symbols occupies exactly `WIDTH` bytes.
//! Dispatch once per subtree, so group loads have no per-symbol lookahead
//! or variable-width shifts. The last short group need not have wire padding.

pub(super) fn unpack(dst: &mut [u8], run: &[u8], lookup: &[u8]) {
    fn groups<const WIDTH: usize, const SYMBOLS: usize>(dst: &mut [u8], run: &[u8], table: &[u8; SYMBOLS]) {
        let (groups, tail) = dst.as_chunks_mut::<8>();
        let (whole, rest) = run.split_at(groups.len() * WIDTH);
        for (group, bytes) in groups.iter_mut().zip(whole.as_chunks::<WIDTH>().0) {
            let mut word = [0; 8];
            word[..WIDTH].copy_from_slice(bytes);
            let bits = u64::from_le_bytes(word);
            for (i, out) in group.iter_mut().enumerate() {
                *out = table[(bits >> (WIDTH * i)) as usize & (SYMBOLS - 1)];
            }
        }
        let mut word = [0; 8];
        word[..rest.len()].copy_from_slice(rest);
        let bits = u64::from_le_bytes(word);
        for (i, out) in tail.iter_mut().enumerate() {
            *out = table[(bits >> (WIDTH * i)) as usize & (SYMBOLS - 1)];
        }
    }
    macro_rules! widths {
        ($($w:literal),*) => { match lookup.len() {
            $(n if n == 1 << $w => groups::<$w, {1 << $w}>(dst, run, lookup.try_into().unwrap()),)*
            _ => unreachable!("a complete byte-alphabet subtree has 2..=256 leaves"),
        } };
    }
    widths!(1, 2, 3, 4, 5, 6, 7, 8);
}
