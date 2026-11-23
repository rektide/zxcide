---
type: Measurement
title: Exact-run AVX2 unpack for width-six PivCo leaves
description: Compare a bounded 32-code shuffle on saved original C7 runs, whole literals, checksum-verified blocks, and serial CLI decode.
resource: /doc/profiling/flat-unpack6.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df8fd29afffepXs04yCk9DZsqv', title: Reviewed and landed the measured width-six patch without changing the throughput claim }
  - { resource: 'opencode://ses_df8354d7bffe7oEH9IvCGwWGfN', title: Implemented, audited and measured the width-six kernel against the shipping codec }
  - { resource: /doc/profiling/decode-gap0.sol.md, title: Attribution and original run-shape count that identify width six }
  - { resource: /doc/profiling/flat-unpack0.astra.md, title: Width-three dispatch, bounded access and replay precedent }
  - { resource: /doc/profiling/decode-compact0.sol.md, title: Prior isolated compact-table trial and serial CLI protocol }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_huffman.c#L1859-L1921', title: Upstream C AVX2 widths two through six, with lane-local table selection }
---

# Exact-run AVX2 unpack for width-six PivCo leaves

**Landed in `0d68cb82`.** The production decoder now selects this kernel on AVX2. The decision is based on the repeatable literal-section win and lower checked-block instruction count, not a demonstrated one-worker CLI gain. The rest of this report records the original scratch trial.

**Keep this kernel as a promotion candidate.** It used **82.9% fewer cycles per symbol** on the original width-six runs and **6.7–7.0% fewer cycles per symbol** in two whole-literal series on `single`. The checksum-verified block path consistently retired **2.27% fewer instructions per plaintext byte**, but its cycle intervals all crossed zero. W1 throughput rose **4.77%** in one `sao` series; on `single`, three series favored the candidate by **1.0–3.1%** but did not resolve a gain. No production files changed in this trial.

## Paired results

The `single` literal replay covers 380 sections and 34,911,224 symbols; checked block replay covers 405 blocks and 211,938,580 plaintext bytes. The saved subset covers 334 width-six runs (8,441,486 symbols in all, 456 to 75,015 per run). Cycles and instructions are per symbol for run/section replay and per plaintext byte for checked blocks. W1 measures decimal plaintext MB/s. Reported deltas are medians of within-pair ratios, not ratios of the arm medians.

| Probe | Measure | A → B, first / second series | Paired change [interval], first / second |
| --- | --- | --- | --- |
| `single` saved width-six | Cycles/symbol | 1.287 → 0.217 | −82.87% [−83.58, −82.49] |
| `single` literals | Cycles/symbol | 3.383 → 3.170 / 3.304 → 3.038 | −6.73% [−8.81, −2.35] / −6.97% [−8.37, −5.63] |
| `single` checked | Cycles/B | 2.241 → 2.089 / 2.171 → 2.136 | −1.94% [−9.54, +1.03] / −2.32% [−4.90, +1.69] |
| `single` W1 | MB/s | 1,427 → 1,507 / 1,610 → 1,622 | +3.14% [−2.39, +8.13] / +1.01% [−1.23, +4.46] |
| `mozilla` checked | Cycles/B | 2.843 → 2.722 / 2.379 → 2.345 | −3.73% [−7.31, +3.75] / −2.65% [−4.93, +1.98] |
| `mozilla` W1 | MB/s | 1,332 → 1,353 / 1,398 → 1,420 | +2.17% [−0.06, +5.16] / +1.53% [−1.43, +4.06] |
| `sao` checked | Cycles/B | 3.151 → 3.041 | −4.12% [−7.17, +4.38] |
| `sao` W1 | MB/s | 815 → 851 | +4.77% [+1.18, +7.45] |
| 4 KiB mixed checked, SIMD | Cycles/B | 12.034 → 11.275 | −3.82% [−8.77, −0.17] |
| 4 KiB mixed W1, SIMD | MB/s | 248 → 251 | +0.47% [−0.67, +1.64] |
| 4 KiB mixed checked, scalar | Cycles/B | 13.379 → 13.245 | −1.19% [−3.70, +3.31] |
| 4 KiB mixed W1, scalar | MB/s | 197 → 207 | −1.11% [−5.35, +2.86] |

