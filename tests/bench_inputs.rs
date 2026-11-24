//! Workloads and tuning controls shared by the native and component benches.
use zxcide::Mode;

pub const UPSTREAM: &[u8] = include_bytes!("conformance/v9/valid/spl_pivco_wide_l7.zxc");
pub const EXPECTED: &[u8] = include_bytes!("conformance/v9/valid/spl_pivco_wide_l7.expected");

pub fn corpora() -> Vec<(String, Vec<u8>)> {
    let text = include_bytes!("conformance/v9/valid/text_64k_level1.expected").repeat(16);
    let mut x = 42u64;
    let noise = (0..text.len())
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    let mixed = include_bytes!("conformance/v9/valid/multiblock_mixed.expected").to_vec();
    let mut corpora = vec![
        ("text".to_owned(), text),
        ("noise".to_owned(), noise),
        ("mixed".to_owned(), mixed),
    ];
    if let Some(path) = std::env::var_os("ZXCIDE_BENCH_INPUT") {
        let path = std::path::PathBuf::from(path);
        corpora.push((
            path.file_name().unwrap().to_string_lossy().into_owned(),
            std::fs::read(path).unwrap(),
        ));
    }
    corpora
}

pub fn settings() -> (u8, &'static [Mode]) {
    let block_log = std::env::var("ZXCIDE_BENCH_BLOCK_LOG").map_or(19, |s| s.parse().unwrap());
    let modes: &[Mode] = match std::env::var("ZXCIDE_BENCH_MODE").as_deref() {
        Ok("all") => &[Mode::Split, Mode::Packed, Mode::Dense],
        Ok("dense") => &[Mode::Dense],
        Ok("packed") => &[Mode::Packed],
        Ok("split") | Err(_) => &[Mode::Split],
        _ => panic!("expected all, dense, packed or split"),
    };
    (block_log, modes)
}
