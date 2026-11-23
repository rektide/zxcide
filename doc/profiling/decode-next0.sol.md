---
type: Measurement
title: SPL16 copy-width and Huffman setup trials
description: Paired Rust checked replay on fixed C7 archives, with SPL16 eligibility counts and a small-block control.
resource: /doc/profiling/decode-next0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df8fd29afffepXs04yCk9DZsqv', title: Ran and verified the independent SPL16 and Huffman trials, plus the Huffman CLI follow-up }
  - { resource: /design/decode-throughput/next0.sol.md, title: Ranked trials and acceptance gates }
  - { resource: /doc/profiling/decode-perf0.sol.md, title: Checked reconstruction and original Huffman section attribution }
  - { resource: /doc/profiling/dense-decode.md, title: Earlier extension path and fallback controls }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Fixed C7 archive and plaintext identities }
---

# SPL16 copy-width and Huffman setup trials

**Drop the copy-width split and skip the distance-one fill. Keep the Huffman bucketing result as a modest codec-level win, not a W8 throughput claim.** The independently repeated checked replay confirms less work on the long frame. The [positioned regular-file input design](../../design/decode-throughput/design0.astra.md#if-the-session-is-not-the-answer) remains the next separate W8 experiment; no reader or queue change was tried here.

## Initial checked replay

The SPL16 copy split missed the **5% checked-block cycles/byte** gate and did not advance to CLI. It executed 0.27% more instructions on `single` and 0.24% more on `mozilla`, despite copying less on its short branch. Huffman bucketing saved work in both long token runs, but the second checked-block interval crossed zero. The long-frame A/A interval also widened in that series, so the checked-block result warranted one targeted repeat, not an end-to-end claim.

Negative deltas mean less work. Absolute instruction costs are from series 1; the sign and rounded relative changes agreed in series 2. The bundle includes thread CPU, absolute cycles and all matched A/A controls.

| Trial and workload | Paired cycles Δ [interval], S1 | Paired cycles Δ [interval], S2 | Instructions / byte or symbol, parent → trial |
| --- | ---: | ---: | ---: |
| Copy: `single` checked | +0.72% [−0.12, +2.08] | +1.11% [−3.59, +5.74] | 8.644 → 8.667 |
| Copy: `mozilla` checked | +0.64% [+0.13, +1.74] | −0.04% [−5.22, +1.85] | 9.545 → 9.569 |
| Copy: `sao` checked | −0.36% [−1.11, +0.45] | −0.51% [−0.88, −0.11] | 12.479 → 12.442 |
| Copy: small SPL8 checked | −0.49% [−1.28, −0.03] | −1.74% [−10.77, +0.37] | 0.755 → 0.755 |
| Buckets: `single` tokens | −8.29% [−8.92, −7.26] | −8.60% [−9.30, −5.68] | 18.209 → 17.143 |
| Buckets: `single` literals | −3.56% [−4.05, −2.48] | −2.06% [−3.27, +0.27] | 14.797 → 14.535 |
| Buckets: `single` checked | −1.95% [−3.13, −1.15] | −1.09% [−5.78, +2.58] | 8.644 → 8.546 |
| Buckets: `sao` tokens | −1.32% [−2.57, −0.67] | −1.95% [−3.35, −1.42] | 14.515 → 14.291 |
| Buckets: `sao` checked | −0.57% [−0.88, +0.39] | −0.40% [−1.06, +0.84] | 12.479 → 12.424 |
| Parent vs. parent: `single` checked | −0.15% [−0.50, +0.61] | −0.64% [−5.77, +4.18] | 8.644 → 8.644 |

`sao` never executes the extension copy. Its small instruction change with the copy build is a code-generation side effect, not an effect of saving copy bytes.

### Targeted repeat: Huffman buckets

Same binaries and checked bytes, two fresh series, only `single` checked and token replay. Both checked intervals now exclude zero; the first repeat series was still noisy, while the second gave the tighter estimate. The retained instruction reduction and same-direction thread CPU changes are stronger corroboration than any one wall or cycle estimate.

| Comparison | Paired cycles Δ [interval], S1 | Paired cycles Δ [interval], S2 | Paired thread CPU Δ [interval], S1 / S2 |
| --- | ---: | ---: | --- |
| Buckets: `single` checked | −4.14% [−18.41, −0.74] | −2.74% [−3.00, −2.33] | −4.13% [−18.51, −0.97] / −2.67% [−3.03, −2.40] |
| Buckets: `single` tokens | −10.91% [−17.00, −5.28] | −8.30% [−10.98, −7.30] | −10.94% [−16.94, −5.40] / −8.30% [−10.85, −7.15] |
| Parent vs. parent: `single` checked | −0.79% [−6.99, +11.92] | −0.03% [−0.48, +2.11] | −0.80% [−6.94, +11.90] / −0.11% [−0.58, +2.30] |
| Parent vs. parent: `single` tokens | +0.18% [−7.02, +8.23] | −0.29% [−1.19, +1.92] | +0.37% [−6.88, +8.23] / −0.27% [−1.04, +1.92] |

### Fixed-archive CLI: no resolved wall gain

Two further balanced 24-pair series with fresh interleaved A/A on each setting. Both CLIs read the same C7 files and write to `/dev/null`. CPUs 0–7 are distinct physical cores; their SMT siblings are not reserved. The one-minute load at series boundaries was 6.56 → 7.42 → 13.78. All 12 binary/archive/worker combinations passed full exact-output checks before timing.

| Archive / workers | Paired wall Δ [interval], S1 / S2 | Paired child CPU Δ [interval], S1 / S2 |
| --- | --- | --- |
| `single` W1 | −2.5% [−6.4, −0.5] / −1.8% [−5.6, +0.7] | −2.6% [−6.5, −0.4] / −2.8% [−5.7, +0.0] |
| `single` W8 | −1.4% [−7.3, +1.7] / −5.2% [−7.5, +3.0] | −2.7% [−5.5, +0.5] / −2.1% [−3.8, −0.5] |
| `single` W8, A/A | +0.9% [−8.7, +6.9] / +0.4% [−12.4, +3.2] | +0.5% [−0.9, +2.9] / −2.6% [−6.0, +2.3] |
| `mozilla` W1 | −4.1% [−9.9, +0.8] / −3.7% [−5.3, −2.0] | −4.0% [−8.4, +0.9] / −3.4% [−5.4, −1.1] |
| `mozilla` W8 | −2.4% [−10.7, +2.8] / −3.2% [−8.7, +1.2] | −1.7% [−4.5, −0.3] / −2.1% [−7.1, +0.3] |
| `sao` W1 | +2.2% [−3.5, +12.7] / −0.9% [−5.0, +3.6] | +3.5% [−3.5, +10.2] / −1.5% [−4.4, +1.3] |
| `sao` W8 | −3.7% [−10.2, +11.3] / −4.8% [−13.1, +2.6] | +1.3% [−4.4, +5.1] / −1.5% [−5.6, +0.8] |

None of the wall contrasts resolves in both series. In particular, `single` W8 used median child CPU of 0.801 → 0.775 and 0.732 → 0.707 s/GB, but its wall intervals cover both a 5% throughput gain and a loss. This is not evidence of equivalence, nor reason to change the worker pool. No new encoder speed or Wasm measurements were made.

The retained Release CLI has byte-identical `.text` to the timed trial and separately matched original plaintext through EOF for `single`, `mozilla` and `sao` at both W1 and W8. It also produced byte-identical Dense encodings versus the parent for `multiblock_checksum`, `text_64k_bs2m` and `spl_tok_huffman_l7` plaintexts. All 69 Release workspace tests and 35 Release codec tests without default features passed. Format checking passed; Clippy passed with the existing `new_instead_of_clear` lint on the deliberate scratch-buffer release suppressed. Strict Clippy reports the same lint on the parent.

## SPL16 eligibility

One untimed checked pass on the original C7 bytes, from an instrumented parent, excluding the preliminary checksum-rejection pass. The two fast columns obey the real initialized-room, budget, literal and offset guards, so they are subsets of the raw length class. Distance-one counts only true overlap already in produced output, not dictionary-derived bytes.

| Archive | SPL16 tokens | Matches 20–64 | Fast 20–32 | Fast 33–64 | Distance-one matches (plaintext bytes) |
| --- | ---: | ---: | ---: | ---: | ---: |
| `single` | 12,132,680 | 1,156,049 | 568,651 (4.69%) | 570,618 | 6,605 (1,563,035) |
| `mozilla` | 2,918,407 | 254,528 | 159,973 (5.48%) | 86,955 | 4,129 (1,273,915) |
| `sao` | 467,648 | 0 | 0 | 0 | 32 (213) |

**Do not build the distance-one fill for these inputs.** It could touch just 0.054% of `single` tokens (0.74% of plaintext), and virtually nothing on `sao`. A 5% checked-block win at the observed baseline would need roughly 3,300 cycles saved per eligible `single` match before paying for an additional branch. This is a prioritization decision, not a timing result or a claim about all archives.

## What changed in the trials

- **SPL16 copy split:** for an already eligible 20–32-byte match extension, snapshot and store 32 initialized bytes instead of the 64-byte `copy_within`. The 33–64-byte path, initialized room guard, logical budget, and cursor commits are unchanged. This is not an overlap shortcut: the logical source must already be in output and nonoverlapping, even though the copy's physical slack can overlap. The counted x86-64 code emits two 16-byte loads and two 16-byte stores for the short branch, not a move call. The old 64-byte copy also inlines (four loads and four stores), so there was no call to remove. The split also introduces a length branch and the short path retains slice checks.
- **Huffman depth buckets:** unpack the same lengths, count by depth, check the same Kraft sum and the depth-one singleton case, then insert symbols in ascending order at bucket cursors. Only symbol ordering changes mechanism; canonical code assignment, node construction, flat lookup construction and section decoding are unchanged. The existing `Tree::new` is shared by encoder and decoder, so this is not a decode-only source change even though the timing comparison holds the archives fixed.

## Method and reproduction

All builds start from `dac698f924ecbe452432425e9047f61b6e118065`. Locked portable Release with default SIMD selection, no Tracy or ledger. The stock CLI checks exact length and SHA-256 through the footer and EOF; the [replay shim](decode-replay0.sol.md#what-ran) additionally tests a flipped intermediate block checksum or invalid section table before serving a timing request. It checks a fingerprint for each timed pass; the full untimed replay is checked against the manifest for plaintext and the parent for original literal and token bytes. The `small` control is the checked 16-block, 4 KiB SPL8 `multiblock_checksum` vector (64 KiB total), not a synthetic SPL16 extension benchmark.

One replay thread on logical CPU 3 (SMT sibling 11 not reserved), warmed resident processes. Two separately shuffled series of 24 balanced adjacent pairs per comparison, with a second copy of the parent process for A/A. A/A is a noise check, not a number subtracted from trial results. [The existing replay harness](decode-replay-data/perf.py) scopes `cycles:u,instructions:u` to each request and requires both counts at 100% running time. Paired percentage changes use the median within-pair ratio and the CLI harness's unadjusted two-sided order-statistic interval (97.7% at 24 independent pairs); temporally correlated host load can still affect the intervals. Cycles and instructions normalize by plaintext byte for checked blocks and decoded symbol for sections. Thread CPU is a separate cross-check.

Ryzen 7 5800X, Linux 7.1.0-debplus.1, `perf` 7.3.4, Rust 1.102.0-nightly (`bdf712ee8`), LLVM 23.1.6. Boost enabled, `powersave` governor (`amd-pstate-epp`). One-minute load at the initial series boundaries was 15.26 → 4.56 → 3.62; a falling host-wide average alone does not guarantee quiet execution on CPU 3 or its sibling.

The [replay evidence bundle](decode-replay-data/decode-next-trials.tar.xz) records the trial patches and test additions, per-block census, disassembly, build and test logs, exact verification identities, section indexes and raw paired order and counters. The copy and bucket scratch variants each passed 28 Release library tests with SIMD and without default features, including the copy boundaries 20/32/33/64, fresh scratch, true overlap, dictionary checks and truncation, and the singleton, invalid-depth and invalid-Kraft table checks for buckets. The [CLI evidence bundle](decode-replay-data/decode-next-cli.tar.xz) contains the CLI adapter, raw pairs, reductions and final verification logs. Reproduce from the repository root with the [prepared fixture cache](decode-pool.md#method-and-evidence):

```sh
RUN=.test-agent/decode-next-repro
FIX=.test-agent/decode-pool/fixtures
mkdir -p "$RUN"
tar -xJf doc/profiling/decode-replay-data/decode-next-trials.tar.xz -C "$RUN" \
  build.sh pairs.py census.py copy.patch bucket.patch census.patch
for label in base copy bucket; do bash "$RUN/build.sh" "$label"; done
python3 -B "$RUN/pairs.py" "$FIX" "$RUN" "$RUN/primary"
python3 -B "$RUN/pairs.py" --confirm-bucket "$FIX" "$RUN" "$RUN/confirm"
python3 -B "$RUN/pairs.py" --summary "$RUN/confirm/pairs.json"
tar -xJf doc/profiling/decode-replay-data/decode-next-cli.tar.xz -C "$RUN" cli.py
taskset -c 0-7 python3 -B "$RUN/cli.py" "$FIX" "$RUN/cli-base" "$RUN/cli-bucket" "$RUN/cli"
```
