---
type: Reference
title: Commit account for the decode-throughput pass
description: Deferred long-form message for each commit from the next0 shortlist to the handoff, keyed by change ID, with an account of how the work moved.
resource: /design/decode-throughput/commit-account0.glm54.md
status: draft
generated: { by: 'model:zai-coding-plan/glm-5.4', at: '2026-11-23' }
sources:
  - { resource: 'opencode://ses_df8fd29afffepXs04yCk9DZsqv', title: Read each commit in the pass and wrote the deferred messages and the account of its movement }
  - { resource: /design/decode-throughput/handoff0.sol.md, title: Trial dispositions and the direct Rust-C comparison }
  - { resource: /doc/profiling/README.md, title: Index of the measurement reports these commits carry }
---

# Commit account for the decode-throughput pass

**Twenty-seven commits carry this pass and 25 of them are a title with no body.** `main` is published on Tangled at `809b5faa`, and 13 of those commit IDs are cited as pins in reports, build scripts and raw evidence, so the retitling is deferred rather than force-pushed. This file keeps what each body should say, written while the whole pass was still in view. No history was changed to produce it.

## How to apply it later

Entries are keyed by change ID, which survives a rebase; the published commit ID is shown only for lookup. The text block under each heading is the whole message, title included, and `jj describe -r <change-id> --stdin` applies one. Rewriting a commit renumbers everything after it, so apply the pass in one sitting and then refresh the citations.

Thirteen IDs are pinned. `0d68cb82` appears in seven docs and scripts plus raw data, `2fd99c21` in both benchmark drivers, and `3f711434` in three build scripts that run `git archive <id>`; ten others appear in one to three places each. Loose files can be edited. The compressed raw measurements keep the old IDs and need an old-to-new table. Named bookmarks for the commits worth commemorating would remove most of that work, because a reference to a name survives renumbering.

## Index

| Change ID | Published | Title | Action |
| --- | --- | --- | --- |
| `wuqxuwpkyklr` | `ae8af6d2` | Record SPL16 copy and Huffman setup replay trials | Extend |
| `vuwnnzkvsxmu` | `7cd6c1e8` | Build canonical Huffman trees with depth buckets | Extend |
| `xkvwuksyyusu` | `3f711434` | Record CLI timing and verification of Huffman buckets | Extend |
| `rpnmknvousqs` | `f12c5dfd` | Design bounded positioned-input trial for Crossbeam decode | Extend |
| `mnrmmvmltwnx` | `8fb13bc1` | Preserve the scratch positioned-read prototype and gate harness | Extend |
| `osqpuwokxmxn` | `6412ac26` | Record the positioned-read verification and timing method | Extend |
| `zxupyrmstlpp` | `88ea5bc0` | Record the positioned-read throughput miss and raw gate evidence | Extend |
| `zulwskoslwkn` | `117482ef` | Preserve the compact inline PivCo trial and paired gate method | Extend |
| `xstmzvmpsmwy` | `8f4045fb` | Record compact PivCo output, conformance and scalar parity checks | Extend |
| `xtqxyslqvqov` | `e2f8e5d0` | Record compact PivCo section gains and the missed serial decode gate | Extend |
| `ntoytntpxpwz` | `72eaabe1` | Attribute the checked decode gap to hot kernels | Extend |
| `sylmnulmtvtt` | `0e2a5196` | Design a bounded BMI2 bitmap merge for Zen 3 | Extend |
| `owvnqxyyrkmu` | `7653fec5` | Preserve the bounded AVX2 width-six unpack trial | Extend |
| `kvwzkppztzlv` | `2fd99c21` | Measure width-six PivCo gains through serial decode | Extend |
| `lpumpupzyuop` | `477254fb` | Measure landed six-bit AVX2 decode | Extend |
| `mtnqykykvmwk` | `b9985a7c` | Record the landed six-bit decoder fast path | Extend |
| `yuozzrvtptpw` | `0d68cb82` | Vectorize six-bit PivCo unpack on AVX2 | Extend |
| `qxqpuntxppzn` | `96de6837` | Run Linux ARM64 and RISC-V codec checks under QEMU | Keep |
| `kyylqswvlmls` | `2474ff3d` | Decode six-bit flat groups with ARM64 NEON | Keep |
| `rrxuyupoqusx` | `94df26aa` | Race bounded PDEP and AVX2 bitmap merge kernels | Extend |
| `snwsvwqutorz` | `94c148f7` | Verify bounded AVX2 merge gains through checked blocks and W1 | Extend |
| `pvsswuzpklwt` | `45415859` | Merge PivCo bitmaps in bounded AVX2 tiles | Extend |
| `rzwqunkmqsur` | `7209e2d9` | Keep AVX2 merge oracle lint-clean in Criterion builds | Extend |
| `wwyymrouzpyq` | `852b369e` | Record the landed AVX2 bitmap merge | Extend |
| `vsmspvntsxyo` | `45b2167b` | Measure the landed AVX2 merge against Rust and C | Extend |
| `uysxwrlzuwpr` | `0cdb6270` | Recheck the original eight-worker concat after the AVX2 merge | Extend |
| `nwnktsnzvtom` | `7bad8f38` | Hand off decoder throughput results and next experiments | Extend |

