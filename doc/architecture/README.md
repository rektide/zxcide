---
type: Design
title: Native Rust ZXC codec
description: Compatibility target, codec boundaries and measurement strategy.
resource: /doc/architecture/README.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0867fb1fffeYLtmeIXhQF2Q0W', title: Dense match selection and entropy cost comparison }
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Port design and implementation }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/docs/FORMAT.md', title: Frozen version 9 specification }
  - { resource: 'opencode://ses_e0b4c34d7ffeCyFJu5o1f0HqJK', title: Sol wire compatibility research }
  - { resource: 'opencode://ses_e0b4c34d4ffeK4VV8rf1hpdri9', title: Sol profiling research and protocol smoke test }
  - { resource: 'opencode://ses_e0b4b0f91fferebvZxGETYGu0X', title: Astra review of sequential framing and output commitment }
  - { resource: 'opencode://ses_e0b4b0f8effeYzY9LUmt2THGq7', title: G review of codec scope, measurement and evidence }
  - { resource: 'opencode://ses_df0cead5cffe4OzCqZ1UFJVHJJ', title: 'Rename to zxcide and the names earlier records keep' }
  - { resource: 'opencode://ses_df0adc49effeO5nAE38QPgmBdo', title: 'Repository links moved to zxcide' }
---

# Native Rust ZXC codec

Target the **v9 wire format** of hellobertrand/zxc at `2992c28e6e1df07feec0cb39678a6d4a9665c6bb`. The existing upstream Rust binding calls C; this codec implements the algorithms in safe Rust. Interoperability means decoding the same plaintext, not duplicating the reference compressor's match choices or assigning its level numbers to different algorithms.

## Names

The project and its core crate are `zxcide`. The satellite crates are `zxcide-seek`, `zxcide-parallel`, `zxcide-cli` and `zxcide-component`, the CLI binary is `zxcide`, the WIT package is `zxcide:codec`, and the bench variables are `ZXCIDE_BENCH_*`, `ZXCIDE_COMPONENT` and `ZXCIDE_JITDUMP`.

`ZXC` as a format, `.zxc` and `.zxd` files, `libzxc`, the upstream `zxc` CLI and its `ZXC_*` switches belong to hellobertrand/zxc and keep their names.

Records written before the rename on 2026-11-24 keep the names of their revisions: project `zxc-rs`, crates `zxc-codec`, `zxc-seek`, `zxc-parallel`, `zxc-cli` and `zxc-component`, binary `zxc`, WIT package `zxc:codec`, and the `ZXC_BENCH_*`, `ZXC_COMPONENT` and `ZXC_JITDUMP` variables. That covers the model-suffixed documents under `design/` and `doc/profiling/`, the captured evidence in `doc/profiling/*-data/` and `doc/profiling/scorecard/`, measurement methods pinned to earlier revisions, and the parts of `xtask/probes/` that are copied into or build those revisions (`decode.rs`, and the `--stock-rust-only` build in `decode-tracy/build.py`).

The repositories live at `rektide/zxcide` on GitHub and `jauntywunderkind.bsky.social/zxcide` on Tangled, and links into earlier revisions follow the repositories.

## Boundaries

- `crates/codec` owns format constants, framing, integrity, dictionary loading, entropy decoding and block coding. They are private modules: a format bump or optimization should not force a coordinated release of five packages. Public entry points transfer `Read` to `Write`, one block at a time. The block layer only sees bounded slices and a dictionary; no I/O or CLI policy. An error stops the operation; a stream sink may already hold verified blocks if a later footer fails.
- `crates/cli` owns arguments, files, pipes and profiler lifetime. Encode, decode and test (decode to sink) are the initial operations. Output paths must not clobber existing files. Diagnostics go to stderr.
- Sequential decoding supports RAW, SPL, PAK, raw/RLE/PivCo sections, checksums, dictionaries, concatenated frames, and SEK traversal. Validate sizes before allocating and never reserve the footer's claimed original size. All counters outside a block are checked 64-bit values. Inside a block a match can overlap itself; it cannot reach into the previous block.
- Encoding starts with a greedy bounded-window hash finder and raw literals, emitting PAK or SPL, falling back to RAW when a block would grow. Match finding and wire serialization are separate concerns. Further encoder levels, entropy selection, parallel workers, random access and dictionary training can be evaluated independently. No SIMD or unsafe fast path until a measured bottleneck and differential oracle justify one.

## Measurement and evidence

Use nagisa's `tracy-client` directly behind a non-default Cargo feature. Put static zones at frame/block, match-finding, entropy, reconstruction and checksum boundaries, never around individual symbols or matches. Build an optimized profile with debug symbols. Keep throughput measurement independent of Tracy and of CLI file I/O: benchmark both directions, with a ratio and uncompressed bytes as the denominator. Include deterministic compressible and high-entropy inputs; add real corpora to compare decisions. Check decoded bytes outside the timed loop.

The pinned upstream conformance corpus is the decode oracle; check Rust output with the upstream C CLI as a separate encode oracle. Corpus checks, boundary/error tests, streaming tests and interop matter more than self-roundtrips. Benchmarks should distinguish stream setup costs from steady-state codec work before motivating optimizations.

`dense` searches up to 24 prior positions per hash chain and chooses matches by estimated bit cost over 32 KiB spans. It then prices actual serialized literal and token sections, including inline Huffman tables or a shared dictionary table for literals. The [Dense parser comparison](../profiling/dense-parser.md) measures the ratio, CPU and workspace tradeoff. Split and Packed use the cheaper greedy parser. [Profiling results and reproduction](../profiling/README.md) document why the initial symbol-by-symbol PivCo decoder was replaced by bottom-up merges.

## Decisions after peer review

The [indexed read and execution design](indexed-execution.md) extends the sequential design with a separate seek crate and defines the admission bound for future parallel backends.

**Streaming contract (Astra).** Fill encode blocks across short reads; decode short interior plain blocks as supplied. Reset disk/plain counters, block index, digest and dictionary on each magic. Construct the canonical footer from these counters (including its own minimal-width size), independent of the reader's prefetch. A named CLI output is a same-directory temporary file published without replacement after verification and flush. A stream cannot retract bytes.

**Seek and dictionary boundaries (both reviews).** `HAS_DICTIONARY`, not an ID's truthiness, selects a dictionary, resolved per frame from a caller-supplied set. Only its prefix and current output form match history. Literal encodings 0–3 and token encodings 0/2 share one section decoder, including shared tables without inline lengths. Require 32 post-literal bytes in both layouts regardless of the scalar copy strategy. Sequential SEK handling derives the 64-bit table size from plaintext size, validates its folded-size header and skips exactly that many bytes in bounded memory. `test` checks frame integrity, not seek entries. Seekable *writing* retains one size per block and emits grouped anchors; opt in to this memory cost.

**Measurable and repeatable (G).** Vendor the small hash-pinned corpus. Use C cross-decode as an independent encoder gate. Lock the compatible RapidHash v3 implementation, using its default-secret C seeding API for nonzero indices. Expose reusable encoder/decoder buffers in addition to one-shot stream functions. Use Criterion for throughput and direct Tracy for attribution; retain named static scopes and compile them away entirely by default. Keep the private block layer available for future range reads and parallel scheduling, but defer a public frameless/push API until there is a consumer with concrete lifetime and backpressure needs.
