# zxcide

[![Codec checks](https://github.com/rektide/zxcide/actions/workflows/check.yml/badge.svg)](https://github.com/rektide/zxcide/actions/workflows/check.yml)
[![License: BSD-3-Clause](https://img.shields.io/badge/license-BSD--3--Clause-blue.svg)](LICENSE)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-brightgreen.svg)](crates/codec/src/lib.rs)

A pure, safe Rust implementation of the [ZXC v9 lossless compression format](https://github.com/hellobertrand/zxc/blob/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/docs/FORMAT.md) — streaming codec, random-access reader, bounded parallel pipeline, CLI, and WebAssembly component.

Pinned reference: **[hellobertrand/zxc](https://github.com/hellobertrand/zxc) at `2992c28`**. Building the default codec and CLI requires no `libzxc` or C compiler.

## Highlights

- **Pure safe Rust:** `#![forbid(unsafe_code)]` across `zxcide`, `zxcide-seek`, and `zxcide-parallel`, including runtime-dispatched SIMD (`merge-simd` via `fearless_simd` for x86-64 SSE4.2/AVX2, ARM64 NEON, and Wasm SIMD128) with bounded portable scalar fallbacks.
- **Full ZXC v9 wire compatibility:** Decodes archives produced by upstream levels 1–7, including RAW/SPL/PAK blocks, raw/RLE/PivCo-Huffman sections, RapidHash checksums, `.zxd` v2 dictionaries, seek tables, and concatenated frames.
- **Three encoder policies:** `split` (default), `packed`, and `dense` (deeper match search with size-priced raw, RLE, and PivCo sections), all falling back to RAW whenever compression would expand a block.
- **O(1) random access:** `--seekable` frames and `zxcide-seek` read arbitrary plaintext byte ranges across concatenated archives without decoding untouched blocks.
- **Deterministic bounded parallelism:** `zxcide-parallel` runs ordered multi-worker encode and decode pipelines with bounded in-flight block slots, producing byte-identical archives regardless of worker count or transport.
- **Zero-import WebAssembly Component:** Standalone WIT component (`zxcide:codec/buffers@0.1.0`) shipped in scalar, SIMD128, and relaxed-SIMD builds.
- **Cross-checked against upstream C:** Guarded by the vendored v9 conformance corpus, bidirectional C oracle interop, seeded generated-case suites, Linux ARM64/RISC-V QEMU CI, and `cargo-fuzz` targets.

## Workspace crates

| Crate | Path | Role |
| --- | --- | --- |
| `zxcide` | [`crates/codec`](crates/codec) | Core streaming and block-level ZXC v9 encoder/decoder, `.zxd` v2 dictionaries, and optional `merge-simd` acceleration |
| `zxcide-seek` | [`crates/seek`](crates/seek) | Positioned `ReadAt` random-access reader (`Archive`, `FileSource`) for seekable single and concatenated frames |
| `zxcide-parallel` | [`crates/parallel`](crates/parallel) | Bounded, ordered multi-worker streaming driver (Crossbeam default; optional Flume, Kanal, Crossfire, Rayon) |
| `zxcide-cli` | [`crates/cli`](crates/cli) | `zxcide` CLI binary (`encode`, `decode`, `test`, `info`, `range`), enabling `merge-simd` by default |
| `zxcide-component` | [`crates/component`](crates/component) | WebAssembly Component Model wrapper implementing [`zxcide:codec/buffers@0.1.0`](crates/component/wit/codec.wit) |
| `xtask` | [`xtask`](xtask) | Dev automation for C interop, benchmarks, component builds, profiling, and fuzz smoke runs |

## CLI quick start

```sh
cargo build --release -p zxcide-cli

target/release/zxcide encode input.bin -o input.zxc
target/release/zxcide test input.zxc
target/release/zxcide decode input.zxc -o restored.bin
```

For streaming commands (`encode`, `decode`, `test`), omit a path or pass `-` to use standard I/O. Named outputs are written to a temporary file in the destination directory and published atomically without clobbering existing files. When writing to a pipe or stdout, a decode error can leave previously verified blocks in the output.

### Encode modes & options

| Mode | Wire layout & strategy |
| --- | --- |
| `split` *(default)* | Greedy LZ match finder, split token/offset streams, raw literals |
| `packed` | Same greedy match finder with fixed-width packed sequence words |
| `dense` | Deeper match search for smaller archives; split streams with size-priced raw, RLE, or PivCo-Huffman sections |

- **Block size:** `--block-log 12..21` selects 2¹² (4 KiB) through 2²¹ (2 MiB); the default is `19` (512 KiB).
- **Integrity:** Block and frame checksums are enabled by default; pass `--no-checksum` on `encode` to omit them.
- **Dictionaries:** Pass `-D dictionary.zxd` on `encode`, `decode`, `test`, and `range`. Repeat `-D` on decode/test/range when reading concatenated frames that reference different dictionaries.
- **Output cap:** `decode` and `test` accept `--max-output BYTES` to bound total decompressed output across all frames.

> [!NOTE]
> The decoder accepts archives from upstream levels 1–7, including dictionaries, seek tables, and concatenated frames. The Rust encoder modes are independent policies, not replicas of upstream level numbers. Sequential `test` verifies payloads, checksums when present, and frame structure; it does not validate individual seek table entries.

### Parallel streaming

`encode`, `decode`, and `test` default to a single thread (running directly on the caller's thread with no worker pool). Pass `--threads` (and optionally `--slots`) for a bounded worker pipeline:

```sh
target/release/zxcide encode --threads 4 --slots 8 input.bin -o output.zxc
target/release/zxcide decode --threads 4 output.zxc -o restored.bin
```

A slot covers a block's full lifetime from admission until ordered commit; the default slot count is `min(2 * threads, 256)`. Parallel compression reuses the serial codec's match choices, so changing `--threads`, `--slots`, or `--backend` produces identical archive bytes.

The default build uses `crossbeam-channel`. To test `flume`, `kanal`, `crossfire`, or fixed-batch `rayon`:

```sh
cargo build --release -p zxcide-cli --features transports
target/release/zxcide decode --threads 4 --backend flume output.zxc -o restored.bin
```

You can cross-check any backend against serial Rust and the pinned C reference with `cargo xtask interop --parallel-backend flume`. See the [parallel transport design](doc/architecture/parallel-transports.md) and [transport benchmark recipe](doc/profiling/README.md#compare-parallel-transports).

### Indexed reads

```sh
target/release/zxcide encode --seekable input.bin -o indexed.zxc
target/release/zxcide info indexed.zxc
target/release/zxcide range indexed.zxc 4096 128 -o excerpt.bin  # plaintext bytes [4096, 4224)
```

`info` and `range` require a seekable file with a seek table in every frame:

- `info` inspects headers and footers to report per-frame metadata (`plain_bytes`, `archive_bytes`, `block_bytes`, `blocks`, `checksum`, `dict_id`) without decoding block payloads.
- `range` extracts half-open byte ranges `[start, start + length)` in concatenated **plaintext** coordinates, reading only the required seek groups and verifying touched blocks (including checksums when present). Pass `-D` for any needed dictionaries. Use `test` for whole-stream verification.

## Rust libraries

### Streaming (`zxcide`)

`zxcide` provides one-shot `encode`/`decode` helpers, reusable `Encoder`/`Decoder` workspaces to avoid per-job allocations, and `Dictionary::from_bytes` to validate `.zxd` v2 dictionaries before use:

```rust
use std::io;
use zxcide::{DecodeOptions, EncodeOptions, Mode, decode, encode};

fn main() -> io::Result<()> {
    let mut archive = Vec::new();
    let enc_opts = EncodeOptions {
        mode: Mode::Dense,
        seekable: true,
        ..Default::default()
    };
    encode(&b"hello ZXC"[..], &mut archive, enc_opts)?;

    let mut plain = Vec::new();
    decode(&archive[..], &mut plain, DecodeOptions::default())?;
    assert_eq!(plain, b"hello ZXC");
    Ok(())
}
```

### Random-access reads (`zxcide-seek`)

`zxcide_seek::Archive` exposes indexed metadata and range extraction over any `ReadAt` source (`&[u8]`, `Vec<u8>`, Unix `File`, or portable `FileSource`):

```rust
use std::io;
use zxcide_seek::{Archive, FileSource};

fn main() -> io::Result<()> {
    let archive = Archive::open(FileSource::open("indexed.zxc")?)?;
    let mut excerpt = Vec::new();
    archive.copy_range(4096..4224, &[], &mut excerpt)?;
    Ok(())
}
```

See the [indexed execution design](doc/architecture/indexed-execution.md) for the API contract and bounded parallel range plans.

### Bounded parallel streaming (`zxcide-parallel`)

```rust
use std::io;
use zxcide::EncodeOptions;
use zxcide_parallel::{Backend, Config, encode};

fn main() -> io::Result<()> {
    let config = Config {
        workers: 4,
        slots: 8,
        backend: Backend::Crossbeam,
    };
    let mut archive = Vec::new();
    encode(&b"hello parallel ZXC"[..], &mut archive, EncodeOptions::default(), config)?;
    Ok(())
}
```

### Cargo features

| Crate | Feature | Default | Purpose |
| --- | --- | :---: | --- |
| `zxcide` | `merge-simd` | off *(on in `zxcide-cli`)* | Safe runtime-dispatched SIMD (`fearless_simd`) for PivCo bitmap merge and flat unpack |
| `zxcide-cli` | `transports` | off | Enables `flume`, `kanal`, `crossfire`, and `rayon` parallel backends (also individually selectable on `zxcide-parallel` / `zxcide-cli`) |
| `zxcide`, `zxcide-seek`, `zxcide-parallel`, `zxcide-cli` | `tracy` | off | On-demand localhost [Tracy](doc/profiling/README.md#capture-phases) zones (omits `tracy-client` entirely when disabled) |

Build `zxcide-cli` with `--no-default-features` to force the scalar codec without `merge-simd`.

## WebAssembly component

Download the `zxcide-component` artifact from a successful **Codec checks** CI run, or build locally:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked wasm-tools --version 1.245.1
cargo xtask component --all
wasm-tools component wit target/component/zxcide.wasm
```

`target/component/` contains three builds sharing the same [WIT contract](crates/component/wit/codec.wit) and BSD-3-Clause license:

| File | Required Wasm features |
| --- | --- |
| `zxcide.wasm` | Component Model, scalar codec |
| `zxcide-simd128.wasm` | Component Model + SIMD128 |
| `zxcide-relaxed-simd.wasm` | Component Model + SIMD128 + relaxed SIMD |

Plain `cargo xtask component` builds the scalar component; pass `--variant simd128` or `--variant relaxed-simd` to build a single target. See [strict vs relaxed measurements](doc/profiling/pivco-merge.md#strict-vs-relaxed-wasm-components) for runtime selection guidance.

The `zxcide:codec/buffers@0.1.0` export takes and returns `list<u8>` with **no host imports**:
- `encode` accepts `mode`, `block-log` (`12..=21`), `checksum`, and `seekable`.
- `decode` accepts `max-output` (maximum cumulative plaintext bytes across frames) and returns `err(string)` on codec failure.

Both operations buffer a full job in memory and handle dictionary-free archives. See [benchmarking the component](doc/profiling/README.md#benchmark-the-component) for Wasmtime measurement and `perf` JIT profiling.

## Performance & profiling

Recent decode work (worker/scratch reuse across frames, canonical depth bucketing, x86-64 SSE4.2/AVX2 and ARM64 NEON flat-unpack kernels, and a bounded 32-output AVX2 PivCo bitmap merge) is tracked in the [decode-throughput handoff](design/decode-throughput/handoff0.sol.md) and [landed merge report](doc/profiling/bitmap-merge-landed0.sol.md).

Direct paired CLI decode measurements on identical fixed upstream C7 Silesia archives (Ryzen 7 5800X, portable Release Rust with `merge-simd` vs native Release C `5719a96f`, two randomized 24-pair series, medians `S1 / S2`, plaintext GB/s, checksums on):

| Fixed C7 input | Workers | `zxcide` (GB/s) | Pinned C (GB/s) | Notes |
| --- | ---: | ---: | ---: | --- |
| `single` (one 212 MB frame) | 1 | `1.79 / 1.80` | `1.81 / 1.80` | Pinned to 1 core; no resolved difference |
| `single` (one 212 MB frame) | 8 | `8.78 / 8.31` | `9.46 / 8.70` | Rust −7.5% / −2.9% wall; +18–20% CPU/GB |
| `concat` (12 frames, 212 MB) | 8 | `5.96 / 7.63` | `5.60 / 8.10` | No repeatable wall winner; Rust +10–15% CPU/GB |
| `mozilla` | 1 | `1.58 / 1.55` | `1.45 / 1.35` | Rust +7.8% / +7.7% |
| `sao` | 1 | `0.89 / 0.90` | `0.76 / 0.75` | Rust +18.1% / +19.4% |

Reproduce benchmarks and comparisons locally via `xtask`:

```sh
# Native Criterion benchmarks (no Wasm toolchain required)
cargo xtask bench native

# Compare native reused buffers, native one-shot, and Wasmtime component on the same archive
cargo xtask compare upstream-l7/decode

# Full Silesia fixed-archive Rust vs pinned C CLI sweep
cargo xtask comparison-report --quick --decode-workers 1,4,8 --concat
```

See the [profiling guide](doc/profiling/README.md) for Criterion baseline comparisons, Linux `perf` (native and Wasmtime JIT), byte-movement ledgers (`cargo xtask ledger`), and direct Tracy capture workflows (`cargo xtask tracy`), and [dense-decode.md](doc/profiling/dense-decode.md) for earlier same-archive comparisons.

## Compatibility checks

### Conformance & unit tests

```sh
cargo nextest run --workspace --locked
cargo test --doc --workspace --locked
sha256sum --quiet -c tests/conformance/v9/vectors.sha256
```

Install `cargo-nextest` with `cargo install --locked cargo-nextest`, or run `cargo test --workspace --locked` for both tests and doctests.

The [upstream v9 conformance corpus](https://github.com/hellobertrand/zxc/tree/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/tests/conformance) is vendored under its [BSD-3-Clause license](LICENSE). Tests verify all 18 valid archives against reference plaintext and check all 35 invalid fixtures against sequential-reader expectations (`sek_forged_entry` is expected to decode sequentially; its defect is caught by a range reader).

### Cross-architecture QEMU checks

See [Linux codec correctness under QEMU](doc/testing/linux-qemu.md) to run the ARM64 (NEON) and baseline RISC-V (`riscv64gc`) CI test matrix locally, both with and without `merge-simd`.

### Bidirectional C oracle & generated interop

```sh
cargo xtask interop
```

Runs the same cross-check as [CI](.github/workflows/check.yml): uses Git, CMake, and a C compiler to build the pinned upstream C CLI under `target/oracle/` (network required on first run), builds the Rust release CLI, compares plaintext in both directions (180 combinations), checks range reads against freshly generated C archives (72 combinations), and runs a bounded suite of [generated valid-input cases](doc/testing/generated-interop.md) covering block edges, long matches, dictionary frames, and concatenation.

```sh
# Supply prebuilt binaries directly
cargo xtask interop /path/to/c-zxc /path/to/zxcide

# Explore another generated seed or replay one case
cargo xtask interop --seed 42 --cases 24
cargo xtask interop --seed 42 --case 7
```

### Fuzzing

Optional `cargo-fuzz` targets exercise streaming decode, indexed decode, and round trips. With Rust nightly and `cargo-fuzz` installed:

```sh
cargo xtask fuzz-smoke --runs 2000
```

This seeds the decoders from reference archives and the round-trip target from plaintext, then runs each target in a temporary writable corpus with a fixed RNG seed. Failures preserve reproducers under `fuzz/artifacts/`. For extended runs, use `cargo +nightly fuzz run <target>` with a persistent corpus.

## Documentation

| Document | Contents |
| --- | --- |
| [Architecture & decisions](doc/architecture/README.md) | Compatibility target, crate boundaries, streaming contract, and peer-review decisions |
| [Indexed execution](doc/architecture/indexed-execution.md) | `zxcide-seek` `Archive` API and bounded parallel range design |
| [Parallel transports](doc/architecture/parallel-transports.md) | Bounded slot scheduling and backend evaluation |
| [Profiling guide](doc/profiling/README.md) | Criterion baselines, Silesia comparison reports, Wasmtime perf, byte ledger, and Tracy captures |
| [Decode throughput handoff](design/decode-throughput/handoff0.sol.md) | Landed SIMD/scalar decode optimizations, rejected trials, and current Rust vs C standings |
| [Generated interop](doc/testing/generated-interop.md) | Deterministic cross-codec test case generator |
| [Linux QEMU testing](doc/testing/linux-qemu.md) | Reproducing ARM64 and RISC-V correctness checks |

## License

Licensed under the [BSD 3-Clause License](LICENSE), matching upstream [hellobertrand/zxc](https://github.com/hellobertrand/zxc).