The two kept commits already explain their kernel, code generation and verification. `809b5faa`, which tracks the beads configuration, came from another process after the pass and is outside this account.

## The movement

The pass moved in five steps, and what changed between them was mostly the yardstick.

**1. Work the shortlist** (`wuqxuwpkyklr` to `xkvwuksyyusu`). The two cheapest ideas from `next0` split. The SPL16 copy split cost more than it saved, and distance one was too rare to build. Canonical depth buckets landed: a real but small setup win whose CLI effect never resolved.

**2. Two instructive misses** (`rpnmknvousqs` to `xtqxyslqvqov`). Worker-side positioned reads did what they promised, cutting coordinator CPU by 96%, and still made the long frame slower. The work moved; it did not disappear. Compact Huffman tables cut real cycles and missed a combined 5% gate in their second series. Dropping them on that number was the wrong call, and the gate was relaxed from here on.

**3. Find where the cycles are** (`ntoytntpxpwz`). Sampling the retained decoder against C replaced guesses with shares: reconstruction 38%, gather 26% and unpack 19% of the checked-block excess, plus a census of run shapes that named width six. Every later kernel came out of that table.

**4. Two kernels land** (`sylmnulmtvtt` to `rzwqunkmqsur`). Width six went first because it was clean: an exact 24 bytes in, 32 symbols out. The merge needed a race. Byte deposit beat the old kernel and lost to a C-style AVX2 loop, and the first AVX2 version carried a skew regression that only resuming the existing eight-output path fixed. ARM64 and RISC-V correctness was added alongside so the x86 work did not narrow the targets (`qxqpuntxppzn`, `kyylqswvlmls`).

**5. Measure against C and hand off** (`wwyymrouzpyq` to `nwnktsnzvtom`). Direct pairs changed the picture again. With both CLIs on one core, Rust and C show no resolved difference on the long frame at one worker, and the old three-to-one concat gap did not reproduce. What remains is CPU: 18–20% more per GB at eight workers on the long frame.

## Messages

### `wuqxuwpkyklr` · Record SPL16 copy and Huffman setup replay trials

```text
Record SPL16 copy and Huffman setup replay trials

Run the first two ranked trials from design/decode-throughput/next0.sol.md
as scratch exports of dac698f9 and keep their evidence.

- SPL16 32-byte copy split for 20-32 byte match extensions: rejected.
  The 64-byte copy already inlined, so the split only added a length
  branch and 0.27% more instructions. Checked single blocks used
  0.72% / 1.11% more cycles per byte.
- Distance-one fill: screened out without building it. Only 6,605
  matches on single (0.054% of tokens) and 32 on sao qualify.
- Canonical depth buckets: advance. The targeted repeat measured 8.3%
  fewer single token cycles and 2.7% fewer checked-block cycles per
  plaintext byte.

Link the results from the original ranking and the profiling guide. The
patches, per-block census and raw counters are in
doc/profiling/decode-replay-data/decode-next-trials.tar.xz.

Tickets: zxcrs-perf.
```

