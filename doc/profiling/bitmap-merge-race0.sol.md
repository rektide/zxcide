---
type: Measurement
title: Bounded bitmap merge race after width six
description: Real mixed-run replay of byte deposit and four-load AVX2 against the retained Rust kernel and two pinned C contracts.
resource: /doc/profiling/bitmap-merge-race0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df821dfb8ffe2ajVc5CiDVaKhO', title: Built and raced both bounded kernels, corrected the AVX2 suffix, and checked original output bytes and generated code }
  - { resource: /design/decode-throughput/bitmap-merge0.astra.md, title: Bounded deposit and 32-output shuffle-and-blend design and gate }
  - { resource: /doc/profiling/decode-gap0.sol.md, title: Original mixed-run census and Rust–C cycle attribution }
  - { resource: /doc/profiling/pivco-avx2-merge0.sol.md, title: Rejected two-group shuffle, not the mechanism tested here }
  - { resource: /doc/profiling/flat-unpack6-landed0.gpt61t.md, title: Landed width-six baseline and paired CLI method }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Fixed original C7 archives and plaintext hashes }
---

# Bounded bitmap merge race after width six

**AVX2 wins the kernel race; stop tuning PDEP.** On the original `single` mixed literal and token runs, the first bounded AVX2 arm used **23.5% and 20.8% fewer cycles than PDEP** in direct pairs. It takes four 16-byte source loads and produces 32 output bytes, not two eight-output groups like the [rejected AVX2 trial](pivco-avx2-merge0.sol.md). Do not promote its first scalar-suffix version: an early-depleted or sparse right child can leave a long suffix. The revised candidate restores the existing checked eight-output continuation from the exact two cursors. This document initially records the discriminator; an end-to-end gate is separate from the decision to advance the kernel.

## Kernel discriminator

Clean `git archive` pin **`477254fb0c93b6a4c54ef9e3ac2b2ad76496e574`**, which has exactly the codec sources of the width-six landing `0d68cb82e1d92024a812b5eeb7243ef7220812e4`. No compact tables, positioned reads, width-six changes, LZ changes, copy-back removal, source guard, or new arena layout in either Rust candidate. Only `Level::Avx2` on x86-64 is eligible. PDEP additionally requires the measured **AuthenticAMD family 19h model 21h stepping 2** signature (Ryzen 7 5800X Vermeer), cached by `OnceLock`, as well as the Fearless SIMD AVX2 token's BMI2/POPCNT proof. This is intentionally not an all-BMI2 gate: Zen 2's PDEP is slow. The shipping recommendation does not require any PDEP gate because the PDEP arm lost.

| Original `single` runs | PDEP vs A cycles Δ [interval] | First AVX2 vs A cycles Δ [interval] | First AVX2 vs PDEP cycles Δ [interval] | A / PDEP / AVX2 instructions per gathered byte |
| --- | ---: | ---: | ---: | ---: |
| Literals | −24.93% [−29.53, −10.23] | −39.31% [−43.31, −21.60] | −23.53% [−32.31, −19.41] | 3.523 / 2.327 / 1.859 |
| Tokens | −22.79% [−24.97, −21.98] | −38.60% [−39.96, −35.27] | −20.79% [−22.36, −18.41] | 3.583 / 2.457 / 1.992 |

Separate pairs with A: **bounded C** −42.43% / −32.72% cycles and **padded C** −47.21% / −47.59% on literals / tokens. The bounded C arm extracts the same 32-output inner loop from upstream `5719a96f778226868b29dba621a8c4e302b74d7c`, adds the 128-output entry and 32-byte right-window limits, and finishes with an exact C bit cursor. The padded arm calls the pinned C function unmodified, including its distinct 16-/8-output suffix, on separately initialized guard bytes; that is not the Rust contract. The same file backs the identical logical child bytes and bitmap in all arms, and Rust receives only the exact node slice. A/A cycle changes were −2.09% [−5.77, +0.05] and +0.30% [−1.14, +0.91]. Host conditions varied; compare within-pair ratios, not unrelated medians. Full raw and reduced entries include cycles/B, instructions/B and thread CPU.

