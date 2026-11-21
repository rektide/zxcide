---
type: Report
title: Silesia profiling status and eight-point decode sweep
description: Full-corpus native CLI encode and fixed-archive decode on the Ryzen 7 5800X, compared with pinned C and the saved Rust baseline.
resource: /doc/profiling/silesia-status-20261120.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e04a90c09ffeJ4gVdSQ4JhIki6', title: Ran the eight-point Silesia sweep and reformatted the upstream comparisons side by side }
  - { resource: /doc/profiling/pipeline-comparison.sol.md, title: Corrected two-file baseline and Rust/C pipeline map }
  - { resource: /doc/profiling/pivco-worker.md, title: Isolated buffer-reuse effect and retention costs }
  - { resource: /doc/profiling/decode-waits.md, title: Four-worker input-side and ordered-delivery evidence }
  - { resource: /doc/profiling/dense-literal-savings.sol.md, title: Rejected Dense literal savings threshold and fixed decoder controls }
---

# Silesia profiling status — 2026-11-20

**Dense encodes the full Silesia corpus in 15.52 s versus pinned C7's 29.86 s, with archives 0.49% larger.** Rust serial decode improved versus the saved build on the same C7 and Dense archives. Parallel Rust closes some of the gap on separate files, but pinned C is still clearly ahead on the concatenated workload. Extra worker counts expose useful points between the defaults and higher CPU costs at 16 workers, not a new default. This was a busy, non-isolated host; use the large effects and consistent CPU changes, not small wall-time rankings.

## Encode: 12 files, 211,938,580 plaintext bytes

| Rust mode | Upstream | Rust archive bytes | Upstream archive bytes | Rust size Δ | Rust / C wall s | Rust / C CPU s/GB |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Split | C3 | 97,412,320 | 99,494,620 | −2,082,300 (−2.09%) | 1.247 / 1.112 | 4.850 / 5.736 |
| Dense | C7 | 70,459,885 | 70,113,423 | +346,462 (+0.49%) | 15.519 / 29.864 | 71.121 / 126.596 |

These are reference points, not equivalent modes. C1 is the fast/large end of this sweep: **129,792,391 B** in **0.405 s** (61.24% of input). The generated report compares both Rust modes against **all** selected C levels.

