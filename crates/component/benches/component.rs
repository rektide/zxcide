use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use wasmtime::{
    Engine, Store,
    component::{Component, Linker},
};

#[path = "../../../tests/bench_inputs.rs"]
mod inputs;
#[path = "../../../tests/ledger_inputs.rs"]
mod ledger_inputs;
wasmtime::component::bindgen!({ world: "codec" });
use exports::zxcide::codec::buffers::{EncodeOptions, Mode};

fn bench(c: &mut Criterion) {
    let path = std::env::var_os("ZXCIDE_COMPONENT")
        .expect("use cargo xtask bench component to build and select the component");
    let mut config = wasmtime::Config::new();
    if let Some(mode) = std::env::var_os("ZXCIDE_JITDUMP") {
        config.profiler(wasmtime::ProfilingStrategy::JitDump);
        config.debug_info(mode == "dwarf");
    }
    let engine = Engine::new(&config).expect("creating Wasmtime engine (jitdump requires Linux and host-perf)");
    let component = Component::from_file(&engine, path).unwrap();
    let mut store = Store::new(&engine, ());
    let guest = Codec::instantiate(&mut store, &component, &Linker::new(&engine)).unwrap();
    let codec = guest.zxcide_codec_buffers();
    let (block_log, modes) = inputs::settings();
    for (name, plain) in inputs::corpora() {
        let bytes = plain.len() as u64;
        for &mode in modes {
            let options = EncodeOptions {
                mode: match mode {
                    zxcide::Mode::Split => Mode::Split,
                    zxcide::Mode::Packed => Mode::Packed,
                    zxcide::Mode::Dense => Mode::Dense,
                },
                block_log,
                checksum: true,
                seekable: false,
            };
            let archive = codec.call_encode(&mut store, &plain, options).unwrap().unwrap();
            assert_eq!(codec.call_decode(&mut store, &archive, bytes).unwrap().unwrap(), plain);
            let id = format!("component/{name}/{mode:?}/b{block_log}");
            eprintln!("{id}: {bytes} source bytes -> {} archive bytes", archive.len());
            let mut g = c.benchmark_group(id);
            g.throughput(Throughput::Bytes(bytes));
            g.bench_function("encode", |b| {
                b.iter(|| {
                    codec
                        .call_encode(&mut store, black_box(&plain), options)
                        .unwrap()
                        .unwrap()
                })
            });
            g.bench_function("decode", |b| {
                b.iter(|| {
                    codec
                        .call_decode(&mut store, black_box(&archive), bytes)
                        .unwrap()
                        .unwrap()
                })
            });
        }
    }
    let archive = inputs::UPSTREAM;
    let bytes = inputs::EXPECTED.len() as u64;
    assert_eq!(
        codec.call_decode(&mut store, archive, bytes).unwrap().unwrap(),
        inputs::EXPECTED
    );
    assert!(codec.call_decode(&mut store, archive, bytes - 1).unwrap().is_err());
    let mut g = c.benchmark_group("component/upstream-l7");
    g.throughput(Throughput::Bytes(bytes));
    g.bench_function("decode", |b| {
        b.iter(|| {
            codec
                .call_decode(&mut store, black_box(archive), bytes)
                .unwrap()
                .unwrap()
        })
    });
    g.finish();

    let (noise, mixed) = ledger_inputs::inputs();
    for (name, plain, mode, block_log) in [
        ("mixed", &mixed, zxcide::Mode::Dense, 12),
        ("mixed", &mixed, zxcide::Mode::Dense, 19),
        ("noise", &noise, zxcide::Mode::Split, 12),
    ] {
        let mut archive = Vec::new();
        zxcide::encode(
            &plain[..],
            &mut archive,
            zxcide::EncodeOptions {
                mode,
                block_log,
                ..Default::default()
            },
        )
        .unwrap();
        let bytes = plain.len() as u64;
        assert_eq!(codec.call_decode(&mut store, &archive, bytes).unwrap().unwrap(), *plain);
        let mut g = c.benchmark_group(format!("component/ledger/{name}/b{block_log}"));
        g.throughput(Throughput::Bytes(bytes));
        g.bench_function("decode", |b| {
            b.iter(|| {
                codec
                    .call_decode(&mut store, black_box(&archive), bytes)
                    .unwrap()
                    .unwrap()
            })
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
