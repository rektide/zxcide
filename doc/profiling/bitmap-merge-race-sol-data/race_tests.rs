use super::*;

fn check(proof: fearless_simd::Avx2, bits: &[u8], n: usize, offset: usize, min_tiles: usize) {
    let mut out = vec![0x5e; offset+n].into_boxed_slice();
    let mut bitmap = vec![0x3c; offset].into_iter().chain(bits.iter().copied()).collect::<Box<[_]>>();
    let mut seq = 0x34571893d812045bu64 ^ n as u64;
    let oracle: Vec<u8> = (0..n).map(|_| {
        seq ^= seq << 13;
        seq ^= seq >> 7;
        seq ^= seq << 17;
        seq as u8
    }).collect();
    let (l, r): (Vec<_>, Vec<_>) = oracle.iter().copied().enumerate().partition(|&(i, _)| bits[i / 8] & (1 << (i % 8)) == 0);
    let region = vec![0x87;offset].into_iter().chain(l.iter().chain(&r).map(|&(_, b)| b)).collect::<Box<[_]>>();
    if !n.is_multiple_of(8) { *bitmap.last_mut().unwrap() |= 0xff << (n % 8); }
    let tiles = ENGINE(proof, &region[offset..], &mut out[offset..], &bitmap[offset..], l.len());
    assert!(tiles >= min_tiles);
    assert_eq!(&out[offset..], oracle);
    assert!(out[..offset].iter().all(|&b| b == 0x5e));
    gather(&region[offset..], &mut out[offset..], &bitmap[offset..], l.len());
    assert_eq!(&out[offset..], oracle);
}

#[test]
fn ordered_pairs_in_each_lane_fast_and_exact_end() {
    let Some(proof) = fearless_simd::Level::new().as_avx2() else { return };
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            let mut bits = [0xaa; 21];
            bits[4..8].copy_from_slice(&[a,b,a,b]);
            // Nonzero child cursors before tile two; >=32 readable at both
            // cursors even if each selected byte so far came from the right.
            check(proof, &bits, 161, usize::from(a.wrapping_add(b)&31), 2);
        }
    }
}

#[test]
fn exact_suffix_and_exhaustion_in_both_directions() {
    let Some(proof) = fearless_simd::Level::new().as_avx2() else { return };
    for n in (0usize..=129).chain([159,160,161,255,256,257,511,512,513]) {
        for pattern in 0..6 {
            let mut bits = vec![0u8; n.div_ceil(8)];
            for (i,b) in bits.iter_mut().enumerate() {
                *b = match pattern {
                    0 => 0, 1 => 255, 2 => if i < 5 { 255 } else { 0 },
                    3 => if i < 5 { 0 } else { 255 },
                    4 => if i % 17 == 0 { 0x81 } else { 0 },
                    _ => (i * 131 + n * 17) as u8,
                };
            }
            for offset in 0..32 { check(proof, &bits, n, offset, 0); }
        }
    }
}
