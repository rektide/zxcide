---
type: Report
title: Silesia profiling status and eight-point decode sweep
description: Full-corpus fixed-archive decode throughput on the Ryzen 7 5800X, compared with pinned C and the saved Rust baseline.
resource: /doc/profiling/silesia-status-20261120.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e04a90c09ffeJ4gVdSQ4JhIki6', title: Ran the eight-point Silesia sweep and made side-by-side decode throughput the primary view }
  - { resource: /doc/profiling/pipeline-comparison.sol.md, title: Corrected two-file baseline and Rust/C pipeline map }
  - { resource: /doc/profiling/pivco-worker.md, title: Isolated buffer-reuse effect and retention costs }
  - { resource: /doc/profiling/decode-waits.md, title: Four-worker input-side and ordered-delivery evidence }
  - { resource: /doc/profiling/dense-literal-savings.sol.md, title: Rejected Dense literal savings threshold and fixed decoder controls }
---

# Silesia profiling status — 2026-11-20

This sweep predates [Crossbeam worker reuse](decode-pool.md); see that follow-up for the current concatenated-decode comparison.

**Decode is closest on separate files.** On fixed C7 archives, the best Rust median was 1,859 MB/s vs upstream's 2,384 MB/s across the measured counts. On the concatenated frames, it was 3,416 vs 8,446 MB/s. This was a busy host; treat the exact worker rankings as tentative.

## Decode throughput by worker count

Both decoders read **identical C7 archive bytes** and were checked against the same plaintext. Throughput is decimal MB/s of plaintext across all 12 Silesia files. Separate launches one process per file; concat launches once on the same 12 frames. The generated report also shows all worker counts for C1, C3, Split and Dense archives, per-file results, and CPU cost.

| Workers | Separate Rust MB/s | Separate C MB/s | Concat Rust MB/s | Concat C MB/s |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 1,254.1 | 1,443.8 | 1,220.2 | 2,491.7 |
| 2 | 1,770.3 | 1,834.8 | 2,338.4 | 3,319.2 |
| 3 | 1,751.8 | **2,383.8** | 2,449.1 | 4,965.0 |
| 4 | 1,681.6 | 1,786.7 | 1,598.5 | 6,347.4 |
| 6 | 1,605.7 | 1,576.7 | 2,122.7 | 7,636.4 |
| 8 | **1,858.7** | 1,291.0 | 2,756.8 | **8,445.6** |
| 12 | 1,395.9 | 1,444.5 | **3,416.3** | 4,789.7 |
| 16 | 1,593.7 | 1,294.1 | 2,444.8 | 4,701.5 |

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

Run `20261120T234306Z-edf3ff614784` (23:43–00:01 UTC). Full per-file tables, saved binaries and raw observations are in the local run directory, with the regenerated decode report in `report-decode.md` and tabular data in `summary.tsv`. `raw.json` SHA-256: `e77db0e7edc6076a5a33c5bcc80a346163509025b9a767030d07417cdb0c03f1`. Re-render without building or measuring:

```sh
RUN=target/profiling-status-20261120/comparison-report/runs/20261120T234306Z-edf3ff614784
python3 -B xtask/comparison_tables.py "$RUN/raw.json"
python3 -B xtask/comparison_tables.py --format tsv "$RUN/raw.json" > summary.tsv
```

To repeat the sweep from the measured source, substitute the trusted [completed baseline run](pipeline-comparison.sol.md#what-the-corrected-run-establishes) for `BASELINE_RUN`. Omit `--before` if only comparing against C. The benchmark now defaults to these eight worker counts; codec CLI defaults are unchanged.

```sh
cargo xtask comparison-report --levels 1,3,7 \
  --decode-workers 1,2,3,4,6,8,12,16 --concat --before "$BASELINE_RUN"
```