Saved-run instructions fell **5.379 → 0.729 per symbol** (−86.45%); whole literals **14.535 → 13.411** (−7.74%). Checked instructions/B fell **8.546 → 8.353** on `single`, **9.400 → 9.042** on `mozilla` (−3.81%), and **12.424 → 11.738** on `sao` (−5.52%). `sao` W1 child CPU likewise fell **5.00% [−7.03, −1.42]**. On mixed 4 KiB blocks the instruction gain was only 0.13% with SIMD and zero without it; the SIMD cycle result alone does not establish a material small-block gain. No control resolved a regression. See the [full reduction](flat-unpack6-sol-data/reduction.tsv) and [raw counter, timing, and exact-output records](flat-unpack6-sol-data/pairs.json.gz).

### Targeted `single` repeat

One additional, independently ordered 24-pair series reused the exact same binaries for checked blocks and W1. One-minute load rose from 9.54 to 14.51. Checked cycles were **2.871 → 2.854 per byte**, with a paired change of **+0.15% [−4.75, +3.41]**. W1 was **1,423 → 1,440 MB/s**, paired **+1.91% [−4.45, +8.00]**; child CPU was **−1.88% [−7.39, +3.96]**. The instruction reduction again held. This repeat is not a replacement for either initial series; it leaves `single` W1 unresolved. See the [repeat reduction](flat-unpack6-sol-data/repeat.tsv) and [raw repeat records](flat-unpack6-sol-data/repeat.json.gz).

## Kernel and verification

The [trial patch](flat-unpack6-sol-data/trial.patch) changes only `entropy/flat.rs` plus differential tests. It starts at `72eaabe18d32d83586624230e2ca87189374f356` (identical codec to `3f711434`, with Huffman buckets and the width-three shuffle, not the compact-table experiment). The existing `merge-simd` feature and `fearless_simd::Level::new().as_avx2()` gate complete 32-symbol groups on x86. Runs shorter than 32, non-AVX2 tiers, non-x86, and no-feature builds retain the original scalar path. On a dispatched run the final 0–31 outputs also use that path. The wire, tree, bitmap gather, and all validation are unchanged.

The original C AVX2 kernel shuffles two sets of windows, multiplies to align them, packs the two sets of eight codes per lane, and selects among four 16-byte table chunks. It needs 28 readable bytes in the run for its 32-output loop at width six. The Rust candidate takes an exact `[u8; 24]` per group. Its two 16-byte loads start at offsets 0 and 8. The low lane keeps bytes 0–11, the high lane bytes 12–23. Within each lane, one shuffle maps triples `abc` to little-endian windows `ab`, `bc`; the two 12-bit code pairs start at bit 0 and bit 4, respectively. After aligning the pairs with a word shift and blend, a second shift moves each odd code from bit 6 to bit 8. Masks isolate the six code bits in each byte. Four lane-local table shuffles select by the low nibble; three blends select the two remaining index bits. The store writes exactly 32 bytes. No lookahead, staging buffer, unsafe code, or new dependency.

The [actual portable disassembly](flat-unpack6-sol-data/codegen.txt) shows four `vbroadcasti128` table loads outside the loop, then `vmovdqu` and a memory `vinserti128` for the two in-bounds input windows. `vpblendw` selects the shifted odd words, and the table masks compile to two word shifts and three `vpblendvb`s without separate comparisons. In total there are five `vpshufb`s per group, one unaligned 32-byte store, and just the counted-loop branch. The same loop is present in both timed CLI and replay binaries. A separate [CPU-3 profile](flat-unpack6-sol-data/dispatch-report.txt) of original B literals put 139 of 7,529 user-cycle samples (1.85%, no losses) in the AVX2 `six_groups` specialization, confirming actual dispatch.

