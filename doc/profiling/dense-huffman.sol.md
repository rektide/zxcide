---
type: Measurement
title: Dense encoding recovers Huffman sections rejected for depth
description: Root cause, wire-compatible fix, and measured size/CPU tradeoff on two Silesia inputs.
resource: /doc/profiling/dense-huffman.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#medium', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0a84f44dffeloHT4F1IaQIiih', title: Candidate traces, implementation, cross-decoding, and timings }
  - { resource: /doc/profiling/upstream-silesia.md, title: Original same-input upstream comparison and corpus hashes }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_huffman.c#L75-L302', title: Pinned C package-merge construction }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_compress.c#L1845-L1929', title: Pinned C literal and token candidate selection }
---

# Why Dense matched Split on `dickens`

The frozen Rust CLI reproduced the [original comparison](upstream-silesia.md) byte for byte, including two identical 6,103,312-byte `dickens` archives. Dense and Split share match parsing; the difference is the entropy pass over the resulting SPL literals and tokens. A scratch build instrumented Dense's section inputs, budgets, RLE sizes, and maximum unconstrained Huffman depths. Its archives matched the frozen binary. This was a candidate-construction rejection, not a pricing loss or a benchmark selecting the wrong mode:

| Input | Literal candidates before the fix | Token candidates before the fix |
| --- | --- | --- |
| `dickens` | RLE loses all 20 to raw; all 20 Huffman trees reach depth 15–17 and are rejected before pricing | No RLE; all 20 Huffman trees reach depth 15–16 and are rejected before pricing |
| `sao` | RLE loses all 14 to raw; all 14 Huffman trees reach depth 9 and beat raw | No RLE; all 14 Huffman trees reach depth 14–16 and are rejected before pricing |

For example, the first `dickens` block has 113,212 raw literal bytes, an RLE candidate of 114,077 bytes, and 63,947 raw token bytes. The first-block Huffman trees used to be rejected at depths 17 and 16; the fixed encoder emits sections of 68,502 and 39,936 bytes respectively (before each four-byte section descriptor). The first `sao` literal winner is unchanged (365,806 bytes before its descriptor); its 27,821 raw token bytes now take 11,002 bytes plus the descriptor.

Pinned C builds length-limited Huffman lengths for both streams at level 7, with the 11-bit wire cap. It then nudges lengths for PivCo run cost and compares candidates with a decode-speed penalty. The Rust fix keeps the original heap result when it fits and uses deterministic package-merge otherwise. It minimizes weighted lengths under the cap, not the exact byte-rounded run cost, so the existing serializer still prices the exact runs and makes the final choice against raw, RLE, and the shared dictionary table where available. Whole-block RAW fallback is unchanged. This changes neither the wire format nor match parsing.

## Section counts and archive size

Counts come from the emitted block/section headers, not just the entropy candidate trace. Every archive below used SPL for every block; section counts are **raw / Huffman**, without any RLE or shared-dictionary sections.

| Input | Encoder | Literals | Tokens | Archive bytes |
| --- | --- | ---: | ---: | ---: |
| `dickens` | Rust Split (unchanged) / old Dense | 20 / 0 | 20 / 0 | 6,103,312 |
| `dickens` | fixed Dense | 0 / 20 | 0 / 20 | 4,679,837 |
| `dickens` | C7 | 0 / 20 | 0 / 20 | 3,745,574 |
| `sao` | Rust Split (unchanged) | 14 / 0 | 14 / 0 | 6,527,281 |
| `sao` | old Dense | 0 / 14 | 14 / 0 | 6,386,860 |
| `sao` | fixed Dense | 0 / 14 | 0 / 14 | 6,140,199 |
| `sao` | C7 | 8 / 6 | 0 / 14 | 5,587,854 |

`dickens` saves **1,423,475 bytes (23.3%)** and is still **934,263 bytes (24.9%)** larger than C7. `sao` saves **246,661 bytes (3.9%)** and is still **552,345 bytes (9.9%)** larger than C7. Percentages use old Dense and C7 respectively as denominators.

The residual gap is not just whether Huffman was selected: `dickens` now has the same section types as C7, but the C7 parse leaves 1,617,412 literal bytes and 999,495 sequences versus Rust's 2,443,017 and 1,215,674. As stored, 453,847 bytes of the gap are in literal sections, 45,030 in token sections, and 435,386 in the remaining SPL payload (offsets, overflows and slack). On `sao`, C7 leaves 4,277,930 literal bytes against Rust's 5,112,689. Fixed Rust's stored token sections are actually 86,536 bytes smaller than C7's, but its literal sections are 729,392 bytes larger. Upstream uses a different match parser as well as different Huffman tuning; these totals do not isolate their individual effects.

## Encode cost

Same Ryzen 7 5800X and inputs as the original comparison, Linux x86-64. Frozen and fixed Rust CLI are portable release builds (rustc 1.101.0-nightly); C is the pinned CMake native Release build (GCC 16.2.0). Default 512 KiB blocks and checksums; Rust serial, C `-T1`. One warmup and 11 shuffled paired passes at CPU affinity 12–15; each command reads a warm source file and writes to `/dev/null`. Entries are median **elapsed / process CPU** milliseconds (user + system, including startup and I/O).

| Input | Rust Split | Rust old Dense | Rust fixed Dense | C7 |
| --- | ---: | ---: | ---: | ---: |
| `dickens` | 67.17 / 66.68 | 76.64 / 70.56 | 123.10 / 122.77 | 874.56 / 876.96 |
| `sao` | 52.05 / 48.52 | 138.64 / 130.06 | 135.25 / 135.10 | 337.75 / 337.66 |

Dense uses about **52 ms more CPU** on `dickens` (+74%) and **5 ms more** on `sao` (+4%). This is the total cost of building the bounded tables, pricing the candidates, and emitting the newly selected sections, not a measurement of table construction in isolation. The small opposite-direction elapsed change on `sao` is not evidence of a speedup.

## Verification

The actual C CLI built from `5719a96f778226868b29dba621a8c4e302b74d7c` decoded both fixed Dense archives to exact source bytes. Fixed Rust decoded both C7 archives and its own output to the same bytes. Fixed Rust's four-worker Dense archives matched the serial ones byte for byte; its Split archives still matched the frozen baseline. The non-corpus regression exercises both section kinds on a 2,583-byte skewed distribution whose unconstrained tree needs 15 bits; it checks full Kraft sum, symbol coverage, optimal capped weighted cost and decoding. Another case checks raw/RLE fallback. The codec, parallel and CLI nextest run passed all 31 tests with `zxc-codec/merge-simd`; codec test Clippy passed with `-D warnings`.

Using those same C and Rust binaries, `xtask interop C_CLI RUST_CLI` passed 180 frozen cross-decodes and 72 range checks, plus 24 cross-decodes and 52 range checks over 12 generated cases (revision 1, seed 99335130674745). The generated run with `--parallel-backend crossbeam` passed the same 24 cross-decodes and 52 range checks.
