//! A group of eight fixed-width symbols occupies exactly `WIDTH` bytes.
//! Dispatch once per subtree, so group loads have no per-symbol lookahead
//! or variable-width shifts. The last short group need not have wire padding.

#[cfg(all(feature = "merge-simd", any(target_arch = "x86", target_arch = "x86_64")))]
fn three_groups(simd: fearless_simd::Sse4_2, dst: &mut [[u8; 16]], run: &[[u8; 6]], table: &[u8; 8]) {
    use fearless_simd::{Bytes, Simd, SimdBase, u8x16, u16x8};

    simd.vectorize(|| {
        let mut symbols = [0; 16];
        symbols[..8].copy_from_slice(table);
        let symbols = u8x16::from_slice(simd, &symbols);
        // Each u16 window contains two codes, starting at bit 0, 6, 4 or 2.
        // Multiplication shifts its six useful bits to the top of the lane.
        let windows = u8x16::from_slice(simd, &[0, 1, 0, 1, 1, 2, 2, 3, 3, 4, 3, 4, 4, 5, 5, 6]);
        let align = u16x8::from_slice(simd, &[1024, 16, 64, 256, 1024, 16, 64, 256]);
        for (out, bytes) in dst.iter_mut().zip(run) {
            let mut src = [0; 16];
            src[..6].copy_from_slice(bytes);
            let words: u16x8<_> = simd.swizzle_dyn_u8x16(u8x16::from_slice(simd, &src), windows).bitcast();
            let pairs = (words * align) >> 10;
            // Low code stays in bits 0..2; move the high code to bits 8..10.
            let indices = (pairs | (pairs << 5)) & 0x0707;
            out.copy_from_slice(simd.swizzle_dyn_u8x16(symbols, indices.bitcast()).as_slice());
        }
    });
}

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
    fn three(dst: &mut [u8], run: &[u8], table: &[u8; 8]) {
        #[cfg(all(feature = "merge-simd", any(target_arch = "x86", target_arch = "x86_64")))]
        if dst.len() >= 16
            && let Some(simd) = fearless_simd::Level::new().as_sse4_2()
        {
            let (full, tail) = dst.as_chunks_mut::<16>();
            let (whole, rest) = run.split_at(full.len() * 6);
            three_groups(simd, full, whole.as_chunks::<6>().0, table);
            return groups::<3, 8>(tail, rest, table);
        }
        groups::<3, 8>(dst, run, table);
    }
    macro_rules! widths {
        ($($w:literal),*) => { match lookup.len() {
            8 => three(dst, run, lookup.try_into().unwrap()),
            $(n if n == 1 << $w => groups::<$w, {1 << $w}>(dst, run, lookup.try_into().unwrap()),)*
            _ => unreachable!("a complete byte-alphabet subtree has 2..=256 leaves"),
        } };
    }
    widths!(1, 2, 4, 5, 6, 7, 8);
}
