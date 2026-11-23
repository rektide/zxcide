---
type: Measurement
title: Compact inline PivCo Huffman decode tables
description: One depth-indexed decode representation on the original C7 sections, checked block replay and serial CLI, with fresh upstream C replay on one core.
resource: /doc/profiling/decode-compact0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#xhigh', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df8551f57ffeVYwh6JpXT5BA5O', title: Built and verified the isolated compact decoder and ran the paired promotion measurements }
  - { resource: /doc/profiling/decode-next0.sol.md, title: Retained Huffman length buckets; SPL16 copy-width trial rejected }
  - { resource: /doc/profiling/decode-positioned0.sol.md, title: Separate positioned reader trial stayed out of production }
  - { resource: /doc/profiling/decode-perf0.sol.md, title: Fixed-section indexes and pre-bucket attribution }
  - { resource: /doc/profiling/decode-replay0.sol.md, title: Checked replay and per-implementation output contracts }
  - { resource: /doc/profiling/pipeline-comparison.sol.md, title: C and Rust codec stage boundaries }
  - { resource: /doc/profiling/pivco-queue.md, title: Prior FIFO reuse trial, small entropy and dictionary fixtures }
  - { resource: /doc/profiling/pivco-scratch.md, title: Prior alternating-buffer trial, not repeated here }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_huffman.c', title: Pinned C level tables, code-to-symbol permutation and count passes }
---

# Compact inline PivCo Huffman decode tables

**Reject this representation for production on the C7 gate.** It cuts cycles on the original `single` literals by **7.4% / 8.5%** and tokens by **19.9% / 21.0%**. But the second series falls below both 5% whole-decode targets: **3.5%** fewer cycles per checked plaintext byte and **3.4%** higher serial throughput. The targeted repeat also misses, despite saving the same instructions. The native scalar 4 KiB entropy control benefits substantially, without changing this decision. No production diff; the retained buckets remain in place.

## One representation, on the retained bucket baseline

Rust A is a clean `git archive` of `3f711434ad83b85fa191a209041777c457c6972b`. The separate positioned-read commits through `88ea5bc0` do not change `crates/` or `tests/`. Rust B applies only the [trial patch](decode-compact-sol-data/trial.patch), in scratch. This is an **inline mode 2 decode-only** experiment. The encoder's `Tree::new` and `Tree::emit` still use the retained bucketing and heap tree; mode 3 still uses the original cached dictionary tree and lookup. Neither test vectors nor the wire format were changed. No positioned reads, scheduler, reconstruction, SPL16, new bit kernels, buffer layout or producer changes were included.

After reading the same nibble lengths and checking 11-bit depth and the complete Kraft sum (with the one-symbol exception), B constructs arrays of per-depth first prefixes and leaf/internal bases. Canonical leaves come before the internal prefixes of each depth. Internal rank `i` therefore has children at ranks `2i` and `2i + 1` at the next depth, before removing that next level's leaf prefix. Full trees with at most 256 leaves have at most 255 internal slots. Bottom-up equality of child heights finds perfect subtrees, including pairs. At a visited perfect subtree, its leaves occupy one contiguous interval at one depth; reversing each code's bits gives the original flat lookup order. There are no per-tree or per-flat heap allocations on the inline path. A singleton still consumes its bitmap, ignores padding, and rejects a selection of the nonexistent right child.

B consumes nonempty runs in the same depth/left-to-right order, keeps the same disjoint intervals, and fills leaves or calls the existing `flat::unpack`. One fixed stack array covers up to 255 internal slots, with four `u32` coordinates per slot (4,080 B); validated section and symbol extents are bounded by the 2 MiB format block limit. A mixed bitmap records its left population and source offset in its slot. Empty/covered slots stay unused, uniform ancestors keep both children in the same output buffer and never gather, and the reverse slot walk invokes the same gather/copy-back only on mixed runs. B trades the heap nodes, lookup allocations, FIFO and reverse-order Vec for bounded stack setup/scans; it is not a promise that every section uses fewer bytes than before. `bitmap_ones` and `merge_back` are shared inline helpers extracted verbatim in behavior, including the scalar Wasm cursor, tail masking, and ledger byte counts. Run reads are still checked and unused bytes still reject before any block is published; neither path changes output growth, checksum checks or scratch ownership. Unlike C, B does not alternate destination and scratch by depth.

## Gate and scope

