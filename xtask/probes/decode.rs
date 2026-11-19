//! Single-threaded one-shot buffer API probe, built in a frozen codec checkout.
use std::{env, fs, time::Instant};
use zxc_codec::DecodeOptions;

fn main() {
    let args: Vec<_> = env::args().collect();
    assert_eq!(args.len(), 4);
    let archive = fs::read(&args[1]).unwrap();
    let plain = fs::read(&args[2]).unwrap();
    let count: usize = args[3].parse().unwrap();
    assert!(count > 0);
    let mut output = Vec::with_capacity(plain.len());
    zxc_codec::decode(&archive[..], &mut output, DecodeOptions::default()).unwrap();
    assert_eq!(output, plain);
    for _ in 0..count {
        output.clear();
        let start = Instant::now();
        zxc_codec::decode(&archive[..], &mut output, DecodeOptions::default()).unwrap();
        println!("{:.6}", start.elapsed().as_secs_f64() * 1000.0);
    }
    assert_eq!(output, plain);
}
