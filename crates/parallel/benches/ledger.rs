//! Identical calls in two builds: diagnostic counts OR Criterion, never both.
use criterion::{Criterion, criterion_group, criterion_main};
use std::io::Write;
use zxc_codec::{DecodeOptions, Dictionary, EncodeOptions, Mode};
use zxc_parallel::Config;
use zxc_seek::{Archive, FileSource, ReadAt};

const MIXED: &[u8] = include_bytes!("../../../tests/conformance/v9/valid/multiblock_mixed.expected");
const WIDE: &[u8] = include_bytes!("../../../tests/conformance/v9/valid/spl_pivco_wide_l7.zxc");
const EXPECTED: &[u8] = include_bytes!("../../../tests/conformance/v9/valid/spl_pivco_wide_l7.expected");

fn measure(c: &mut Criterion, id: &str, n: usize, expected: &[u8], mut run: impl FnMut(&mut Vec<u8>)) {
    let mut dst = Vec::with_capacity(expected.len());
    #[cfg(feature = "ledger")]
    zxc_codec::ledger::take();
    run(&mut dst);
    assert_eq!(dst, expected);
    #[cfg(feature = "ledger")]
    {
        let _ = c;
        zxc_codec::ledger::add("output:destination", dst.len());
        let mut sites: Vec<_> = zxc_codec::ledger::take().into_iter().collect();
        sites.sort_by_key(|&(_, b)| std::cmp::Reverse(b));
        for (site, b) in sites {
            println!("site\t{id}\t{n}\t{site}\t{b}\t{:.5}", b as f64 / n as f64);
        }
    }
    #[cfg(not(feature = "ledger"))]
    {
        let mut g = c.benchmark_group("ledger");
        g.throughput(criterion::Throughput::Bytes(n as u64));
        g.bench_function(id, |b| {
            b.iter(|| {
                dst.clear();
                run(&mut dst);
                std::hint::black_box(&dst);
            })
        });
    }
}

fn range(c: &mut Criterion, id: &str, src: impl ReadAt, plain: &[u8]) {
    let archive = Archive::open(src).unwrap();
    for n in [4096, 524288] {
        let r = 17..17 + n;
        measure(c, &format!("{id}/r{n}"), n, &plain[r.clone()], |w| {
            archive.copy_range(r.start as u64..r.end as u64, &[], w).unwrap();
        });
    }
}

fn bench(c: &mut Criterion) {
    let mut x = 42u64;
    let noise: Vec<_> = (0..1048576)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    // Alternate fixture text/records with skewed literals so Dense actually
    // emits entropy sections at both block sizes.
    let mixed: Vec<_> = noise
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
    // Pinned v2 header: binds these 65535 bytes and an absent shared tree.
    let mut zxd = vec![199, 209, 176, 156, 2, 0, 255, 255, 63, 201, 69, 204, 0, 0, 22, 191];
    zxd.extend_from_slice(&noise[..65535]);
    zxd.extend([0; 128]);
    let dict = Dictionary::from_bytes(&zxd).unwrap();
    let records = noise[32768..32768 + 4096].repeat(256);
    for log in [12, 19] {
        for (family, plain, mode, d) in [
            ("noise", &noise, Mode::Split, None),
            ("mixed", &mixed, Mode::Dense, None),
            ("dict", &records, Mode::Split, Some(&dict)),
        ] {
            let opt = EncodeOptions {
                block_log: log,
                mode,
                dictionary: d,
                ..Default::default()
            };
            let dec = DecodeOptions {
                dictionaries: d.map_or(&[], std::slice::from_ref),
                max_output: Some(plain.len() as u64),
            };
            let mut archive = Vec::new();
            zxc_codec::encode(&plain[..], &mut archive, opt).unwrap();
            for workers in [1, 4] {
                let config = Config {
                    workers,
                    slots: 8,
                    ..Default::default()
                };
                let id = format!("{family}/b{log}/w{workers}");
                measure(c, &format!("{id}/encode"), plain.len(), &archive, |w| {
                    zxc_parallel::encode(&plain[..], w, opt, config).unwrap()
                });
                measure(c, &format!("{id}/decode"), plain.len(), plain, |w| {
                    zxc_parallel::decode(&archive[..], w, dec, config).unwrap()
                });
            }
        }
        let mut indexed = Vec::new();
        zxc_codec::encode(
            &noise[..],
            &mut indexed,
            EncodeOptions {
                block_log: log,
                seekable: true,
                ..Default::default()
            },
        )
        .unwrap();
        range(c, &format!("range/b{log}/slice"), &indexed, &noise);
        let mut file = tempfile::NamedTempFile::new_in(std::env::current_dir().unwrap()).unwrap();
        file.write_all(&indexed).unwrap();
        range(
            c,
            &format!("range/b{log}/file"),
            FileSource::open(file.path()).unwrap(),
            &noise,
        );
    }
    #[cfg(not(feature = "ledger"))]
    {
        let mut g = c.benchmark_group("ledger/dict-first");
        for (blocks, workers) in [(1, 1), (2, 1), (8, 1), (8, 4)] {
            let plain = &records[..blocks * 4096];
            let config = Config {
                workers,
                slots: 8,
                ..Default::default()
            };
            let opt = EncodeOptions {
                block_log: 12,
                ..Default::default()
            };
            g.throughput(criterion::Throughput::Bytes(plain.len() as u64));
            g.bench_function(format!("b{blocks}/w{workers}/encode"), |b| {
                b.iter_batched_ref(
                    || (Dictionary::from_bytes(&zxd).unwrap(), Vec::with_capacity(plain.len())),
                    |(dict, dst)| {
                        zxc_parallel::encode(
                            plain,
                            &mut *dst,
                            EncodeOptions {
                                dictionary: Some(dict),
                                ..opt
                            },
                            config,
                        )
                        .unwrap();
                        std::hint::black_box(dst);
                    },
                    criterion::BatchSize::PerIteration,
                );
            });
        }
    }
    for workers in [1, 4] {
        let config = Config {
            workers,
            slots: 8,
            ..Default::default()
        };
        measure(c, &format!("wide/w{workers}/decode"), EXPECTED.len(), EXPECTED, |w| {
            zxc_parallel::decode(
                WIDE,
                w,
                DecodeOptions {
                    max_output: Some(EXPECTED.len() as u64),
                    ..Default::default()
                },
                config,
            )
            .unwrap()
        });
    }
}
criterion_group!(benches, bench);
criterion_main!(benches);