### `vuwnnzkvsxmu` · Build canonical Huffman trees with depth buckets

```text
Build canonical Huffman trees with depth buckets

Tree::new sorted the 256 (depth, symbol) pairs with sort_unstable for
every inline Huffman table, and an older token profile put 16.9% of its
samples in that setup. Count the symbols per depth instead, check the
same Kraft sum and depth-one singleton case, then place symbols in
ascending order at each depth's bucket cursor. Canonical code assignment,
node construction and flat lookups are unchanged.

The encoder shares this path, so the change is not decode-only: Dense
encodings of three conformance plaintexts are byte-identical. On the
long C7 frame it removes 5.85% of token-section instructions. A quieter
targeted repeat measured 8.3% and 10.9% fewer token cycles and 2.7%
fewer checked-block cycles per plaintext byte. No eight-worker CLI
effect resolved. Report: doc/profiling/decode-next0.sol.md.

Tickets: zxcrs-perf.
```

### `xkvwuksyyusu` · Record CLI timing and verification of Huffman buckets

```text
Record CLI timing and verification of Huffman buckets

Add two 24-pair CLI series for the retained depth buckets and archive the
adapter with its raw pairs. Single wall moved -2.5% / -1.8% at one
worker and -1.4% / -5.2% at eight. No contrast resolved in both series,
so the result stands as a codec-level win rather than a CLI one. mozilla
and sao show no repeatable control regression.

The retained Release CLI has byte-identical .text to the timed trial and
decoded all three archives to the manifest plaintext at one and eight
workers.

Tickets: zxcrs-perf.
```

### `rpnmknvousqs` · Design bounded positioned-input trial for Crossbeam decode

```text
Design bounded positioned-input trial for Crossbeam decode

Design ranked trial 2 from design/decode-throughput/next0.sol.md. The
caller reads only each 8-byte block header and hands the file position
and the existing credit buffer to a worker, which preads the body,
revalidates header and extent, and calls the unchanged decoder. Sixteen
credits, ordered commit and the footer barrier stay as they are.

Select on the open handle so pipes, stdin and non-regular files keep the
stream path. Gate on 10% lower W8 wall on both fixed inputs with at most
3% more child CPU per GB and no pipe regression. The follow-up
measurement missed that gate:
doc/profiling/decode-positioned0.sol.md.

Tickets: zxcrs-parallel.
```

### `mnrmmvmltwnx` · Preserve the scratch positioned-read prototype and gate harness

```text
Preserve the scratch positioned-read prototype and gate harness

Add the trial patch, the separate diagnostic overlay, the build and
benchmark scripts and the opening report for worker-side positioned
reads. The patch gives a scratch export of 3f711434 an opt-in
ZXC_TRIAL_INPUT=positioned mode: on Unix, with Crossbeam and more than
one worker, a regular named file of positive length sends its header,
offset and extent to a worker together with the credit buffer. The
worker re-reads and compares the header, then exact-reads the body into
that buffer. Every other input keeps the buffered stream path, and no
production source changes.

The report opens with what the prototype proves before any timing:
selection and parity across RAW, SPL and PAK, checked, indexed, empty
and mixed-size frames, dictionary changes, rejection cases, controlled
mutation between admission and the worker read, and output publication.

Tickets: zxcrs-parallel.
```

### `osqpuwokxmxn` · Record the positioned-read verification and timing method

