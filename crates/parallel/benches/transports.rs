use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use zxcide::{DecodeOptions, EncodeOptions};
use zxcide_parallel::{Backend, Config};

fn bench(c: &mut Criterion) {
    // Alternate repeatable structured and noisy 4 KiB spans. At the small
    // block size these have quite different costs; don't price only queue hops.
    let mut x = 42u64;
    let plain: Vec<u8> = (0..4 * 1024 * 1024usize)
        .map(|i| {
            if (i / 4096) % 2 == 0 {
                (i % 67) as u8
            } else {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                x as u8
            }
        })
        .collect();
    let workers = std::env::var("ZXCIDE_BENCH_THREADS").map_or(4, |s| s.parse::<usize>().unwrap());
    let slots = std::env::var("ZXCIDE_BENCH_SLOTS").map_or(2 * workers, |s| s.parse().unwrap());
    for log in [12, 19] {
        let opt = EncodeOptions {
            block_log: log,
            ..Default::default()
        };
        let mut archive = Vec::new();
        zxcide::encode(&plain[..], &mut archive, opt).unwrap();
        let mut group = c.benchmark_group(format!("transports/b{log}"));
        group.throughput(Throughput::Bytes(plain.len() as u64));
        for (name, backend, n, k) in [
            ("serial", Backend::Crossbeam, 1, 1),
            ("crossbeam", Backend::Crossbeam, workers, slots),
            #[cfg(feature = "flume")]
            ("flume", Backend::Flume, workers, slots),
            #[cfg(feature = "kanal")]
            ("kanal", Backend::Kanal, workers, slots),
            #[cfg(feature = "crossfire")]
            ("crossfire", Backend::Crossfire, workers, slots),
            #[cfg(feature = "rayon")]
            ("rayon", Backend::Rayon, workers, slots),
        ] {
            let config = || Config {
                workers: n,
                slots: k,
                backend,
            };
            let mut dst = Vec::with_capacity(plain.len());
            zxcide_parallel::encode(&plain[..], &mut dst, opt, config()).unwrap();
            assert_eq!(dst, archive);
            dst.clear();
            zxcide_parallel::decode(&archive[..], &mut dst, DecodeOptions::default(), config()).unwrap();
            assert_eq!(dst, plain);
            group.bench_function(format!("encode/{name}-w{n}-k{k}"), |b| {
                b.iter(|| {
                    dst.clear();
                    zxcide_parallel::encode(black_box(&plain[..]), &mut dst, opt, config()).unwrap();
                    black_box(&dst);
                })
            });
            group.bench_function(format!("decode/{name}-w{n}-k{k}"), |b| {
                b.iter(|| {
                    dst.clear();
                    zxcide_parallel::decode(black_box(&archive[..]), &mut dst, Default::default(), config()).unwrap();
                    black_box(&dst);
                })
            });
        }
    }
}
criterion_group!(benches, bench);
criterion_main!(benches);
