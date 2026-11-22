---
type: Measurement
title: Checked C7 block replay on one core
description: Paired in-memory Rust and upstream C worker-kernel measurements, with separate probes of section decode, SPL16 reconstruction and checksum.
resource: /doc/profiling/decode-replay0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_dfa3dcfb4ffexifC8qSaQ6ZHUG', title: Built and validated paired replay of the checked C7 blocks; measured the clean kernels and separated the three decode stages }
  - { resource: /doc/profiling/decode-pool.md, title: Pool reuse and previous fixed-archive CLI comparison }
  - { resource: /design/decode-throughput/design0.astra.md, title: Design for isolating block work from process and frame orchestration }
  - { resource: /doc/profiling/pipeline-comparison.sol.md, title: Rust and C paths, and prior decoding controls }
  - { resource: 'https://github.com/hellobertrand/zxc/tree/5719a96f778226868b29dba621a8c4e302b74d7c', title: Upstream performance pin }
  - { resource: 'https://github.com/rektide/zxc-rs/tree/8b3b88974f40b46808ce13e2f5f2306c11a1bc19', title: Rust baseline revision with pool reuse }
---

# Checked C7 block replay on one core

**The CPU gap survives removal of the frame drivers.** On the exact 405 checked blocks from the long C7 frame, C used **38.9% / 39.8% less thread CPU** than portable Release Rust in two independent paired series. That is about **0.22–0.23 CPU seconds per plaintext GB**, with C at 2.64–2.65 GB/s wall and Rust at 1.60–1.63 GB/s. This is larger than the **21–22%** C child-CPU advantage in the [historical eight-worker CLI comparison](decode-pool-data/gate.md) on those same bytes; it is not an estimate of what share of that multi-threaded difference comes from the codec. The timed work, active thread count, output handoff, cache locality and CPU metric all changed. In separate instrumented replays, section preparation (mostly Huffman work) accounted for the larger part of the long-frame difference; SPL16 reconstruction also mattered. The checksum did not explain it.

## Clean replay

Each cell lists series 1 / series 2. MB/s is decimal plaintext throughput; CPU s/GB is user+system **for the one replay thread** per decimal plaintext GB. Differences are medians of within-pair ratios, with at-least-95% order-statistic intervals in brackets (unadjusted, assuming independent pairs). Negative favors C; B is the second Rust process in A/A.

| Workload | Rust wall MB/s | C (or B) wall MB/s | Rust CPU s/GB | C (or B) CPU s/GB | Paired CPU change, C (or B) vs Rust |
| --- | ---: | ---: | ---: | ---: | --- |
| Long frame | 1,635 / 1,600 | 2,643 / 2,646 | 0.573 / 0.577 | 0.351 / 0.348 | −38.9% [−39.5, −38.5] / −39.8% [−40.6, −38.4] |
| `mozilla` | 1,328 / 1,481 | 2,139 / 2,429 | 0.682 / 0.635 | 0.419 / 0.389 | −38.7% [−41.5, −36.9] / −38.8% [−39.5, −38.3] |
| `sao` | 1,089 / 1,149 | 2,039 / 2,164 | 0.836 / 0.815 | 0.443 / 0.431 | −47.0% [−48.1, −45.0] / −47.0% [−47.5, −46.5] |
| Long frame, Rust A/A | 1,701 / 1,659 | 1,696 / 1,642 | 0.588 / 0.569 | 0.590 / 0.570 | −0.34% [−1.83, +1.01] / +0.26% [−0.21, +0.72] |

Long-frame paired wall change was −38.6% [−39.5, −38.3] / −40.5% [−41.6, −38.2]. The machine was shared (initial/final load averages over one minute 21.6/15.3). Median elapsed lost off-core was roughly 6–8% on the long Rust/C pairs. Affinity and pairing stabilize the comparison, but they do not reserve the SMT sibling, normalize boost or rule out correlated host load.

## Where the extra time went

Two separate **12-pair** series used scratch builds with clocks around (1) coded section parsing and decode, ending when literals/tokens are ready; (2) reconstruction, including loop setup, output growth/initialization if needed and trailing literals; and (3) reading/checking the stored checksum. No instrumentation was used in the primary table. The stage builds reproduced the long-frame thread-CPU advantage (C −40.0% / −38.6%). These stage numbers are paired **elapsed seconds per plaintext GB**, not CPU seconds. Positive means Rust took longer; the order-statistic intervals apply to the paired differences.

