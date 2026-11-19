---
type: Design
title: Native Rust ZXC codec
description: Compatibility target, codec boundaries and measurement strategy.
resource: /doc/architecture/README.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Port design and implementation }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/docs/FORMAT.md', title: Frozen version 9 specification }
---

# Native Rust ZXC codec

Target the **v9 wire format** of hellobertrand/zxc at `2992c28e6e1df07feec0cb39678a6d4a9665c6bb`. The existing upstream Rust binding calls C; this codec implements the algorithms in safe Rust. Interoperability means decoding the same plaintext, not duplicating the reference compressor's match choices or assigning its level numbers to different algorithms.

## Boundaries

- `crates/codec` owns format constants, framing, integrity, dictionary loading, entropy decoding and block coding. They are private modules: a format bump or optimization should not force a coordinated release of five packages. Public entry points transfer `Read` to `Write`, one block at a time. The block layer only sees bounded slices and a dictionary; no I/O or CLI policy. An error stops the operation; a stream sink may already hold verified blocks if a later footer fails.
- `crates/cli` owns arguments, files, pipes and profiler lifetime. Encode, decode and test (decode to sink) are the initial operations. Output paths must not clobber existing files. Diagnostics go to stderr.
- Sequential decoding supports RAW, SPL, PAK, raw/RLE/PivCo sections, checksums, dictionaries, concatenated frames, and SEK traversal. Validate sizes before allocating and never reserve the footer's claimed original size. All counters outside a block are checked 64-bit values. Inside a block a match can overlap itself; it cannot reach into the previous block.
- Encoding starts with a greedy bounded-window hash finder and raw literals, emitting PAK or SPL, falling back to RAW when a block would grow. Match finding and wire serialization are separate concerns. Further encoder levels, entropy selection, parallel workers, random access and dictionary training can be evaluated independently. No SIMD or unsafe fast path until a measured bottleneck and differential oracle justify one.

## Measurement and evidence

Use nagisa's `tracy-client` directly behind a non-default Cargo feature. Put static zones at frame/block, match-finding, entropy, reconstruction and checksum boundaries, never around individual symbols or matches. Build an optimized profile with debug symbols. Keep throughput measurement independent of Tracy and of CLI file I/O: benchmark both directions, with a ratio and uncompressed bytes as the denominator. Include deterministic compressible and high-entropy inputs; add real corpora to compare decisions. Check decoded bytes outside the timed loop.

The pinned upstream conformance corpus is the decode oracle; check Rust output with the upstream C CLI as a separate encode oracle. Corpus checks, boundary/error tests, streaming tests and interop matter more than self-roundtrips. Benchmarks should distinguish stream setup costs from steady-state codec work before motivating optimizations.
