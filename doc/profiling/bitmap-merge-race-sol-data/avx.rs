use super::{cursor, tiles32};
use core::arch::x86_64::{__m128i, _mm256_blendv_epi8, _mm256_castsi128_si256, _mm256_inserti128_si256,
    _mm256_shuffle_epi8, _mm256_slli_epi16, _mm_set_epi64x};
use fearless_simd::{SimdBase, SimdInto, u8x16, u8x32};

// Low nibble selects within one 16-byte lane; bit 4 selects the child.
// Row zero is the first eight selectors. Remaining rows start after eight
// outputs, indexed by the number of right bytes taken for those outputs.
static SELECTORS: [[u64; 256]; 10] = {
    let mut table = [[0; 256]; 10];
    let mut row = 0;
    while row < 10 {
        let mut bits = 0;
        while bits < 256 {
            let mut cursors = if row == 0 { [0, 16] } else { [9 - row, 15 + row] };
            let mut k = 0;
            while k < 8 {
                let side = (bits >> k) & 1;
                table[row][bits] |= (cursors[side] as u64) << (8 * k);
                cursors[side] += 1;
                k += 1;
            }
            bits += 1;
        }
        row += 1;
    }
    table
};

fearless_simd::kernel!(
    #[inline(always)]
    fn select32(simd: Avx2, a: &[u8; 32], b: &[u8; 32], bits: u32) -> [u8; 32] {
        let [b0, b1, b2, b3] = bits.to_le_bytes();
        let pa0 = b0.count_ones() as usize;
        let pb0 = b2.count_ones() as usize;
        let pa = (bits as u16).count_ones() as usize;
        let load = |slab: &[u8; 32], offset: usize| -> __m128i {
            u8x16::from_slice(simd, &slab[offset.min(16)..][..16]).into()
        };
        macro_rules! join {
            ($lo:expr, $hi:expr) => { _mm256_inserti128_si256::<1>(_mm256_castsi128_si256($lo), $hi) };
        }
        let l = join!(load(a, 0), load(a, 16 - pa));
        let r = join!(load(b, 0), load(b, pa));
        let ia = _mm_set_epi64x(SELECTORS[pa0 + 1][usize::from(b1)] as i64, SELECTORS[0][usize::from(b0)] as i64);
        let ib = _mm_set_epi64x(SELECTORS[pb0 + 1][usize::from(b3)] as i64, SELECTORS[0][usize::from(b2)] as i64);
        let ix = join!(ia, ib);
        let v: u8x32<_> = _mm256_blendv_epi8(
            _mm256_shuffle_epi8(l, ix), _mm256_shuffle_epi8(r, ix), _mm256_slli_epi16::<3>(ix)
        ).simd_into(simd);
        v.into()
    }
);

fearless_simd::kernel!(
    pub(super) fn merge32_avx(simd: Avx2, region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) -> usize {
        let (groups, l, r) = tiles32(region, dst, bitmap, left, |out, a, b, bits| *out = select32(simd, a, b, bits));
        cursor(region, &mut dst[32 * groups..], &bitmap[4 * groups..], [l, r]);
        groups
    }
);
