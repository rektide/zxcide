---
type: Measurement
title: Bounded bitmap merge race after width six
description: Real mixed-run replay of byte deposit and four-load AVX2 against the retained Rust kernel and two pinned C contracts.
resource: /doc/profiling/bitmap-merge-race0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df821dfb8ffe2ajVc5CiDVaKhO', title: Raced both bounded kernels, corrected the AVX2 suffix, verified exact output, and measured checked replay and W1 }
  - { resource: /design/decode-throughput/bitmap-merge0.astra.md, title: Bounded deposit and 32-output shuffle-and-blend design and gate }
  - { resource: /doc/profiling/decode-gap0.sol.md, title: Original mixed-run census and Rust–C cycle attribution }
  - { resource: /doc/profiling/pivco-avx2-merge0.sol.md, title: Rejected two-group shuffle, not the mechanism tested here }
  - { resource: /doc/profiling/flat-unpack6-landed0.gpt61t.md, title: Landed width-six baseline and paired CLI method }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Fixed original C7 archives and plaintext hashes }
---

# Bounded bitmap merge race after width six

**Promote the revised bounded 32-output AVX2 merge; drop PDEP.** On the Ryzen 7 5800X, the revised candidate cut original `single` mixed-gather cycles by **43%** for both literals and tokens. This reached checked-block replay and the full serial CLI: `single` W1 gained **5.31% and 7.81%** in independent paired series. Promote only the version that finishes with the existing eight-output path from the current cursors; a direct scalar suffix regressed when the right child ran low early. The [patch](bitmap-merge-race-sol-data/avx2.patch) is a handoff, not a production change in this measurement.

## Checked decode and W1

Checked-block replay uses the original frame's blocks and verifies each checksum. Literal and token replay use the original inline tables and run bytes. In the first series, `single` literal cycles fell **26.68% [−29.45, −22.61]** and token cycles fell **14.61% [−18.19, −7.20]**. All three checked-block fixtures saved both cycles and instructions:

| Checked replay | A → trial cycles / plaintext byte, S1 / S2 | Paired cycles Δ [interval], S1 / S2 | A → trial instructions / plaintext byte |
| --- | ---: | ---: | ---: |
| `single` | 2.098 → 1.946 / 2.054 → 1.901 | −8.50% [−12.51, −4.84] / −9.65% [−11.59, −6.48] | 8.353 → 7.376 |
| `mozilla` | 2.268 → 2.017 / 2.249 → 2.014 | −10.52% [−14.66, −9.04] / −10.05% [−11.12, −5.43] | 9.042 → 7.792 |
| `sao` | 2.995 → 2.775 / 2.899 → 2.641 | −7.90% [−10.41, −6.53] / −8.59% [−10.01, −7.30] | 11.738 → 10.259 |

The independently shuffled second replay repeated just the three checked cases and the `single` sections, after host one-minute load had fallen from 7.97 at the first-series start to 5.62. It confirmed **−25.38%** literal and **−17.26%** token cycles on `single`. `single` checked A/A was −1.50% [−8.63, +2.37] in S1 and −0.10% [−4.05, +2.77] in S2. No further timing retries were taken.

CLI timing includes process startup, archive reads, checksum validation, and decode through footer/EOF to `/dev/null`. `S1 / S2` are independently randomized 24-pair series. Absolute throughput is decimal plaintext MB/s; child CPU combines user plus system time. Each row is its own A/B comparison.

| W1 fixture | A → trial MB/s, S1 / S2 | Throughput Δ [interval], S1 / S2 | Child CPU Δ [interval], S1 / S2 |
| --- | ---: | ---: | ---: |
| `single` | 1683 → 1804 / 1702 → 1807 | +5.31% [+3.27, +9.66] / +7.81% [+4.64, +9.54] | −5.07% [−8.81, −3.76] / −7.21% [−8.73, −4.49] |
| `mozilla` | 1417 → 1564 / 1491 → 1600 | +8.85% [+6.71, +11.09] / +7.65% [+5.88, +9.48] | −8.22% [−10.07, −6.45] / −7.68% [−8.63, −5.79] |
| `sao` | 848 → 923 / 890 → 926 | +8.07% [+3.35, +11.43] / +5.15% [+1.05, +9.78] | −5.85% [−9.65, −3.26] / −5.14% [−9.31, −1.83] |
| Dense 4 KiB | 219 → 224 / 227 → 236 | −0.35% [−5.23, +4.60] / +3.04% [+0.68, +6.04] | +0.32% [−5.28, +5.31] / −3.22% [−5.10, −0.02] |
| RAW 4 KiB | 583 → 631 / 674 → 663 | +5.34% [−12.34, +21.84] / −4.62% [−16.86, +11.03] | −9.07% [−17.00, +11.94] / +5.33% [−6.06, +20.67] |

