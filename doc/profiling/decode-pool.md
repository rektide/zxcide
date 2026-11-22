---
type: Measurement
title: Reusing decode workers across frames
description: Paired comparison of the frame-local and persistent Crossbeam worker pools on checksummed C7 archives.
resource: /doc/profiling/decode-pool.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-22 }
sources:
  - { resource: 'opencode://ses_e04a90c09ffeJ4gVdSQ4JhIki6', title: Measured the scoped pool against its parent, long single frame, and upstream C; confirmed all eight worker counts and regular-file output }
  - { resource: 'opencode://ses_dfa5efef0ffegNlsonhifPF2FE', title: Implemented and tested the bounded operation-scoped Crossbeam pool }
  - { resource: /design/decode-throughput/design0.astra.md, title: Independent design and predeclared performance gates }
  - { resource: /doc/profiling/silesia-status-20261120.md, title: Full historical worker sweep and original C7 bundle }
---

# Reusing decode workers across frames

**Keep `7d6492a0` for Crossbeam concatenated decode.** At eight workers it cut wall by **41–42%** and child CPU by **23%** over its parent (`8371b379`) on the same 12 checksummed C7 frames. The runtime trace went from **96 to 8 worker starts**. There was no corresponding single-frame win: both revisions start eight workers there, and paired wall deltas at eight workers were **+1.1% / −0.3%**. This is a faster multi-frame pipeline, not a faster block decoder.

The caller still drains verified output in order and validates the footer before reading the next frame. Crossbeam reuses the threads, bounded window, and scratch across equal block limits; a change of block limit drops them even if the next frame is empty. The worker count, slot budget, wire format, serial path and caller-owned I/O contract are unchanged. This trial did not add worker-local file reads or cross-frame work in flight.

## Primary gate: two 24-pair series

All throughput figures are decimal plaintext MB/s. `S1 / S2` denotes the two series; wall change is the median within-pair change, not a ratio of the throughput medians. Negative is faster.

| Layout | Workers | Before MB/s (S1 / S2) | Pool reuse MB/s (S1 / S2) | Paired wall Δ (S1 / S2) |
| --- | ---: | ---: | ---: | ---: |
| 12 frames, one process | 1 | 1,366 / 1,365 | 1,362 / 1,438 | −1.7% / −2.9% |
| 12 frames, one process | 4 | 3,044 / 3,234 | 3,968 / 4,571 | −24.0% / −26.6% |
| 12 frames, one process | 8 | 2,664 / 3,137 | 4,849 / 5,283 | −42.0% / −40.9% |
| 12 frames, one process | 12 | 2,575 / 2,713 | 4,186 / 4,821 | −41.9% / −42.9% |
| One long frame | 8 | 5,059 / 5,294 | 5,432 / 5,778 | +1.1% / −0.3% |
| 12 separate processes | 8 | 2,175 / 2,412 | 2,082 / 2,461 | +3.4% / +0.2% |

For the primary eight-worker comparison, the paired wall intervals were [−43.7%, −27.7%] / [−42.4%, −35.1%], well beyond the 5% target; child CPU fell 23.6% / 23.2%. The base/base control gave −4.7% / +3.7% (intervals [−11.1%, +0.8%] / [+0.0%, +19.3%], rounded). Host load was high; the primary effect is much larger than the A/A shifts. Separate-file results do not establish equivalence, in particular +3.4% / +4.3% at 12 workers with intervals spanning zero in both series. The long single frame is re-encoded, not a byte-preserving reframe: always compare decoders *within* a row. Upstream still used about 21–22% less CPU than old Rust on that one long frame at eight workers, where pool reuse had effectively no CPU impact.

## Confirmation: full concat sweep and regular-file output

The [wider check](decode-pool-data/confirm.md) repeated the 24-pair series with the same locked binaries and concat bytes. All seven parallel counts improved in both series; one worker remained on the serial path. The 12-worker separate-file retest moved to +0.5% / −1.8% wall, so the initial +3–4% slowdown was not reproduced. Neither single-frame nor separate-file controls prove equivalence to within 3%.

