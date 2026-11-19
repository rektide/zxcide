use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;

#[path = "../src/entropy/merge.rs"]
mod merge;

type Gather = fn(&[u8], &mut [u8], &[u8], usize);

fn bench(c: &mut Criterion) {
    let mut kernels: Vec<(&str, Gather)> = vec![
        ("cursor", merge::scalar),
        ("table", merge::table),
        ("selected", merge::gather),
    ];
    #[cfg(feature = "merge-simd")]
    kernels.push(("simd", merge::simd));
    #[cfg(not(feature = "merge-simd"))]
    let _ = &mut kernels;
    for n in [8usize, 16, 24, 32, 64, 256, 1024] {
        for (name, masks) in [
            ("balanced", &[0x17, 0xf0, 0xa5, 0xc6][..]),
            ("skewed", &[0x40, 0, 0, 0]),
        ] {
            let bits: Vec<_> = masks.iter().copied().cycle().take(n / 8).collect();
            let left = n - bits.iter().map(|x: &u8| x.count_ones() as usize).sum::<usize>();
            let region: Vec<_> = (0..n).map(|i| (i as u8).wrapping_mul(19)).collect();
            let mut output = vec![0; n];
            let mut group = c.benchmark_group(format!("merge/{name}/{n}"));
            group.throughput(Throughput::Bytes(n as u64));
            for &(kernel, gather) in &kernels {
                group.bench_function(kernel, |b| {
                    b.iter(|| {
                        gather(
                            black_box(&region),
                            black_box(&mut output),
                            black_box(&bits),
                            black_box(left),
                        )
                    })
                });
            }
        }
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
