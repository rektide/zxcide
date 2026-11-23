---
type: Measurement
title: Two-group AVX2 PivCo mixed merge
description: Bounded lane-local shuffle trial, paired checked replay and CLI results on fixed C7 archives.
resource: /doc/profiling/pivco-avx2-merge0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df935c64bffeMLcYFZfIGlqQvI', title: Implemented and audited the isolated AVX2 trial and measured the paired replay and CLI gate }
  - { resource: /doc/profiling/decode-perf0.sol.md, title: Original C7 Huffman and checked-block profiles }
  - { resource: /doc/profiling/flat-unpack0.astra.md, title: Retained three-bit flat unpack and exact-output replay method }
  - { resource: /doc/profiling/pivco-merge.md, title: Prior one-group merge kernels and skewed controls }
  - { resource: /doc/profiling/pivco-scratch.md, title: Prior rejected alternating buffer layout }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Fixed C7 archive and plaintext identities and pinned upstream C CLI }
---

# Two-group AVX2 PivCo mixed merge

**Drop this candidate.** It used 12.5% more cycles for long-frame literals and 6.5% more for checked blocks, with resolved regressions on `sao` too. The candidate never entered production; keep the existing merge and the three-bit flat shuffle. Use the next bounded decode experiment on SPL16 reconstruction: the [checked profile](decode-perf0.sol.md#one-next-candidate-3-bit-flat-unpack) attributed about half of portable Rust's checked long-block samples there. This trial did not change or measure a replacement SPL16 kernel.

The gate was a material reduction in Huffman and checked-block cycles, followed by a repeatable **at least 5% throughput** gain on eight-worker CLI decode of the long single C7 frame, exact output at every measured setting, and no repeatable `sao` or scalar regression.

## Original sections and checked blocks

Absolute columns are medians in cycles per decoded symbol for literals and tokens, and per plaintext byte for checked blocks. The deltas are medians of within-pair ratios; positive means extra work.

| Archive and replay | Rust before cycles/B | Trial cycles/B | Paired cycles Δ [interval] | Instructions/B before → trial |
| --- | ---: | ---: | ---: | ---: |
| Long C7 literals | 3.353 | 3.804 | +12.49% [+10.49, +13.17] | 14.797 → 16.459 |
| Long C7 tokens | 4.495 | 4.994 | +9.96% [+7.69, +12.44] | 18.209 → 19.786 |
| Long C7 checked blocks | 2.221 | 2.328 | +6.47% [+4.26, +9.17] | 8.644 → 9.008 |
| `sao` literals | 3.611 | 4.067 | +13.02% [+11.67, +14.71] | 16.406 → 18.230 |
| `sao` tokens | 3.306 | 3.683 | +11.30% [+9.57, +17.72] | 14.515 → 15.870 |
| `sao` checked blocks | 3.165 | 3.315 | +4.90% [+2.96, +5.78] | 12.479 → 13.029 |

The portable trial increased cycles and retired instructions in all six cases. Checked long-block thread CPU also increased by 6.10% [+4.43, +9.26]. Long checked-block cycle changes in the two chronological halves were +5.19% and +7.26%; `sao` checked blocks regressed in both halves as well (+5.65% and +4.64%). The `sao` control has just six Huffman literal sections and 14 token sections, versus 380 and 405 on the long frame.

In the no-feature control, checked-block cycles changed −2.43% [−6.13, +1.19] on the long frame and −0.13% [−5.00, +4.16] on `sao`. One scalar section contrast (long tokens, +2.94% [+0.74, +6.52]) excluded zero; neither half resolved the change, the compiled replay `.text` sections are byte-identical, and every instruction delta rounded to 0.00%. This is evidence of timing variability on an unchanged path, not a kernel regression. All 432 replay requests returned both PMU events at 100% running time.

## Fixed-archive CLI

Two independently shuffled, balanced series (S1 / S2). Throughput is decimal plaintext MB/s; positive paired throughput delta is faster. Pinned C has its own pairs with the unchanged Rust parent. Do not divide the displayed medians to reconstruct the paired ratios.

| Archive and workers | Paired binaries | Rust parent MB/s, S1 / S2 | Other MB/s, S1 / S2 | Paired throughput Δ [interval], S1 / S2 |
| --- | --- | ---: | ---: | --- |
| Long C7, W8 | Rust before → trial | 7,496 / 7,876 | 7,609 / 7,434 | −2.16% [−7.26, +2.62] / −4.17% [−10.56, +1.48] |
| Long C7, W1 | Rust before → trial | 1,517 / 1,555 | 1,484 / 1,491 | −2.95% [−4.59, −1.55] / −4.73% [−6.82, −1.50] |
| Long C7, W8 | Rust before → pinned C | 7,614 / 7,738 | 8,654 / 8,511 | +13.78% [+9.00, +21.42] / +9.60% [+4.51, +22.00] |
| `sao`, W8 | Rust before → trial | 1,321 / 1,287 | 1,291 / 1,237 | −0.07% [−10.84, +6.83] / +0.05% [−9.14, +6.94] |
| Long C7, W8 | Scalar before → trial | 5,668 / 5,768 | 5,721 / 5,731 | −1.71% [−7.71, +2.86] / +0.64% [−3.04, +4.26] |
| `sao`, W8 | Scalar before → trial | 1,221 / 1,159 | 1,164 / 1,136 | −2.02% [−11.18, +3.38] / +1.56% [−8.15, +10.99] |

The long-frame W8 intervals both exclude the required **+5% gain**, but neither establishes a wall-throughput loss by itself. W1 throughput did regress in both series. Long W8 trial child CPU rose +3.27% [+1.74, +5.82] and +3.81% [+1.72, +5.80]; pinned C instead used 19.7% and 16.8% less child CPU than Rust before. `sao` W8 does not resolve a wall change and does not contradict the replay regression. The two scalar CLI executables are byte-identical; their intervals show how much CLI noise survives pairing, not the performance of a different scalar kernel. All 11 measured CLI binary/archive/worker settings passed the exact plaintext check.

## Kernel and bounds

Both portable Rust Release builds are based on `3ce5ff5b4c2116ad7515a579feedd2a1c445d20a`, including the retained three-bit shuffle `845a76c6`. Trial code lives in a scratch export. The feature gate is unchanged; only the `Level::Avx2` proof selects a different kernel. The two 128-bit lanes of one AVX2 `vpshufb` gather one eight-output group each. Each lane uses the original mask table and puts two initialized eight-byte child windows in its input. It writes only the lower eight bytes per lane. The mixed-node scratch and copy-back, flat unpack, validation, format, and API are unchanged. AVX-512, SSE4.2, NEON, Wasm and the scalar paths keep their existing algorithm.

Let `z` be the zero count of the first mask. The second lane reads from `i+z` and `j+(8-z)`, not from `i+8` and `j+8`. For each child the first and last eight bytes of a checked range of length `advance+8` are the two windows; overlap is expected, including identical windows for zero consumption. The iteration advances by the sum of both masks, separately for each child. If the second group cannot read eight bytes from one side, processing the first group alone (if possible) leaves that child below the original SIMD threshold, so at most one old eight-output shuffle fits after the paired loop. A single full group or partial group at the output end follows this same handoff. The unchanged cursor then finishes all remaining full or partial groups, including arbitrary unused high bits of the last mask. No uninitialized byte, child overread, required wire padding or unsafe Rust was added.

The retained [differential test](../../crates/codec/src/entropy/tests.rs) constructs child streams by independently stable-partitioning expected output by individual bits. It runs all 65,536 ordered mask pairs at nonzero cursors with room for both windows, plus skewed and near-end runs across lengths 24–64. It varies input/output/bitmap offsets through 31, checks the untouched destination prefix, and ends all three slices at their allocation end.

Verification in the trial: 28 codec library tests passed in portable Release SIMD and separately with no default features. Three bitmap-related tests passed under each of the non-AVX QEMU CPU models Conroe and Nehalem, exercising the existing table and SSE4.2 paths respectively. Nine Release entropy tests passed with AddressSanitizer, including the exact-allocation test. This is not a native performance measurement for the fallback ISAs.

The actual counted trial executable emits one 256-bit `vpshufb`, one `vpermq` and one unaligned 16-byte store for the two outputs. It still emits range/chunk checks, stack spills/reloads and lane assembly before that shuffle. A separate long-literal capture sampled 5,700 of 9,561 IPs in the paired loop itself (no lost events), not just its AVX2 dispatcher or one-group suffix. Fewer shuffle instructions did not mean fewer retired instructions overall; the data do not quantify the cost of each extra instruction.

## Method and reproduction

The [existing replay](decode-replay-data/perf.py) verifies every emitted literal/token byte against the paired build and verifies checked-block plain bytes against the manifest. It uses section indexes into the original archive, includes the original table bytes, checks rejection of an invalid table or changed intermediate checksum, and checks fingerprints during timing. Each case uses balanced AB/BA orders in shuffled two-order sets, 24 rounds for portable SIMD and 12 for the no-feature control. Equal repeats per request (block/literals/tokens) are 2/5/22 for `single` and 52/95/540 for `sao`. Resident workers are pinned to CPU 3 and counted only for warmed requests with grouped `cycles:u,instructions:u`. CLI runs are separate, using the [existing CLI harness](decode-pool-data/bench.py): two series of 24 balanced AB/BA pairs each, interleaved across comparisons, with verification against exact expected plaintext lengths and SHA-256. It times fresh processes writing to `/dev/null` on CPUs 0–7. C is the original native Release CLI pinned at `5719a96f778226868b29dba621a8c4e302b74d7c` and verified by the fixture manifest, not a substitute for the Rust parent.

Both tools take `.test-agent/zxc-perf.lock`. Ryzen 7 5800X, Linux 7.1.0-debplus.1, perf 7.3.4, locked Rust 1.102.0-nightly `bdf712ee8` (LLVM 23.1.6), portable default target CPU. Rust production CLI builds omit Tracy and ledger, and replay adds only the existing section shim with line tables. Eight physical cores are available to the CLI, but neither they nor their SMT siblings are reserved. Boost was enabled, governor `powersave`, and one-minute load fell from 14.39 to 8.15 over the SIMD replay and from 7.51 to 6.61 over the CLI series. The intervals are unadjusted two-sided order-statistic intervals for medians of within-pair ratios (97.7% for 24 pairs, 96.1% for 12 under independent rounds). Shared-host correlations and contention while scheduled are not removed by pairing. Across runs, do not compare absolute wall medians as though observed simultaneously.

The [evidence bundle](decode-replay-data/pivco-avx2-merge-trial.tar.xz) contains the scratch-only candidate patch, small adaptations of the existing harnesses, raw paired measurements, section indexes, exactness and build identities, test logs and disassembly/sampling evidence. To reproduce from the repository root with the [original prepared fixtures](decode-pool.md#method-and-evidence):

```sh
RUN=.test-agent/pivco-avx2-repro
FIX=.test-agent/decode-pool/fixtures
mkdir -p "$RUN/tmp"
export TMPDIR="$PWD/$RUN/tmp"
tar -xJf doc/profiling/decode-replay-data/pivco-avx2-merge-trial.tar.xz -C "$RUN" \
  build.sh pair.py verify.sh summarize.py merge.patch
bash "$RUN/build.sh" base
bash "$RUN/build.sh" trial
flock .test-agent/zxc-perf.lock bash "$RUN/verify.sh"
python3 -B "$RUN/pair.py" perf "$FIX" "$RUN" "$RUN/counts" --cycles 12
python3 -B "$RUN/pair.py" reduce "$RUN/counts/perf.json"
python3 -B "$RUN/pair.py" perf --scalar "$FIX" "$RUN" "$RUN/scalar-counts" --cycles 6 --no-record
python3 -B "$RUN/pair.py" reduce --scalar "$RUN/scalar-counts/perf.json"
taskset -c 0-7 python3 -B "$RUN/pair.py" cli "$FIX" "$RUN/cli-base" "$RUN/cli-trial" "$RUN/cli" --pairs 24
python3 -B "$RUN/summarize.py" "$RUN"
```