| Workers | Before MB/s (S1 / S2) | Pool reuse MB/s (S1 / S2) | Paired wall Δ (S1 / S2) |
| ---: | ---: | ---: | ---: |
| 1 | 1,447 / 1,437 | 1,444 / 1,428 | 0.0% / +1.0% |
| 2 | 2,368 / 2,345 | 2,712 / 2,761 | −12.6% / −15.3% |
| 3 | 3,076 / 3,039 | 3,836 / 3,831 | −20.2% / −19.8% |
| 4 | 3,389 / 3,274 | 4,526 / 4,602 | −24.8% / −26.1% |
| 6 | 3,437 / 3,520 | 5,534 / 5,264 | −36.7% / −33.0% |
| 8 | 3,139 / 3,065 | 5,545 / 5,420 | −43.3% / −43.3% |
| 12 | 2,737 / 2,737 | 4,565 / 4,902 | −42.8% / −41.7% |
| 16 | 2,476 / 2,417 | 4,226 / 4,366 | −41.0% / −42.2% |

In the **direct eight-worker comparison** (separate pairs from the rows above), Rust measured **5,470 / 5,201 MB/s** and upstream C **5,566 / 5,709 MB/s**. C's paired wall advantage was 6.1% / 6.9%, with intervals reaching zero after rounding; C still used **18.8% / 23.6% less child CPU**. Against the nearly 2× C throughput advantage over the old Rust build in the primary run, this identifies frame-local resource churn as a major cause on this bundle and setup. It does not establish wall parity or attribute the benefit to thread starts rather than retaining scratch and buffers.

With stdout redirected to a **regular file**, pool reuse saved **18.1% / 18.7% wall** at eight workers (two 12-pair series, both intervals below zero). C's wall advantage was not resolved, but C again used less CPU. This covers buffered writes, not `fsync` or durable output. See the [file-output table](decode-pool-data/file-output.md) and [raw confirmation](decode-pool-data/confirm.json.gz) / [file-output samples](decode-pool-data/file-output.json.gz). The large concat gains survived the wider sweep and sink change; no further scheduler change is implied. If CPU efficiency is the next target, use the [checked block replay design](../../design/decode-throughput/design0.astra.md#isolating-the-other-costs) to separate codec work from input and output costs.

## Method and evidence

Ryzen 7 5800X, Linux, CPUs 0–7 (distinct physical cores, SMT siblings not reserved). Same locked portable Release CLI build recipe for parent and trial, default `merge-simd`, empty rustflags; pinned C `5719a96f` is the original native Release comparator. Warmups and timing sequences are randomized within each series; the two members of each comparison are adjacent with balanced AB/BA order. Wall includes process launch and validating/decompressing to `/dev/null`; separate sums the 12 process times per observation. CPU is summed child user+system (`wait4`); peak RSS is the maximum for a CLI invocation, not scratch capacity (the parent's pre-exec footprint can dominate a small child's high-water mark). Median intervals are distribution-free order statistics but do not account for correlated host load. Trace-only `clone/clone3` counts are from separate invocations, not timed ones.

Before the primary series, all **115** executable/archive/worker combinations produced the exact plaintext length and SHA-256 through EOF; every timed child also exited successfully with empty stderr. Sol's implementation passed 66 workspace tests with all transports and 66 with default features disabled, and the pinned C interoperability matrix for serial and the five backends. No wire format, dictionary or checksum change was required. First-write and frame-completion p95 were not measured, so the result is a throughput decision, not a latency guarantee.

Artifacts: [complete primary table](decode-pool-data/gate.md), [raw paired samples and identities](decode-pool-data/gate.json.gz), [fixture manifest](decode-pool-data/fixture-manifest.json), [fixture preparation](decode-pool-data/prepare.py), [paired benchmark](decode-pool-data/bench.py). Each binary/exact test command is recorded in the raw samples. To reproduce, rebuild the two Rust revisions with `CARGO_ENCODED_RUSTFLAGS='' cargo build --release --locked -p zxc-cli`, saving each executable at a distinct path:

```sh
DATA=doc/profiling/decode-pool-data
python3 -B "$DATA/prepare.py" "$ORIGINAL_RUN" "$SILESIA_ZIP" "$FIXTURES"
taskset -c 0-7 python3 -B "$DATA/bench.py" "$FIXTURES" "$BASE" "$TRIAL" "$OUT"
taskset -c 0-7 python3 -B "$DATA/bench.py" "$FIXTURES" "$BASE" "$TRIAL" "$WIDE_OUT" --confirm --wide
taskset -c 0-7 python3 -B "$DATA/bench.py" "$FIXTURES" "$BASE" "$TRIAL" "$FILE_OUT" --confirm --real-output --pairs 12
```
