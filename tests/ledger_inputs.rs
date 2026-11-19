const MIXED: &[u8] = include_bytes!("conformance/v9/valid/multiblock_mixed.expected");

pub fn inputs() -> (Vec<u8>, Vec<u8>) {
    let mut x = 42u64;
    let noise: Vec<_> = (0..1048576)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    // Alternating fixture text/records and skewed literals price differently
    // at each block size, so keep both plain and archive construction fixed.
    let mixed = noise
        .iter()
        .enumerate()
        .map(|(i, &b)| {
            if i % 4096 < 2048 {
                MIXED[i % MIXED.len()]
            } else {
                b & 31
            }
        })
        .collect();
    (noise, mixed)
}
