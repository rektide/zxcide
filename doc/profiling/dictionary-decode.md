---
type: Measurement
title: Borrowing dictionary history on block decode
description: A paired native decode comparison against the contiguous dictionary-prefix preload.
resource: /doc/profiling/dictionary-decode.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0ac424eeffe4kk65VvnMwl4oQ', title: Implemented, validated, and timed the borrowed-history decoder }
  - { resource: '/doc/profiling/ledger.md', title: Baseline logical byte traffic, fixtures, and counting conventions }
  - { resource: '/crates/parallel/benches/ledger.rs', title: Unchanged benchmark driver }
---

# Borrowing dictionary history on block decode

**Keep `5d0d8d81`.** On the large-dictionary fixture, 4 KiB decode latency fell 78.8–79.1% at one worker and 16.5–29.4% at four. No control cleared the repeatable-regression screen with this revision. Four-worker 512 KiB dictionary timings remain variable; this is not a claim of a 512 KiB speedup.

The candidate changes only SPL/PAK reconstruction. Match distances refer to the logical concatenation of the dictionary and the plaintext written so far. A match that starts in the dictionary copies only the bytes it needs. If it crosses the boundary, it continues at plaintext byte zero; the existing overlap loop handles subsequent repetitions. RAW, the encoder, and checksum/publication ordering do not change.

## Method