```text
Record the positioned-read verification and timing method

Add the measurement method ahead of any result: clean parent, forced
stream and positioned modes on the same named fixtures, a real pipe fed
by a separate process, fresh output files, and a diagnostic build that
times the same successful block-admission edges in the caller and the
workers with thread CPU and monotonic clocks.

The gate is two independently seeded series of 24 balanced pairs per
contrast. It passes only if both fixed inputs resolve 10% lower W8 wall
in both series, with at most 3% more child CPU per GB, less reader work
and no pipe regression. Diagnostic binaries never supply the clean
numbers.

Tickets: zxcrs-parallel.
```

### `zxupyrmstlpp` · Record the positioned-read throughput miss and raw gate evidence

```text
Record the positioned-read throughput miss and raw gate evidence

Add the clean and diagnostic results and reject the trial. Coordinator
CPU inside successful block admission fell about 96% on both layouts,
but that work moved rather than disappeared: clean W8/K16 wall on the
long single frame was 5.1% / 2.0% slower against the parent, and concat
moved 1.0% / -4.4%. Both single intervals exclude the required 10% gain.
The 3% CPU ceiling and pipe nonregression were unresolved, so no quieter
repeat was spent. Nothing entered production.

Tickets: zxcrs-parallel.
```

### `zulwskoslwkn` · Preserve the compact inline PivCo trial and paired gate method

```text
Preserve the compact inline PivCo trial and paired gate method

Add the scratch trial patch, its differential tests, the build and
paired drivers and the opening report for compact inline Huffman decode
tables. From the canonical lengths the patch builds per-depth first
prefixes and leaf and internal bases, then walks runs in depth order
from one fixed stack array. That replaces the heap nodes, per-tree
lookups, traversal FIFO and reverse-order Vec while keeping the existing
flat unpack, mixed gather and copy-back.

It is decode-only for inline tables: the encoder's Tree::new and
Tree::emit and the cached dictionary tree are untouched. The report
states the gate before the numbers: a material literal-section
reduction, then at least 5% fewer checked-block cycles per byte and 5%
higher serial throughput in each of two series.

Tickets: zxcrs-perf.
```

### `xstmzvmpsmwy` · Record compact PivCo output, conformance and scalar parity checks

```text
Record compact PivCo output, conformance and scalar parity checks

Add the verification behind the compact-table trial. Release nextest
passed 71 workspace tests with and without default features and 37 codec
tests without them. The CLI preflight records 236 conformance
invocations: all 18 valid vectors decode to the exact bytes and all 35
invalid-category vectors match the parent's status, stderr and committed
stdout, with native and scalar builds. Literals coded against a
dictionary match as well, and Dense encodings are byte-identical with
and without one. The per-ISA C compile commands for the reference build
are kept beside them.

Tickets: zxcrs-perf.
```

### `xtqxyslqvqov` · Record compact PivCo section gains and the missed serial decode gate

```text
Record compact PivCo section gains and the missed serial decode gate

Measure the compact tables through checked replay and the serial CLI,
and hold them out of production. On the long C7 frame, literal sections
used 7.4% / 8.5% fewer cycles and tokens 19.9% / 21.0%, and every
checked run retired 3.35% fewer instructions per byte. Checked-block
cycles fell 5.3% / 3.5% and only 0.8% on the targeted repeat; serial
throughput rose 5.2% / 3.4% and 4.9% on the repeat. That misses the
original combined 5% gate, but one entropy-heavy 4 KiB scalar fixture
gained 68-71%, so the patch stays a candidate for small blocks pending
Wasm and broader controls.

The same sessions give a fresh one-core C reference: 1.40-1.47 cycles
per literal symbol against the candidate's 3.05-3.26. The remaining
question is per-symbol kernel cost, not table setup.
Report: doc/profiling/decode-compact0.sol.md.

Tickets: zxcrs-perf.
```

### `ntoytntpxpwz` · Attribute the checked decode gap to hot kernels