No resolved control loss at W1. Do not claim a dense or RAW speedup: the first dense interval crosses zero and both RAW intervals are wide (the RAW A/A controls are comparably noisy). `single` W1 same-A wall changes were −0.23% [−1.48, +2.44] and −0.36% [−1.76, +1.71]. Separately paired pinned C versus A on `single` was **+9.78% / +7.30%** W1 and **−34.82%** cycles in first checked replay. There is no direct trial/C CLI comparison; do not divide separate comparisons' medians to claim Rust–C parity or a remaining percentage. W8 was not measured.

The [raw W1 series](bitmap-merge-race-sol-data/w1.json.gz) and [reduction](bitmap-merge-race-sol-data/w1.tsv) include all A/A and pinned-C pairs, verification and engine paths. The [first](bitmap-merge-race-sol-data/replay1.json.gz) and [second](bitmap-merge-race-sol-data/replay2.json.gz) replay series and their [reduction](bitmap-merge-race-sol-data/replay.tsv) include grouped counters, exact hashes, startup fingerprints, command lines and host snapshots. C references use the pinned native C implementation; neither is the portable Rust before/after denominator. All 5,472 PMU requests across the kernel and decoder replays returned both events at 100% running time.

## Kernel discriminator

Clean `git archive` pin **`477254fb0c93b6a4c54ef9e3ac2b2ad76496e574`**, which has exactly the codec sources of the width-six landing `0d68cb82e1d92024a812b5eeb7243ef7220812e4`. No compact tables, positioned reads, width-six changes, LZ changes, copy-back removal, source guard, or new arena layout in either Rust candidate. Only `Level::Avx2` on x86-64 is eligible. PDEP additionally requires the measured **AuthenticAMD family 19h model 21h stepping 2** signature (Ryzen 7 5800X Vermeer), cached by `OnceLock`, as well as the Fearless SIMD AVX2 token's BMI2/POPCNT proof. This is intentionally not an all-BMI2 gate: Zen 2's PDEP is slow. The shipping recommendation does not require any PDEP gate because the PDEP arm lost.

| Original `single` runs | PDEP vs A cycles Δ [interval] | First AVX2 vs A cycles Δ [interval] | First AVX2 vs PDEP cycles Δ [interval] | A / PDEP / AVX2 instructions per gathered byte |
| --- | ---: | ---: | ---: | ---: |
| Literals | −24.93% [−29.53, −10.23] | −39.31% [−43.31, −21.60] | −23.53% [−32.31, −19.41] | 3.523 / 2.327 / 1.859 |
| Tokens | −22.79% [−24.97, −21.98] | −38.60% [−39.96, −35.27] | −20.79% [−22.36, −18.41] | 3.583 / 2.457 / 1.992 |

Separate pairs with A: **bounded C** −42.43% / −32.72% cycles and **padded C** −47.21% / −47.59% on literals / tokens. The bounded C arm extracts the same 32-output inner loop from upstream `5719a96f778226868b29dba621a8c4e302b74d7c`, adds the 128-output entry and 32-byte right-window limits, and finishes with an exact C bit cursor. The padded arm calls the pinned C function unmodified, including its distinct 16-/8-output suffix, on separately initialized guard bytes; that is not the Rust contract. The same file backs the identical logical child bytes and bitmap in all arms, and Rust receives only the exact node slice. A/A cycle changes were −2.09% [−5.77, +0.05] and +0.30% [−1.14, +0.91]. Host conditions varied; compare within-pair ratios, not unrelated medians. Full raw and reduced entries include cycles/B, instructions/B and thread CPU.

