---
type: Measurement
title: PivCo traversal queue reuse after byte-buffer reuse
description: Fixed-archive promotion test of a bounded worker-local decode FIFO, with separate attribution and complete-call WIT controls.
resource: /doc/profiling/pivco-queue.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e04ee00ceffe67FhEvegYdJa7m', title: Implemented the isolated queue trial, measured its promotion gates and checked correctness }
  - { resource: /design/pivco-metadata/assessment0.astra.md, title: Single-variable queue experiment and predeclared promotion gates }
  - { resource: /doc/profiling/pivco-worker.md, title: Retained byte-buffer optimization and fixed-archive protocol reused here }
  - { resource: /doc/profiling/pivco-queue-data/allocations.tsv, title: Allocations and transient capacities by fixture and worker setting }
  - { resource: /doc/profiling/pivco-queue-data/native.tsv, title: Clean paired native wall and child CPU observations }
  - { resource: /doc/profiling/pivco-queue-data/wit.tsv, title: Clean paired whole-call default scalar component observations }
---

# PivCo traversal queue reuse after byte-buffer reuse

**Drop the queue reuse change.** Serial 4 KiB mixed removed **765 of 768 FIFO events (99.61%)**, but elapsed wall time fell only **4.0% / 0.9%** across the two series, short of the required **5% in both**. Child CPU fell **3.1% / 1.1%**. Correctness and memory bounds passed. Keep the post-byte-buffer decoder unchanged; retain the opt-in trial patch and evidence here.

## What was tested

The [design](../../design/pivco-metadata/assessment0.astra.md) requires correctness, bounded retention, at least 90% fewer FIFO capacity events, at least 5% lower serial 4 KiB mixed wall time in **both** series with lower child CPU in both, and no repeatable regression over 3% on controls, dictionary, full corpora or WIT. The [baseline](pivco-worker.md) at `3db98c67065f6bb6d49c88a1fae83ccd05efcee1` already reuses the three Huffman byte buffers. It creates a local `VecDeque` and reserves a separate reverse-merge `Vec` for every Huffman section.

The [trial patch](pivco-queue-data/candidate.diff) lends a worker-local FIFO to literal and token Huffman sections in sequence. `mem::take` leaves the cache empty; an error inside Huffman drops the loan even if work remains. Only a successfully drained FIFO of capacity ≤16 is returned. Larger frontiers can grow as before; the cap is on retention, not wire admission. RAW/RLE don't prepare the FIFO. The trial doesn't change traversal order, wire validation, reverse-merge records, gather/copy-back, the dictionary, or the scheduler. The ledger already observes the local loan and counts it once in the active section. Unlike the [earlier scratch-layout trial](pivco-scratch.md), this changes allocation lifetime without moving a single bitmap or merge.

## Allocation and retention gates

Separate instrumented builds counted initial creation, capacity growth, reuse and discard; these are capacity events, not a census of `malloc` calls. Inline tree construction (including its own queue and lookup tables) is outside the count. Both revisions had identical section counts, order lengths/capacities, logical frontier maxima, flat symbol counts and gather/return traffic at the same block/section positions, including at four workers. Dense `sao` had 28 Huffman sections, 4,542,662 flat symbols, and 15,059,269 B **each** gathered and copied back.

| Fixed archive, serial | Sections | Baseline FIFO cold + growth | Trial FIFO cold + growth | Trial reuses | Max live / capacity slots | Trial retained slots |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Mixed, 4 KiB | 256 | 256 + 512 | 1 + 2 | 255 | 5 / 8 | 8 |
| Mixed, 512 KiB | 4 | 4 + 6 | 1 + 2 | 3 | 5 / 8 | 8 |
| Dense `sao` | 28 | 28 + 56 | 1 + 2 | 27 | 8 / 8 | 8 |
| C7 `sao` | 20 | 20 + 30 | 1 + 2 | 19 | 7 / 8 | 8 |
| Dense Silesia | 813 | 813 + 1,962 | 1 + 3 | 812 | 12 / 16 | 16 |
| C7 Silesia | 797 | 797 + 1,167 | 1 + 3 | 796 | 12 / 16 | 16 |
| Sparse entropy, 4 KiB | 1 | 1 + 2 | 1 + 2 | 0 | 5 / 8 | 8 |
| Sparse entropy, 32× | 32 | 32 + 64 | 1 + 2 | 31 | 5 / 8 | 8 |
| Dictionary (one block) | 2 | 2 + 1 | 1 + 1 | 1 | 2 / 4 | 4 |
| Dictionary (256 blocks) | 512 | 512 + 256 | 1 + 1 | 511 | 2 / 4 | 4 |
| Split `sao` and both all-RAW noise controls | 0 | 0 + 0 | 0 + 0 | 0 | 0 / 0 | 0 |