| Workload | Section prep, Rust − C (S1 / S2) | SPL16 reconstruction, Rust − C (S1 / S2) | Checksum, Rust − C (S1 / S2) |
| --- | --- | --- | --- |
| Long frame | +0.159 [+0.145, +0.185] / +0.161 [+0.144, +0.179] | +0.099 [+0.090, +0.131] / +0.101 [+0.089, +0.109] | −0.003 [−0.005, +0.002] / −0.007 [−0.010, −0.005] |
| `mozilla` | +0.190 [+0.182, +0.197] / +0.199 [+0.186, +0.217] | +0.082 [+0.074, +0.085] / +0.080 [+0.071, +0.101] | −0.006 [−0.007, −0.003] / −0.006 [−0.009, +0.001] |
| `sao` | +0.207 [+0.197, +0.214] / +0.201 [+0.194, +0.216] | +0.216 [+0.212, +0.233] / +0.221 [+0.209, +0.230] | −0.005 [−0.005, +0.003] / −0.004 [−0.006, −0.001] |

Section prep cost about **0.257 / 0.265 elapsed s/GB in Rust** vs **0.100 / 0.110 in C** on the long frame. Reconstruction cost **0.353 / 0.363** vs **0.248 / 0.271**. `mozilla` tilts more toward section work, while `sao` separates the two roughly evenly, despite having no extended matches. This neither singles out the Rust match-extension path nor reduces every workload to a Huffman bottleneck. Checksum was about **0.034–0.045 elapsed s/GB** across both decoders and all cases; if anything, Rust was slightly faster in this cache regime.

These partitions are not an instruction-level cost model. Section prep also includes header/extent checks, tree build, RLE and scratch work; C's token-Huffman specialization parses the SPL descriptors twice. Elapsed stages include preemption (9–12% median off-core in long stage samples), clock insertion can alter code generation, and paired medians need not add. In particular, do not sum or subtract these elapsed values from the clean CPU table. The result identifies a stage to isolate, not which Huffman loop/ISA deserves a change.

A separate symbol-level `perf` capture of each uninstrumented binary confirms the **C AVX2** block variant and Rust's **AVX2** PivCo merge on this core. In Rust, SPL16 reconstruction was the largest single symbol (51% of sampled cycles); the AVX2 merge, flat unpack, tree build and `decode_into` together took 35%. A large cost *within Rust* need not be the largest *gap versus C*. The captures include startup and warmup in addition to 32 long-frame passes, and the inlined C paths do not yield comparable symbol-level stage totals. See the [separate captures](decode-replay-data/perf-dispatch.txt).

**Next experiment:** replay only the original literal and token sections from the long frame, with `mozilla` and `sao` as controls. Validate the decoded sections byte-for-byte and report cycles per decoded symbol separately for literal and token tables. Compare portable Rust with `target-cpu=native` on the same pinned core, and retain C's normal ISA variant as the reference. In separate attribution runs, split tree construction from bit unpack and mixed-node merge. If the portable/native contrast does not materially close the residual, target the step and section class that account for the absolute gap before changing any kernel. Confirm any later win on checked full-block replay and the eight-worker CLI; this replay does not justify another pool change or weaker verification.

## What ran

