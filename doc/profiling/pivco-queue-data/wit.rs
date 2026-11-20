use std::{fs, hint::black_box, path::Path, time::Instant};
use wasmtime::{Engine, Store, component::{Component, Linker}};

wasmtime::component::bindgen!({ world: "codec" });

fn main() {
    let work = std::env::args_os().nth(1).unwrap();
    let work = Path::new(&work);
    let e = Engine::default();
    let linker = Linker::new(&e);
    let mut instances = ["base", "trial"].map(|who| {
        let c = Component::from_file(&e, work.join(format!("bin/{who}.wasm"))).unwrap();
        let mut store = Store::new(&e, ());
        let guest = Codec::instantiate(&mut store, &c, &linker).unwrap();
        (store, guest)
    });
    println!("run\tpair\tarchive\tworkers\tposition\tvariant\twall_ms\tcpu_ms");
    let args: Vec<_> = std::env::args().collect();
    let verify_only = args.iter().any(|s| s == "--verify-only");
    let samples = if args.iter().any(|s| s == "--confirm") { 24 } else { 12 };
    for (name, source, iterations) in [
        ("sao.dense", "sao", 6), ("sao.c7", "sao", 6), ("sao.split", "sao", 6),
        ("mixed12", "mixed", 12), ("mixed19", "mixed", 32), ("noise12", "noise", 32), ("light12", "light12", 32),
    ] {
        if samples == 24 && !args.iter().skip(3).any(|s| s == name) { continue; }
        let archive = fs::read(work.join(format!("fixtures/{name}.zxc"))).unwrap();
        let plain = fs::read(work.join(format!("fixtures/{source}"))).unwrap();
        let limit = plain.len() as u64;
        for (store, guest) in &mut instances {
            let codec = guest.zxc_codec_buffers();
            assert_eq!(codec.call_decode(&mut *store, &archive, limit).unwrap().unwrap(), plain);
            assert!(codec.call_decode(&mut *store, &archive, limit - 1).unwrap().is_err());
            assert!(codec.call_decode(&mut *store, &archive[..archive.len() - 1], limit).unwrap().is_err());
            assert_eq!(codec.call_decode(&mut *store, &archive, limit).unwrap().unwrap(), plain);
        }
        if verify_only { continue; }
        for run in 1..=2 {
            let mut order: Vec<_> = (0..samples).map(|i| i % 2).collect();
            let mut state = 5761u64 + run + archive.len() as u64;
            for i in (1..order.len()).rev() {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                order.swap(i, ((state >> 32) as usize) % (i + 1));
            }
            for pair in -1..samples {
                let first = if pair < 0 { (run % 2) as i32 } else { order[pair as usize] };
                for (pos, index) in [first as usize, 1-first as usize].into_iter().enumerate() {
                    let (store, guest) = &mut instances[index];
                    let codec = guest.zxc_codec_buffers();
                    let now = Instant::now();
                    for _ in 0..iterations {
                        black_box(codec.call_decode(&mut *store, black_box(&archive), limit).unwrap().unwrap());
                    }
                    if pair >= 0 {
                        let ms = now.elapsed().as_secs_f64() * 1000.0 / iterations as f64;
                        let who = ["base", "trial"][index];
                        println!("{run}\t{pair}\t{name}\t1\t{}\t{who}\t{ms:.6}\t", pos + 1);
                    }
                }
            }
        }
    }
}