- **Dense versus C7:** 1.92× the throughput by median wall, with 44% less child CPU per GB. Dense took less wall and less CPU in all seven matched passes. The earlier [`dickens` + `sao` result](pipeline-comparison.sol.md#what-the-corrected-run-establishes) was only 1.13× and 0.78% smaller. Those two files are still 0.78% smaller and 1.19× as fast in this run; they weren't representative of the full-corpus gap. The largest size deficits are still `webster` (+151,370 B) and `mozilla` (+129,936 B).
- **Split versus C3:** 2.09% smaller and 15% lower median CPU cost. Separate wall medians favor C3 (1.11 s vs 1.25 s), but Split won four of seven matched passes and both ranges were wide: 1.03–2.39 s for C3 and 1.00–2.69 s for Split. No stable wall winner here.
- **Current versus saved Rust:** all 12 Split and all 12 Dense files match archive SHA-256 exactly. Between these Rust revisions the codec changes are decoder-side only; the encode timing difference is not evidence of an encoder improvement.

## Decode: same archives, eight worker points

Cells are **Rust / C median wall ms**. Each column uses one set of fixed archives for both decoders; comparing *across* columns changes the input bytes. C1/3/7 name the upstream archive sources; Split and Dense are Rust-produced. See the full generated tables for per-file breakdowns, before-build rows, throughput, CPU and ranges.

### Separate launches (12 files per pass)

| Workers | C1 | C3 | C7 | Split | Dense |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 133.2 / 138.3 | 113.3 / 118.3 | 169.0 / 146.8 | 137.8 / 188.5 | 199.2 / 190.0 |
| 2 | 183.7 / 99.1 | 90.6 / 101.7 | 119.7 / 115.5 | 106.6 / 96.1 | 239.8 / 108.4 |
| 3 | 151.7 / 140.4 | 121.4 / 89.1 | 121.0 / 88.9 | 88.1 / 97.4 | 153.9 / 113.7 |
| 4 | 88.0 / 76.2 | 79.7 / 75.3 | 126.0 / 118.6 | 111.6 / 90.9 | 112.6 / 99.4 |
| 6 | 140.1 / 80.6 | 90.5 / 110.6 | 132.0 / 134.4 | 132.0 / 90.4 | 130.5 / 103.5 |
| 8 | 174.9 / 105.4 | 93.9 / 109.8 | 114.0 / 164.2 | 117.4 / 127.2 | 125.9 / 137.3 |
| 12 | 111.8 / 133.0 | 103.3 / 119.4 | 151.8 / 146.7 | 142.1 / 118.4 | 145.8 / 160.1 |
| 16 | 131.9 / 189.8 | 124.3 / 111.9 | 133.0 / 163.8 | 117.4 / 160.2 | 145.2 / 146.1 |

### One concatenated launch (same 12 frames)

| Workers | C1 | C3 | C7 | Split | Dense |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 91.8 / 112.1 | 105.8 / 80.7 | 173.7 / 85.1 | 99.8 / 78.6 | 150.0 / 100.6 |
| 2 | 58.0 / 31.1 | 52.7 / 39.0 | 90.6 / 63.9 | 86.6 / 46.2 | 110.4 / 86.3 |
| 3 | 49.4 / 27.4 | 61.0 / 34.7 | 86.5 / 42.7 | 53.2 / 35.6 | 81.6 / 41.3 |
| 4 | 50.8 / 31.0 | 55.7 / 38.0 | 132.6 / 33.4 | 59.9 / 32.9 | 66.2 / 32.8 |
| 6 | 61.6 / 44.3 | 62.3 / 22.2 | 99.8 / 27.8 | 53.4 / 37.2 | 98.0 / 35.7 |
| 8 | 69.4 / 32.6 | 73.0 / 20.8 | 76.9 / 25.1 | 61.5 / 29.9 | 91.7 / 32.8 |
| 12 | 61.8 / 37.0 | 60.3 / 44.7 | 62.0 / 44.2 | 95.8 / 50.8 | 126.0 / 41.8 |
| 16 | 104.6 / 43.6 | 81.7 / 37.4 | 86.7 / 45.1 | 202.0 / 39.2 | 98.2 / 43.2 |

**What the extra points show.** For Rust's separate launches, the lowest medians were at 3 workers for Split, 4 for C1/C3/Dense, and 8 for C7. On concat, 2 for C3, 3 for C1/Split, 4 for Dense and 12 for C7. These are observed minima, not tuning recommendations. On separate C7 the Rust 8-worker range was 71–387 ms and it was the fastest Rust point in only two of seven passes. It beat C at the same count in six of seven passes, but C's own best median was 89 ms at 3 workers, versus Rust's 114 ms at 8. On concat C7 at 8 workers, C took **25.1 ms** versus **76.9 ms** for Rust on identical bytes; C was faster in all seven passes. Even comparing each decoder's best median, C was 2.47× faster on this bundle. No point at 16 workers had the lowest median for either decoder on either layout.

**CPU cost matters.** On separate C7 files, the current Rust medians went from **169 ms / 0.777 CPU s/GB** at one worker to **114 ms / 1.501 CPU s/GB** at eight and **133 ms / 1.811 CPU s/GB** at 16. On separate Dense, four workers took **113 ms / 1.279 CPU s/GB** versus **199 ms / 0.873 CPU s/GB** serial. The 16-worker point used more child CPU than eight in all seven passes for each of C7, Split and Dense in separate launches. For throughput tuning, rerun 3/4/6/8 first on the deployment workload and a quiet machine. Keep the default serial path for CPU cost and don't infer a universal worker count or a slot-size change from these tests.

## Decode: how current Rust compares with the saved Rust build

Still at one worker on fixed archive bytes. Wall and CPU columns are full-corpus medians; the last column instead compares current and before within each shuffled pass and takes the median of those changes. Small differences are not resolved.

| Layout / archive | Before → current wall ms | Before → current CPU s/GB | Matched wall change (faster passes) |
| --- | ---: | ---: | ---: |
| Separate C7 | 206.0 → 169.0 | 0.877 → 0.777 | −12.1% (7/7) |
| Separate Dense | 228.6 → 199.2 | 1.008 → 0.873 | −4.4% (5/7) |
| Concat C7 | 217.1 → 173.7 | 0.960 → 0.781 | −11.5% (6/7) |
| Concat Dense | 173.1 → 150.0 | 0.811 → 0.703 | −15.1% (7/7) |

Serial CPU fell in all seven passes on the separate C7 and Dense archives. The span from the saved Rust build includes both the short SPL16 extension path and Huffman byte-buffer reuse; this sweep does not isolate them. At higher worker counts, before/current wall results flip by source, layout and worker count. The same saved C and Rust binaries also took 16.32/21.76 ms on the C7 `dickens` + `sao` serial subset in this run versus 13.12/17.92 ms in the [earlier run](pipeline-comparison.sol.md#what-the-corrected-run-establishes). That drift is why the baseline was remeasured alongside current, not subtracted from old tables.

## Which profiling decisions still hold

- **Shipped, fixed archive decode:** [SPL16 batching and short extensions](dense-decode.md#follow-up-batch-two-ordinary-spl-sequences) address reconstruction; [worker-owned Huffman byte buffers](pivco-worker.md) cut serial Dense `sao` wall by 11–13% in the isolated experiment, with no repeatable four-worker wall gain. Retention reached 661,111 bytes on serial `sao` before a smaller frame released the buffers. That is scratch capacity, not process memory.
- **Not shipped:** the [PivCo traversal FIFO trial](pivco-queue.md) removed 765 of 768 queue allocations or growths on 4 KiB mixed input but missed its elapsed gate. The [alternating merge buffers](pivco-scratch.md) removed copy-back but missed serial and component performance gates. The [Dense literal threshold](dense-literal-savings.sol.md) helped `sao` on both decoders, but did not clear the 5% full-corpus serial wall target; the default archive policy stayed unchanged.
- **Next diagnosis:** on four-worker `mozilla`, workers were out of jobs while the Rust coordinator prepared input for 2.02–2.19 ms/frame, versus 0.29–0.46 ms of conservatively identified order-induced idle. [Separate coordinator on-CPU read/resize time from off-CPU time](decode-waits.md#ranked-disposition) before changing slots or the scheduler. The new sweep measures end-to-end scaling, not the source of this wait.
- **Different workloads:** the [logical byte ledger](ledger.md), [small-block dictionary decode](dictionary-decode.md) and [encoder prefix seeding](dictionary-encode.md) studies remain the evidence for dictionaries and ranges. The [component and Criterion baselines](README.md#compare-native-and-component) have not been rerun here; this is not a new Wasmtime, API, transport or peak-memory scorecard.

## Measurement and replay

Ryzen 7 5800X, Linux `7.1.0-debplus.1`, 8 physical cores / 16 logical CPUs, all eligible, no fixed CPU placement and no finite cgroup CPU quota discovered. Host load, boost and file-cache residency were not controlled; one-minute load at runner start was 29.7. C `5719a96f` uses native Release with `-TN`. Current Rust `956e0000` and saved Rust `f76005b0` use rustc `1.101.0-nightly (822182db4)`, portable Release with default CLI features (`merge-simd`) and no extra rustflags. The build records list working-tree changes only outside codec source. Rust runs serial at one worker, Crossbeam at higher counts, with two slots per worker in this sweep. Worker count is not total process thread count or a guarantee of dedicated cores. Checksums are on; each file is one frame with the default 512 KiB block limit. C encodes at `-T1`; Rust encodes serially. Only decode workers vary.

One shuffled warmup and seven measured passes, 18,144 total timed/warmup launches. Separate-file figures sum all 12 launches **within each pass**, then take the median. CPU cost is median child user+system CPU seconds per decimal GB of plaintext, summed over all CLI threads separately from wall time. Range is observed min–max, not a confidence interval. All 2,184 decode combinations passed exact output verification through EOF before timing; timed runs discard stdout and require successful exit. The concatenated control appends those same frames in manifest order; it changes launch, input-open, and pool lifetime effects together, so it cannot isolate startup or diagnose order stalls.

Run `20261120T234306Z-edf3ff614784` (23:43–00:01 UTC). Full per-file tables, saved binaries and raw observations are in the local run directory, with regenerated upstream tables in `report-upstream.md` and tabular data in `summary.tsv`. `raw.json` SHA-256: `e77db0e7edc6076a5a33c5bcc80a346163509025b9a767030d07417cdb0c03f1`. Re-render without building or measuring:

```sh
RUN=target/profiling-status-20261120/comparison-report/runs/20261120T234306Z-edf3ff614784
python3 -B xtask/comparison_tables.py "$RUN/raw.json"
python3 -B xtask/comparison_tables.py --format tsv "$RUN/raw.json" > summary.tsv
```

To repeat the sweep from the measured source, substitute the trusted [completed baseline run](pipeline-comparison.sol.md#what-the-corrected-run-establishes) for `BASELINE_RUN`. Omit `--before` if only comparing against C. This is an explicit sweep, not a change to benchmark or CLI defaults.

```sh
cargo xtask comparison-report --levels 1,3,7 \
  --decode-workers 1,2,3,4,6,8,12,16 --concat --before "$BASELINE_RUN"
```
