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

## One representation, on the retained bucket baseline

Rust A is a clean `git archive` of `3f711434ad83b85fa191a209041777c457c6972b`. The separate positioned-read commits through `88ea5bc0` do not change `crates/` or `tests/`. Rust B applies only the [trial patch](decode-compact-sol-data/trial.patch), in scratch. This is an **inline mode 2 decode-only** experiment. The encoder's `Tree::new` and `Tree::emit` still use the retained bucketing and heap tree; mode 3 still uses the original cached dictionary tree and lookup. Neither test vectors nor the wire format were changed. No positioned reads, scheduler, reconstruction, SPL16, new bit kernels, buffer layout or producer changes were included.

After reading the same nibble lengths and checking 11-bit depth and the complete Kraft sum (with the one-symbol exception), B constructs arrays of per-depth first prefixes and leaf/internal bases. Canonical leaves come before the internal prefixes of each depth. Internal rank `i` therefore has children at ranks `2i` and `2i + 1` at the next depth, before removing that next level's leaf prefix. Full trees with at most 256 leaves have at most 255 internal slots. Bottom-up equality of child heights finds perfect subtrees, including pairs. At a visited perfect subtree, its leaves occupy one contiguous interval at one depth; reversing each code's bits gives the original flat lookup order. There are no per-tree or per-flat heap allocations on the inline path. A singleton still consumes its bitmap, ignores padding, and rejects a selection of the nonexistent right child.

B consumes nonempty runs in the same depth/left-to-right order, keeps the same disjoint intervals, and fills leaves or calls the existing `flat::unpack`. One fixed stack array covers up to 255 internal slots, with four `u32` coordinates per slot (4,080 B); validated section and symbol extents are bounded by the 2 MiB format block limit. A mixed bitmap records its left population and source offset in its slot. Empty/covered slots stay unused, uniform ancestors keep both children in the same output buffer and never gather, and the reverse slot walk invokes the same gather/copy-back only on mixed runs. B trades the heap nodes, lookup allocations, FIFO and reverse-order Vec for bounded stack setup/scans; it is not a promise that every section uses fewer bytes than before. `bitmap_ones` and `merge_back` are shared inline helpers extracted verbatim in behavior, including the scalar Wasm cursor, tail masking, and ledger byte counts. Run reads are still checked and unused bytes still reject before any block is published; neither path changes output growth, checksum checks or scratch ownership. Unlike C, B does not alternate destination and scratch by depth.

## Gate and scope

- **Promotion:** require a material reduction on `single` literals (use a 5% paired median screen and intervals below zero); tokens are secondary. On `single` checked blocks require at least 5% fewer cycles/plaintext byte, and on the serial W1 CLI at least 5% more plaintext MB/s, in each of two series with both intervals on the favorable side of zero. A +5% throughput change means −4.76% elapsed, not −5%. Check instructions/thread CPU and child CPU respectively. A repeatable control cost exceeding 3% with positive intervals blocks promotion; noisy non-regression does not establish equivalence. A native/Wasm component check is conditional on advancing. At most one quieter targeted repeat may resolve an otherwise viable but uncertain decision.
- **Inputs:** hash-checked `single.zxc` is the independently encoded original 405-block C7 frame, not concat. `mozilla` and `sao` are fixed member frames. Section-only calls use views of the original SPL16 inline tables and runs. The native `--no-default-features` scalar control is the fixed 1 MiB `mixed12.zxc` fixture with 256 checksummed 4 KiB SPL8 blocks and inline-Huffman literals, from the [queue study](pivco-queue.md#allocation-and-retention-gates); not the short, uncoded SPL8 checksum vector. All inputs retain their exact original compressed bytes.
- **Pairing:** Ryzen 7 5800X on Linux. CPU 3 pinned; SMT sibling 11 is not reserved. Two seeds (`713531`, `396833`), each with 24 adjacent pairs, balanced AB/BA separately for each contrast, shuffled and interleaved with a fresh same-A control. C has a separate A/C contrast, also balanced, and is not the Rust B or an A/A correction. Each replay worker starts fresh per case/series and performs a startup pass and a separate warmup; each CLI binary has a separate warm launch. The repository's `.test-agent/zxc-perf.lock` covers file verification, warmups and both series. Files are prefaulted, not necessarily LLC-resident. No sample from a previous report is included.
- **Timed boundaries:** replay counters are grouped userspace `cycles:u,instructions:u` attached to one resident thread and must run 100%; clocks inside that thread cover only the repeat loop. Both kernels reconstruct the same logical output into private reused buffers, verifying each block checksum and folding the same first/middle/last byte and length fingerprint on every pass. C uses its normal AVX2 entry points and padded physical scratch; Rust uses initialized `Vec` scratch and retains strict consumption. This matches the logical output contract, not the physical memory layout or all malformed-input policy. The full CLIs verify through footer and EOF before timing. W1 Rust decodes named files into `/dev/null`; elapsed is pinned child launch through `wait4`, CPU is child user+system. C CLI is exact-output preflight only; fresh C **timing** is from resident replay, not from the threaded C file driver.
- **Statistics:** per-series absolute medians and medians of within-pair ratios are reported separately. Cycles and instructions divide by decoded symbols for sections and plaintext bytes for checked blocks. CPU seconds use decimal output GB. Use the unadjusted two-sided 97.7% order-statistic median interval at 24 independent pairs; host load can be serially correlated. A/A is a noise control, not something to subtract. For replay and CPU costs negative is better; for throughput positive is better.

## Verification

The [independent malformed and differential tests](decode-compact-sol-data/tests.patch) leave A's original tests intact. Two additional tests give a literal wire example with nested merges under a uniform ancestor and deliberately set padding, then check short/trailing runs, zero count, singleton selection, incomplete/oversubscribed tables, and both high and low invalid depth nibbles. A seeded differential test checks 600 valid shapes (including all eight flat widths and random leaf depths up to 11) against the original tree's emitted bytes, then compares exact outputs or error kind/message for truncated, trailing, and bit-flipped runs. The same output and merge Vecs are reused across successes and failures. The existing block tests also cover a single worker alternating dictionary/inline entropy, dictionary crossings, output limits, and checksums.

Release nextest passed **71 workspace / 71 workspace without default features / 37 codec without default features**. Workspace fmt and Clippy passed; the Clippy command suppresses the already-known `new_instead_of_clear` diagnostic on deliberate scratch release. The [build and tests log](decode-compact-sol-data/build.log.gz) records the commands, binary hashes and toolchain (Rust 1.102.0-nightly `bdf712ee8`, LLVM 23.1.6; C GCC 16.2.0). The [per-ISA C compile commands](decode-compact-sol-data/c-compile.json.gz) identify the reference build. Rust uses locked portable Release with default merge SIMD for the primary cases and no default features for the small scalar control. Replay-only builds carry line tables and the original probe shim; the standalone CLIs do not. No ledger or Tracy flags were used for timing.

Outside the paired runs, the [CLI conformance preflight](decode-compact-sol-data/verify.json.gz) records 236 invocations (59 cases × A/B and their scalar variants). All 18 valid vectors produced the exact bytes; all 35 invalid-category vectors agreed in status, stderr and committed stdout (the existing expected success is `sek_forged_entry`). This includes dictionary fixture vectors with the three bundled dictionaries supplied, including ID zero. A/B also matched plaintext for one and 256 blocks with dictionary-coded literals from the queue study, rejected both streams without the dictionary, and emitted byte-identical Dense encodings with and without a dictionary. These are parity checks, not measurements of dictionary throughput.

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
```