The primary reduction is **99.61%** (768 → 3); all three trial events are from the first section, then there is no growth. The unchanged order Vec still allocates once per section. For every primary section it reserves 511 records, writes 8, and replays at most those 8. The native64 request is 20,440 B for order alone (320 B of occupied records), and 20,632 B for order plus peak FIFO capacity. At serial full Silesia the observed maximum order lengths are 33 Dense and 32 C7, with 511-slot maximum reserves. A separate **local order-reserve** experiment is worth testing for allocation size; this trial gives no evidence that it would save time. Changing the record representation, merge order, or order lifetime would confound the reserve question.

- **End of block.** `BlockDecoder` grew from 96 to 128 B native64 and from 48 to 64 B wasm32, even on RAW-only workers. A fresh RAW-only worker allocates no FIFO elements. Retained FIFO element capacity is at most 384 B per native64 worker (192 B wasm32), additional to the byte Vec limit `2B + floor(B/5)` for frame limit `B`. At 4 KiB the two capacities together are bounded by 9,395 B per native64 worker, excluding headers, reconstruction, input and allocator overhead. Live frontier and reserved FIFO capacity are different quantities.
- **Frame end before drop.** Serial keeps its last queue across frames and calls; a Crossbeam worker drops it with the frame, and WIT drops it with the guest decoder after each call. The [per-worker frame-end table](pivco-queue-data/worker-ends.tsv) reports each worker's last completed block, not the capacity of a single global pool. Four active native64 workers can retain at most 1,536 B of FIFO elements plus 128 B of inline headers; at 4 KiB they can retain at most 37,580 B of byte Vec plus FIFO elements combined. The largest observed four-worker frame-end FIFO totals were 768 B on 4 KiB mixed or Dense `sao` and 1,536 B on both full corpora. These are capacities, not RSS or transient allocator overhead.
- **Frame downsize.** On serial Dense `sao` → RAW 4 KiB → mixed 4 KiB, `(literal,token,merge; FIFO slots)` at the last block of each frame was `(312652,35807,312652; 8)` → `(0,0,0; 8)` → `(2304,0,2304; 8)`. The first and last RAW blocks both had zero byte Vec capacity. The baseline had the same byte capacities, with no retained FIFO. No valid fixture needed >16 FIFO slots, so the focused test supplied an empty 32-slot buffer; even a successful call had to drop it.

See [allocation totals](pivco-queue-data/allocations.tsv), raw [section](pivco-queue-data/sections.tsv.gz) and [block](pivco-queue-data/blocks.tsv.gz) records, and [error probes](pivco-queue-data/error-traces.txt). `discarded_slots` counts the sites' local capacity being dropped, not bytes or deallocation calls. The baseline drops every local FIFO; candidate success only discards oversized capacity.

## Correctness

The new worker test truncates the left run of a multi-level tree with its right sibling still queued, then decodes a shorter payload under a different tree in the same worker. It compares exact prefixes and Rust `InvalidData` messages for short and trailing runs, missing singleton children, and a nonempty trailing payload at count zero. Empty count with no run succeeds. The loan is gone after Huffman errors; late decoded-size or checksum failure leaves a drained cached FIFO, and `decode_owned` still publishes only the exact verified prefix. The [stream recovery harness](pivco-queue-data/stream-recovery.rs) checks both a cumulative limit and a corrupt second block: only the first verified block is written, then the same serial `Decoder` decodes the original stream through EOF. Grouping, indexed ranges, EOF and footer checks remain covered by the workspace and interop gates.

