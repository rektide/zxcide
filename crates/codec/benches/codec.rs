use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use zxc_codec::{DecodeOptions, Decoder, EncodeOptions, Encoder};

fn bench(c: &mut Criterion) {
    let text = include_bytes!("../../../tests/conformance/v9/valid/text_64k_level1.expected").repeat(16);
    let mut x = 42u64;
    let noise = (0..text.len())
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect();
    for (name, plain) in [("text", text), ("noise", noise)] {
        let (mut enc, mut dec) = (Encoder::default(), Decoder::default());
        let (mut compressed, mut output) = (Vec::new(), Vec::new());
        let opt = EncodeOptions::default();
        enc.encode(&plain[..], &mut compressed, opt).unwrap();
        dec.decode(&compressed[..], &mut output, DecodeOptions::default()).unwrap();
        assert_eq!(plain, output);
        eprintln!("{name}: {} source bytes -> {} archive bytes", plain.len(), compressed.len());
        let mut group = c.benchmark_group(name);
        group.throughput(Throughput::Bytes(plain.len() as u64));
        group.bench_function("encode", |b| {
            b.iter(|| {
                compressed.clear();
                enc.encode(black_box(&plain[..]), &mut compressed, opt).unwrap();
                black_box(&compressed);
            })
        });
        group.bench_function("decode", |b| {
            b.iter(|| {
                output.clear();
                dec.decode(black_box(&compressed[..]), &mut output, DecodeOptions::default()).unwrap();
                black_box(&output);
            })
        });
    }
    let fixture = include_bytes!("../../../tests/conformance/v9/valid/spl_pivco_wide_l7.zxc");
    let expected = include_bytes!("../../../tests/conformance/v9/valid/spl_pivco_wide_l7.expected");
    let mut group = c.benchmark_group("upstream-l7");
    group.throughput(Throughput::Bytes(expected.len() as u64));
    let mut dec = Decoder::default();
    let mut output = Vec::new();
    dec.decode(&fixture[..], &mut output, Default::default()).unwrap();
    assert_eq!(output, expected);
    group.bench_function("decode", |b| {
        b.iter(|| {
            output.clear();
            dec.decode(black_box(&fixture[..]), &mut output, Default::default()).unwrap();
            black_box(&output);
        })
    });
}
criterion_group!(benches, bench);
criterion_main!(benches);
