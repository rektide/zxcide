use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use zxcide::{DecodeOptions, Decoder, EncodeOptions, Encoder};

#[path = "../../../tests/bench_inputs.rs"]
mod inputs;

fn bench(c: &mut Criterion) {
    let (block_log, modes) = inputs::settings();
    for (name, plain) in inputs::corpora() {
        for &mode in modes {
            bench_roundtrip(
                c,
                &name,
                &plain,
                EncodeOptions {
                    mode,
                    block_log,
                    ..Default::default()
                },
            );
        }
    }
    bench_upstream(c);
}

fn bench_roundtrip(c: &mut Criterion, name: &str, plain: &[u8], opt: EncodeOptions<'_>) {
    let (mut enc, mut dec) = (Encoder::default(), Decoder::default());
    let (mut compressed, mut output) = (Vec::new(), Vec::new());
    enc.encode(plain, &mut compressed, opt).unwrap();
    dec.decode(&compressed[..], &mut output, DecodeOptions::default())
        .unwrap();
    assert_eq!(plain, output);
    eprintln!(
        "{name}/{:?}/b{}: {} source bytes -> {} archive bytes",
        opt.mode,
        opt.block_log,
        plain.len(),
        compressed.len()
    );
    let mut group = c.benchmark_group(format!("{name}/{:?}/b{}", opt.mode, opt.block_log));
    group.throughput(Throughput::Bytes(plain.len() as u64));
    group.bench_function("encode", |b| {
        b.iter(|| {
            compressed.clear();
            enc.encode(black_box(plain), &mut compressed, opt).unwrap();
            black_box(&compressed);
        })
    });
    group.bench_function("decode", |b| {
        b.iter(|| {
            output.clear();
            dec.decode(black_box(&compressed[..]), &mut output, DecodeOptions::default())
                .unwrap();
            black_box(&output);
        })
    });
    group.finish();
    let mut group = c.benchmark_group(format!("oneshot/{name}/{:?}/b{}", opt.mode, opt.block_log));
    group.throughput(Throughput::Bytes(plain.len() as u64));
    group.bench_function("encode", |b| {
        b.iter(|| {
            let mut archive = Vec::new();
            zxcide::encode(black_box(plain), &mut archive, opt).unwrap();
            archive
        })
    });
    group.bench_function("decode", |b| {
        b.iter(|| decode_once(black_box(&compressed), plain.len()))
    });
}

fn decode_once(archive: &[u8], len: usize) -> Vec<u8> {
    let mut plain = Vec::new();
    zxcide::decode(
        archive,
        &mut plain,
        DecodeOptions {
            max_output: Some(len as u64),
            ..Default::default()
        },
    )
    .unwrap();
    plain
}

fn bench_upstream(c: &mut Criterion) {
    let fixture = inputs::UPSTREAM;
    let expected = inputs::EXPECTED;
    let mut group = c.benchmark_group("upstream-l7");
    group.throughput(Throughput::Bytes(expected.len() as u64));
    let mut dec = Decoder::default();
    let mut output = Vec::new();
    dec.decode(fixture, &mut output, Default::default()).unwrap();
    assert_eq!(output, expected);
    group.bench_function("decode", |b| {
        b.iter(|| {
            output.clear();
            dec.decode(black_box(fixture), &mut output, Default::default()).unwrap();
            black_box(&output);
        })
    });
    group.finish();
    let mut group = c.benchmark_group("oneshot/upstream-l7");
    group.throughput(Throughput::Bytes(expected.len() as u64));
    group.bench_function("decode", |b| b.iter(|| decode_once(black_box(fixture), expected.len())));
}
criterion_group!(benches, bench);
criterion_main!(benches);
