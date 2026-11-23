#![forbid(unsafe_code)]
use std::io::{self, Write};

fn main() {
    assert!(std::env::var_os("ZXC_MERGE_CAPTURE").is_some());
    let args: Vec<_> = std::env::args_os().collect();
    let data = std::fs::read(&args[1]).unwrap();
    let idx = std::fs::read_to_string(&args[2]).unwrap();
    let (mut out, mut merge) = (Vec::new(), Vec::new());
    for ln in idx.lines() {
        let nums: Vec<u32> = ln.split_whitespace().map(|s| s.parse().unwrap()).collect();
        let [off,len,n] = nums[..] else { panic!("bad index") };
        for x in [u32::MAX,off,len,n] { io::stdout().write_all(&x.to_le_bytes()).unwrap(); }
        zxc_codec::replay_huf(&data[off as usize..(off+len) as usize], n as usize, &mut out, &mut merge).unwrap();
    }
}
