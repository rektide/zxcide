---
type: Measurement
title: Landed six-bit AVX2 CLI measurement
description: Fresh paired portable release comparison of the landed decoder, its immediate parent, and pinned upstream C on checked C7 frames.
resource: /doc/profiling/flat-unpack6-landed0.gpt61t.md
status: draft
generated: { by: 'model:openai/gpt-6.1-terra#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df827da7cffeYLSb6RnEiu651D', title: Built and measured the landed revision against its parent and pinned C }
  - { resource: /doc/profiling/flat-unpack6.sol.md, title: Kernel design, pre-landing attribution and differential verification }
  - { resource: /doc/profiling/decode-pool.md, title: Fixed C7 fixture and prior worker-pool measurement protocol }
---

# Landed six-bit AVX2 CLI measurement

**The landed build shows a small, not conclusively resolved `single` W1 gain.** Its paired throughput changes were **+1.63% [+0.35, +4.01]** and **+1.45% [−0.25, +4.86]**; child CPU fell 1.55% and 1.46%, respectively. One interval crosses zero, and parent/parent controls show comparable order and load variation. This is consistent with the prior trial's unresolved whole-CLI effect, not proof of a fixed gain or of equivalence. `sao` W1 favored the landed build more strongly, with lower CPU in both series, but these controls and multiple comparisons do not establish a universal gain. W8 patch effects are unresolved here.

## Results

`S1 / S2` denote independently randomized series. MB/s and CPU/GB are the individual-arm medians **within each comparison**; do not divide medians from different rows or runs to infer speed. Percentages and intervals are within-pair ratios. All CPU values are child user plus system seconds per decimal GB of plaintext.

| Fixture / workers | Parent → landed MB/s S1 / S2 | Landed vs parent throughput % [interval] S1 / S2 | Parent → landed CPU/GB S1 / S2 | Landed vs parent CPU % [interval] S1 / S2 |
| --- | --- | --- | --- | --- |
| single W1 | 1598 → 1613 / 1623 → 1660 | +1.63 [+0.35, +4.01] / +1.45 [−0.25, +4.86] | 0.625 → 0.619 / 0.613 → 0.601 | −1.55 [−2.80, −0.36] / −1.46 [−3.89, +0.25] |
| single W8 | 7180 → 7098 / 8122 → 8010 | +0.12 [−2.77, +6.57] / +0.62 [−4.43, +3.57] | 0.807 → 0.817 / 0.739 → 0.737 | −1.55 [−5.59, +0.06] / −1.19 [−2.15, +1.24] |
| mozilla W1 | 1239 → 1304 / 1389 → 1397 | +2.61 [−3.14, +8.64] / +1.67 [+0.34, +3.26] | 0.794 → 0.758 / 0.712 → 0.711 | −2.83 [−8.19, +2.57] / −1.57 [−3.32, −0.36] |
| mozilla W8 | 4044 → 4190 / 4543 → 4609 | +1.68 [−4.95, +11.24] / +3.48 [−2.60, +5.86] | 1.034 → 1.008 / 0.978 → 0.962 | −1.89 [−5.82, +1.01] / −2.17 [−5.31, +1.59] |
| sao W1 | 789 → 803 / 829 → 866 | +5.40 [−0.10, +10.29] / +6.99 [+2.35, +8.89] | 1.242 → 1.218 / 1.178 → 1.132 | −4.37 [−8.94, −1.28] / −6.21 [−7.92, −2.35] |
| sao W8 | 1104 → 1080 / 1243 → 1264 | −2.90 [−10.49, +8.62] / +0.41 [−6.08, +7.05] | 2.514 → 2.458 / 2.260 → 2.180 | −3.17 [−7.34, +4.01] / −3.12 [−7.28, +0.51] |

| Fixture / workers | C → landed MB/s S1 / S2 | Landed vs C throughput % [interval] S1 / S2 | C → landed CPU/GB S1 / S2 | Landed vs C CPU % [interval] S1 / S2 |
| --- | --- | --- | --- | --- |
| single W1 | 1556 → 1313 / 1823 → 1664 | −9.60 [−21.25, −1.02] / −7.92 [−8.84, −6.28] | 0.641 → 0.746 / 0.547 → 0.599 | +8.08 [+1.28, +20.41] / +8.21 [+6.72, +9.49] |
| single W8 | 8377 → 7306 / 9548 → 8101 | −10.79 [−19.15, −5.64] / −14.54 [−21.36, −8.78] | 0.625 → 0.790 / 0.615 → 0.755 | +28.28 [+21.18, +34.25] / +24.97 [+18.79, +26.46] |
| mozilla W1 | 1234 → 1317 / 1478 → 1399 | −2.71 [−5.94, +7.40] / −5.26 [−7.57, −3.10] | 0.801 → 0.737 / 0.671 → 0.709 | +1.41 [−5.63, +4.32] / +5.08 [+3.05, +8.36] |
| mozilla W8 | 4579 → 4239 / 4995 → 4479 | −8.94 [−17.58, −0.24] / −9.78 [−14.11, −7.12] | 0.993 → 1.028 / 0.948 → 0.947 | +14.13 [−0.34, +22.35] / +3.60 [−4.17, +9.86] |
| sao W1 | 724 → 778 / 807 → 855 | +6.03 [−0.11, +10.55] / +5.89 [−0.61, +9.99] | 1.355 → 1.237 / 1.211 → 1.140 | −5.59 [−10.77, −1.34] / −5.47 [−8.25, +0.67] |
| sao W8 | 1122 → 1079 / 1251 → 1203 | −2.55 [−9.32, +10.17] / −5.63 [−8.47, −1.64] | 2.436 → 2.417 / 2.142 → 2.213 | +5.17 [−0.86, +10.66] / +3.08 [−3.67, +11.19] |

