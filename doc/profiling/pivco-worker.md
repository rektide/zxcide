---
type: Measurement
title: Reusing Huffman byte buffers per decode worker
description: Fixed-archive decode timing, allocation counts, and the retention cost of keeping PivCo output and merge buffers between blocks.
resource: /doc/profiling/pivco-worker.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e0867fb1fffeYLtmeIXhQF2Q0W', title: Connected the byte-buffer result to the subsequent traversal-queue trial }
  - { resource: 'opencode://ses_e055e0d34ffe9jui9j3JSrIqMx', title: Implemented and measured the worker-buffer experiment and verified interoperability }
  - { resource: '/doc/profiling/pipeline-comparison.sol.md', title: Rank 2 and fixed-archive promotion criteria }
  - { resource: '/doc/profiling/pivco-scratch.md', title: Rejected merge-layout trial, distinct from lifetime reuse }
  - { resource: '/doc/profiling/pivco-worker-data/timings.tsv', title: Clean paired measurements, including four-worker confirmation }
  - { resource: '/doc/profiling/pivco-worker-data/allocations.tsv', title: Separate diagnostic observations by worker }
---

# Reusing Huffman byte buffers per decode worker

**Keep the reuse change (`530db0a0`).** Serial Rust took **10.8–12.8% less wall time** to decode fixed Dense `sao` and **6.4–8.1% less** on fixed C7 `sao`. Both gains also show up in child CPU. Dense `sao` needed seven byte-buffer allocations or growths instead of 56 and retained 661,111 bytes after its last block instead of zero. It released all three oversized Huffman buffers at the first 4 KiB RAW block of a following frame. Full-corpus gains were modest, four-worker wall did not reliably improve, and the default scalar WIT component did not show a repeatable `sao` win or material regression. This is not evidence for a new merge layout.

## What changed

[`BlockDecoder`](../../crates/codec/src/block.rs) owns independent literal and token Vecs and one merge Vec shared by the two Huffman sections in sequence. It exposes only the exact decoded prefixes. New capacity is reserved exactly on growth; later smaller sections under the same frame limit keep initialized tails but do not read or publish them. RAW still borrows the input, RLE still owns its temporary result, and traversal order/queue and inline codebook allocations are unchanged. The [PivCo traversal](../../crates/codec/src/entropy.rs), native and scalar gather, and per-node copy-back are unchanged. So are LZ, checksum and output-limit checks, archive bytes, encoder policy, and the scheduler. The dictionary’s immutable Huffman tree uses the same byte buffers as an inline tree; it is not rebuilt per block.

For a validated block with frame limit `B`, the three byte capacities sum to at most `2B + floor(B/5)` (1,153,433 bytes at 512 KiB; 4,613,734 bytes at the 2 MiB format limit). This excludes allocator overhead, temporary allocator internals, reconstruction/input buffers, metadata, and dictionary storage. The first block of a smaller frame discards any capacity above the new per-buffer bound, before the RAW/coded dispatch. A short final block under an unchanged frame limit does not downsize, and an empty concatenated frame never calls the block decoder. Parallel pools create fresh worker contexts per frame; the serial decoder keeps its context across frames and calls. WIT creates a decoder per call, not per instance. No unsafe Rust or new dependency.

## Fixed-archive timing

Ryzen 7 5800X, Linux `7.1.0-debplus.1`, rustc `1.101.0-nightly (822182db4)`, Wasmtime `46.0.3`. Baseline Rust `50cb06993313cc58e7f1151a7501f3e151492476` includes the short one-byte SPL16 match-extension path; candidate `530db0a02f0559fee5a568698075702153dbff7e` adds the three buffers. Locked `cargo build --release -p zxc-cli` with CLI defaults (`merge-simd`), no extra rustflags, Tracy, ledger, or ad hoc counters. Both built in a fresh experiment target and saved under distinct paths before running. WIT used locked Release `zxc-component` (`wasm32-unknown-unknown`, default scalar), wrapped with `wasm-tools component new`. The [SHA-256 manifest](pivco-worker-data/sha256.txt) identifies binaries, components and archives. No timing from `20261120T080219Z` was used.