Compare the preload decoder at `75f676db` with borrowed history on a Ryzen 7 5800X, Linux 7.1.0-debplus.1, rustc 1.101.0-nightly (`822182db4`), locked dependencies and default optimized bench settings. Use the [ledger workloads](ledger.md#inputs-and-host): checksummed 1 MiB streams with 4 KiB (`b12`) or 512 KiB (`b19`) block limits. The four-worker case uses crossbeam with eight slots, and includes worker startup and joins. The dictionary fixture has a 65,535-byte dictionary and repeated records drawn from it; it is not evidence about arbitrary hit rates or dictionary sizes.

Build `cargo bench --locked -p zxc-parallel --bench ledger --no-run` at each revision and save the resulting binaries. Run baseline, candidate, candidate, baseline on CPUs `2-7` (six distinct cores). Give each process an empty `CRITERION_HOME`; use the same filter and arguments for each:

```sh
taskset -c 2-7 "$bench" --bench '^ledger/(dict|mixed|noise)/b(12|19)/w(1|4)/decode$' \
  --sample-size 30 --warm-up-time 0.45 --measurement-time 1 --noplot
```

Report Criterion whole-call 95% slope intervals in µs (mean if no slope) and relative changes in point estimates. Keep only a repeatable ≥10% latency reduction at `dict/b12/w1`, with no repeatable material regressions on `w4`, `b19`, or dictionary-free controls. The regression screen uses slowdowns of at least 3% with separated intervals in both pairs. A separate build with `--features ledger` checks byte counts; none of those probes are enabled for timing.

The first prototype also replaced the owned verified block's plaintext range with a length. It cut `dict/b12/w1` latency by 79.7–80.5%, but `noise/b19/w4` rose from 155.96–159.82 to 166.77–187.18 µs and from 156.38–159.37 to 161.94–174.96 µs in the two run pairs. The refinement preserves the original owned verified block representation and handoff. That removes a confounding change; the comparison does not isolate its cost.

## Results

Each row is a separate `ledger/<case>/decode`; negative changes indicate less time. The second pair ran candidate before baseline.

| Case | Baseline 1 (µs) | Candidate 1 (µs) | Baseline 2 (µs) | Candidate 2 (µs) | Change 1 | Change 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `dict/b12/w1` | 370.18–374.85 | 76.94–82.02 | 376.06–380.79 | 78.13–80.21 | −78.8% | −79.1% |
| `dict/b12/w4` | 217.79–225.12 | 173.89–197.71 | 221.19–228.77 | 155.08–163.79 | −16.5% | −29.4% |
| `dict/b19/w1` | 86.84–88.52 | 83.11–84.61 | 87.72–88.70 | 85.26–87.12 | −4.5% | −2.4% |
| `dict/b19/w4` | 114.47–117.01 | 114.27–118.65 | 113.03–115.37 | 119.65–127.59 | +0.4% | +8.0% |
| `noise/b12/w1` | 73.34–75.46 | 77.76–78.97 | 75.33–78.60 | 76.48–78.62 | +5.4% | +1.1% |
| `noise/b12/w4` | 267.66–312.00 | 260.97–264.89 | 278.88–302.57 | 270.84–282.96 | −8.8% | −4.7% |
| `noise/b19/w1` | 88.70–89.61 | 90.54–94.75 | 88.38–89.70 | 87.85–89.03 | +3.7% | −0.7% |
| `noise/b19/w4` | 157.02–161.18 | 158.71–162.73 | 156.36–161.68 | 162.27–193.27 | +1.0% | +9.6% |
| `mixed/b12/w1` | 6399.07–6524.16 | 6225.28–6359.27 | 6302.05–6444.69 | 6426.65–6797.02 | −2.8% | +3.5% |
| `mixed/b12/w4` | 1761.98–1947.67 | 1777.71–1839.66 | 1951.03–2290.56 | 1738.05–1777.76 | −1.4% | −16.7% |
| `mixed/b19/w1` | 113.12–114.92 | 114.46–116.01 | 115.58–118.70 | 114.92–116.84 | +1.1% | −1.1% |
| `mixed/b19/w4` | 149.61–187.09 | 151.92–171.40 | 158.31–210.53 | 144.01–150.20 | −5.0% | −19.7% |

The wider matrix showed isolated slowdowns in three controls. Recheck those cases in the same run order with 50 samples, 0.65 s warmup and a 3 s measurement target (filter `^ledger/((dict|noise)/b19/w4|noise/b12/w1)/decode$`):

| Case | Baseline 1 (µs) | Candidate 1 (µs) | Baseline 2 (µs) | Candidate 2 (µs) | Change 1 | Change 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `dict/b19/w4` | 116.38–119.40 | 120.95–134.88 | 116.63–125.07 | 115.36–123.66 | +8.0% | −1.4% |
| `noise/b12/w1` | 74.93–76.66 | 73.51–74.40 | 74.58–79.05 | 74.04–75.33 | −2.4% | −2.2% |
| `noise/b19/w4` | 161.98–164.39 | 159.83–168.32 | 163.84–170.90 | 167.12–172.87 | +0.3% | +1.6% |

The first prototype's repeatable RAW slowdown did not persist. The 512 KiB four-worker dictionary case was about 8% slower in one pair in each experiment and within the other pair's uncertainty or slightly faster; watch that case if it is important to a deployment. These intervals are within-run uncertainty, not a bound on variation across independent runs.

## Byte counts and validation

The separate diagnostic ledger counts zero dictionary preload bytes in all four dictionary decode cases, versus **16,776,960 bytes** per call at `b12` and **131,070 bytes** at `b19`, at either worker count. `reconstruct:matches` still counts **1,048,576 bytes** per call, including the bytes copied directly out of the dictionary. The encoder's `copy:history prefix` and `copy:history plain` counts are unchanged.

Hand-built SPL and PAK tests cover a match starting in a borrowed dictionary and overlapping into output, minimum available history for the maximum encoded distance (65,536 bytes), valid and excessive block output sizes, and checksum and stream-output limits before the writer sees the block. Existing tests exercise dictionary ID zero and concatenation, shared entropy, indexed reads, invalid archives, and ordered worker publication. The native and C checks are reproducible with:

```sh
cargo nextest run --locked --workspace --features zxc-cli/transports
cargo nextest run --locked -p zxc-codec --release block::tests
cargo xtask interop --cases 36
for case in 11 23 35; do
  cargo xtask interop --parallel-backend crossbeam --case "$case"
done
```

Nextest passed 36 workspace tests and all four focused tests in release. The default oracle pins upstream C at `2992c28e6e1df07feec0cb39678a6d4a9665c6bb`. It passed 180 frozen cross-decodes and 72 frozen indexed reads, plus 72 generated cross-decodes and 156 generated indexed reads. The 36 generated recipes use generator revision 1 and seed `99335130674745`; cases 11/23/35 are the large-HTTP-dictionary recipe across dense, split, and packed modes. The parallel replays added six cross-decodes and 18 indexed reads across the two producers.
