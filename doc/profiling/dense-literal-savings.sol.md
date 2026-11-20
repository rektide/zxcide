---
type: Measurement
title: Dense literal Huffman savings sweep on Silesia
description: Wire-exact literal selection thresholds at 0%, 1%, 2%, and 4%, with paired native Rust and pinned C decode timings and encode cost.
resource: /doc/profiling/dense-literal-savings.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e04f22460ffeiMpWBTvOxfQzAN', title: Built the threshold candidates and ran the paired Silesia sweep }
  - { resource: '/doc/profiling/pipeline-comparison.sol.md#experiments-in-priority-order', title: Proposal to price decode work when emitting new Dense archives }
  - { resource: '/doc/profiling/pivco-worker.md', title: Kept decoder-buffer reuse and frozen archive controls }
  - { resource: '/doc/profiling/dense-decode.md#next-targets', title: Earlier 2% scratch experiment on sao }
  - { resource: '/doc/profiling/dense-literal-savings-data/provenance.txt', title: Exact build, host, and artifact identities }
---

# Dense literal Huffman savings sweep on Silesia

**Keep Dense's existing 0% policy.** All positive thresholds stayed below the 1% corpus size budget, but none delivered a credible 5% full-corpus serial wall-time reduction on either decoder. At 4%, Rust used 5.70% less serial CPU but took only 3.36% less wall time, with enough variation to allow no wall gain. Without `sao`, its CPU gain disappeared. Pinned C did not show a corresponding full-corpus serial CPU or wall gain. No production code or tests changed.

