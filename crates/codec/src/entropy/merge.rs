// The caller decoded and counted the whole bitmap, masking unused tail bits.
// Its children occupy `region[..left]` and `region[left..]` in that order.
#[inline]
pub(super) fn scalar(region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) {
    cursor(region, dst, bitmap, [0, left]);
}

#[inline]
fn cursor(region: &[u8], dst: &mut [u8], bitmap: &[u8], mut cursor: [usize; 2]) {
    for (chunk, &bits) in dst.chunks_mut(8).zip(bitmap) {
        for (j, dst) in chunk.iter_mut().enumerate() {
            let side = usize::from((bits >> j) & 1);
            *dst = region[cursor[side]];
            cursor[side] += 1;
        }
    }
}

#[derive(Clone, Copy)]
struct Mask {
    index: [u8; 16],
    zeroes: u8,
}

const MASKS: [Mask; 256] = {
    let mut table = [Mask {
        index: [0; 16],
        zeroes: 0,
    }; 256];
    let mut mask = 0;
    while mask < table.len() {
        let mut cursor = [0, 8];
        let mut j = 0;
        while j < 8 {
            let side = (mask >> j) & 1;
            table[mask].index[j] = cursor[side];
            cursor[side] += 1;
            j += 1;
        }
        table[mask].zeroes = cursor[0];
        mask += 1;
    }
    table
};

// Exactly eight readable bytes on each side are required for the fast path.
// Once either side is shorter it stays shorter. Scalar finishes both that
// suffix and a partial output byte, without relying on any wire padding.
#[inline(always)]
fn packed(
    region: &[u8],
    dst: &mut [u8],
    bitmap: &[u8],
    left: usize,
    mut select: impl FnMut(&mut [u8; 8], &[u8; 8], &[u8; 8], &Mask),
) {
    let (a, b) = region.split_at(left);
    let (mut i, mut j, mut groups) = (0, 0, 0);
    for (chunk, &bits) in dst.as_chunks_mut::<8>().0.iter_mut().zip(bitmap) {
        let (Some(x), Some(y)) = (
            a.get(i..).and_then(|s| s.first_chunk()),
            b.get(j..).and_then(|s| s.first_chunk()),
        ) else {
            break;
        };
        let mask = &MASKS[usize::from(bits)];
        select(chunk, x, y, mask);
        i += usize::from(mask.zeroes);
        j += 8 - usize::from(mask.zeroes);
        groups += 1;
    }
    cursor(region, &mut dst[groups * 8..], &bitmap[groups..], [i, left + j]);
}

#[inline]
pub(super) fn table(region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) {
    packed(region, dst, bitmap, left, |out, a, b, mask| {
        let mut window = [0; 16];
        window[..8].copy_from_slice(a);
        window[8..].copy_from_slice(b);
        for (d, &src) in out.iter_mut().zip(&mask.index) {
            *d = window[usize::from(src & 15)];
        }
    });
}

#[cfg(feature = "merge-simd")]
pub(super) fn simd(region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) {
    use fearless_simd::{Level, Simd, SimdBase, u8x16};

    #[inline(always)]
    fn kernel<S: Simd>(simd: S, region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) {
        packed(region, dst, bitmap, left, |out, a, b, mask| {
            let mut src = [0; 16];
            src[..8].copy_from_slice(a);
            src[8..].copy_from_slice(b);
            let x = u8x16::from_slice(simd, &src);
            let control = u8x16::from_slice(simd, &mask.index);
            let result = simd.swizzle_dyn_u8x16(x, control);
            out.copy_from_slice(&result.as_slice()[..8]);
        });
    }

    let level = Level::new();
    if level.is_fallback() || cfg!(any(target_arch = "x86", target_arch = "x86_64")) && matches_sse2(level) {
        table(region, dst, bitmap, left);
        return;
    }
    fearless_simd::dispatch!(level, token => kernel(token, region, dst, bitmap, left));
}

#[cfg(feature = "merge-simd")]
fn matches_sse2(level: fearless_simd::Level) -> bool {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        matches!(level, fearless_simd::Level::Sse2(_))
    }
    #[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
    {
        let _ = level;
        false
    }
}

#[inline]
pub(super) fn gather(region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize) {
    if dst.len() < 32 || left < 8 || region.len() - left < 8 {
        scalar(region, dst, bitmap, left);
        return;
    }
    #[cfg(feature = "merge-simd")]
    simd(region, dst, bitmap, left);
    #[cfg(not(feature = "merge-simd"))]
    table(region, dst, bitmap, left);
}