The dictionary tests use literal mode 3 plus inline token mode 2 in the same worker, check missing and wrong dictionaries, switch to a different matching dictionary/tree, and cover dictionary-to-output crossings. The fixed dictionary timing sample uses a length-1/2/2 codebook, explicit dictionary-to-output overlap and a checksum on every block; the diagnostic observed 256 mode-3 literal and 256 mode-2 token sections in the repeated case. The cold sample is one block, including dictionary parse in a fresh process; the repeated sample is one parse plus 256 full blocks in a single frame. The [verification log](pivco-queue-data/verification.txt) contains the Rust and pinned-C results. WIT has no dictionary argument: its typed harness checks exact bytes, rejects truncation and too-small output limits, then decodes valid input again on the same instance.

## Timing protocol and reproduction

On the Ryzen 7 5800X, Rust `1.101.0-nightly (822182db4)` and Wasmtime `46.0.3`, separate `git archive` exports and separate Cargo targets produced both builds. The only source delta was the [candidate patch](pivco-queue-data/candidate.diff). Both CLIs used `cargo build --locked --release -p zxc-cli` with the default `merge-simd` feature; both components used `cargo build --locked --release -p zxc-component --target wasm32-unknown-unknown` with default scalar settings, then `wasm-tools component new`. No native tuning, ledger, Tracy, or ad hoc counters in either timing binary. The [environment](pivco-queue-data/environment.txt) and [SHA-256 manifest](pivco-queue-data/sha256.txt) identify the artifacts, patch, archives and plaintext. The C7 files and the separate interoperability CLI use upstream `5719a96f778226868b29dba621a8c4e302b74d7c`. The two full corpora are checksummed 12-frame concatenations of 211,938,580 plaintext bytes each. None of the fixed archives were re-encoded by the candidate.

The [native driver](pivco-queue-data/native.py) hashes stdout through EOF for both decoders at each worker setting before sampling. It requires exit 0 and empty stderr on every invocation, with stdout redirected to `/dev/null` during measurement. Native timings include process launch, warm-cache archive reads and integrity checks. Serial was pinned to CPU 2; four-worker Crossbeam used eight slots and distinct physical cores 2–5. The [typed WIT host](pivco-queue-data/wit.rs) was pinned to CPU 2. It instantiates each component outside timing; each sample averages 6 complete calls for `sao`, 12 for 4 KiB mixed, or 32 for the other controls, including both list transfers and disposal. Each job uses an exclusive `flock` on `/home/rektide/src/zxc-rs/.test-agent/zxc-perf.lock`. Each fixture gets two series of 12 adjacent A/B pairs, shuffled and balanced within each series, after one warmup pair per series. Deltas below are medians of `100 × (trial/base − 1)` within pairs, not ratios of independent medians. Native CPU is child user+system summed across threads; WIT has no child process metric.