**Keep the eight-output suffix.** The original `single` data contain no mixed node with either whole child at or below 1/16 of the run; the primary replay includes all actual population counts and original bitmap order, and the short control uses the real runs below 128 (141 literal, 399 token). Separate synthetic controls drain a 40-byte minority child early or distribute its bits at 1/32 density. First AVX2 regressed right-minority sparse and early cycles by **86.2% / 20.1%**; the revision was **−7.0% [−8.7, −5.1] / −0.4% [−4.3, +5.8]**. Left-minority cases improved 78–91% by keeping their physically valid windows even after left exhaustion. Revised short-token change was unresolved; the short-literal microcase measured +3.09% [+1.00, +4.95] and +1.96% instructions from the new gate/compiled fallback, on 10,328 bytes (0.011% of the original literal gather). This isolated small-run result does not establish a dense or whole-frame effect.

| Revised AVX2 kernel | A → trial cycles / gathered byte | Paired cycles Δ [interval] | A → trial instructions / gathered byte |
| --- | ---: | ---: | ---: |
| `single` literals | 0.759 → 0.425 | −43.34% [−45.69, −42.03] | 3.523 → 1.841 |
| `single` tokens | 0.980 → 0.525 | −42.61% [−47.66, −39.11] | 3.583 → 1.914 |
| `mozilla` literals | 0.797 → 0.420 | −43.54% [−45.55, −41.94] | 3.518 → 1.833 |
| `mozilla` tokens | 1.130 → 0.512 | −41.94% [−51.45, −39.39] | 3.613 → 1.940 |
| `sao` literals | 0.779 → 0.419 | −44.55% [−51.05, −37.72] | 3.504 → 1.818 |
| `sao` tokens | 0.718 → 0.403 | −42.69% [−46.85, −39.34] | 3.560 → 1.896 |

This fresh series includes [direct PDEP/AVX2 pairs and separate C contracts](bitmap-merge-race-sol-data/k2.json.gz). The Rust-to-Rust comparisons produce the same outputs with the same physical span rules for the 32-output portion, not C's readable slack. The revised AVX2 loop retains the 20 KiB read-only selector table; the PDEP table is 2 KiB.

## Bounds, saved work and code generation

The decoder counts the entire node before gathering. The children in the initialized slice are `L | R`; current physical offsets satisfy `l <= left <= r`. Both 32-byte slab views must fit. The second 16-output lane loads left at `l + 16 - popcnt(low16)` and right at `r + popcnt(low16)`; the displacements are each at most 16, so neither 16-byte load escapes its slab. The left load can overlap right bytes, but every selected index is within the number of bytes actually taken from that child. AVX2 uses the low nibble of each selector for lane-local `vpshufb`, then moves bit 4 to the sign bit for the blend. The old eight-byte path resumes with `[l, r-left]` relative to the original two children; the bit cursor takes over with exact remaining bitmap and output slices. Unused high bits of a partial last bitmap byte are ignored. The codec keeps `forbid(unsafe_code)`.

An untimed reference capture records each original section offset/index, node start, mask, initialized children and selected bytes in bottom-up gather order. The real `single` counts match the independent census: **4,157 / 92,741,351 bytes** literal and **5,112 / 30,483,177 bytes** token. Respectively **92,492,192 (99.73%)** and **30,155,840 (98.93%)** bytes fit the no-padding bulk proof. Every recorded run is checked byte-by-byte against a separate bit-by-bit selector over the independent child iterators, and its concatenated output matches the capture SHA-256. The resident path also fingerprints each timed pass. [Saved identities and shape totals](bitmap-merge-race-sol-data/captures.json) and [section indexes and disassembly](bitmap-merge-race-sol-data/codegen-and-indexes.tar.xz) preserve this work without checking in hundreds of MB of derived bytes.

Each candidate passed 33 SIMD and 30 no-default-feature codec tests, plus the three differential tests under AddressSanitizer. Those tests force all **65,536 ordered bitmap-byte pairs** into both halves of the second fast tile after nonzero child advances, exercise every actual pointer alignment modulo 32, check canaries and allocation ends, and cover lengths 0–129 and larger boundaries ±1 with both exhaustion directions and arbitrary high tail bits. Eleven entropy tests passed on each QEMU x86 fallback (Conroe and Nehalem); ARM64 and RISC-V passed cross-checks, not perf measurement. Scalar, explicit SIMD128 and relaxed SIMD Wasm WIT smoke passed on seven fixtures, including output-cap and truncation rejection; built with available `822182db4`, not the native timing compiler. The CLIs passed 59 conformance/dictionary cases with native and scalar before/after parity and checked the fixed C7 and 4 KiB plaintext hashes and checksum rejection before timing. See [target and ASan tests](bitmap-merge-race-sol-data/tests.log.gz), [conformance](bitmap-merge-race-sol-data/verify.json.gz), [Wasm smoke](bitmap-merge-race-sol-data/wasm.log.gz) and [cross checks](bitmap-merge-race-sol-data/cross.log).

