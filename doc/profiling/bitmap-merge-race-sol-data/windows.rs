// The counted bitmap ensures l <= left <= r and that only the logical
// bytes in each window are selected. A left load can cross into the right
// child, but neither load can cross the initialized node end.
#[inline(always)]
fn tiles32(
    region: &[u8], dst: &mut [u8], bitmap: &[u8], left: usize,
    mut tile: impl FnMut(&mut [u8; 32], &[u8; 32], &[u8; 32], u32),
) -> (usize, usize, usize) {
    let (mut l, mut r, mut groups) = (0, left, 0);
    for (out, ctrl) in dst.as_chunks_mut::<32>().0.iter_mut().zip(bitmap.as_chunks::<4>().0) {
        let (Some(a), Some(b)) = (
            region.get(l..).and_then(|s| s.first_chunk()),
            region.get(r..).and_then(|s| s.first_chunk()),
        ) else { break };
        let bits = u32::from_le_bytes(*ctrl);
        tile(out, a, b, bits);
        let ones = bits.count_ones() as usize;
        l += 32 - ones;
        r += ones;
        groups += 1;
    }
    (groups, l, r)
}
