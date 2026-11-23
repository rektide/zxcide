---
type: Measurement
title: Linux perf replay of the C7 Huffman section gap
description: Counts and profiles of identical Huffman literals and tokens, with checked block context, comparing normal upstream C AVX2 and two Rust Release builds.
resource: /doc/profiling/decode-perf0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#xhigh', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_dfa241394ffeeebYvVj1RlKXQq', title: Captured original sections and measured paired Linux PMU counts and profiles of the three checked replay builds }
  - { resource: /doc/profiling/decode-replay0.sol.md, title: Checked block baseline and separate elapsed-clock section-prep measurements }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Fixed corpus, archive and plaintext identities }
  - { resource: 'https://github.com/hellobertrand/zxc/tree/5719a96f778226868b29dba621a8c4e302b74d7c', title: Upstream C performance pin }
  - { resource: 'https://github.com/rektide/zxc-rs/tree/8b3b88974f40b46808ce13e2f5f2306c11a1bc19', title: Rust performance pin with pool and buffer reuse }
---

# Linux perf replay of the C7 Huffman section gap

**The Huffman section gap survives `target-cpu=native`.** On the long frame, C AVX2 used **1.92 cycles per decoded symbol** for each of literals and tokens, versus portable Rust's **4.87 / 5.28** and native Rust's **4.26 / 4.80**. C retired roughly one-third as many instructions per symbol as portable Rust. In the checked block replay, C again used **39% less thread CPU** than portable Rust on the long frame. The section-only result pinpoints costly work that survives removing the rest of the block; it does not assign the entire block gap to Huffman.

## Paired counts

Each cell lists **C AVX2 / Rust portable / Rust native**. Rates are medians across 12 matched rounds per case. A Huffman symbol is one decoded byte (including a token byte, not its expanded match); its MB/s and CPU s/GB are per decimal GB of *decoded symbols*. The block rows instead use plaintext bytes. Cycles and instructions are user-mode counts. CPU time is user+system for the replay thread.

| Case | Blocks or sections | Output B/pass | Cycles/B | Instructions/B | Wall MB/s | CPU s/GB |
| --- | ---: | ---: | --- | --- | --- | --- | --- |
| Long block | 405 | 211,938,580 | 1.63 / 2.66 / 2.51 | 4.21 / 8.93 / 8.44 | 1926 / 1205 / 1279 | 0.435 / 0.709 / 0.667 |
| Long Huffman literals | 380 | 34,911,224 | 1.92 / 4.87 / 4.26 | 5.58 / 16.17 / 14.45 | 1592 / 662 / 747 | 0.512 / 1.297 / 1.135 |
| Long Huffman tokens | 405 | 12,132,680 | 1.92 / 5.28 / 4.80 | 6.22 / 19.22 / 17.51 | 1949 / 713 / 783 | 0.509 / 1.403 / 1.276 |
| `mozilla` block | 98 | 51,220,480 | 1.94 / 3.10 / 2.88 | 4.51 / 9.70 / 9.16 | 1715 / 1022 / 1128 | 0.519 / 0.827 / 0.768 |
| `mozilla` Huffman literals | 87 | 10,164,384 | 1.93 / 5.12 / 4.44 | 6.15 / 17.41 / 15.70 | 1914 / 696 / 831 | 0.514 / 1.362 / 1.179 |
| `mozilla` Huffman tokens | 98 | 2,918,407 | 2.00 / 5.47 / 4.95 | 6.70 / 20.62 / 18.85 | 1729 / 610 / 677 | 0.531 / 1.455 / 1.318 |
| `sao` block | 14 | 7,251,944 | 1.96 / 3.50 / 3.42 | 5.32 / 12.57 / 11.83 | 1657 / 1011 / 970 | 0.522 / 0.941 / 0.912 |
| `sao` Huffman literals | 6 | 1,838,566 | 1.48 / 3.90 / 3.58 | 5.83 / 16.76 / 14.97 | 2211 / 849 / 924 | 0.394 / 1.038 / 0.951 |
| `sao` Huffman tokens | 14 | 467,648 | 1.34 / 3.88 / 3.31 | 5.05 / 14.57 / 12.92 | 2518 / 962 / 1021 | 0.356 / 1.032 / 0.883 |