On `single`, C has a consistent measured throughput and CPU advantage at W1 and W8 under these specific masks; C's native build and driver design preclude attributing that gap to the six-bit kernel. S1 was especially noisy: the `single` W1 parent/parent paired throughput shift was +1.20% [−5.35, +4.37], versus −0.19% [−2.72, +1.77] in S2. The full [reduction](flat-unpack6-landed-terra-data/summary.tsv) includes all A/A controls and CPU intervals. S1 one-minute load rose 11.52 → 18.20; S2 fell 18.20 → 12.42. These conditions also explain why arm medians can appear inconsistent with paired ratios: observations were not contemporaneous across comparisons and their distributions were skewed. No small-block timing was attempted in this CLI pass.

## Method

The checked C7 archive and plaintext identities are from the [fixture manifest](decode-pool-data/fixture-manifest.json). Builds are from clean `git archive` snapshots of immediate parent `2fd99c21` and landed `0d68cb82`, with `cargo build --locked --release -p zxc-cli`, empty Rust flags and default features; no ledger, Tracy, replay shim, or target-cpu override. The Rust compiler is 1.102.0-nightly (`bdf712ee8`, LLVM 23.1.6). Upstream C is the pinned native Release `5719a96f` binary and thus is a separate implementation reference, not the patch baseline.

The harness takes `.test-agent/zxc-perf.lock` across verification, warmup and timing. Each binary reads the same named compressed file to EOF and writes `/dev/null`; untimed verification hashes and counts the entire plaintext. The landed scalar CLI is separately built with `--no-default-features`. A one-bit corruption in the first `sao` block checksum must be rejected; Rust reports checksum failure, while C's quiet mode exits nonzero without a diagnostic. Each timed invocation must exit successfully and emit no stderr. The timings include process creation and `wait4`, with summed user and system child CPU.

Each series contains 24 adjacent pairs per comparison, independently shuffled with 12 AB and 12 BA orders, and one warmup pair. Comparisons are shuffled within each round, including parent/parent controls. Throughput percentage is the median of within-pair `wall(reference)/wall(landed) − 1`, not a ratio of independently gathered medians. CPU percentage is `CPU(landed)/CPU(reference) − 1`. Intervals are unadjusted 97.7% distribution-free order-statistic intervals for 24 independent pairs; correlated host load and multiple comparisons limit their interpretation. Absolute MB/s uses decimal plaintext bytes, and CPU seconds/GB uses decimal GB.

Ryzen 7 5800X, Linux 7.1.0-debplus.1, 32 MiB shared L3, `powersave` governor and boost enabled. All W1 processes, including C, use CPU 3; W8 processes use CPUs 0–7, eight distinct physical cores. C's `-T1` may still employ driver threads, which are constrained to the same CPU as Rust; equal masks are not equal thread architecture. SMT siblings 8–15 were not reserved. Runs are warm-cache, not cold-storage tests. Load, governor and boost snapshots accompany the [raw data](flat-unpack6-landed-terra-data/raw.json.gz). No checked replay counters were collected in this CLI-focused confirmation; see the earlier [replay measurements](flat-unpack6.sol.md) for that evidence and its unresolved cycle interval.

The landed default build passed [70 locked release workspace tests](flat-unpack6-landed-terra-data/tests.log); the no-default-features codec passed 36. All 21 valid binary/fixture/worker checks produced the manifest plaintext count and SHA-256; all four checksum smoke tests rejected. [Build logs](flat-unpack6-landed-terra-data/build.log), [raw pairs](flat-unpack6-landed-terra-data/raw.json.gz), and [run log](flat-unpack6-landed-terra-data/bench.log) preserve identities and observations. A first preflight stopped before timing because the harness incorrectly expected a quiet C diagnostic; its assertion was corrected and the entire verification rerun under the lock.

Reproduction from the repository root, with fixtures prepared as in [decode pool](decode-pool.md#method-and-evidence) and an absent `.test-agent/flat6-terra/src-{a,b}`:

```sh
mkdir -p .test-agent/flat6-terra/tmp
bash doc/profiling/flat-unpack6-landed-terra-data/build.sh > .test-agent/flat6-terra/build.log 2>&1
python3 -B doc/profiling/flat-unpack6-landed-terra-data/bench.py > .test-agent/flat6-terra/bench.log 2>&1
```