Portable optimized binaries (Rust `bdf712ee8`, LLVM 23.1.6, no `target-cpu=native`) show eight register `pdep` and four qword stores per PDEP tile, with no calls in the tile. AVX2 uses two low `vmovdqu` XMM loads, two memory-operand `vinserti128` upper loads, two YMM `vpshufb`, one `vpsllw`, one `vpblendvb`, and one full YMM store. The checked loop still contains span tests; neither kernel's tile calls the panic paths. This is not a benchmark of unchecked loads. CPU 3 on a Ryzen 7 5800X (unreserved sibling 11, boost enabled, `powersave`); warm resident requests, locked with `.test-agent/zxc-perf.lock`, shuffled balanced adjacent AB/BA pairs with interleaved A/A. Perf counts grouped `cycles:u,instructions:u`, rejecting every request below 100% PMU running time. Intervals are unadjusted two-sided order-statistic median intervals (97.7% for 24 pairs, 96.1% for 12); contention and correlated host load can survive pairing.

[Reduced kernel results](bitmap-merge-race-sol-data/kernel.tsv) · [Original raw pairs](bitmap-merge-race-sol-data/k1.json.gz) · [Initial controls](bitmap-merge-race-sol-data/k-controls.json.gz) · [Revised controls](bitmap-merge-race-sol-data/k2-controls.json.gz) · [AVX2 patch](bitmap-merge-race-sol-data/avx2.patch) · [PDEP patch](bitmap-merge-race-sol-data/deposit.patch) · [Exact engine paths and hashes](bitmap-merge-race-sol-data/engine-identity.txt)

### Reproduction

Start from the repository root with the [fixed C7 fixtures](decode-pool.md#method-and-evidence), [4 KiB controls](pivco-queue.md#timing-protocol-and-reproduction), and the pinned upstream C replay from the [compact-table recipe](decode-compact0.sol.md#reproduction). Use a new scratch directory at the path below (the source snapshots must not already exist). The drivers produce raw and reduced results separately; they lock each timed series internally. `avx2` is the label for the revised 32-output AVX2 arm, not an additional ISA tier.

```sh
s=.test-agent/bitmap-merge-race-sol
d=doc/profiling/bitmap-merge-race-sol-data
mkdir -p "$s/tmp"
cp "$d"/*.py "$d"/*.sh "$d"/*.rs "$d"/kernel.c "$s/"
export TMPDIR="$PWD/$s/tmp"
bash "$s/build.sh" a deposit avx avx2 capture
python3 -B "$s/build_c.py"
python3 -B "$s/save.py"
python3 -B "$s/skew.py"
python3 -B "$s/race.py" kernel "$s/k1" --seed 620331 --only single-lit single-tok
python3 -B "$s/race.py" kernel "$s/k-controls" --seed 621376 --pairs 12 \
    --only short-lit short-tok skew-r-early skew-r-sparse skew-l-early skew-l-sparse
python3 -B "$s/race.py" kernel "$s/k2-controls" --winner avx2 --seed 621401 --pairs 12 \
    --only short-lit short-tok skew-r-early skew-r-sparse skew-l-early skew-l-sparse
python3 -B "$s/race.py" kernel "$s/k2" --winner avx2 --seed 621516 \
    --only single-lit single-tok mozilla-lit mozilla-tok sao-lit sao-tok
bash "$s/cli-build.sh" a avx2
python3 -B "$s/race.py" replay "$s/replay1" --winner avx2 --seed 621871
python3 -B "$s/cli.py" avx2
python3 -B "$s/race.py" replay "$s/replay2" --winner avx2 --seed 621881 \
    --only single-block single-lit single-tok mozilla-block sao-block
flock .test-agent/zxc-perf.lock bash "$s/test.sh" deposit avx avx2
flock .test-agent/zxc-perf.lock bash "$s/wasm-smoke.sh"
```