Relative to portable Rust, C's paired median cycle reductions were **61–62% for literals, 63–64% for tokens**. For the long frame they were −61.60% [−65.52, −56.92] for literals and −62.80% [−65.05, −60.29] for tokens. Native Rust's cycle changes there were −13.14% [−21.45, +0.08] and −5.83% [−11.44, −3.97]; it consistently retired fewer instructions (−10.62% / −8.89%). These are within-round ratios with unadjusted 96% order-statistic intervals under independent rounds. The shared host makes those intervals descriptive, not guarantees. The native build's long-block CPU change (−6.45% [−13.53, +1.19]) does not establish a full-block win. The long frame's much larger literal volume matters: multiplying the section-only median differences by their symbol counts gives about **103M excess cycles in literals vs 41M in tokens per pass** for portable Rust. This weights the two section probes; it is not an additive partition of full-block cycles.

## One next candidate: 3-bit flat unpack

**Test a runtime-gated x86 byte-shuffle kernel for *only* `WIDTH == 3` in Rust's [`entropy::flat::unpack`](../../crates/codec/src/entropy/flat.rs).** Process 16 decoded symbols from exactly six readable bytes using the eight-entry code-to-symbol table. Keep the scalar fallback and tail; don't change tree construction, mixed-node merge or scratch layout, and don't require a native build to get the fast path. C's AVX2 object already vectorizes flat runs; Rust's generic loop still performs the indexed lookup per symbol. A width-specific trial keeps the change and its test surface small despite merge being the larger single profile bucket.

In the long literal replay, portable Rust's AVX2 mixed merge, flat unpack and `Tree::new` (self only) took **49.86%, 32.24% and 5.09%** of sampled cycles; native was **51.49%, 30.11% and 5.80%**. For long tokens, those were **37.21%, 24.66% and 16.94%** in portable Rust; sorting adds further codebook cost there. C inlines much of the work: matching inline frames inside `zxc_pivco_decode_core` identified **66.72% merge / 15.37% flat** of C's long-literal samples. These shares use different denominators and are not elapsed stage comparisons. The checked long block profiles confirm the normal C `_avx2` wrapper and Rust's runtime AVX2 merge; Rust's SPL16 reconstruction still accounts for about half of its checked block samples.

Inline DWARF identifies `groups<3, 8>` as **1,412/12,659 (11.15%)** of long-literal samples in portable Rust and **1,080/10,731 (10.06%)** in native Rust. It was also **710/17,475 (4.06%)** in portable checked long-block replay. The predicted first signal is a drop in long-literal cycles and instructions per symbol; long tokens are secondary (6.44% of their portable samples), while `sao` literals are a weaker-response control (2.79%, with width six dominant). If the trial halves the sampled width-three cost with no overhead, the rough scale is **5–6% in section cycles and ~2% in checked long-block cycles**, not a measured speedup. Hold all other widths fixed; compare exact outputs including partial tails, then validate the signal with paired section counts and checked blocks before any eight-worker CLI check. No optimization was implemented in this pass.

The profiles are separate, non-precise fixed-period cycle samples, not differential traces or instrumented timers. There were 21 captures (all six section cases and the long block, three builds), 243,941 sampled IPs, with no lost-event records or misordered timestamps reported. 342 samples (0.14%; at most 0.96% in one capture) had unresolved symbols; 9,771 (4.01%) had no usable first source line or line zero. These remain in the denominator. Inlining, skid and optimized line mappings can move attribution within a kernel; the data do not establish exact per-phase or per-instruction cycle costs.

## Boundary and exactness

- The driver takes the `single`, `mozilla` and `sao` C7 archives from the [fixture manifest](decode-pool-data/fixture-manifest.json). `single` is one long checked frame, not the concatenation of frames. For each archive it captures two ordered indexes of `(absolute offset, coded byte length, decoded symbol count)` for SPL16 inline Huffman sections only. Workers use those views of the original file bytes, including the original 128-byte code lengths, not a stream rebuilt from decoded symbols. The block comparison walks the original archive with the existing checked replay.
- The section call starts with the length table and fills the entire logical decoded byte stream. It includes table validation, tree construction, run walk, flat unpack, mixed merge and scratch work. It excludes descriptor parsing, RAW/RLE, offsets/extras, reconstruction and checksums. C calls `zxc_huf_decode_section_avx2`, the routine both literal and token setup call in the AVX2 SPL block object; the unsuffixed `zxc_huf_decode_section` would use the **default** tier. The full blocks still use normal C runtime dispatch. Rust calls its existing `block::section` through a shim appended **only** to the archived scratch source. Rust portable here means default x86-64 code generation **with** `merge-simd` and its runtime dispatch, not scalar Rust. The native build is separate, with `-Ctarget-cpu=native`. No ledger, Tracy, validation changes, or production source edits.
- Before the counts, every build emitted **all** output for each case. All three outputs matched byte-for-byte (`filecmp` with `shallow=False`); the nine block emissions also matched the manifest plaintext lengths and SHA-256. Block runs reject an intermediate block with a one-bit checksum change; both section routines reject an invalid code-length table. Each build materializes output in its own reused buffers; the block calls still verify every checksum, and both modes consume first/middle/last bytes of each output and verify pass fingerprints. These fingerprints make output observable during timing but are not substitutes for the prior exact comparisons. C uses its normal output/scratch padding; Rust retains initialized `Vec` storage and enforces exact run consumption, which the C section entry point does not require in the same way. Section-only cache locality is not identical to alternating literal decode, token decode and reconstruction in the full block.