The unit test decodes individual wire bits independently of the candidate. It varies all source/output offsets modulo 32 independently, table offset and contents, arbitrary unused final bits, lengths 0–96 and longer runs through 4,097. All three slices end at their allocations, including the exact 24-byte groups. The tree test includes two SIMD groups and all 0–31 output-tail counts, checks every canonical code, and verifies errors for truncation and trailing bytes. [Release verification](flat-unpack6-sol-data/build.log.gz): 70 workspace tests with defaults and 70 without, 36 codec tests without default features, and the focused exact-allocation test under AddressSanitizer. Formatting and Clippy passed with the previously known `new_instead_of_clear` lint allowed. The scalar CLI and replay `.text` sections are [pairwise byte-identical](flat-unpack6-sol-data/scalar-text.txt).

## Method and reproduction

The saved-run probe copies original run bytes and reconstructs the 64-entry wire-code lookup for each run using an independent canonical trie over the [original literal-section index](decode-gap0.sol.md#method-and-reproduction). It requires exact byte exhaustion for every section. Only the width-six runs are replayed: both binaries invoke their own shipping or candidate `flat::unpack` on the same bytes, one reusable output buffer, no mixed traversal. The standalone arm lacks the surrounding tree work, so its savings are not a prediction of whole-read throughput. This trial does not time the C kernel.

Both pipeline builds are locked, portable Release builds of the archive with only the trial patch overlaid for B. They use Rust 1.102.0-nightly `bdf712ee8` (LLVM 23.1.6), and neither ledger nor Tracy. The [existing replay shim](decode-replay-data/section-entry.rs) and line tables are added only after building the normal CLI. Fixed `single`, `mozilla`, and `sao` frames are checked against the [C7 fixture manifest](decode-pool-data/fixture-manifest.json). The small input is the [existing 4 KiB `mixed12.zxc` fixture](pivco-queue.md#allocation-and-retention-gates), run with both portable SIMD and no-default-feature binaries. Untimed preflight compares every output byte between arms and checks decoded plaintext hashes for whole frames and checked blocks. The independent table and altered checksum rejection checks run at replay startup. Every measured replay request folds the same fingerprint per pass; W1 CLI calls decode named files through EOF to `/dev/null`.

All timings and the dispatch capture took `.test-agent/zxc-perf.lock`, on Ryzen 7 5800X CPU 3 (SMT sibling 11 unreserved, boost on, `powersave`; Linux 7.1.0-debplus.1; perf 7.3.4). Each series uses 24 adjacent pairs per case, shuffle-balanced 12 AB and 12 BA, with warmed resident replay workers and warm CLI invocations. All 528 timed replay requests returned grouped userspace `cycles:u,instructions:u` at 100% PMU running time. CLI measurements include fresh process startup through `wait4`. The unadjusted order-statistic median intervals are 97.7% under independent pairs; pinning and pairing do not eliminate correlated load. There is no noise correction or subtraction. The saved run used 32 passes per request; literals used four; checked passes were 1/4/24/24 for `single`/`mozilla`/`sao`/mixed. The first two pipeline series started with one-minute load 7.54 and 11.35; the first series included all controls, the second only `single` and `mozilla`.

For reproduction, prepare the [original C7 fixtures](decode-pool.md#method-and-evidence) and [4 KiB mixed fixture](pivco-queue.md#timing-protocol-and-reproduction). Then run from the repository root, with a fresh scratch path:

```sh
d=doc/profiling/flat-unpack6-sol-data
s=.test-agent/flat6-repro
fix=.test-agent/decode-pool/fixtures
small=.test-agent/pivco-queue/fixtures
mkdir "$s"
bash "$d/build.sh" "$s" a b tests > "$s/build.log" 2>&1
bash "$d/build-saved.sh" "$s" > "$s/build-saved.log" 2>&1
TMPDIR="$PWD/$s/tmp" python3 -B "$d/pair.py" "$fix" "$small" "$s" "$s/pairs"
python3 -B "$d/pair.py" --summary "$s/pairs/pairs.json.gz"
perf report --stdio --show-nr-samples --percent-limit 0.5 -i "$s/pairs/dispatch.perf.data"
TMPDIR="$PWD/$s/tmp" python3 -B "$d/pair.py" "$fix" "$small" "$s" "$s/repeat" --repeat
python3 -B "$d/pair.py" --summary "$s/repeat/pairs.json.gz"
```