```text
Attribute the checked decode gap to hot kernels

Profile the retained decoder, the compact-table prototype and pinned C on
the same long C7 frame before choosing another kernel. In one-core replay
Rust used 2.95 cycles per checked plaintext byte against C's 1.94 and
retired twice the instructions. Sampling splits Rust's excess roughly
into match reconstruction 38%, mixed bitmap gather 26%, flat unpack 19%,
table setup 10% and copy-back 5%. Within literal sections, gather and
unpack are 82% of the gap.

A wire-order census adds the run shapes: width-six flat runs carry 24% of
the literal symbols and 35% of the unpack samples, and 92.74M bytes pass
through 4,157 mixed nodes. Those two numbers chose the width-six unpack
and the AVX2 merge. The shares predate both.
Report: doc/profiling/decode-gap0.sol.md.

Tickets: zxcrs-perf, zxcrs-simd, zxcrs-copies.
```

### `sylmnulmtvtt` · Design a bounded BMI2 bitmap merge for Zen 3

```text
Design a bounded BMI2 bitmap merge for Zen 3

Reopen the mixed-node merge only with a mechanism that removes work.
Recommend BMI2 byte deposit first on the 5800X: two PDEPs and an OR
place eight output bytes directly, unrolled to 32, with no SIMD lane
assembly. Race it against the pinned C 32-output AVX2 shuffle-and-blend
loop on the same captured child streams, skewed and short runs included,
and promote a smaller reproducible win if it carries through checked
blocks and W1.

An untimed census shows bounded loads cover 99.73% of literal and 98.93%
of token gather bytes, so the first prototype needs no padding. PDEP is
slow on Zen 2, so support alone is not a reason to enable it. The race
chose the AVX2 arm: doc/profiling/bitmap-merge-race0.sol.md.

Tickets: zxcrs-simd.
```

### `owvnqxyyrkmu` · Preserve the bounded AVX2 width-six unpack trial

```text
Preserve the bounded AVX2 width-six unpack trial

Keep the width-six trial before any timing claim: the patch, clean build
scripts, the saved-run generator and its oracle, the paired driver,
portable Release disassembly, and proof that the scalar CLI and replay
.text are byte-identical between parent and candidate.

Tickets: zxcrs-simd.
```

### `kvwzkppztzlv` · Measure width-six PivCo gains through serial decode

```text
Measure width-six PivCo gains through serial decode

Measure a bounded AVX2 kernel that decodes 32 six-bit symbols from an
exact 24-byte group on the original C7 runs. Saved width-six runs used
82.9% fewer cycles and 86.5% fewer instructions per symbol, and whole
single literals 6.7% / 7.0% fewer cycles. Checked blocks retired 2.27%
fewer instructions per byte on all three fixtures, but none of the three
cycle intervals excluded zero, and single W1 throughput moved 3.1%,
1.0% and 1.9% without resolving. sao W1 gained 4.77% in one series. No
control showed a clear regression.

The kernel keeps the scalar tail and falls back below 32 symbols or
without AVX2. Report: doc/profiling/flat-unpack6.sol.md.

Tickets: zxcrs-simd.
```

### `lpumpupzyuop` · Measure landed six-bit AVX2 decode

```text
Measure landed six-bit AVX2 decode

Confirm the landed width-six unpack with fresh paired CLI series against
its direct parent and pinned C. Single W1 throughput moved 1.63% /
1.45%; the second interval crosses zero, so the end-to-end gain stays
unresolved. sao W1 favored the landed build with lower child CPU in both
series. Pinned C stayed ahead on single at W1 and W8 in these series,
and the first series was especially noisy. Small-block timing and fresh
replay counters were not collected.

Report: doc/profiling/flat-unpack6-landed0.gpt61t.md.

Tickets: zxcrs-simd.
```

### `mtnqykykvmwk` · Record the landed six-bit decoder fast path

```text
Record the landed six-bit decoder fast path

Mark the width-six AVX2 unpack as landed in its report and link the
trial and the bitmap-merge design from the profiling guide. The landing
rests on the repeatable literal-section win and the lower checked-block
instruction count, not on a demonstrated CLI gain.

Tickets: zxcrs-simd.
```

### `yuozzrvtptpw` · Vectorize six-bit PivCo unpack on AVX2

