---
type: Guide
title: Profiling the Rust ZXC codec
description: Reproducible throughput benchmarks, direct Tracy captures, and a measured decoder optimization.
resource: /doc/profiling/README.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0b145b97ffeZCgqrI2NHSkUTX', title: WIT component build and Wasmtime Criterion tuning workflow }
  - { resource: 'opencode://ses_e0b1ee216ffelb33FpbVf2g8SE', title: All-mode Criterion suite and named baseline comparison workflow }
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Codec measurements, interoperability and capture verification }
  - { resource: 'opencode://ses_e0b4c34d4ffeK4VV8rf1hpdri9', title: Tracy protocol compatibility investigation }
  - { resource: 'https://github.com/nagisa/rust_tracy_client', title: Rust client and bundled Tracy sources }
---

# Profile a codec change

Use Criterion for speed and ratio comparisons; use Tracy to find which phase is worth changing. Production builds omit Tracy, including the dependency. [Architecture](../architecture/README.md) explains the instrumentation boundaries.

## Measure throughput

```sh
cargo bench --locked --bench codec
# Compare a real corpus with the same block size and mode each time:
ZXC_BENCH_INPUT=/path/to/silesia/dickens ZXC_BENCH_BLOCK_LOG=19 ZXC_BENCH_MODE=dense \
  cargo bench --locked --bench codec -- dickens
```

The default workloads are 1 MiB of repetitive corpus text, 1 MiB of deterministic xorshift noise, the 64 KiB mixed conformance plaintext, and a separate upstream level‑7 PivCo decode. Inputs are in memory and verified before timing. The benchmark reuses contexts and destination capacity but includes frame setup, section scratch, checksums when present, and writing the actual output bytes. Throughput is measured in **uncompressed** bytes; source/archive byte counts are printed separately. For results worth retaining, record corpus SHA-256, CPU, toolchain, revision, options and Criterion intervals. Compare a fixed workload with Tracy disabled. Do not treat the highly repetitive fixture as typical text performance.

A baseline on the 5800X below (30 samples, checksums on for `split`, 512 KiB blocks):

| Input | Archive bytes | Encode | Decode |
| --- | ---: | ---: | ---: |
| Repeated text, 1 MiB | 788 | 5.61 GiB/s | 12.86 GiB/s |
| Noise, 1 MiB | 1,048,639 | 177 MiB/s | 12.26 GiB/s |
| Upstream wide Huffman, 16 KiB | reference fixture | — | 132 MiB/s |

## Compare a change

On a quiet machine, save a baseline before editing, then compare against it in the same target directory. For example, to tune dense encoding on mixed input:

```sh
cargo nextest run --locked --workspace
export ZXC_BENCH_MODE=dense
cargo bench --locked --bench codec -- mixed/Dense/b19/encode --save-baseline before
# Apply the change, run the correctness tests again, then:
cargo bench --locked --bench codec -- mixed/Dense/b19/encode --baseline before
```

Criterion reports the estimated change and its confidence interval. Repeat the comparison on unchanged code to see how much the machine varies. Keep CPU power settings, build flags, and corpus fixed; check archive size alongside throughput so a speedup from writing a larger archive is visible. For decode-only changes the fixed upstream archive also isolates decoder speed from changes in encoder output.

For a full sweep, set `ZXC_BENCH_MODE=all` and omit the filter in both commands. For a block-size experiment set `ZXC_BENCH_BLOCK_LOG` (12–21); the size is part of the benchmark ID. CI uses Criterion's `--test` mode to exercise every case once. Use a quiet, consistent host for timing decisions; nextest's parallel test durations are for diagnosing slow tests.

## Benchmark the component

Install the [component build prerequisites](../../README.md#webassembly-component) first. The wrapper rebuilds the release component and runs Criterion on the host using Wasmtime's typed bindings to the shipped WIT interface. For example:

```sh
ZXC_BENCH_MODE=all scripts/bench-component.sh --test
export ZXC_BENCH_MODE=dense ZXC_BENCH_BLOCK_LOG=19
scripts/bench-component.sh component/mixed/Dense/b19/encode --save-baseline wasm-before
# Change the codec or guest build flags, then repeat with the same workload:
scripts/bench-component.sh component/mixed/Dense/b19/encode --baseline wasm-before
```

The component and native suites use the same inputs (including `ZXC_BENCH_INPUT`), mode selection and uncompressed-byte throughput. Each component case verifies the output before measurement. The timer covers one call on an existing instance, including byte-list transfers, per-call codec workspaces, and output allocation and disposal. Component loading, JIT compilation, instantiation, and preparing the decode archive are outside the loop. The native suite reuses workspaces and buffers, so the difference between suites is not solely the cost of WebAssembly execution.

Keep the host runtime and input fixed while tuning the guest. For example, set `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS='-C target-feature=+simd128'` for a comparison on SIMD-capable runtimes. Record flags, component size, archive size, and component SHA-256 with the Criterion result. To measure an already distributed artifact, set `ZXC_COMPONENT` to its **absolute path** and run `cargo bench --locked -p zxc-component --features host-bench --bench component` directly.

## Capture phases

`tracy-client 0.19.0` and the locked `tracy-client-sys 0.30.1` use **Tracy 0.14.1 (protocol 82)**. Use the [matching capture tool](https://github.com/wolfpld/tracy/releases/tag/v0.14.1); 0.15.0's live protocol is incompatible. The build enables on-demand, localhost-only spans with static labels and no call stacks. Building this feature needs a C++ compiler. The profiling profile retains line tables.

```sh
cargo build --locked --profile profiling -p zxc-cli --features tracy
# Terminal A: wait for attachment before running even a tiny job.
TRACY_PORT=18086 target/profiling/zxc --wait-for-tracy test archive.zxc
# Terminal B (within 30 seconds):
tracy-capture -a 127.0.0.1 -p 18086 -o decode.tracy
tracy-csvexport decode.tracy > zones.csv
```

Use `encode --mode dense input -o new.zxc` in place of `test` for match finding, packing and entropy pricing. The frame zone includes I/O; use block and nested codec zones to separate it. Nested zone totals overlap. To sample native code without Tracy, build with `cargo build --profile profiling -p zxc-cli` and use `perf record -e cycles:u --call-graph dwarf -- target/profiling/zxc test archive.zxc`.

## First trace: wide Huffman decode

On a Ryzen 7 5800X, Linux x86‑64, rustc 1.101.0-nightly (`822182db4`), a capture of 1,000 concatenated `spl_pivco_wide_l7.zxc` fixtures showed **574 ms of 598 ms** of frame time in PivCo. The initial scalar loop traversed the tree for every symbol. Switching to bottom-up list merging removed that traversal without unsafe or SIMD: the same in-memory Criterion quick test went from **29.7 to 134.6 MiB/s**. The follow-up capture put 140 ms in PivCo out of 163 ms of frame time. This result concerns the pinned 16 KiB wide-Huffman fixture, not all archives.

To recreate the CLI comparison (release binaries, no attached profiler):

```sh
python3 -c 'from pathlib import Path; p=Path("tests/conformance/v9/valid/spl_pivco_wide_l7.zxc"); Path("repeated.zxc").write_bytes(p.read_bytes()*1000)'
hyperfine --warmup 2 --runs 5 -N 'target/release/zxc test repeated.zxc' '/path/to/c-zxc -qt -T1 repeated.zxc'
```

After the merge change, the Rust CLI took **122 ± 2 ms**, versus **31 ± 1 ms** for the pinned upstream native C release build. Both read the same archive and discard output. This end-to-end gap includes frame setup and I/O; the trace points to entropy decoding as the next place to look on this workload.