Both Rust binaries decoded the **same** archives. The `sao` Dense, C7 and Split hashes match the [corrected comparison](pipeline-comparison.sol.md#what-the-corrected-run-establishes); C7 comes from upstream `5719a96f778226868b29dba621a8c4e302b74d7c`, built native Release, encoded `-7 -T1`. Each full-corpus case concatenates all 12 checksummed 512 KiB-block frames in filename order (211,938,580 plaintext bytes). `Mixed` and `noise` use the repository's [ledger inputs](../../tests/ledger_inputs.rs): encode the former Dense at 4 KiB or 512 KiB and the latter Split at 4 KiB (all RAW). The sparse-entropy control prepends one 4 KiB mixed block to the noise and encodes Dense at 4 KiB. Downshift concatenates Dense `sao`, RAW noise at 4 KiB, then 4 KiB Dense mixed.

CLI timing includes one process launch, warm-cache archive reads, integrity checks and output to `/dev/null`; worker 1 runs serial on CPU 2 and worker 4 uses Crossbeam (eight slots) on distinct cores 2–5. Two series of 12 adjacent baseline/candidate pairs with balanced shuffled order, after one warmup pair per series. Before timing, each pair of binaries produced the plaintext SHA-256 through EOF at each worker setting, and the driver required empty stderr and status 0 on **every** invocation. WIT ran one typed Wasmtime host executable on CPU 2, instantiating each component outside timing. Each measurement is the mean of 6 (`sao`), 12 (4 KiB mixed), or 32 (512 KiB mixed or RAW noise) complete WIT calls, including list transfer and disposal; two balanced 12-pair series, one warmup pair each. Figures below are medians of within-pair change for each series, rather than ratios of unpaired medians. Negative means less time.

| Archive | Native 1 wall | Native 1 child CPU | Native 4 wall | WIT wall |
| --- | ---: | ---: | ---: | ---: |
| Dense `sao` | −10.8% / −12.8% | −11.3% / −12.9% | −9.1% / −0.4% | −0.6% / −0.8% |
| C7 `sao` | −6.4% / −8.1% | −5.8% / −8.2% | −0.7% / +0.0% | −4.2% / −0.2% |
| Dense Silesia | −2.5% / −2.7% | −2.7% / −2.5% | −2.6% / −1.4% | — |
| C7 Silesia | −4.9% / −2.8% | −5.5% / −2.9% | +5.4% / +0.0% | — |
| Split `sao` (no Huffman) | +2.5% / −1.6% | +2.8% / −1.4% | −5.2% / −1.2% | −1.3% / −0.7% |
| Mixed, 4 KiB | +0.3% / −2.3% | +0.7% / −2.5% | −5.2% / +5.4% | −0.4% / −1.2% |
| Mixed, 512 KiB | −0.4% / −11.4% | −2.2% / −8.1% | −14.1% / −2.5% | −0.7% / −0.4% |
| Sparse entropy, 4 KiB | +7.0% / +1.2% | +8.4% / +1.2% | −2.0% / −1.4% | — |
| RAW noise, 4 KiB | −4.4% / −7.2% | −0.4% / −3.9% | +2.2% / −0.6% | −0.8% / +0.4% |
| 512 KiB → 4 KiB, concat | −10.1% / −9.2% | −10.2% / −9.0% | — | — |

The focused four-worker rerun (two series of 24 pairs) measured wall changes of **+1.1% / +1.6%** on C7 Silesia, +1.2% / +0.2% on Dense Silesia, −0.3% / +1.2% on 4 KiB mixed and +3.9% / +1.3% on RAW noise. CPU deltas on the RAW control were −0.6% / +8.7%; this ~2 ms launch/worker control does not establish a reproducible regression, but limits what can be inferred from parallel deltas of that size. To reduce startup noise, repeat both short 4 KiB controls 32 times *within one frame* (repeat the plaintext before encoding; not 32 pool launches). Serial RAW wall was +0.7% / +1.1%; sparse-entropy wall +3.5% / +0.6% with CPU +0.6% / +1.1%. Four-worker sparse-entropy wall was +2.4% / +1.6%, and RAW wall ranged from −18.2% to −0.5%. These controls leave room for a small cost on entropy-light inputs, but no repeatable >3% wall regression. Dense and C7 `sao` four-worker child CPU did improve in both initial series (−9.0% / −4.1% and −4.5% / −3.9%), without a matching repeatable wall result. Do not claim a parallel scaling gain.

Separate whole-process `perf stat` on `sao` found essentially unchanged instructions on Dense (<0.2% difference); it does not isolate PivCo or explain the time delta. The larger savings in explicit zero-init volume are **not** the same as fewer physical stores (e.g. baseline zero allocations may be demand-zeroed).

## Allocation and retention

The [follow-up traversal queue trial](pivco-queue.md) isolated metadata allocation from the byte buffers measured here. On 4 KiB mixed it removed 765 of 768 queue allocations or growths, but the two serial elapsed improvements (4.0% and 0.9%) fell short of the 5% adoption criterion. Only the byte-buffer reuse described here shipped.

Separate diagnostic executables count successful sections, initial allocations and capacity growths at the **two byte-buffer sites**, and traversal Vec/queue allocations/growth separately. Bytes below are Vec capacities, not process RSS or peak per request across all workers. Baseline peak includes a Huffman literal output still live while its token section decodes. For the trial, each retained buffer can record its own high water from a different block. Figures for four workers in the [allocation table](pivco-worker-data/allocations.tsv) are individual active worker contexts; Silesia runs more than four contexts in total because pools are frame-local. Timing never uses diagnostic executables.

| Serial archive | Huffman sections | Byte-buffer allocs total (per section) | Zero-init bytes | Peak bytes | Retained after last block |
| --- | ---: | ---: | ---: | ---: | ---: |
| Dense `sao` | 28 | 56 → 7 (2 → 0.25) | 9,492,508 → 661,111 | 625,304 → 661,111 | 0 → 661,111 |
| C7 `sao` | 20 | 40 → 12 (2 → 0.60) | 4,612,428 → 660,154 | 625,794 → 660,154 | 0 → 660,154 |
| Dense Silesia | 813 | 1,626 → 17 (2 → 0.021) | 98,050,428 → 960,535 | 897,900 → 960,535 | 0 → 960,535 |
| C7 Silesia | 797 | 1,594 → 33 (2 → 0.041) | 93,727,200 → 855,128 | 802,696 → 855,128 | 0 → 855,128 |
| Mixed, 4 KiB | 256 | 512 → 2 (2 → 0.008) | 1,179,620 → 4,608 | 4,608 → 4,608 | 0 → 4,608 |
| Mixed, 512 KiB | 4 | 8 → 4 (2 → 1) | 1,050,168 → 524,838 | 524,504 → 524,838 | 0 → 524,838 |
| Sparse entropy, 4 KiB | 1 | 2 → 2 (2 → 2) | 4,608 → 4,608 | 4,608 → 4,608 | 0 → 4,608 |
| Split `sao` / RAW noise | 0 | 0 → 0 (—) | 0 → 0 | 0 → 0 | 0 → 0 |

Dense `sao` still allocates 112 traversal buffers/growths in either build, excluding inline tree construction. That is (56 + 112) / 28 = 6.00 counted allocation/growth events per decoder section before, and (7 + 112) / 28 = 4.25 after. It still unpacks 4,542,662 flat symbols, gathers 15,059,269 mixed-run bytes and copies 15,059,269 bytes back. The four-worker Dense `sao` diagnostic counted 56 → 21 byte-buffer allocations, with about 643–660 kB retained per active worker at that frame's end (before pool drop). The longer sparse-entropy case had exactly 32 Huffman sections and needed 64 → 2 byte-buffer allocations in serial, retaining only 4,608 bytes. The serial [downshift trace](pivco-worker-data/downshift.tsv) shows 661,111 bytes retained after `sao`, zero at the first and last 4 KiB RAW block, then just 4,608 bytes after decoding 4 KiB mixed. The last short `sao` block alone does not shrink the buffers. Metadata compaction would be a separate experiment.

## Verification and evidence

`cargo nextest run --locked --workspace --features zxc-cli/transports` passed 57 tests; release scalar codec nextest passed 32. The new worker test decodes dictionary Huffman literals and inline Huffman tokens together, rejects a truncated body and a corrupt token bitmap selecting a missing child, reuses the same worker on a shorter valid body, verifies the RAW downshift, and exercises `decode_owned`. Existing tests cover partial bitmap tails, run exhaustion, dictionary history/crossings, checksums, ordered publication, indexed ranges and cumulative limits. The typed WIT harness checked exact bytes, output limits and truncation before timing, then decoded the same valid input again. Codec Clippy (`merge-simd,ledger`, all targets, warnings denied) and component validation passed.

`cargo xtask interop <pinned-C> <trial-Rust> --cases 36` passed 180 frozen cross-decodes and 72 indexed reads, plus 72 generated cross-decodes and 156 indexed reads; `--parallel-backend crossbeam --cases 36` independently passed the generated counts. The oracle here is the **same** upstream performance pin as the C7 fixture. See the [verification log](pivco-worker-data/verification.txt), [paired samples](pivco-worker-data/timings.tsv), [allocation and worker capacities](pivco-worker-data/allocations.tsv), and [fixture and build identities](pivco-worker-data/sha256.txt). Full build logs, compressed per-block diagnostics, harnesses and frozen artifacts remain locally under `.test-agent/pivco-worker/`.