```text
Vectorize six-bit PivCo unpack on AVX2

Width-six flat runs carry 24% of the long frame's literal symbols and
35% of its flat-unpack samples, yet were decoded by a scalar table lookup
per symbol. Add an AVX2 kernel that takes an exact 24-byte group and
emits 32 symbols: two 16-byte loads, one window shuffle, four table
shuffles with three blends, and one 32-byte store. There is no lookahead,
staging buffer, unsafe code or new dependency, and the portable Release
loop is five vpshufb and one 32-byte store.

Dispatch once per subtree on the existing merge-simd feature and AVX2
proof, for runs of at least 32 symbols. Shorter runs, the final 0-31
symbols, other tiers and no-feature builds keep the scalar path.

A differential test decodes individual wire bits as its oracle across
all source and output offsets modulo 32, arbitrary tables and padding
bits, and lengths from 0 to 4097, with every slice ending at its
allocation. Saved width-six runs used 83% fewer cycles and whole single
literals 6.7-7.0% fewer; checked blocks retired 2.27% fewer instructions
per byte, and the CLI gain is unresolved. Scalar .text is byte-identical.
Report: doc/profiling/flat-unpack6.sol.md.

Tickets: zxcrs-simd.
```

### `rrxuyupoqusx` · Race bounded PDEP and AVX2 bitmap merge kernels

```text
Race bounded PDEP and AVX2 bitmap merge kernels

Replay the original single literal and token gathers to choose between
BMI2 byte deposit and a bounded 32-output AVX2 shuffle-and-blend loop.
Against the retained kernel, PDEP used 24.9% / 22.8% fewer cycles and
the first AVX2 version 39.3% / 38.6%; AVX2 beat PDEP by 23.5% / 20.8%
head to head, so the deposit arm is dropped. PDEP also needs a measured
CPU allowlist because it is slow on Zen 2.

The skew controls exposed the cost of the first AVX2 version: its direct
scalar suffix regressed 86.2% on sparse right-minority runs and 20.1%
when that child drains early. Captures, kernel sources, both patches and
the raw pairs are kept under doc/profiling/bitmap-merge-race-sol-data.

Tickets: zxcrs-simd.
```

### `snwsvwqutorz` · Verify bounded AVX2 merge gains through checked blocks and W1

```text
Verify bounded AVX2 merge gains through checked blocks and W1

Finish the merge race with the corrected suffix and take the winner
through checked replay and the serial CLI. Resuming the existing
eight-output path from the exact cursors removes the skew regression.
On the original single gathers the revised kernel uses 43% fewer cycles
and about half the instructions; checked blocks use 8.5% / 9.7% fewer
cycles per plaintext byte. Serial throughput rose 5.3% / 7.8% on single,
8.9% / 7.7% on mozilla and 8.1% / 5.2% on sao.

Neither 4 KiB CLI control resolved a loss. The isolated short-literal
kernel case was 3.09% slower on 0.011% of the gather volume. Ordered mask
pairs, alignment, exact-end, ASan, no-default, x86 fallback and Wasm
smoke checks passed. W8 was not measured here.
Report: doc/profiling/bitmap-merge-race0.sol.md.

Tickets: zxcrs-simd, zxcrs-perf.
```

### `pvsswuzpklwt` · Merge PivCo bitmaps in bounded AVX2 tiles