## Timing validity

Ryzen 7 5800X, Linux `7.1.0-debplus.1`, perf `7.3.4`; Rust `1.102.0-nightly (bdf712ee8)`, GCC `16.2.0`. Optimized Release code with line tables (Rust locked Cargo builds, no stripping; C `-O3 -DNDEBUG -g`, native per-ISA objects, no LTO). The extra Rust CPU flag enables features including POPCNT and AVX2, but this is a whole-build comparison, not a test of just one instruction.

All verification, timing and profiles took `.test-agent/zxc-perf.lock`. Three single-threaded resident processes per case ran on CPU 3; only one worked at a time after startup, with each request following the previous response. A startup pass and a separate warmup prefault outputs and inputs. There are two shuffled sets of all six orders of the builds, with a fixed seed. PMU events attach to the resident PID; `perf stat` starts disabled, acknowledges enable before the request, and acknowledges disable after the result. Counters include the small userspace request and reply protocol but exclude archive open, verification, decoding warmup, launch and teardown. Worker wall and thread CPU clocks enclose only the replay loop. Each request repeats the same workload across builds (`single` 2/5/22, `mozilla` 8/17/85, `sao` 52/95/540, ordered block/literal/token). The separate profiles use seven times those repeat counts.

All 324 grouped `{cycles:u,instructions:u}` requests returned both nonzero counters at 100.00% reported running time. Neither kernel cycles nor branch/cache counters were collected; the experiment cannot apportion stalls or memory bandwidth from PMU counts. CPU 11 (the SMT sibling) was unreserved; boost was enabled, governor `powersave`. One-minute load went from 50.36 to 22.34. Per-case/build median off-core elapsed was 0.06–16.64%; thread CPU and user cycles agree in direction, but neither removes contention while scheduled. In particular, the absolute wall rates are lower than in the earlier run; the long-frame *relative* CPU gap is consistent. No claim about a reserved core, multithreaded CLI throughput, or malformed-input performance follows.

## Reproduce

Evidence: [all counter requests and verification identities](decode-replay-data/perf-counts.json.gz), [raw perf script sample exports, section indexes, and dispatch evidence](decode-replay-data/perf-profiles.tar.xz), [inline attribution counts](decode-replay-data/perf-sites.tsv), [build log](decode-replay-data/perf-build.log.gz) and [per-ISA C compile commands](decode-replay-data/perf-c-compile.json.gz). The sample tarball includes profile headers, self-symbol reports, per-sample period/IP/DSO offset/source line, loss checks and the offline `classify-sites.py` used for the attribution. Original `perf.data` and the matching ELF files are in `.test-agent/decode-perf-sol/{main,symbolized}` on the measurement host.

From the repository root, supply `UPSTREAM_REPO` with the C pin present as a Git object. The build reads both pins via `git archive` and writes only into the new scratch build directory. If the fixture files are not available, follow [the original preparation recipe](decode-pool.md#method-and-evidence). The driver rejects archives that do not match the tracked manifest. Requires Linux x86-64 AVX2, an available pinned CPU, and `perf` with same-user PID attachment, `--control` and JSON stat support. Unsupported or multiplexed counters fail the run rather than estimate a value.

```sh
DATA=doc/profiling/decode-replay-data
RUN=.test-agent/decode-perf-repro
FIXTURES=.test-agent/decode-pool/fixtures
mkdir -p "$RUN"
bash "$DATA/build.sh" "$RUN/build" "$UPSTREAM_REPO" perf >"$RUN/build.log" 2>&1
python3 -B "$DATA/perf.py" "$FIXTURES" "$RUN/build" "$RUN/main" --cpu 3
python3 -B "$DATA/reduce.py" "$RUN/main/perf.json"
perf report --stdio --no-children --percent-limit 1 -s dso,symbol \
  -i "$RUN/main/single-lit-rust.perf.data"
perf script --show-lost-events -F period,ip,dsoff,dso,sym,symoff,srcline \
  -i "$RUN/main/single-lit-rust.perf.data"
```
