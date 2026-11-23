---
type: Measurement
title: Keep the three-bit x86 flat Huffman shuffle
description: Paired section, checked-block and eight-worker CLI measurements of an exact six-byte shuffle on the long C7 frame and sao control.
resource: /doc/profiling/flat-unpack0.astra.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df9f765f0ffel0TKk8pDiPlkN2', title: Implemented and tested the bounded shuffle and compared the archived builds under the perf lock }
  - { resource: /doc/profiling/decode-perf0.sol.md, title: Width-three attribution and predicted scale of the checked-block result }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Identities of the original C7 frames and plaintext }
---

# Keep the three-bit x86 flat Huffman shuffle

**Keep the kernel.** On the long C7 frame, the normal portable Release build used **8.1% fewer cycles per Huffman literal** and **4.4% fewer cycles per checked plaintext byte**. The latter includes reconstruction and checksum verification; 23 of 24 pairs favored the candidate. The `sao` checked-block control did not resolve a change. The eight-worker CLI measurements were too variable to establish a throughput gain on either frame.

The [earlier profile](decode-perf0.sol.md#one-next-candidate-3-bit-flat-unpack) predicted about 2% in checked blocks if width-three unpack cost halved. Before timing, the keep rule was a paired median reduction of at least 3% in long-literal cycles and 1% in checked-block cycles **or** eight-worker elapsed, with both qualifying intervals below zero, exact outputs, and no clear control regression. No length threshold was fitted. The checked-block result clears the gate independently of CLI timing.

## Original sections and checked blocks

Portable x86-64 Release, runtime SIMD enabled on both sides. Sections use decoded symbols as the denominator; checked blocks use plaintext bytes. Absolute numbers are medians for each build. Deltas are medians of **within-pair** ratios, so they need not equal the ratio of the displayed medians.

| C7 archive and replay | Items/pass | Parent cycles/B | Shuffle cycles/B | Paired cycles Δ [interval] | Instructions/B, parent → shuffle |
| --- | ---: | ---: | ---: | ---: | ---: |
| Long frame, literals | 380 | 3.681 | 3.421 | −8.10% [−9.88, −5.22] | 16.170 → 14.797 |
| Long frame, tokens | 405 | 4.643 | 4.422 | −5.16% [−6.32, −3.49] | 19.219 → 18.209 |
| Long frame, checked blocks | 405 | 2.155 | 2.071 | −4.36% [−5.18, −3.40] | 8.928 → 8.644 |
| `sao`, literals | 6 | 3.706 | 3.667 | −2.98% [−4.22, −2.02] | 16.760 → 16.406 |
| `sao`, tokens | 14 | 4.121 | 3.942 | −1.64% [−6.46, +3.88] | 14.575 → 14.515 |
| `sao`, checked blocks | 14 | 3.155 | 3.189 | −0.22% [−4.01, +7.38] | 12.572 → 12.479 |

The long-literal improvement was present in each chronological half (−6.16%, −9.66%). Thread CPU for the checked long frame fell from 0.573 to 0.551 s/GB (paired −4.26%, interval [−5.25, −3.40]). `sao` has much less width-three literal work; the smaller instruction reduction is consistent with that prior observation. Its checked-block interval does not exclude regressions of several percent.

A second, separately paired replay compiled **both** sides with `-Ctarget-cpu=native`. Long literals fell from 3.236 to 3.001 cycles/symbol (paired −7.72%, [−8.42, −5.69]), tokens from 5.851 to 5.581 (−4.94%, [−6.96, −1.73]), and checked blocks from 2.105 to 2.031 cycles/B (−2.24%, [−4.43, −1.50]). All three native `sao` intervals included zero; checked blocks were −0.78% [−5.95, +1.04]. Native and portable binaries were measured at different times: compare **within** each pair, not across builds. All 576 replay requests across both experiments returned both PMU counters at 100% running time.

## Eight-worker CLI

Fresh `zxc decode --threads 8 ARCHIVE` processes writing to `/dev/null`, pinned to CPUs 0–7 (eight physical cores). Each series uses 12 balanced AB/BA pairs after warmup, interleaving the two cases. All four binary/archive combinations first emitted the expected plaintext length and SHA-256.

| Archive | Series | Parent ms | Shuffle ms | Paired elapsed Δ [interval] |
| --- | ---: | ---: | ---: | ---: |
| `single.zxc` | 1 | 28.13 | 27.23 | −4.7% [−6.2, +5.3] |
| `single.zxc` | 2 | 40.10 | 42.27 | −2.3% [−8.9, +19.3] |
| `sao.zxc` | 1 | 5.49 | 6.38 | +5.6% [−18.6, +16.1] |
| `sao.zxc` | 2 | 7.46 | 7.68 | +6.1% [−11.7, +17.1] |

## Kernel, dispatch and bounds

The [unpacker](../../crates/codec/src/entropy/flat.rs) selects the new code only for eight-entry tables, with at least one full 16-symbol group. It uses the existing optional `merge-simd` dependency (on by default in the CLI). `fearless_simd::Level::new()` caches CPU detection. The mixed-node merge dispatches to the best available tier; this kernel asks for its `as_sse4_2()` proof once per eligible subtree. That tier includes the required byte shuffle. Higher x86 tiers use the same kernel; SSE2-only and other targets use the old scalar unpack, as does the no-feature build.

Each iteration takes a `&[u8; 6]` and a `&mut [u8; 16]`. It copies the input into a zeroed vector, shuffles eight little-endian windows into word lanes, aligns one six-bit pair per lane, then shuffles the lookup table with the separated three-bit indices. The caller already bounds the entire run. The final 0–15 symbols go to the existing scalar routine; no read from following bytes, padding contract, or unsafe Rust is needed. Other widths and validation stay on their existing paths.

In the **portable binary actually measured**, the group loop has a four-byte `movd`, a two-byte `pinsrw` at offset four, two `pshufb`s, and one 16-byte `movdqu` store. The table load and constants are outside the loop; its sole branch counts groups. A separate CPU-3 `perf record` on the candidate long literals placed 2.10% of samples in the `Sse4_2::vectorize` specialization for `flat::three_groups` (zero lost samples). This verifies the dispatched path, rather than inferring it from CPU flags.

The [differential test](../../crates/codec/src/entropy/tests.rs) reads individual wire bits as its oracle. It varies all input/output alignments modulo 16, arbitrary table bytes and unused padding bits, lengths 0–80 and longer runs through 4,097, with slices ending at their allocations. The tree test also checks truncated and trailing data across full-group boundaries. All 33 codec tests passed with portable SIMD, without default features, and with native Release SIMD; the extended malformed test passed in all three. The exact-allocation test also passed Release AddressSanitizer. Formatting passed. Strict Clippy flagged an existing `new_instead_of_clear` at `block.rs:36`; allowing that lint for the check left no warnings.

## Measurement and reproduction

Both builds came from `git archive 09c0c9284cbebde06c53733f01827ff60c996efb`; only `crates/codec/src/entropy/flat.rs` was overlaid for the candidate. Builds used `cargo --locked`, Rust 1.102.0-nightly (`bdf712ee8`, LLVM 23.1.6), no ledger or Tracy. The normal CLI was built before adding the [existing replay entry point](decode-replay-data/section-entry.rs) to the archived source. Replay has line tables. Archive indexes refer to original section bytes, including code lengths. Before counts, the two workers emitted byte-identical sections and checked blocks; the plaintext length and SHA-256 matched the manifest. Both rejected an invalid table and a changed intermediate checksum. The same count, fingerprint, and rejection checks applied to every resident worker.

All timings and the dispatch profile took `.test-agent/zxc-perf.lock`. The [existing perf driver](decode-replay-data/perf.py) pinned resident workers to CPU 3 and counted grouped `{cycles:u,instructions:u}` only around warmed equal-work requests. Per case: 12 shuffled sets of both orders, 24 pairs. Repeats per request (block/literal/token): 2/5/22 for `single`, 52/95/540 for `sao`. The [CLI driver](decode-pool-data/bench.py) measured whole-process elapsed time. Intervals for paired medians are unadjusted two-sided order-statistic intervals (97.7% for 24 pairs; 96.1% for 12, assuming independent rounds). This 5800X was shared: SMT sibling 11 was unreserved, boost on, `powersave`, Linux 7.1.0-debplus.1, perf 7.3.4. One-minute load fell from 23.31 to 18.65 over the portable replay. Pinning and pairing do not remove contention while scheduled.

The [evidence bundle](decode-replay-data/flat3-trial.tar.xz) contains the raw counter/CLI JSON, exact-output and binary identities, codegen and dispatch evidence, test/build logs, and small scratch overrides of the existing recipes. To reproduce from the repository root with the [original fixtures](decode-pool.md#method-and-evidence), use a fresh directory:

```sh
RUN=.test-agent/flat3-repro
FIX=.test-agent/decode-pool/fixtures
mkdir "$RUN"
tar -xJf doc/profiling/decode-replay-data/flat3-trial.tar.xz -C "$RUN" build.sh pair.py
git show 845a76c6a9873535ccf6e3d3a5cab98a049eb165:crates/codec/src/entropy/flat.rs > "$RUN/flat.rs"
bash "$RUN/build.sh" base
bash "$RUN/build.sh" candidate "$PWD/$RUN/flat.rs"
python3 -B "$RUN/pair.py" perf "$FIX" "$RUN" "$RUN/counts" --cycles 12 --no-record
python3 -B "$RUN/pair.py" reduce "$RUN/counts/perf.json"
taskset -c 0-7 python3 -B "$RUN/pair.py" cli "$FIX" "$RUN/cli-base" "$RUN/cli-candidate" "$RUN/cli" --pairs 12
NATIVE=1 bash "$RUN/build.sh" base
NATIVE=1 bash "$RUN/build.sh" candidate "$PWD/$RUN/flat.rs"
python3 -B "$RUN/pair.py" perf "$FIX" "$RUN/native" "$RUN/native-counts" --cycles 12 --no-record
```
