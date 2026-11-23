---
type: Measurement
title: Where Rust loses cycles to C on the long C7 frame
description: One-core counters, run-shape census, and leaf-IP attribution separating mixed gather, flat unpack, copy-back and match reconstruction.
resource: /doc/profiling/decode-gap0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df8fd29afffepXs04yCk9DZsqv', title: Reused the clean replay builds, measured the residual Rust–C cost, and reconsidered the compact-table result }
  - { resource: /doc/profiling/decode-compact0.sol.md, title: Prior compact-table trial, build identities and serial results }
  - { resource: /doc/profiling/decode-perf0.sol.md, title: Earlier checked-block and section profiles }
  - { resource: /doc/profiling/pivco-avx2-merge0.sol.md, title: Rejected two-group merge and its exact-end constraints }
  - { resource: /doc/profiling/pivco-scratch.md, title: Earlier copy-back elimination without a repeatable serial win }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_huffman.c', title: C PivCo tree, unpack and merge implementation }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_decompress.c', title: C SPL bulk and tail loops }
---

# Where Rust loses cycles to C on the long C7 frame

**Mixed gather and flat unpack account for roughly 82% of the excess Rust literal-section cycles on `single`.** Copy-back accounts for about 8%; table setup about 8%. Across checksum-verified blocks (including token decoding), **match reconstruction is the biggest individual difference**, but entropy stages together account for more. These are sampled estimates from the actual portable Rust and AVX2 C paths, not savings a prototype is guaranteed to realize.

## Fresh one-core comparison