**Keep the eight-output suffix.** The original `single` data contain no mixed node with either whole child at or below 1/16 of the run; the primary replay includes all actual population counts and original bitmap order, and the short control uses the real runs below 128 (141 literal, 399 token). Separate synthetic controls drain a 40-byte minority child early or distribute its bits at 1/32 density. First AVX2 regressed right-minority sparse and early cycles by **86.2% / 20.1%**; the revision was **−7.0% [−8.7, −5.1] / −0.4% [−4.3, +5.8]**. Left-minority cases improved 78–91% by keeping their physically valid windows even after left exhaustion. Revised short-token change was unresolved; the short-literal microcase measured +3.09% [+1.00, +4.95] and +1.96% instructions from the new gate/compiled fallback, on 10,328 bytes (0.011% of the original literal gather). This isolated small-run result does not establish a dense or whole-frame effect.

## Bounds, saved work and code generation

The decoder counts the entire node before gathering. The children in the initialized slice are `L | R`; current physical offsets satisfy `l <= left <= r`. Both 32-byte slab views must fit. The second 16-output lane loads left at `l + 16 - popcnt(low16)` and right at `r + popcnt(low16)`; the displacements are each at most 16, so neither 16-byte load escapes its slab. The left load can overlap right bytes, but every selected index is within the number of bytes actually taken from that child. AVX2 uses the low nibble of each selector for lane-local `vpshufb`, then moves bit 4 to the sign bit for the blend. The old eight-byte path resumes with `[l, r-left]` relative to the original two children; the bit cursor takes over with exact remaining bitmap and output slices. Unused high bits of a partial last bitmap byte are ignored. The codec keeps `forbid(unsafe_code)`.

An untimed reference capture records each original section offset/index, node start, mask, initialized children and selected bytes in bottom-up gather order. The real `single` counts match the independent census: **4,157 / 92,741,351 bytes** literal and **5,112 / 30,483,177 bytes** token. Respectively **92,492,192 (99.73%)** and **30,155,840 (98.93%)** bytes fit the no-padding bulk proof. Every recorded run is checked byte-by-byte against a separate bit-by-bit selector over the independent child iterators, and its concatenated output matches the capture SHA-256. The resident path also fingerprints each timed pass. [Saved identities and shape totals](bitmap-merge-race-sol-data/captures.json) and [section indexes and disassembly](bitmap-merge-race-sol-data/codegen-and-indexes.tar.xz) preserve this work without checking in hundreds of MB of derived bytes.

Portable optimized binaries (Rust `bdf712ee8`, LLVM 23.1.6, no `target-cpu=native`) show eight register `pdep` and four qword stores per PDEP tile, with no calls in the tile. AVX2 uses two low `vmovdqu` XMM loads, two memory-operand `vinserti128` upper loads, two YMM `vpshufb`, one `vpsllw`, one `vpblendvb`, and one full YMM store. The checked loop still contains span tests; neither kernel's tile calls the panic paths. This is not a benchmark of unchecked loads. CPU 3 on a Ryzen 7 5800X (unreserved sibling 11, boost enabled, `powersave`); warm resident requests, locked with `.test-agent/zxc-perf.lock`, shuffled balanced adjacent AB/BA pairs with interleaved A/A. Perf counts grouped `cycles:u,instructions:u`, rejecting every request below 100% PMU running time. Intervals are unadjusted two-sided order-statistic median intervals (97.7% for 24 pairs, 96.1% for 12); contention and correlated host load can survive pairing.

[Reduced kernel results](bitmap-merge-race-sol-data/kernel.tsv) · [Original raw pairs](bitmap-merge-race-sol-data/k1.json.gz) · [Initial controls](bitmap-merge-race-sol-data/k-controls.json.gz) · [Revised controls](bitmap-merge-race-sol-data/k2-controls.json.gz) · [AVX2 patch](bitmap-merge-race-sol-data/avx2.patch) · [PDEP patch](bitmap-merge-race-sol-data/deposit.patch) · [Exact engine paths and hashes](bitmap-merge-race-sol-data/engine-identity.txt)