- **The original bytes.** The [pool study's fixture manifest](decode-pool-data/fixture-manifest.json) binds the **211,938,580-byte** long plaintext and **70,139,019-byte** C7 single frame; the long frame is not the 12-frame concat. Two unchanged member frames give contrast: `mozilla` has 98 SPL16 blocks and prior evidence of frequent length extensions; `sao` has 14 SPL16 blocks, no extended matches in the [prior census](dense-decode.md), and more Huffman symbols per output byte (0.318 vs 0.255 on `mozilla` and 0.222 on the long frame). This does not imply every `sao` literal is Huffman-coded: just 6 of its 14 literal sections are. The replay walks bounded views on the exact archive bytes in order. Both parse the prologue and validate physical data headers **before** timing. Both use frame-local ordinals starting at zero against the original checksum trailers.
- **The worker kernels, not the buffer API.** C calls the runtime dispatcher used by the [file worker](https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_driver.c#L437-L491), with `zxc_cctx_init(..., mode=0, checksum_enabled=1, dict_size=0)`. It gives the [checked block wrapper](https://github.com/hellobertrand/zxc/blob/5719a96f778226868b29dba621a8c4e302b74d7c/src/lib/zxc_decompress.c#L1598-L1672) the original 8-byte header and body, and uses its padded fast-output contract (`512 KiB + 2084`), not the exact-capacity buffer API tail. Rust calls [`BlockDecoder::decode`](https://github.com/rektide/zxc-rs/blob/8b3b88974f40b46808ce13e2f5f2306c11a1bc19/crates/codec/src/block.rs#L70-L136) with the parsed header and exact body slice, no dictionary and no separately supplied expected output size. It uses its initialized `Vec` scratch and logical prefix, not a copy of C's padded contract. The parallel Rust worker uses this decode via `decode_owned`; its truncate/swap and ownership handoff are deliberately **not** timed here. The [old C buffer probe](../../xtask/probes/decode.c) uses a checksum-skipping entry point and was not used.
- **Real plaintext, comparable consumption.** Each implementation reconstructs into private reused output memory, runs its normal check over the logical bytes, then folds the first, middle and last bytes and length of every block into the same observable fingerprint. No second full-size output copy or timed hash into a separate sink; this is not a retained whole-plaintext array. Each pass checks its byte total and fingerprint, and both implementations' fingerprints agree. Before *each measurement build* ran, both replay binaries emitted all their output and both stock CLIs decoded each archive through EOF: all 12 results matched the expected length and SHA-256. Each replay binary also flipped just the low bit of the stored checksum on the middle block (seed 202, 49 or 7), required its specific checksum-mismatch error, and restored the byte before the first complete pass. That check ran at each resident process startup too.
- **Pin and warmup.** Ryzen 7 5800X, Linux 7.1.0-debplus.1; one thread on logical CPU 3 (sibling 11 unreserved), boost enabled and `powersave` governor. Each series uses fresh resident processes for every group, one complete untimed startup pass plus an untimed warmup request, then 24 adjacent AB/BA-balanced pairs in independently seeded order. Requests replay one long frame, two copies of `mozilla`, or six copies of `sao`, without reinitializing decoder scratch. An archive fits neither entirely in private cache nor necessarily in LLC; repeated timings are prefaulted, not a claim of L1/L2 residency. The repository's exclusive `.test-agent/zxc-perf.lock` covers verification and all runs. Clock calls surround just the replay loops; archive load, preparse, allocation warmup, process start, pipe commands, and reporting are outside. Wall is `CLOCK_MONOTONIC`, CPU `CLOCK_THREAD_CPUTIME_ID`. Neither timer measures cold startup.
- **Remaining asymmetries.** C Release is GCC 16.2.0, `-O3 -DNDEBUG`, native build with ISA-specific kernels; Rust is rustc 1.102.0-nightly (`bdf712ee8`), locked Release, empty extra rustflags, CLI-default `merge-simd`. No ledger/Tracy in clean replay. The C wrapper re-reads type and size from the physical header; Rust checks exact body size and dictionary match and fits section scratch per call. Their scratch layouts, tail room, allocator behavior, validation and compiler lowering are not identical, and one reused output block can keep checksums hotter than a real slot window. Headers' checksums are not re-read each iteration, nor do the probes time footer size/digest checks (full stock CLI preflight does). No result for RAW/PAK, dictionaries, indexed or malformed archives beyond the checksum control, streaming latency or several active lanes.

## Reproduce

The [primary samples](decode-replay-data/clean.json.gz) contain all samples, exact verification and serve commands, case order, binary and input hashes, counts, environment and output identities. The [stage samples](decode-replay-data/stage.json.gz) are separate. [Build logs](decode-replay-data/build-logs.txt.gz) record toolchains and command lines; [C compile commands](decode-replay-data/clean-c-compile.json.gz) preserve the per-ISA flags. Probe sources: [Rust](decode-replay-data/replay.rs), [C](decode-replay-data/replay.c), [stage injection](decode-replay-data/instrument.py), [paired driver](decode-replay-data/bench.py), [reducer](decode-replay-data/reduce.py).

Run from the repository root, with both pinned git objects and the pool study's fixtures available. If needed, recreate the fixtures using the [original preparation instructions](decode-pool.md#method-and-evidence) and require the manifest identities. `UPSTREAM_REPO` can be the source cache of the original comparison or another Git clone containing `5719a96f`.

```sh
DATA=doc/profiling/decode-replay-data
RUN=.test-agent/decode-replay-repro
FIXTURES=.test-agent/decode-pool/fixtures
mkdir -p "$RUN"
bash "$DATA/build.sh" "$RUN/clean" "$UPSTREAM_REPO" clean >"$RUN/build-clean.log" 2>&1
python3 -B "$DATA/bench.py" "$FIXTURES" "$RUN/clean" "$RUN/clean" "$RUN/clean.json" --cpu 3
bash "$DATA/build.sh" "$RUN/stage" "$UPSTREAM_REPO" stage >"$RUN/build-stage.log" 2>&1
python3 -B "$DATA/bench.py" "$FIXTURES" "$RUN/stage" "$RUN/clean" "$RUN/stage.json" --cpu 3 --stage --pairs 12
python3 -B "$DATA/reduce.py" "$RUN/clean.json"
python3 -B "$DATA/reduce.py" "$RUN/stage.json"
```

For the separate dispatch capture (not a timing input):

```sh
for impl in c rust; do
  printf '32\n' | flock -x .test-agent/zxc-perf.lock perf record -q -e cycles:u -F 499 \
    -o "$RUN/$impl.perf.data" -- taskset -c 3 "$RUN/clean/replay-$impl" serve "$FIXTURES/single.zxc"
  perf report --stdio --no-children --percent-limit 1 -s dso,symbol -i "$RUN/$impl.perf.data"
done
```
