---
type: Measurement
title: Decoding the same Dense archives twice as fast
description: Full Silesia CLI comparison, isolated Huffman and reconstruction changes, and remaining hotspots.
resource: /doc/profiling/dense-decode.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e084e54dbffeln5bePhwUUQIqM', title: Two-token reconstruction, fallback controls and targeted tests }
  - { resource: 'opencode://ses_e0867fb1fffeYLtmeIXhQF2Q0W', title: Huffman decode, oracle checks, corpus benchmark and final profiles }
  - { resource: 'opencode://ses_e085d94a5ffe9b48Y0zz9CuFdu', title: SPL and PAK reconstruction and differential tests }
  - { resource: /doc/profiling/dense-parser.md, title: Frozen Dense archives and upstream build }
---

# Decoding the same Dense archives twice as fast

Across all 12 frozen Dense Silesia archives, serial Rust CLI throughput went from **448 to 972 MB/s**, versus **1,782 MB/s** for pinned upstream C `-T1` on the same host. Every file decoded faster. These figures divide 211,938,580 plaintext bytes by the median of seven full pass totals (473.4 / 218.1 / 118.9 ms, including a CLI invocation per file). The remaining Rust/C elapsed ratio is **1.83×**, down from 3.98×. No recompression is needed.

| Fixed archive | Old Rust ms | New Rust ms | Pinned C ms |
|---|---:|---:|---:|
| Dense `dickens` | 30.41 | 13.01 | 6.91 |
| Dense `sao` | 30.49 | 13.35 | 7.49 |
| Dense `webster` | 97.35 | 45.36 | 18.91 |
| Upstream C3 `dickens` | 20.80 | 9.19 | 5.81 |
| Upstream C7 `sao` | 19.41 | 10.03 | 6.86 |

Dense rows are per-file medians from the [full run](scorecard/dense-decode-20261120.tsv). The upstream rows are from a separate 11-pass shuffled comparison on C-generated archives. Both runs used warm files and redirected stdout to `/dev/null`. The mostly RAW C3 `sao` control was essentially flat (Rust 2.67 → 2.62 ms).

## What changed

- **Huffman:** `perf record -F 997 -e cycles:u` attributed 75% of old `sao` cycles to `Tree::decode`. `58275e6f` decodes a pair of leaves directly from its bitmap, unpacks groups of eight fixed-width symbols from exactly their on-wire bytes, and counts complete words when sizing mixed runs. The native CLI enables the runtime-selected [SIMD merge](pivco-merge.md) by default; scalar builds remain available. With reconstruction held fixed, `sao` went from 30.6 ms to 23.2 ms with the scalar changes, then to 16.2 ms with the SIMD merge. In a separate scalar WIT measurement the Huffman change took `sao` from 50.3 to 44.1 ms (Criterion point estimates).
- **LZ reconstruction:** `f0d517ad` selects the token layout outside the loop and uses bounded 8- or 16-byte copies for common short runs. Only the logical prefix is valid output/history; a real overlap or a dictionary-boundary match still uses the checked path. With entropy held fixed, `zxc test` time fell from 30.6 to 18.2 ms on Dense `dickens`, and from 20.4 to 8.6 ms on raw-section C3 `dickens`. These isolated `test` results do not include writing plaintext to stdout.

## Reproduce

Ryzen 7 5800X, Linux x86-64, affinity 12–15. Rust CLI before: `02e59646`; after: `f0d517ad`. Both used `cargo build --locked --release -p zxc-cli`, ordinary portable flags, rustc 1.101.0-nightly (`822182db4`). C: `5719a96f`, GCC 16.2.0, native CMake Release, as in the [scorecard](scorecard/README.md). The full matrix uses one warmup and seven shuffled passes; these are elapsed CLI measurements, not isolated CPU or in-memory codec timings.

Generate each `archives/NAME.zxc` using the before CLI's `encode --mode dense` on the pinned Silesia plaintext. The probe verifies every archive hash against the frozen scorecard and each decoder's plaintext hash before timing. Python 3.11+:

```sh
taskset -c 12-15 python xtask/probes/dense_decode.py \
  doc/profiling/scorecard/dense-full-smoke-20261120.json archives \
  /path/to/old-zxc /path/to/new-zxc /path/to/c-zxc > samples.tsv
```

The combined CLI passed the frozen oracle (180 cross-decodes and 72 indexed reads), 36 generated serial cases (72 cross-decodes and 156 indexed reads) and 12 crossbeam cases (24 and 52). Focused tests cover widths 1–8, partial and truncated runs, overlap, dictionary crossings and reuse of scratch after an error.