Original C7 `single`: 380 inline Huffman literal sections with 34,911,224 symbols, 405 checked blocks with 211,938,580 plaintext bytes. Same three replay executables as the [compact-table trial](decode-compact0.sol.md#one-representation-on-the-retained-bucket-baseline). Rust A is the shipping decoder at codec pin `3f711434`; B is the scratch-only compact-table patch; C is upstream `5719a96f`. C uses its normal padded scratch and runtime-dispatched block or AVX2 section kernel. Both deliver identical logical bytes, but C does not enforce the same exact wire-run consumption as Rust. Replay excludes file input, pool and output sink from counts.

| Probe | Measure | Rust | Compact trial | C |
| --- | --- | ---: | ---: | ---: |
| Literals | Cycles / symbol | 4.517 | 4.165 | 2.009 |
| Literals | Instructions / symbol | 14.535 | 13.825 | 5.584 |
| Checked blocks | Cycles / plaintext B | 2.952 | 2.821 | 1.937 |
| Checked blocks | Instructions / plaintext B | 8.546 | 8.260 | 4.207 |

Versus shipping Rust, the median within-round change was **−57.3% [−58.3, −54.5]** literal cycles and **−34.8% [−36.0, −33.4]** checked cycles for C. Compact tables gave **−9.2% [−12.0, −5.9]** and **−4.9% [−6.2, −2.7]** respectively; thread CPU changes were **−9.0% [−11.1, −5.2]** and **−4.8% [−5.9, −3.0]**. This diagnostic round did not time the CLI and does not replace the earlier CLI promotion series. The host was busier than in some earlier runs; compare ratios and instructions in these rounds, not absolute cycles from different sessions.

## Attribution: distinguish traffic from hot loops

For each build and probe, multiply each category's exclusive share of sampled user-cycle IPs by that build's median cycles/unit from the clean counter rounds. This is a scale estimate from a separate capture, not a per-stage stopwatch or an interval for individual rows. `—` means no distinct C copy-back stage.

| Phase | Literal cycles/symbol: Rust / compact / C | Checked cycles/byte: Rust / compact / C | Rust excess on checked blocks |
| --- | ---: | ---: | ---: |
| Mixed bitmap gather | 2.517 / 2.471 / 1.312 | 0.569 / 0.566 / 0.302 | +0.267 |
| Flat unpack and C leaf-pair emission | 1.206 / 1.199 / 0.345 | 0.283 / 0.287 / 0.085 | +0.198 |
| Copy gathered bytes back | 0.197 / 0.207 / — | 0.049 / 0.046 / — | +0.049 |
| Bitmap population count | 0.152 / 0.156 / 0.186 | 0.038 / 0.039 / 0.047 | −0.009 |
| Table setup | 0.308 / 0.050 / 0.096 | 0.141 / 0.016 / 0.035 | +0.106 |
| SPL match reconstruction and length extras | — | 1.658 / 1.659 / 1.268 | +0.390 |
| Checksum | — | 0.156 / 0.176 / 0.173 | −0.016 |

The literal gap is about **2.51 cycles/symbol**: gather adds **1.20**, unpack **0.86**, explicit copy-back **0.20** and setup **0.21**. The compact tables leave the gather and unpack costs essentially where they were. A fixed cost per table can still dominate when sections are tiny. The checked-block gap is **1.02 cycles/byte**; the reconstruction estimate is **38%** of it. The listed entropy categories together explain about **60%**, without extrapolating the literal-only probe to the rest of the block. Other work is not omitted from the sample denominators. Sampling skid, inlining, cache effects that move into another phase, and imperfect line tables limit this split; it cannot price the effect of a rewrite.

### What to try next

1. **Flat unpack is the cleanest kernel trial.** On the long literals, 6-bit flat runs are **8.44M symbols (24%)** and account for **35% of Rust's flat-unpack samples**, more than any other width. All 334 width-six runs contain at least 128 symbols. Width five is the next-largest sampled flat cost (21%, 4.97M symbols). Rust still uses a scalar indexed lookup per symbol at those widths; [C AVX2](https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_huffman.c#L1859-L1921) extracts and maps 32 codes per iteration using lane-local shuffles and blends, with an exact-input guard. Replay the saved width-six runs first against scalar Rust and the C path with exact bytes. Try width six on its own with bounded SIMD loads and the existing tail; then check whole literals, blocks and W1. This is a tractable incremental win, not large enough alone to close the gap.
2. **Mixed gather is the largest entropy opportunity.** The census finds **92.74M** symbols moving through 4,157 mixed nodes (2.66 gather bytes per decoded literal); nearly all of that volume is in runs ≥128 symbols. [Rust](../../crates/codec/src/entropy/merge.rs) gathers eight outputs at a time and checks both child windows; [C AVX2](https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_huffman.c#L1439-L1474) uses two shuffles and a blend for 32 outputs and a different scratch contract. Use original masks and child streams to compare the real per-run kernels and inspect the hot loop's retired instructions. Seek fewer checks or less lane assembly in a bounded bulk loop with an exact suffix. Do not retry the [rejected pairing](pivco-avx2-merge0.sol.md), assume C's readable slack, or bundle a copy-back layout change; [eliminating copy-back alone already failed](pivco-scratch.md).
3. **Reconstruction merits a separate controlled trial.** C amortizes checks across a wider interior and stops checking history underflow once the produced prefix covers all representable offsets; its bulk copies use a padded contract and its tail differs. Rust's `[literal, match]` fast pair falls back in more classes. Census fallbacks and sample the costly ones on `single`, `mozilla`, and extension-free `sao` before selecting a change. Compare a proven interior bounds hoist with the existing checked terminal loop before adding special cases for rare match lengths or distances. No weaker validation or new input padding is implied.

### Compact tables are a real smaller win

The old 5% rule was a trial target, not a rule that makes smaller wins disappear. The [earlier independent CLI pairs](decode-compact0.sol.md#paired-rust-results) put W1 throughput **+5.15% / +3.42%** on `single`, **+4.51% / +4.60%** on `mozilla`, and **+4.92%** on the targeted `single` repeat. That repeat did not resolve checked-block cycles, but all checked runs retired **3.35% fewer instructions/byte**. Native scalar throughput on the one entropy-heavy 4 KiB control improved **68–71%**. Treat B as a candidate for a modest serial win, not a solution to the Rust–C gap. Before adopting a second representation in the decoder, confirm whole-call Wasm and broaden the 4 KiB controls to non-Huffman and sparse-table inputs; the earlier trial skipped Wasm. Keep the compact change separate from kernel work so its gain and regression risk remain attributable. Nothing new ships in this diagnostic pass.

## Method and reproduction

Ryzen 7 5800X, Linux `7.1.0-debplus.1`, perf `7.3.4`, logical CPU 3 with sibling 11 unreserved; boost on, governor `powersave`. One-minute load rose from 11.38 to 24.92 over the run. One diagnostic series per probe, four shuffles of all six A/B/C orders (24 adjacent rounds); separate startup and request warmups on each of three resident processes. All 144 counter requests used grouped `cycles:u,instructions:u` at 100% PMU running time. The intervals are unadjusted 97.7% order-statistic intervals assuming independent rounds, and there is no new same-A A/A in this diagnostic figure. All three full literal outputs compared byte for byte, and checked plaintext matched the manifest. The replay shims separately rejected the invalid table or flipped block checksum, and fingerprinted each timed pass.

Separate fixed-period `cycles:u` captures (500,000, DWARF stack size 1024) yielded 20,566 / 18,680 / 9,214 literal samples and 15,088 / 14,220 / 9,052 checked samples, with no reported lost records. The classifier uses the leaf IP and inline frames, with callers for sampled libc sites (including copy-back `memcpy` call sites confirmed in disassembly). For C match reconstruction it uses the source interval after entropy setup in `zxc_decode_block_spl_impl`, including inlined copy/extras helpers. Rust width attributions use the monomorphized `groups` names plus `three_groups`; C's shared AVX2 IPs cannot distinguish most widths. Unassigned and residual classes are retained in the [evidence bundle](decode-replay-data/decode-gap0-sol.tar.xz), with raw counters and sample exports. The independent run-shape census walks canonical runs in wire order and requires exact section exhaustion; its results are counts, not inferred from PMU samples.

```sh
s=.test-agent/decode-attribution-repro
b=.test-agent/decode-compact-sol/build # or rebuild via decode-compact0.sol.md
z=.test-agent/decode-pool/fixtures/single.zxc
mkdir -p "$s"
tar -xJf doc/profiling/decode-replay-data/decode-gap0-sol.tar.xz -C "$s" capture.py attribute.py shapes.py
python3 -B "$s/capture.py" "$b" "$z" "$s/run"
for probe in lit block; do
  for arm in a b c; do
    f="$s/run/$probe-$arm"
    perf script --show-lost-events --inline -i "$f.perf.data" -F period,ip,sym,dso,dsoff,srcline > "$f.samples"
  done
done
python3 -B "$s/attribute.py" "$s/run"
python3 -B "$s/shapes.py" "$z" "$s/run/single-lit.idx" "$s/run/single-tok.idx"
```