- **Promotion:** require a material reduction on `single` literals (use a 5% paired median screen and intervals below zero); tokens are secondary. On `single` checked blocks require at least 5% fewer cycles/plaintext byte, and on the serial W1 CLI at least 5% more plaintext MB/s, in each of two series with both intervals on the favorable side of zero. A +5% throughput change means −4.76% elapsed, not −5%. Check instructions/thread CPU and child CPU respectively. A repeatable control cost exceeding 3% with positive intervals blocks promotion; noisy non-regression does not establish equivalence. A native/Wasm component check is conditional on advancing. At most one quieter targeted repeat may resolve an otherwise viable but uncertain decision.
- **Inputs:** hash-checked `single.zxc` is the independently encoded original 405-block C7 frame, not concat. `mozilla` and `sao` are fixed member frames. Section-only calls use views of the original SPL16 inline tables and runs. The native `--no-default-features` scalar control is the fixed 1 MiB `mixed12.zxc` fixture with 256 checksummed 4 KiB SPL8 blocks and inline-Huffman literals, from the [queue study](pivco-queue.md#allocation-and-retention-gates); not the short, uncoded SPL8 checksum vector. All inputs retain their exact original compressed bytes.
- **Pairing:** Ryzen 7 5800X, Linux `7.1.0-debplus.1`, perf `7.3.4`. CPU 3 pinned; SMT sibling 11 is not reserved. Boost on, `powersave` governor. Two seeds (`713531`, `396833`), each with 24 adjacent pairs, balanced AB/BA separately for each contrast, shuffled and interleaved with a fresh same-A control. C has a separate A/C contrast, also balanced, and is not the Rust B or an A/A correction. Each replay worker starts fresh per case/series and performs a startup pass and a separate warmup; each CLI binary has a separate warm launch. The repository's `.test-agent/zxc-perf.lock` covers file verification, warmups and both series. Files are prefaulted, not necessarily LLC-resident. No sample from a previous report is included.
- **Timed boundaries:** replay counters are grouped userspace `cycles:u,instructions:u` attached to one resident thread and must run 100%; clocks inside that thread cover only the repeat loop. Both kernels reconstruct the same logical output into private reused buffers, verifying each block checksum and folding the same first/middle/last byte and length fingerprint on every pass. C uses its normal AVX2 entry points and padded physical scratch; Rust uses initialized `Vec` scratch and retains strict consumption. This matches the logical output contract, not the physical memory layout or all malformed-input policy. The full CLIs verify through footer and EOF before timing. W1 Rust decodes named files into `/dev/null`; elapsed is pinned child launch through `wait4`, CPU is child user+system. C CLI is exact-output preflight only; fresh C **timing** is from resident replay, not from the threaded C file driver.
- **Statistics:** per-series absolute medians and medians of within-pair ratios are reported separately. Cycles and instructions divide by decoded symbols for sections and plaintext bytes for checked blocks. CPU seconds use decimal output GB. Use the unadjusted two-sided 97.7% order-statistic median interval at 24 independent pairs; host load can be serially correlated. A/A is a noise control, not something to subtract. For replay and CPU costs negative is better; for throughput positive is better.

## Verification

The [independent malformed and differential tests](decode-compact-sol-data/tests.patch) leave A's original tests intact. Two additional tests give a literal wire example with nested merges under a uniform ancestor and deliberately set padding, then check short/trailing runs, zero count, singleton selection, incomplete/oversubscribed tables, and both high and low invalid depth nibbles. A seeded differential test checks 600 valid shapes (including all eight flat widths and random leaf depths up to 11) against the original tree's emitted bytes, then compares exact outputs or error kind/message for truncated, trailing, and bit-flipped runs. The same output and merge Vecs are reused across successes and failures. The existing block tests also cover a single worker alternating dictionary/inline entropy, dictionary crossings, output limits, and checksums.

Release nextest passed **71 workspace / 71 workspace without default features / 37 codec without default features**. Workspace fmt and Clippy passed; the Clippy command suppresses the already-known `new_instead_of_clear` diagnostic on deliberate scratch release. The [build and tests log](decode-compact-sol-data/build.log.gz) records the commands, binary hashes and toolchain (Rust 1.102.0-nightly `bdf712ee8`, LLVM 23.1.6; C GCC 16.2.0). The [per-ISA C compile commands](decode-compact-sol-data/c-compile.json.gz) identify the reference build. Rust uses locked portable Release with default merge SIMD for the primary cases and no default features for the small scalar control. Replay-only builds carry line tables and the original probe shim; the standalone CLIs do not. No ledger or Tracy flags were used for timing.

Outside the paired runs, the [CLI conformance preflight](decode-compact-sol-data/verify.json.gz) records 236 invocations (59 cases × A/B and their scalar variants). All 18 valid vectors produced the exact bytes; all 35 invalid-category vectors agreed in status, stderr and committed stdout (the existing expected success is `sek_forged_entry`). This includes dictionary fixture vectors with the three bundled dictionaries supplied, including ID zero. A/B also matched plaintext for one and 256 blocks with dictionary-coded literals from the queue study, rejected both streams without the dictionary, and emitted byte-identical Dense encodings with and without a dictionary. These are parity checks, not measurements of dictionary throughput.

## Paired Rust results

Cycles/unit for replay; decimal plaintext MB/s for W1. S1 and S2 are independent. The absolute entries are independent arm medians within the A/B contrast, not the calculation of the paired percentage. Costs with a minus sign and throughput with a plus sign favor B.

| Probe | Absolute A → B, S1 / S2 | Paired change [interval], S1 | Paired change [interval], S2 |
| --- | --- | ---: | ---: |
| `single` literals (380 sections; 34,911,224 symbols) | 3.269 → 3.051 / 3.535 → 3.259 | −7.43% [−9.48, −5.28] | −8.46% [−11.79, −5.24] |
| `single` tokens (405 sections; 12,132,680 symbols) | 4.612 → 3.522 / 4.076 → 3.278 | −19.91% [−24.64, −16.30] | −20.97% [−22.72, −17.87] |
| `single` checked (405 blocks; 211,938,580 B) | 2.096 → 1.995 / 2.042 → 1.978 | −5.28% [−7.10, −0.97] | **−3.54% [−5.20, −2.77]** |
| `single` serial W1 | 1,621 → 1,705 / 1,602 → 1,640 | +5.15% [+3.15, +7.28] | **+3.42% [+0.25, +7.12]** |
| `mozilla` checked | 2.392 → 2.308 / 2.409 → 2.317 | −3.84% [−9.65, −0.77] | −5.24% [−8.61, −1.02] |
| `mozilla` W1 | 1,393 → 1,459 / 1,366 → 1,435 | +4.51% [+3.09, +5.09] | +4.60% [+2.91, +5.42] |
| `sao` checked | 4.016 → 3.913 / 3.100 → 2.982 | −1.84% [−10.87, +2.46] | −2.45% [−5.61, −1.26] |
| `sao` W1 | 662 → 690 / 910 → 931 | +2.73% [−7.29, +7.80] | +1.99% [+0.50, +2.90] |
| 4 KiB mixed checked, scalar | 12.950 → 6.305 / 14.982 → 6.151 | −55.14% [−55.86, −52.56] | −57.95% [−60.22, −55.89] |
| 4 KiB mixed W1, scalar | 138 → 231 / 216 → 364 | +67.94% [+65.69, +72.65] | +70.60% [+67.96, +73.39] |

The retired-instruction savings reproduce: literals **14.535 → 13.825** instructions/symbol (−4.89%), tokens **17.143 → 14.194** (−17.20%), checked single **8.546 → 8.260** instructions/plaintext byte (−3.35%), and scalar mixed **47.792 → 19.827** per plaintext byte (−58.51%). The small-block result removes setup work on 256 inline sections per MiB; it is specific to this entropy-heavy small-block control, not a proxy for C7, all SPL8 inputs, or WIT. No control had a repeatable meaningful regression.

Single checked thread CPU was **0.556 → 0.530 / 0.543 → 0.525 s/GB**, paired **−5.18% [−7.15, −0.77] / −3.42% [−5.45, −2.70]**. W1 child CPU was **0.616 → 0.586 / 0.623 → 0.609 s/GB**, paired **−4.90% [−6.79, −3.09] / −3.30% [−6.66, −0.25]**. Same-A checked cycle changes were **+0.44% [−11.68, +3.99] / +0.19% [−2.26, +2.19]**; same-A W1 throughput **+1.09% [−2.46, +3.30] / −0.22% [−6.26, +5.05]**. One-minute load at the timing boundaries fell from **18.67 → 10.11 → 5.57**, without establishing a reserved core. Thread CPU follows cycles, but neither rules out contention while scheduled. See [all absolute and paired metrics](decode-compact-sol-data/reduction.tsv), [raw measurements and completed exact-output checks](decode-compact-sol-data/pairs.json.gz), and [section indexes](decode-compact-sol-data/sections.tar.xz). All 2,784 timed replay requests across the initial series returned both nonzero PMU counts with 100% reported running time.

### One targeted repeat: still no pass

S2's intervals still touched both promotion thresholds, so the repeat used the exact same binaries and one new balanced 24-pair series (`502661`), restricted to `single` checked and W1 with interleaved A/A. One-minute load was **5.49 → 6.83** during timing, quieter than S1 overall but not necessarily quieter on the pinned core. Checked was **2.161 → 2.119 cycles/B**, paired **−0.79% [−4.86, +2.14]** (thread CPU **−0.72% [−4.63, +2.20]**). W1 was **1,517 → 1,680 MB/s**, paired **+4.92% [+1.87, +19.08]** with child CPU **−4.37% [−16.02, −1.84]**. The ratios are noisier than the absolute instruction savings; same-A checked **−0.39% [−3.48, +3.46]** and W1 **+0.83% [−2.02, +8.89]** both include zero. All 96 timed replay requests met the PMU criteria. No section rerun, code change, substitution for S2, or further repeat was made. Results are in the [repeat raw data](decode-compact-sol-data/repeat.json.gz) and [reduction](decode-compact-sol-data/repeat.tsv). The conditional native/Wasm component comparison was not run because the trial did not advance.

## Fresh one-core C reference

These are separate A/C pairs from the same sessions, not an arithmetic splice with earlier publications or paired B/C samples. All three checked C7 outputs matched the manifests through EOF; every literal and token emission matched Rust byte-for-byte. Invalid oversized length nibbles and a flipped intermediate block checksum were independently rejected at replay startup. C section calls use the AVX2 section routine; checked blocks use the normal runtime dispatcher.

| `single` replay | A → C cycles/unit, S1 / S2 | Paired cycle change [interval], S1 / S2 | C instructions/unit |
| --- | --- | --- | ---: |
| Literals | 3.457 → 1.400 / 3.438 → 1.469 | −57.50% [−58.57, −55.86] / −57.18% [−58.18, −53.77] | 5.584 |
| Tokens | 4.581 → 2.013 / 4.080 → 1.690 | −57.45% [−60.25, −53.69] / −58.64% [−59.82, −57.43] | 6.224 |
| Checked | 2.097 → 1.350 / 2.035 → 1.304 | −36.48% [−39.83, −34.66] / −36.14% [−40.92, −35.15] | 4.207 |

For checked `single`, C used **0.358 / 0.346 thread s/GB** at **2,791 / 2,892 MB/s**; its separately paired A/C thread-CPU change was **−36.24% / −36.31%**. Even after eliminating heap traversal metadata, the Rust literal replay still takes **3.05–3.26 cycles/symbol** versus C's **1.40–1.47**. As a rough scale from the separate arm medians, weighting B and C's remaining section differences by symbol counts gives **58–63M literal cycles vs 18–20M token cycles** per single pass. This is neither a paired B/C estimate nor an additive decomposition of checked-block cycles.

**Next discriminator:** measure the remaining *per-symbol* cost on the original `single` literal run shapes, not another metadata rewrite. Attribute flat-unpack widths and mixed popcount/gather/copy-back separately in the current Rust and C AVX2 paths, then verify the largest remaining delta in a saved-run replay with exact logical output and separately disclosed padding/copy contracts. The [earlier profiles](decode-perf0.sol.md#paired-counts) motivate that split, but predate retained changes and cannot tell how much of today's residual is unpack, gather, copy-back, or code shape. More setup savings alone have not closed the checked-block or W1 gap. The SPL16 reconstruction and scheduler remain outside this trial.

## Reproduction

From the repository root, use the [fixed C7 fixture recipe](decode-pool.md#method-and-evidence) and [4 KiB mixed/dictionary fixtures](pivco-queue.md#timing-protocol-and-reproduction). The drivers check input hashes against the published manifests. The C git repository must contain `5719a96f778226868b29dba621a8c4e302b74d7c`. Requires Linux x86-64 AVX2 and grouped, same-user PID attachment with `perf stat -j --control` as in the [original driver](decode-replay-data/perf.py).

```sh
d=doc/profiling/decode-compact-sol-data
s=.test-agent/decode-compact-sol/repro
small=.test-agent/pivco-queue/fixtures
mkdir -p "$s"
bash "$d/build.sh" "$s/build" target/upstream-comparison/current b a c tests >"$s/build.log" 2>&1
python3 -B "$d/verify.py" "$s/build" "$small" >"$s/verify.json"
python3 -B "$d/bench.py" .test-agent/decode-pool/fixtures "$small" "$s/build" "$s/gate" --cpu 3
python3 -B "$d/bench.py" --summary "$s/gate/pairs.json.gz" >"$s/reduction.tsv"
python3 -B "$d/bench.py" .test-agent/decode-pool/fixtures "$small" "$s/build" "$s/repeat" --cpu 3 --repeat
python3 -B "$d/bench.py" --summary "$s/repeat/pairs.json.gz" >"$s/repeat.tsv"
```