```text
Merge PivCo bitmaps in bounded AVX2 tiles

Mixed bitmap gather was the largest entropy cost left after the width-six
unpack, about 26% of Rust's checked-block excess over C. The old kernel
produced eight outputs per step and checked both child windows each
time. On x86-64 AVX2, for nodes of at least 128 outputs, emit 32 per
step: two 16-output lanes, two vpshufb, one blend and one full 32-byte
store, selected by a 20 KiB table built at compile time.

Both 32-byte child windows must lie inside the counted node, so no
padding, wire slack or unsafe code is involved. A left load may overlap
right bytes but never selects them. The loop hands the exact cursors to
the existing eight-output path, which matters: a scalar suffix regressed
badly on right-skewed runs. Other tiers, no-feature builds and non-x86
targets keep the previous kernel.

Gather cycles fell 43% on the original runs and checked single blocks
8.5% / 9.7%. Serial throughput rose 5.2-8.9% across the three C7
fixtures. Landed against its parent, single gained 7.9-10.2% at one
worker and 8.0-8.6% at eight. The tests force all 65,536 ordered mask
pairs through both halves of the second tile at every alignment, with
canaries and exact allocation ends. Reports:
doc/profiling/bitmap-merge-race0.sol.md and
doc/profiling/bitmap-merge-landed0.sol.md.

Tickets: zxcrs-simd, zxcrs-perf.
```

### `rzwqunkmqsur` · Keep AVX2 merge oracle lint-clean in Criterion builds

```text
Keep AVX2 merge oracle lint-clean in Criterion builds

benches/pivco_merge.rs includes merge.rs by path, and a bench target
compiles with cfg(test), so the ordered-pair oracle's helper looked
unused under cargo clippy --all-targets and failed the check. Allow dead
code on that one helper with the reason. No behavior changes.

Tickets: zxcrs-simd.
```

### `wwyymrouzpyq` · Record the landed AVX2 bitmap merge

```text
Record the landed AVX2 bitmap merge

Mark the bounded AVX2 merge as landed in its race report and link the
race from the profiling guide. PDEP did not land. The numbers in that
report describe the scratch comparison; the direct landed CLI comparison
follows separately.

Tickets: zxcrs-simd.
```

### `vsmspvntsxyo` · Measure the landed AVX2 merge against Rust and C

```text
Measure the landed AVX2 merge against Rust and C

Pair the landed merge, on the tree that also carries the ARM64 unpack,
against its direct Rust parent and pinned C at one and eight workers on
the three fixed C7 archives. Against the parent, single throughput rose
10.2% / 7.9% at W1 and 8.0% / 8.6% at W8, with 8.0% / 7.5% less child
CPU at W8.

Against C on one pinned core, single shows no resolved difference at W1
while Rust still spends 18-20% more CPU per GB at W8; mozilla and sao
W1 favor Rust by 7.7-7.8% and 18-19%. Both CLIs and all their threads
share the W1 mask here, unlike earlier runs that let C's driver threads
use extra cores. The integrated decoder passed 73 release workspace
tests, 36 no-default codec tests and the ARM64 and RISC-V QEMU suites.
Report: doc/profiling/bitmap-merge-landed0.sol.md.

Tickets: zxcrs-perf, zxcrs-simd.
```

### `uysxwrlzuwpr` · Recheck the original eight-worker concat after the AVX2 merge

```text
Recheck the original eight-worker concat after the AVX2 merge

Rerun the 12-frame C7 concat at W8 that once measured 8.45 GB/s for C
against 2.76 GB/s for Rust, using the landed merge, its parent and
pinned C. That gap did not reproduce: Rust paired 3.4% / -5.9% against C
on a busy host, with both intervals crossing zero, while Rust
consistently used 10-15% more CPU per GB. Against its parent the merge
cut CPU 6.1-7.5% and resolved a throughput gain only in the second
series.

concat is a different compressed input from the long single frame, not a
reframing of it.

Tickets: zxcrs-perf, zxcrs-parallel.
```

### `nwnktsnzvtom` · Hand off decoder throughput results and next experiments

```text
Hand off decoder throughput results and next experiments

Write the consolidated account of the decode-throughput pass and point
the original next0 ranking at it. The handoff records what landed, what
was rejected and why, the direct Rust-C comparisons at one and eight
workers, the next experiments in order, and the comparison rules that
keep the evidence paired. Its order: reprofile the landed decoder before
ranking what remains, then open up the SPL16 bulk interior, and keep the
compact tables as a small-block candidate.

Tickets: zxcrs-perf.
```