## Next targets

1. **Existing archives:** optimize the two-byte-offset SPL sequence loop first. On final `webster`, it accounts for 64% of sampled user cycles, versus 11% in the SIMD merge. On `sao` the corresponding shares are 43% and 18%. Compare a batched common-token loop with the current checked loop, keeping overflow, dictionary and short-tail handling in the slow path. Measure both against these frozen archives.
2. **New archives:** price decode work when Huffman barely beats raw. In a scratch experiment, requiring 2% literal savings left six `sao` literal sections raw. This cost 35,538 bytes (+0.64% of the archive); on the isolated Huffman-improved decoder it cut decode time from 16.7 to 14.0 ms, and on C from 6.9 to 5.9 ms. `dickens` stayed byte-identical. This is a promising encoder policy to test across the corpus; it is not enabled in Dense.

## Follow-up: batch two ordinary SPL sequences

`58f39c8a` takes two inline tokens together when both use two-byte offsets and their matches fit entirely in already-produced plaintext. One bound covers the fixed-width stores; either token can cause the pair to fall back to the checked loop. In 15 shuffled passes using the same Dense archives, median elapsed time per corpus pass fell from **218.8 to 188.1 ms** (14%). `webster` benefited most in absolute time (45.61 → 34.94 ms). `sao` was effectively flat; extended literal lengths make fewer pairs eligible. In a 31-pass control repeat, synthetic `same-prefix` was 6% slower and 4 KiB-block `sao` was 8% slower. These regressions matter for small-block use despite the improvement at the default block size.

An [independent seven-pass check](scorecard/spl-batch-20261120.tsv) used the frozen decode probe above with `--encoder rust_dense`, `c3` and `c7`, each on its corresponding archives. Rust before → after, in milliseconds per full pass: Dense **224.3 → 196.3**, C3 **138.6 → 104.1**, C7 **206.0 → 178.4**. All 36 archive hashes and decoded plaintext hashes passed; the pinned C interop suite passed 252 cross-decodes and 228 indexed reads. The aggregate improvement was large enough to keep the pair path despite the small-block tradeoff.

## Follow-up: short match-length extensions

The [Rust/C architecture comparison](pipeline-comparison.sol.md) distinguishes sequence reconstruction from entropy and driver costs. For wide-offset SPL, an exact count on the pinned C7 files found 254,528 extended matches of length 20–64 on `mozilla` and 345,106 on `webster`. Almost every extension fits in one byte; `sao` C7 has **zero** extended matches. `50cb0699` handles the 20–64 byte case after the inline batch misses, with a short literal count, checked logical budget, nonoverlapping logical match, and space in initialized scratch for the fixed-width copies. Only a successful copy consumes an extension byte. Broadening both lanes of the ordinary pair with 32/48/64-byte stores slowed the predominantly inline C3 `webster` path; the selected variant keeps extension handling outside that pair.

On a Ryzen 7 5800X, quiet portable release binaries (one Rust worker, default features), the final C7 sweep at CPU 7 (`xtask/probes/dense_decode.py --encoder c7 --passes 25`) took **161.93 → 158.08 ms** per pass across the 12 files, `webster` **30.19 → 28.51 ms**, `mozilla` **38.62 → 37.86 ms**, `sao` **8.51 → 8.80 ms**. A separate 21-pass check alternating the two CLIs by file on CPU 12 measured C7 **176.75 → 174.44 ms**, C3 **104.11 → 101.77 ms**, and Rust Dense **193.20 → 190.02 ms**. The 21 mixed fallback controls instead took **265.82 → 271.33 ms**. Keep this as a modest targeted improvement: a prototype selection sweep had a slower full C7 pass despite lower `webster` medians. All times include CLI setup and are medians, not confidence bounds. Output matched all 36 archived reference files; 56 workspace tests and the 252 cross-decode / 228 range oracle checks passed. Raw paired samples: [25-pass C7](scorecard/spl-short-extensions-20261120.tsv).
The later seven-pass standard comparison on CPU 5, run `20261120T081851Z-43033b878064`, gives a useful counterpoint for the same binary hashes. On C7 `mozilla` and `webster`, individual medians fell from 38.335 to 37.841 ms and 30.325 to 29.475 ms, respectively, but the median of the two-file pass totals **rose from 68.553 to 70.110 ms**. The pass-total median is not the sum of the per-file medians. The targeted run did not reproduce an aggregate speedup.