To reproduce, stage `baseline`, `candidate`, `diag-baseline` and `diag-candidate` source exports from `3db98c67065f6bb6d49c88a1fae83ccd05efcee1` under `.test-agent/pivco-queue/`. Apply the trial patch with `patch -p1 -d candidate` and to the independent `diag-candidate`. Place the [fixed worker fixtures](pivco-worker.md#fixed-archive-timing) in `fixtures/`. For dictionary samples, copy the [generator](pivco-queue-data/dictionary-fixture.rs) into the diagnostic baseline codec as a test module and run it with `QUEUE_FIXTURES` set to the absolute fixture directory. Run [instrument.py](pivco-queue-data/instrument.py) alongside those exports before building the diagnostic binaries; [trace.py](pivco-queue-data/trace.py) uses `bin/diag-baseline` and `bin/diag-candidate` only. Freeze the clean CLIs as `bin/base` and `bin/trial` and their components as `bin/base.wasm` and `bin/trial.wasm`. Build the host from the same baseline export as example `queue_wit` with `cargo build --locked --release -p zxc-component --features host-bench --example queue_wit` and freeze it as `bin/wit`. Run from the scratch directory with the drivers copied there:

```sh
lock=/home/rektide/src/zxc-rs/.test-agent/zxc-perf.lock
flock "$lock" python3 native.py > native.tsv 2> native.log
flock "$lock" taskset -c 2 bin/wit "$PWD" > wit.tsv 2> wit.log
python3 summarize.py native.tsv > native-summary.tsv
python3 summarize.py wit.tsv > wit-summary.tsv
```

## Native paired results

| Fixed archive | Serial wall | Serial child CPU | Crossbeam 4 wall | Crossbeam 4 child CPU |
| --- | ---: | ---: | ---: | ---: |
| Mixed, 4 KiB | **−4.0% / −0.9%** | **−3.1% / −1.1%** | +4.4% / −4.3% | +2.0% / −4.7% |
| Mixed, 512 KiB | −2.7% / +1.1% | −0.2% / +0.4% | −8.6% / −7.7% | −5.6% / −7.0% |
| Dense `sao` | +0.1% / +0.9% | −0.1% / +1.0% | −0.3% / +2.0% | −1.1% / −0.2% |
| C7 `sao` | −1.2% / +0.1% | −1.0% / −0.6% | −2.3% / +2.7% | +0.1% / +2.2% |
| Split `sao` | +1.2% / +3.4% | +0.3% / +3.6% | +5.4% / +0.2% | +0.7% / −1.5% |
| Dense Silesia | +0.1% / +0.1% | +0.1% / −0.1% | −2.1% / −4.5% | +0.7% / +0.7% |
| C7 Silesia | +1.2% / −0.1% | +1.2% / 0.0% | −0.6% / −4.8% | +0.4% / −2.7% |
| RAW, 4 KiB | +2.9% / +0.7% | +1.9% / −0.3% | +3.1% / −0.2% | +1.6% / −0.3% |
| Sparse entropy, 4 KiB | +4.8% / −1.2% | +4.4% / −1.7% | +6.0% / +3.2% | +2.9% / +7.3% |
| RAW, 32× | −0.4% / +2.3% | −0.2% / +1.9% | +1.9% / +1.4% | −0.8% / −0.5% |
| Sparse entropy, 32× | +1.6% / −0.8% | +2.6% / −1.0% | +0.2% / −1.3% | +1.0% / −1.1% |
| Dictionary, one block | +3.3% / +1.7% | +2.2% / −0.8% | 0.0% / +1.2% | +1.0% / −4.5% |
| Dictionary, 256 blocks | −0.3% / +1.9% | +0.5% / −1.8% | +6.8% / −5.2% | +1.7% / −0.5% |

The primary paired wall interquartile ranges were [−5.30%, −1.79%] and [−5.57%, +1.42%]; neither median clears −5%. Dense/C7 `sao` and the full corpora show no meaningful serial speedup. The short sparse four-worker control was over +3% in both series, but its longer single-frame counterpart was not; that leaves the short control's slowdown unresolved rather than establishing queue cost on low-entropy input. A few other native control deltas cross +3% only once. The primary wall result already excludes promotion, so no extra 24-pair control confirmation was needed to make the decision. No parallel scaling claim follows from these comparisons. See [native samples](pivco-queue-data/native.tsv) and the [paired summary including interquartile ranges](pivco-queue-data/native-summary.tsv).

## Default scalar WIT paired results

| Fixed archive | Complete-call wall |
| --- | ---: |
| Mixed, 4 KiB | +2.2% / +2.5% |
| Mixed, 512 KiB | +2.5% / +1.0% |
| Dense `sao` | +2.4% / +3.3% |
| C7 `sao` | +1.1% / +1.7% |
| Split `sao` | +0.3% / −0.9% |
| RAW, 4 KiB | +1.1% / +1.5% |
| Sparse entropy, 4 KiB | +0.5% / −1.2% |

The primary WIT case moved in the slower direction in both series, but both medians were below +3%; Dense `sao` crossed +3% once. No WIT workload had both medians above +3%. The result is not a claim of equal performance and does not alter the native drop decision. See [WIT samples](pivco-queue-data/wit.tsv) and [paired summary](pivco-queue-data/wit-summary.tsv).