The [earlier two-file scratch trial](dense-decode.md#next-targets) saw a promising `sao` speedup at 2%. The sweep reproduces its `sao` size delta (+35,538 bytes) and unchanged `dickens` archive. It tests whether the policy generalizes, rather than comparing the earlier absolute decode times to the newer builds.

## Exactly what changed

Use the unmodified post-buffer-reuse Rust revision `530db0a02f0559fee5a568698075702153dbff7e` for **all Rust decoding** and the 0% encode control. Candidates change only literal selection after Dense parsing. Let `N` be the raw literal bytes and `H` the Huffman payload (the individual byte-rounded PivCo runs, plus the 128-byte table for inline Huffman). Huffman must be **strictly smaller than RAW**, including the non-RAW four-byte descriptor. At positive threshold `p`, it must also meet `100 × (N − H − 4) ≥ p × N`; equality at the percentage boundary qualifies. The patch computes an integer ceiling in bytes and reduces the existing exclusive payload budget *before* emission. There is no floating-point estimate or truncated percentage. Common SPL, frame, and checksum fields are not charged to a single candidate. Block padding and whole-block RAW fallback remain unchanged; actual archive lengths, not section estimates, determine the size gate.

RLE is not taxed. It must strictly beat raw including its own descriptor. A Huffman candidate that clears the threshold against raw still has to beat RLE if RLE is smaller than raw. Dictionary Huffman, where eligible, gets the same threshold with the descriptor but no inline table charge. Both Huffman candidates share the cap. The original strict-size comparison means earlier candidates win equal wire costs: RAW before RLE before shared-dictionary Huffman before inline Huffman. This corpus used no dictionaries, so the dictionary treatment is a property of the scratch patch, not a measured dictionary outcome. The token path never enters the new branch; no change to parser, histogram, lengths, bit layout, RLE candidate construction, or decode policy.

## Archive size and changed work

All **411 data blocks** were SPL. At 0%, 402 had inline Huffman literals (35,853,686 symbols), eight had raw literals and one used RLE. Token bytes were identical across variants. The only changes in literal mode were Huffman to raw. The 1%, 2% and 4% variants dropped respectively **2 / 11 / 21** Huffman literal sections and **0.896 / 3.290 / 6.100 million** decoded Huffman symbols. Away from `sao`, this affected `samba` (2 / 5 / 5 sections removed) and `mozilla` (0 / 0 / 2). `sao` accounts for **57% / 70%** of the removed symbols at 2% / 4% and **84% / 83%** of the corresponding archive growth. For every file, the descriptor-adjusted literal section growth exactly accounted for the archive growth. The 0% corpus archive also matched the prior worker-buffer Dense fixture byte for byte.

| File | 0% bytes | 1% bytes | 2% bytes | 4% bytes |
| --- | ---: | ---: | ---: | ---: |
| `dickens` | 3,729,868 | same | same | same |
| `mozilla` | 21,203,827 | same | same | 21,214,220 |
| `mr` | 3,519,447 | same | same | same |
| `nci` | 3,163,506 | same | same | same |
| `ooffice` | 3,386,452 | same | same | same |
| `osdb` | 3,560,951 | same | same | same |
| `reymont` | 1,876,568 | same | same | same |
| `samba` | 5,588,569 | 5,589,259 | 5,595,401 | 5,595,401 |
| `sao` | 5,531,098 | same | 5,566,636 (+0.643%) | 5,617,186 (+1.556%) |
| `webster` | 12,057,866 | same | same | same |
| `x-ray` | 6,155,668 | same | same | same |
| `xml` | 686,065 | same | same | same |
| **Full corpus** | **70,459,885** | **70,460,575 (+0.001%)** | **70,502,255 (+0.060%)** | **70,563,198 (+0.147%)** |
| Without `sao` | 64,928,787 | 64,929,477 | 64,935,619 | 64,946,012 |

## Decode and encode results

Full-corpus median **wall / child CPU** for 0% in ms; the other columns are the median **within-round percent changes** vs 0% (negative takes less time). The [summary script](dense-literal-savings-data/summarize.py) prints intervals and separate half-run medians for every cell.

| Decoder | Workers | 0% wall / CPU ms | 1% Δ wall / CPU | 2% Δ wall / CPU | 4% Δ wall / CPU |
| --- | ---: | ---: | ---: | ---: | ---: |
| Rust | 1 | 218.41 / 201.08 | +5.63% / +2.41% | +1.98% / +0.07% | −3.36% / −5.70% |
| Rust | 4 | 158.79 / 284.54 | +0.31% / −1.52% | +4.21% / −1.87% | −0.90% / −5.78% |
| Rust | 8 | 160.63 / 320.35 | −4.03% / −0.85% | −2.28% / −2.86% | −15.25% / −5.45% |
| Pinned C | 1 | 189.81 / 193.02 | +4.64% / +2.73% | +0.46% / −1.43% | +7.18% / +0.64% |
| Pinned C | 4 | 122.57 / 174.48 | −3.31% / +1.37% | −6.70% / +0.42% | −3.79% / −4.12% |
| Pinned C | 8 | 83.55 / 188.08 | −2.59% / −1.22% | +0.60% / −1.29% | +2.32% / −4.34% |

**Rust.** At 4%, full-corpus serial wall change was −3.36% [−15.36%, +5.25%], with half-run medians of −8.22% and −0.67%. Serial CPU change was −5.70% [−7.92%, −2.13%]. That is a CPU saving, not evidence for the serial wall gate. The CPU saving also appeared with 4 and 8 workers; the apparent eight-worker wall gain still had an interval crossing zero [−22.28%, +1.92%].

**Pinned C.** The best full-corpus serial wall point among candidates was +0.46% at 2% [−9.04%, +16.52%]. At 4%, serial wall was +7.18% [−9.01%, +22.80%], and the CPU change was +0.64% [−5.53%, +5.54%]. Four- and eight-worker CPU at 4% did fall (intervals [−6.94%, −2.95%] and [−7.37%, −2.20%]); neither showed a reliable wall gain. Neither decoder independently meets the promotion gate.

**Sensitivity to `sao`.** These are separate one-worker launches, with the same pairing and metric as above. `sao` at 1% has exactly the same bytes as 0%, so its measured differences indicate noise rather than policy effects.

| Decoder / input | 0% wall / CPU ms | 1% Δ wall / CPU | 2% Δ wall / CPU | 4% Δ wall / CPU |
| --- | ---: | ---: | ---: | ---: |
| Rust / `sao` | 17.10 / 15.93 | −4.28% / −3.64% | −20.11% / −21.56% | −44.38% / −44.20% |
| Pinned C / `sao` | 14.74 / 15.97 | −3.63% / −2.65% | −3.23% / −7.25% | −20.54% / −18.80% |
| Rust / without `sao` | 208.70 / 181.91 | +4.09% / +0.62% | +1.81% / +2.04% | −1.33% / −0.10% |
| Pinned C / without `sao` | 176.92 / 186.37 | +0.78% / +0.88% | −3.51% / −1.43% | −2.21% / −0.31% |

At 4%, `sao` wall intervals excluded zero on both Rust [−53.98%, −40.02%] and C [−24.73%, −9.79%]. Without `sao`, all wall and CPU intervals included zero for both decoders. The 4% serial CPU intervals without it were [−3.78%, +4.12%] in Rust and [−4.66%, +1.93%] in C. These sensitivities do not partition the full-corpus wall medians; they show that the evidence for a benefit does not extend convincingly past `sao`.

**Encode cost.** CPU and elapsed time include the Dense parser and the entire encode process, not just the entropy pass. The medians below are sums over 12 launches per pass. The last column pairs the CPU sums before taking the median; it need not match a ratio of the unpaired medians. Small encode differences at 1% and 2% are inconclusive. The 4% build used more encode CPU than 0% in every pass; this does not isolate the cost of threshold arithmetic from compiler layout and host variability.

| Threshold | Median wall ms | Median CPU ms | Paired CPU change (min–max) |
| --- | ---: | ---: | ---: |
| 0% | 16,986.0 | 16,027.6 | baseline |
| 1% | 15,167.6 | 14,883.2 | +0.18% (−12.81% to +21.56%) |
| 2% | 15,711.0 | 15,184.4 | +1.04% (−11.04% to +31.45%) |
| 4% | 18,720.2 | 17,685.8 | +8.64% (+1.52% to +27.75%) |

## Measurement and replay

The 12 plaintexts are the [frozen Silesia files](upstream-silesia.md), 211,938,580 bytes total. Dense used one encode worker, checksums on, 512 KiB blocks and no dictionary or seek table. Each variant's corpus archive is the 12 separately encoded frames concatenated in filename order. Both decoders read the **same regular file** for each variant; sensitivities use the other 11 frames concatenated and `sao` alone, at one worker. No archive or plaintext bytes are checked into the repository; see [SHA-256s and sizes](dense-literal-savings-data/sizes.tsv) and the [block census](dense-literal-savings-data/blocks.tsv).

The Ryzen 7 5800X used the same eight distinct physical cores for every decode setting. Rust one worker is the serial CLI; upstream C `-T1` retains reader/writer pipeline threads. Decode timing includes one process launch, warm archive reads, checksum verification, and stdout to `/dev/null`. CPU time is the child's user plus system time, accumulated over its threads by `wait4`, not the elapsed time multiplied by worker count. After exact output checks and one discarded decode warmup per cell and threshold, each of the 24 measured rounds shuffled decoder/worker/corpus cells and ran all four thresholds adjacently per cell, using a different one of the 24 threshold permutations per round. Decode statistics pair against the 0% result in the same round and cell, then take the median. For the serial results, `[lo, hi]` uses the 7th and 18th observations of the sorted paired deltas (a conservative 95% sign interval if rounds are independent). These are local comparisons on one host, not independent corpus replications. Encode timing used seven randomized passes (paired file order within pass), one CPU, per-file process launches and stdout to `/dev/null`; corpus CPU is the sum over 12 files in a pass.

Before timing, both decoders reproduced the bytes directly through EOF for all four variants at 1/4/8 workers on all 12 frames, and at one worker on each sensitivity input: [40 exact checks](dense-literal-savings-data/verification.tsv). Every check and timed command exited 0 with empty stderr. The census additionally required unchanged block kind, sequence/literal counts, token mode, bytes from the end of the literals (including tokens and the entire offset/extra/slack tail), and block checksums. For each inline-to-raw transition, it checked that the wire savings actually failed the threshold. Unchanged literal modes had byte-identical literal bodies.

Use a fresh scratch path from the repository root. Obtain the hash-checked ZIP from [the pinned Silesia corpus](https://github.com/DataCompression/corpus-collection/tree/d59143a41cc7d65063a70e86063a3c39aeae6d9f/Silesia-Corpus) or equivalent previously frozen files. For the C CLI, use [upstream `5719a96f`](https://github.com/hellobertrand/zxc/tree/5719a96f778226868b29dba621a8c4e302b74d7c) built Release, `ZXC_NATIVE_ARCH=ON`, SIMD enabled, LTO/PGO off, and vendored rapidhash. The run reused the same pinned C ELF as the worker-buffer study. Full commands, build hashes and host are in [provenance](dense-literal-savings-data/provenance.txt); the threshold patch is applied only in scratch.

```bash
data=doc/profiling/dense-literal-savings-data
scratch=.test-agent/dense-literal-savings-replay
bash "$data/build.sh" "$scratch" target/upstream-comparison/build/zxc
flock -x -w 900 "$PWD/.test-agent/zxc-perf.lock" \
  python3 "$data/run.py" "$scratch" /path/to/frozen/silesia
python3 "$data/summarize.py" "$scratch/run"
```

Raw samples: [decode](dense-literal-savings-data/decode.tsv), [encode](dense-literal-savings-data/encode.tsv), and [run log](dense-literal-savings-data/run.log). Run `python3 "$data/summarize.py"` without an argument to summarize the committed samples.
