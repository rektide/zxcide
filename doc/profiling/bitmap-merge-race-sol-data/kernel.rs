
#[path = "../entropy/merge.rs"]
mod merge;

fn u32le(s: &[u8]) -> usize { u32::from_le_bytes(s[..4].try_into().unwrap()) as usize }

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let emit = args[1] == "verify";
    assert!(emit || args[1] == "serve");
    // Compact replay stream uses a 16-byte record header, exact masks and
    // node input, plus 32 initialized guard bytes only for the C arm. Rust
    // still receives the exact n-byte view on precisely the same storage.
    let data = std::fs::read(&args[2]).unwrap();
    let mut runs = Vec::new();
    let mut src = &data[..];
    while !src.is_empty() {
        let n = u32le(src);
        let l = u32le(&src[4..]);
        let (mask, more) = src[16..].split_at(n.div_ceil(8));
        let (region, next) = more.split_at(n + 32);
        runs.push((n,l,mask,&region[..n]));
        src = next;
    }
    let mut out = vec![0; runs.iter().map(|r| r.0).max().unwrap()];
    replay(runs.len(), "merge-oracle", emit, |emit| {
        let mut result = (0,0);
        for &(n,l,bits,region) in &runs {
            let dst = &mut out[..n];
            merge::gather(region,dst,bits,l);
            if emit {
                let (mut zeros, mut ones) = (region[..l].iter(), region[l..].iter());
                for (i,actual) in dst.iter().enumerate() {
                    let expected = if bits[i/8] & (1<<(i%8)) == 0 { zeros.next() } else { ones.next() };
                    assert_eq!(Some(actual), expected, "original merge differs from bit oracle at {i}/{n}");
                }
                assert!(zeros.next().is_none() && ones.next().is_none());
            }
            consume(&mut result,dst,emit);
        }
        result
    });
}
