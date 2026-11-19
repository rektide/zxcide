use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use wasmtime::{
    Engine, Store,
    component::{Component, Linker},
};

#[path = "../../../tests/bench_inputs.rs"]
mod inputs;
wasmtime::component::bindgen!({ world: "codec" });
use exports::zxc::codec::buffers::{EncodeOptions, Mode};

fn bench(c: &mut Criterion) {
    let path =
        std::env::var_os("ZXC_COMPONENT").expect("use scripts/bench-component.sh to build and select the component");
    let mut config = wasmtime::Config::new();
    if std::env::var_os("ZXC_JITDUMP").is_some() {
        config.profiler(wasmtime::ProfilingStrategy::JitDump);
    }
    let engine = Engine::new(&config).expect("creating Wasmtime engine (jitdump requires Linux and host-perf)");
    let component = Component::from_file(&engine, path).unwrap();
    let mut store = Store::new(&engine, ());
    let guest = Codec::instantiate(&mut store, &component, &Linker::new(&engine)).unwrap();
    let codec = guest.zxc_codec_buffers();
    let (block_log, modes) = inputs::settings();
    for (name, plain) in inputs::corpora() {
        let bytes = plain.len() as u64;
        for &mode in modes {
            let options = EncodeOptions {
                mode: match mode {
                    zxc_codec::Mode::Split => Mode::Split,
                    zxc_codec::Mode::Packed => Mode::Packed,
                    zxc_codec::Mode::Dense => Mode::Dense,
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
}

criterion_group!(benches, bench);
criterion_main!(benches);
