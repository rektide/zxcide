---
type: Measurement
title: Landed AVX2 bitmap merge versus Rust parent and C
description: Direct paired CLI measurements on identical C7 archives after landing the bounded 32-output merge.
resource: /doc/profiling/bitmap-merge-landed0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df8fd29afffepXs04yCk9DZsqv', title: Integrated the AVX2 winner and measured the landed tree directly against its parent and pinned C }
  - { resource: /doc/profiling/bitmap-merge-race0.sol.md, title: Original kernel race and checked replay }
  - { resource: /doc/profiling/flat-unpack6-landed0.gpt61t.md, title: Paired CLI protocol and prior C comparisons }
  - { resource: /doc/testing/linux-qemu.md, title: ARM64 and RISC-V baseline correctness checks }
---

# Landed AVX2 bitmap merge versus Rust parent and C

**The landed kernel improves the long frame at W1 and W8.** It is 7.9–10.2% faster at W1 and 8.0–8.6% faster at W8 than the direct Rust parent. On one pinned core, `single` has no resolved throughput difference against C; `mozilla` and `sao` favor Rust on this mask. At W8, Rust still spends 18–20% more CPU per GB than C on `single`. That is not general parity, nor a contradiction of the earlier W1 runs that let C's driver threads use more than one core.

## Direct CLI pairs

Each cell reports two independently randomized series, with paired throughput ratios (positive favors landed Rust) and unadjusted 97.7% order-statistic intervals. Exact values, absolute MB/s, paired child CPU/GB, and A/A controls are in the [reduction](bitmap-merge-landed-sol-data/summary.tsv) and [raw pairs](bitmap-merge-landed-sol-data/raw.json.gz).

| Fixture | Workers | Landed vs Rust parent | Landed vs pinned C |
| --- | ---: | --- | --- |
| `single` | 1 | +10.18% [+6.64, +17.43] / +7.90% [+1.37, +10.56] | −1.27% [−7.57, +0.88] / +0.58% [−9.67, +2.17] |
| `single` | 8 | +8.02% [+1.78, +9.88] / +8.56% [+6.19, +13.62] | −7.54% [−9.54, −5.96] / −2.92% [−6.57, +3.53] |
| `mozilla` | 1 | +11.17% [+8.27, +12.98] / +9.72% [+4.64, +13.29] | +7.83% [+6.37, +10.58] / +7.73% [+4.72, +13.36] |
| `mozilla` | 8 | +3.70% [−1.73, +9.56] / +8.97% [+3.93, +12.37] | +1.90% [−4.85, +15.24] / −4.48% [−11.21, +1.67] |
| `sao` | 1 | +5.84% [+2.09, +10.42] / +7.45% [+2.78, +14.57] | +18.05% [+12.76, +23.78] / +19.35% [+14.74, +25.45] |
| `sao` | 8 | +3.16% [−4.91, +8.96] / +2.08% [−0.78, +7.80] | +8.39% [+2.89, +15.80] / −4.34% [−8.63, +4.71] |

The landed `single` build delivered **1.81 / 1.75 GB/s W1** and **8.70 / 8.49 GB/s W8** in the Rust parent pairs. W1 C/Rust pairs put both at about 1.8 GB/s, with no resolved throughput or CPU/GB difference; the intervals do not establish equivalence. At W8, Rust used **0.683 / 0.727 CPU s/GB** versus C's **0.572 / 0.605** (paired **+19.56% [+17.00, +21.29] / +18.06% [+14.95, +20.30]**). Against the Rust parent, landed W8 `single` used **8.03% / 7.51% less CPU** per GB, with both intervals below zero. The direct C pairs use separate samples from the parent pairs. Do not divide medians across them or splice in older throughput or checked-replay samples.

## Conditions and verification

`94c148f7` → `7209e2d9`, clean, locked portable Release builds with no extra flags. The latter includes `45415859` and the test-only lint fix. Pinned C `5719a96f` is the native Release binary with its own driver, not the patch baseline. Each of the original checked `single`, `mozilla`, and `sao` C7 files was read through EOF, warm regular file to `/dev/null`. Twenty-one untimed output checks matched manifest byte count and SHA-256 (A/B/C at W1 and W8, scalar B at W1); all four corrupted-checksum runs rejected. Recorded wall includes launch through child exit, and CPU sums the child's user and system time.

Ryzen 7 5800X, Linux 7.1.0, `powersave`, boost enabled, SMT siblings unreserved. W1 pinned **both** CLIs and any driver threads to CPU 3; W8 used CPUs 0–7. These masks matter, especially because C may overlap driver work on extra cores under a wider `-T1` affinity mask. One-minute host load went **17.64 → 14.57 → 13.73**. The shared lock covers native verification, warmup and timing. Two independently shuffled 24-pair series, balanced 12 AB / 12 BA, with interleaved same-A controls. CPU, wall and A/A are reported separately; no noise correction or equivalence claim. W8 covers these three warm files, not a full corpus or cold storage. No new kernel counters were collected here; see the [original race](bitmap-merge-race0.sol.md).

Native release verification passed **73 workspace** and **36 no-default-features codec** tests; formatting and targeted Clippy passed (allowing the existing deliberate `new_instead_of_clear`). The integrated decoder also passed the [Linux QEMU tests](../testing/linux-qemu.md) on ARM64 and baseline RISC-V (`v=false`), each with and without `merge-simd`. No ARM64 or RISC-V performance was measured. The [verification archive](bitmap-merge-landed-sol-data/verification.tar.gz) includes build, native, Clippy and QEMU logs.

From the repository root, using the [prepared fixture cache](decode-pool.md#method-and-evidence) and an unused scratch path:

```sh
python3 -B doc/profiling/bitmap-merge-landed-sol-data/bench.py \
  94c148f7 7209e2d9 .test-agent/bitmap-merge-landed-repro
```
