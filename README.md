# zxc-rs

A native Rust streaming codec and CLI for the [ZXC v9 format](https://github.com/hellobertrand/zxc/blob/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/docs/FORMAT.md). The reference is **hellobertrand/zxc at `2992c28`**. Building the default codec does not require libzxc or a C compiler.

```sh
cargo build --release -p zxc-cli
target/release/zxc encode input.bin -o input.zxc
target/release/zxc test input.zxc
target/release/zxc decode input.zxc -o restored.bin
```

For streaming commands, omit a path or use `-` for standard I/O. Named outputs must be new files and are published only on success. For pipes, a decode error can leave previously written blocks in the output.

| Encode mode | What it writes |
| --- | --- |
| `split` (default) | Greedy LZ matches, split token/offset streams, raw literals |
| `packed` | The same match finder with fixed-width packed sequence words |
| `dense` | Split streams with size-based selection of raw, RLE, inline or dictionary PivCo literals, and raw or PivCo tokens |

All modes fall back to RAW when compression would expand the block. `--block-log 12..21` controls the block size; the default is 19 (512 KiB). Checksums are on by default. `--seekable` writes a grouped index for readers that support range access. Use `-D dictionary.zxd` on encode and decode; decode accepts repeated `-D` for concatenated frames. `decode` and `test` optionally take `--max-output BYTES` across the entire stream.

The decoder accepts archives from upstream levels 1–7, including dictionaries, seek tables and concatenated frames. These encoder modes are independent policies, not replicas of upstream level numbers. The implementation is single-threaded; dictionary training and SIMD backends are not implemented. Sequential `test` verifies payloads, checksums when present, and frame structure; it does not validate individual seek entries.

### Indexed reads

```sh
target/release/zxc encode --seekable input.bin -o indexed.zxc
target/release/zxc info indexed.zxc
target/release/zxc range indexed.zxc 4096 128 -o excerpt.bin  # bytes [4096, 4224)
```

`info` and `range` require a file with a seek table in every frame. `info` reports frame metadata without decoding. `range` uses offsets in the concatenated **plaintext** and accepts `-D` for each needed dictionary. It reads the relevant seek groups and verifies touched blocks, including their checksums when present. Use `test` for whole-stream verification. `zxc_seek::Archive` provides the same operations to Rust callers over positioned I/O; the [indexed execution design](doc/architecture/indexed-execution.md) describes the API and the planned bounded parallel driver.

## Library

`crates/codec` exports streaming functions and reusable `Encoder` / `Decoder` workspaces. Both write to caller-provided sinks. `Dictionary::from_bytes` validates `.zxd` v2 dictionaries before use.

```rust
use zxc_codec::{decode, encode};

fn main() -> std::io::Result<()> {
    let mut archive = Vec::new();
    encode(&b"hello ZXC"[..], &mut archive, Default::default())?;
    let mut plain = Vec::new();
    decode(&archive[..], &mut plain, Default::default())?;
    assert_eq!(plain, b"hello ZXC");
    Ok(())
}
```

See the [architecture and peer-review decisions](doc/architecture/README.md) and [profiling guide](doc/profiling/README.md) for the benchmark, measured baseline, and direct Tracy capture workflow.

## WebAssembly component

Download `zxc-codec-component` from a successful **Codec checks** workflow run, or build it locally:

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked wasm-tools --version 1.245.1
cargo xtask component
wasm-tools component wit target/component/zxc-codec.wasm
```

Ship `target/component/`: it contains the validated release component, its [WIT contract](crates/component/wit/codec.wit), and the license. The `zxc:codec/buffers@0.1.0` export takes and returns byte lists and requires no host imports. Encode accepts mode, block size log, checksum and seek-index options. Decode takes a maximum plaintext length in bytes; codec errors return `err(string)`. Both operations buffer a whole job in memory and support dictionary-free archives. See [benchmarking the component](doc/profiling/README.md#benchmark-the-component) to tune this API.

## Compare and tune

`cargo xtask bench native` benchmarks the Rust library directly with no Wasm toolchain. With the component prerequisites above, `cargo xtask compare upstream-l7/decode` times the same archive three ways: native with reused buffers, native with fresh buffers, and the component through its WIT export. See the [profiling guide](doc/profiling/README.md#compare-native-and-component) for baselines, workload selection and perf captures.

## Compatibility checks

```sh
cargo nextest run --workspace --locked
cargo test --doc --workspace --locked
sha256sum --quiet -c tests/conformance/v9/vectors.sha256
```

Install nextest with `cargo install --locked cargo-nextest`, or use `cargo test --workspace --locked` for both tests and doctests. For timing changes, see [comparing a named Criterion baseline](doc/profiling/README.md#compare-a-change).

The [upstream corpus](https://github.com/hellobertrand/zxc/tree/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/tests/conformance) is vendored under its [BSD-3-Clause license](LICENSE). Tests compare all 18 valid archives to reference plaintext and check all 35 invalid fixtures against sequential-reader expectations. `sek_forged_entry` is expected to decode sequentially; its defect is visible to a range reader.

For a two-way test against the C implementation, build its CLI at the pinned revision and run `cargo xtask interop /path/to/c-zxc target/release/zxc`. The [CI workflow](.github/workflows/check.yml) has the exact build commands. The task compares plaintext for every fixture input in both directions (180 combinations), and tests range reads against freshly generated C archives (72 combinations).

Optional `cargo-fuzz` targets exercise streaming decode, indexed decode and round trips. With nightly and `cargo-fuzz` installed:

```sh
mkdir -p fuzz/corpus/decode
cp tests/conformance/v9/{valid,invalid}/*.zxc fuzz/corpus/decode/
cargo +nightly fuzz run decode -- -runs=2000
cargo +nightly fuzz run roundtrip -- -runs=2000
mkdir -p fuzz/corpus/indexed
cp tests/conformance/v9/valid/*seekable*.zxc tests/conformance/v9/invalid/sek*.zxc fuzz/corpus/indexed/
cargo +nightly fuzz run indexed -- -runs=2000
```
