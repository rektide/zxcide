---
type: Measurement
title: Reproducible Silesia scorecard
description: CLI operating points and single-thread buffer decode on a pinned corpus and frozen binaries.
resource: /doc/profiling/scorecard/README.md
status: draft
generated: { by: 'model:openai/gpt-6.1-terra#high', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0a84f44affeaEMU9gc5TU02Bd', title: Scorecard implementation and frozen snapshot measurements }
  - { resource: /doc/profiling/upstream-silesia.md, title: Earlier measurements and profile }
  - { resource: 'https://github.com/DataCompression/corpus-collection/tree/d59143a41cc7d65063a70e86063a3c39aeae6d9f/Silesia-Corpus', title: Pinned corpus source }
---

# Silesia scorecard

`cargo xtask scorecard` runs the CLI matrix against explicitly supplied binaries; it **never builds or chooses the working tree**. Python 3's standard library is required. Download is cached and the entire ZIP must match SHA-256 `1b39576418fa3ebb09cf0ceab59bf1cb9fa262c19d9dc6d3cda1e4fae7910a09`; corrupt cached files fail closed, not silently replaced. ZIP member names are fixed and the program hashes each extracted plaintext and generated archive. Download is from the pinned revision in the source metadata. Do not substitute another Silesia ZIP without deliberately updating the pin.

```sh
cargo xtask scorecard -- --help
cargo xtask scorecard -- \
  --c-cli /absolute/path/to/c-zxc --rust-cli /absolute/path/to/zxcide \
  --c-build 'C revision, compiler and flags' \
  --rust-build 'Rust revision, compiler, features and flags' \
  --output target/scorecard/subset.json
# Add --full for all 12 members, --samples N and --warmups N as appropriate.
```

The descriptions are operator assertions, not derived from binary hashes; use known builds and retain build logs. JSON records paths, hash, descriptions, input sizes/hashes, archive sizes/hashes, commands, sample order within each series, CPU, inherited affinity, timestamp and run parameters. Temporary input/archive paths in command arrays describe the actual run but are removed after it; reconstruct them by rerunning. The seed shuffles operating points together per pass; each command is warmed once by default. Wall-clock samples include process startup, warm filesystem I/O, worker/streaming overhead and writing to `/dev/null`; they are not codec-only measurements. Correctness decoding and exact byte comparison happen before timing, on the same archive later supplied to both decoders, for archives from **all four encoders**. Timed output is discarded and isn't separately checked. All failures abort without publishing a complete result.

## Frozen baseline: subset

[Raw JSON, all samples and metadata](subset-20261119.json). 2026-11-19, AMD Ryzen 7 5800X, Linux x86-64, seven samples following one warmup, shuffled with seed 42. C revision `5719a96f778226868b29dba621a8c4e302b74d7c`, GCC 16.2.0, CMake Release `-O3 -DNDEBUG -march=native`; Rust revision `7dbfd7277255`, rustc 1.101.0-nightly `822182db4`, Cargo release, ordinary portable target flags and `zxc-codec/merge-simd`. SHA-256 binary hashes and affinity are recorded in JSON. These existing frozen binaries are the ones from the [earlier study](../upstream-silesia.md), not a live tree with ongoing dense changes. C's native ISA versus Rust's portable build is an important limitation. Operating points aren't equivalent algorithms: C `-T1 -3` / `-7` has a streaming driver and worker, Rust serial `split` / `dense`. CLI checksums are enabled.

Archive/plain is compressed size divided by original size; lower is better. Median milliseconds; raw samples are in JSON, not confidence intervals. Decode columns each use the **same archive in that row**.

| Input | Archive | Bytes | Archive/plain | Encode ms | C decode ms | Rust decode ms |
|---|---|---:|---:|---:|---:|---:|
| dickens | C3 | 5,831,415 | .5721 | 80.22 | 5.84 | 21.16 |
| dickens | C7 | 3,745,574 | .3675 | 855.06 | 7.14 | 28.90 |
| dickens | Rust split | 6,103,312 | .5988 | 65.57 | 6.66 | 23.23 |
| dickens | Rust dense | 6,103,312 | .5988 | 69.95 | 6.37 | 23.54 |
| sao | C3 | 7,247,411 | .9994 | 6.02 | 5.72 | 2.68 |
| sao | C7 | 5,587,854 | .7705 | 335.63 | 6.44 | 16.58 |
| sao | Rust split | 6,527,281 | .9001 | 48.82 | 5.84 | 9.41 |
| sao | Rust dense | 6,386,860 | .8807 | 130.16 | 7.46 | 25.25 |

The medians do not establish stable small differences: the system was not isolated, frequency and competing load were uncontrolled, and seven measurements are too few for strong tail claims. The raw distributions show the observed spread. Archive size and same-archive decode are more directly comparable than similarly named encode modes.

## Full-corpus smoke

A separate `--full --samples 1 --warmups 0` run exercises every member and the entire encode/cross-decode matrix. Its single samples are execution evidence, **not** a benchmark estimate. See [full run JSON](full-smoke-20261119.json).

## Single-thread buffer API probe

The independent probes in `xtask/probes/` decode a preloaded identical archive into a preallocated buffer, validate exact output outside timing, and print raw in-process samples. They isolate decoding from CLI startup, file I/O and C's streaming driver. They do **not** measure encode. The C one-shot `zxc_decompress` does not verify checksums, whereas Rust's default decode does; both are one-shot APIs with internal setup, not equivalent reusable-context costs. Avoid treating the API comparison as a direct decomposition of the CLI times. Build Rust from the frozen `7dbfd7277255` source, not the current tree, and C against the pinned static library; the probes themselves are development-only.

The same frozen C3/C7 archives from the baseline were compared on each input. Seven consecutive samples per process after a warmup; C always ran before Rust, so drift/order bias is not controlled. [Raw API samples and checksums](api-20261119.json):

| Archive | C median ms (min–max) | Rust median ms (min–max) |
|---|---:|---:|
| dickens C3 | 2.51 (2.44–2.76) | 19.39 (19.31–19.72) |
| dickens C7 | 3.19 (3.12–3.47) | 26.45 (26.37–26.99) |
| sao C3 | 0.19 (0.18–0.60) | 0.98 (0.95–1.23) |
| sao C7 | 2.78 (2.76–3.22) | 14.65 (14.44–14.77) |

Each interval is the observed range, not a confidence interval. Rust's source was exported from `7dbfd7277255` using `git archive`; its probe was built with `cargo build --locked --release -p zxc-codec --example scorecard_decode --features merge-simd`, rustc as above, portable target. C was linked against the above frozen `libzxc.a` using GCC 16.2.0, `-O3 -march=native -std=c17 -lm -pthread`. These are not the CLI binary hashes; probe binary hashes are in the API JSON. The library was not changed between the CLI and probe builds.

Rebuild and run against known source and a matching archive/plaintext pair (paths are examples, `baseline` must be a fresh directory):

```sh
mkdir -p target/scorecard/baseline
git archive 7dbfd7277255 | tar -x -C target/scorecard/baseline
mkdir -p target/scorecard/baseline/crates/codec/examples
cp xtask/probes/decode.rs target/scorecard/baseline/crates/codec/examples/scorecard_decode.rs
(cd target/scorecard/baseline && cargo build --locked --release -p zxc-codec --example scorecard_decode --features merge-simd)
cc -O3 -march=native -std=c17 -I target/upstream-comparison/current/include \
  xtask/probes/decode.c target/upstream-comparison/build/libzxc.a -lm -pthread -o target/scorecard/decode-c
target/scorecard/decode-c target/upstream-comparison/dickens.c3.zxc target/upstream-comparison/dickens 7
target/scorecard/baseline/target/release/examples/scorecard_decode target/upstream-comparison/dickens.c3.zxc target/upstream-comparison/dickens 7
```
