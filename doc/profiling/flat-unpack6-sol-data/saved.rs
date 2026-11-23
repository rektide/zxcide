// Appended in scratch to the unchanged clock/replay protocol from replay.rs.
#[path = "../entropy/flat.rs"]
mod flat;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3);
    let emit = args[1] == "verify";
    assert!(emit || args[1] == "serve");
    let data = std::fs::read(&args[2]).unwrap();
    let mut runs = Vec::new();
    let mut left = &data[..];
    while !left.is_empty() {
        let (head, next) = left.split_at(68);
        let n = u32::from_le_bytes(head[..4].try_into().unwrap()) as usize;
        assert!(n > 0 && n <= 1 << 21);
        let (run, next) = next.split_at((6 * n).div_ceil(8));
        runs.push((n, &head[4..], run));
        left = next;
    }
    let mut out = vec![0; runs.iter().map(|r| r.0).max().unwrap()];
    replay(runs.len(), "flat6", emit, |emit| {
        let mut result = (0, 0);
        for &(n, table, run) in &runs {
            let dst = &mut out[..n];
            flat::unpack(dst, run, table);
            consume(&mut result, dst, emit);
        }
        result
    });
}
