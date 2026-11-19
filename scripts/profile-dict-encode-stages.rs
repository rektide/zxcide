//! Estimate baseline stage costs with the ledger's seeded dictionary and plain.
//! Run standalone: `rustc -O scripts/profile-dict-encode-stages.rs -o <scratch>`,
//! then pin the executable to a core. This is not a whole-encoder benchmark.
use std::{hint::black_box, time::Instant};

fn main() {
    let mut x = 42u64;
    let noise: Vec<u8> = (0..65535)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    for size in [4096, 524288] {
        let plain = noise[32768..32768 + 4096].repeat(size / 4096);
        let mut history = Vec::with_capacity(noise.len() + plain.len());
        let mut table = Vec::<u32>::with_capacity(65536);
        let mut times = [Vec::new(), Vec::new(), Vec::new()];
        for i in 0..4160 {
            let a = Instant::now();
            history.clear();
            history.extend_from_slice(black_box(&noise));
            history.extend_from_slice(black_box(&plain));
            black_box(&history);
            let b = Instant::now();
            table.clear();
            table.resize(65536, u32::MAX);
            black_box(&table);
            let c = Instant::now();
            let data = black_box(history.as_slice());
            for pos in 0..noise.len().min(data.len().saturating_sub(4)) {
                let word = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                table[(word.wrapping_mul(0x9e3779b1) >> 16) as usize] = pos as u32;
            }
            black_box(&table);
            let d = Instant::now();
            if i >= 64 {
                for (samples, delta) in times.iter_mut().zip([b - a, c - b, d - c]) {
                    samples.push(delta.as_nanos() as f64 / 1000.);
                }
            }
        }
        for (name, samples) in ["history copy", "table reset", "prefix seed"]
            .into_iter()
            .zip(&mut times)
        {
            samples.sort_unstable_by(f64::total_cmp);
            let n = samples.len();
            println!(
                "{size:6} {name:12}: median {:6.2} µs (p10–p90 {:.2}–{:.2})",
                samples[n / 2],
                samples[n / 10],
                samples[9 * n / 10]
            );
        }
    }
}
