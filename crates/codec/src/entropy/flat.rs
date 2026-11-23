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

#[cfg(all(
    feature = "merge-simd",
    any(
        target_arch = "x86",
        target_arch = "x86_64",
        all(target_arch = "aarch64", target_endian = "little")
    )
))]
#[inline(always)]
fn six_indices<S: fearless_simd::Simd>(simd: S, bytes: &[u8; 24]) -> fearless_simd::u8x32<S> {
    use fearless_simd::{Bytes, SimdBase, u8x16, u8x32, u16x16};

    let load = |bytes| u8x16::from_slice(simd, bytes);
    let raw = simd.combine_u8x16(load(&bytes[..16]), load(&bytes[8..]));
    // Each lane expands four triples abc into windows ab, bc. The upper
    // lane starts at byte 12, four bytes into the second 16-byte load.
    let windows = u8x32::from_slice(
        simd,
        &[
            0, 1, 1, 2, 3, 4, 4, 5, 6, 7, 7, 8, 9, 10, 10, 11, 4, 5, 5, 6, 7, 8, 8, 9, 10, 11, 11, 12, 13, 14, 14, 15,
        ],
    );
    let words: u16x16<_> = simd.swizzle_dyn_within_blocks_u8x32(raw, windows).bitcast();
    // One 12-bit pair starts at bit 0 of ab, the next at bit 4 of bc.
    // Move the second code from bit 6 to bit 8 of each word.
    let odd_words = u16x16::from_slice(simd, &[0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]).simd_ne(0);
    let pairs = simd.select_u16x16(odd_words, words >> 4, words);
    ((pairs & 0x003f) | ((pairs << 2) & 0x3f00)).bitcast()
}

#[cfg(all(feature = "merge-simd", any(target_arch = "x86", target_arch = "x86_64")))]
fn six_groups(simd: fearless_simd::Avx2, dst: &mut [[u8; 32]], run: &[[u8; 24]], table: &[u8; 64]) {
    use fearless_simd::{Bytes, Simd, SimdBase, i8x32, u8x16, u16x16};

    simd.vectorize(|| {
        let load = |bytes| u8x16::from_slice(simd, bytes);
        let broadcast = |bytes| simd.combine_u8x16(load(bytes), load(bytes));
        let t0 = broadcast(&table[..16]);
        let t1 = broadcast(&table[16..32]);
        let t2 = broadcast(&table[32..48]);
        let t3 = broadcast(&table[48..]);

        for (out, bytes) in dst.iter_mut().zip(run) {
            let indices = six_indices(simd, bytes);
            // Each lane-local shuffle indexes a 16-entry table with the low
            // nibble. Move bits 4 and 5 to the byte sign bits for the blends.
            let bit4: i8x32<_> = (indices.bitcast::<u16x16<_>>() << 3).bitcast();
            let bit5: i8x32<_> = (indices.bitcast::<u16x16<_>>() << 2).bitcast();
            let swizzle = |t| simd.swizzle_dyn_within_blocks_u8x32(t, indices);
            let upper_chunk = bit4.simd_lt(0);
            let lo = simd.select_u8x32(upper_chunk, swizzle(t1), swizzle(t0));
            let hi = simd.select_u8x32(upper_chunk, swizzle(t3), swizzle(t2));
            out.copy_from_slice(simd.select_u8x32(bit5.simd_lt(0), hi, lo).as_slice());
        }
    });
}

#[cfg(all(feature = "merge-simd", target_arch = "aarch64", target_endian = "little"))]
pub(super) fn six_groups(simd: fearless_simd::Neon, dst: &mut [[u8; 32]], run: &[[u8; 24]], table: &[u8; 64]) {
    use fearless_simd::{Simd, SimdBase, u8x32};

    simd.vectorize(|| {
        let lo = u8x32::from_slice(simd, &table[..32]);
        let hi = u8x32::from_slice(simd, &table[32..]);
        for (out, bytes) in dst.iter_mut().zip(run) {
            // Indices are 0..=63: a four-register TBL per output half.
            let indices = six_indices(simd, bytes);
            out.copy_from_slice(simd.concat_swizzle_dyn_u8x32(lo, hi, indices).as_slice());
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
    fn six(dst: &mut [u8], run: &[u8], table: &[u8; 64]) {
        #[cfg(all(
            feature = "merge-simd",
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                all(target_arch = "aarch64", target_endian = "little")
            )
        ))]
        if dst.len() >= 32 {
            let level = fearless_simd::Level::new();
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            let simd = level.as_avx2();
            #[cfg(target_arch = "aarch64")]
            let simd = level.as_neon();
            if let Some(simd) = simd {
                let (full, tail) = dst.as_chunks_mut::<32>();
                let (whole, rest) = run.split_at(full.len() * 24);
                six_groups(simd, full, whole.as_chunks::<24>().0, table);
                return groups::<6, 64>(tail, rest, table);
            }
        }
        groups::<6, 64>(dst, run, table);
    }
    macro_rules! widths {
        ($($w:literal),*) => { match lookup.len() {
            8 => three(dst, run, lookup.try_into().unwrap()),
            64 => six(dst, run, lookup.try_into().unwrap()),
            $(n if n == 1 << $w => groups::<$w, {1 << $w}>(dst, run, lookup.try_into().unwrap()),)*
            _ => unreachable!("a complete byte-alphabet subtree has 2..=256 leaves"),
        } };
    }
    widths!(1, 2, 4, 5, 7, 8);
}
