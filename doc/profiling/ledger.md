---
type: Measurement
title: Logical byte movement in ZXC
description: Ranked block, entropy, dictionary and range probes with separate native and WIT timing intervals.
resource: /doc/profiling/ledger.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0acf0f83ffeOu7mBN3OsQ5Dc8', title: Built and ran the diagnostic and uninstrumented benchmark pair }
  - { resource: 'opencode://ses_e0ac424eeffe4kk65VvnMwl4oQ', title: Measured the dictionary decode, encoder prefix seed, and scalar PivCo scratch follow-ups }
  - { resource: '/crates/parallel/benches/ledger.rs', title: Workloads and validation }
  - { resource: '/xtask/src/ledger.rs', title: Report generation and interval selection }
---

# Where bytes go

The largest **payload copy** in the full-frame cases is the 65,535-byte dictionary prefix: 16 bytes copied per plaintext byte in both encode and decode at 4 KiB. For a different access pattern, returning just 4 KiB from a checksummed 512 KiB RAW block stages 128 bytes per returned byte. Address dictionary history first for small-block streams; prioritize range scratch/borrowing separately for repeated short reads. Neither result calls for bypassing verification.

These are baseline counts. The [decode follow-up](dictionary-decode.md) eliminates the prefix preload by borrowing match history. The [encode follow-up](dictionary-encode.md) keeps contiguous history but avoids repeating the more expensive prefix-indexing pass on eligible small blocks.

The [PivCo scratch experiment](pivco-scratch.md) isolates the merge copy-back from the scalar gather, comparing whole native and WIT calls as well as logical traffic.

## Reproduce

```sh
cargo xtask ledger --component
# Native only (no Wasm toolchain):
cargo xtask ledger
```

Use the [component prerequisites](../../README.md#webassembly-component) and a quiet host. The command prints the path to `target/ledger-*/report.tsv` and saves the underlying Criterion JSON alongside it. Each case is sorted by descending B/B. See the [recorded report](ledger-5800x.tsv) for **every site, exact byte count and timing interval**, including both worker counts and range backends. To time a single case for longer, e.g. `cargo bench --locked -p zxcide-parallel --bench ledger -- --exact ledger/dict/b12/w1/decode`. Do not enable `ledger` for timings.

* **Diagnostic process:** feature-gated call-site probes count logical destination lengths once, not read+write pairs. A global mutex collects all workers' counts. The driver clears counts after case setup, performs one call, joins all workers, checks output, then takes the counts. This can perturb scheduling and thus buffer reuse. No counters, locks, snapshots or diagnostic allocations are compiled into ordinary builds. The code changes neither block lifetimes nor output limits.
* **Timing process:** same fixture and driver, `ledger` disabled. 20 Criterion samples, 300 ms warmup, 600 ms measurement target; report is the 95% interval for **one entire call**, in µs (slope when available, otherwise mean). The repeated interval beside each site is not time spent at that site. Native stream calls use fresh codec contexts with preallocated destination capacity; w4 includes thread startup and joins. WIT and its native oneshot control allocate fresh output per call. Setup, archive construction, opening the range index, component instantiation and correctness comparisons are outside timers.
* **Units:** `plaintext_B` is source/uncompressed size, or bytes **returned** for ranges. `copy` counts logical payload copies, including reading a slice into block storage. `input:positioned body` counts bytes delivered by slice copy or positioned file read. `output` is required caller destination materialization, archive including framing for encode, verified plaintext for decode/range. `reconstruct` separates literal and match writes into verified block storage. `produce` counts entropy intermediate symbols. `zero` counts explicit logical initialization; `fill` is the **nonzero** parser table reset. `peak-section` is a maximum allocation capacity per entropy section, not cumulative traffic; use the absolute byte count. Compiler elision, lazy zero pages, cache effects, device I/O and memory-bus transfers are not measured.
* **Capacity is a separate observation.** `capacity:growth delta` sums positive **actual Vec capacity changes** around input/body `resize`; `capacity:request fits` sums request lengths when old capacity already suffices, including reacquired worker buffers. Fits can still need zeroing. Neither quantity measures allocator copying or total allocation; never add them to traffic. Moves, swaps and channel handoffs transfer ownership and contribute no payload bytes. No inferred allocator relocation is included.

## Inputs and host

2026-11-19, Ryzen 7 5800X, x86-64 Linux 7.1.0-debplus.1, rustc 1.101.0-nightly (`822182db4`), Cargo.lock, optimized bench/release defaults, no Tracy. Code and recipe at `b6747016`. Wasmtime 46.0.3; release component SHA-256 `20f6e7eae7f2b2db3f9f832a76a166db566dcdd8af317ed95593a473668a12a2`. Timings characterize this host and these generated cases, not cycle attribution or a performance guarantee.

`b12`/`b19` are 4 KiB/512 KiB **block limits**. The stream families each process 1 MiB with checksums, serial (`w1`) and crossbeam (`w4`, eight slots); every encode must match the serial archive and every decode must return the original with the plaintext limit enforced. Noise is xorshift64 seed 42. Dense alternates 2 KiB from `multiblock_mixed.expected` with 2 KiB low-five-bit xorshift literals. Dictionary input repeats bytes `[32768,36864)` of the same noise 256 times; the validated v2 dictionary contains its first 65,535 bytes, no shared tree. Thus even the first record can match external history. Range reads use checksummed indexed RAW noise, byte 17 through `17+n`, from both a slice and a page-cache-warm file, with fresh per-call scratch. The fixed upstream `spl_pivco_wide_l7.zxc` is one 16 KiB decode; relabeling it with different block limits would not exercise a different payload.

## Interpreting the rankings

| Family | Finding (logical B/B unless specified) | Implication |
| --- | --- | --- |
| RAW/noise | Native decode reads ~1 body byte and writes 1 output byte. At b12, encode resets **64 B/B** of `u32::MAX` table entries vs **0.50** at b19. | Table reset is the biggest full-frame *fill*, not a payload copy. Profile initialization alongside match finding before redesigning the stream writer. |
| Dictionary | Prefix alone **16.00** at b12 vs **0.125** at b19; encode also copies **1.00** plaintext byte into contiguous history. At b12 decode the archive body is only **0.0117**. | Strong small-block target beyond unavoidable reconstruction/output; measure prefix indexing separately from prefix copying on encode. |
| Wide Huffman | Merge gather **3.970** and copy-back **3.970**, vector zeroing **1.964**, literal reconstruction **0.982**, result **1.00**. | The merge stages outweigh the final literal copy. Four workers count the same 65,042 bytes at each merge site as serial even though the work runs off the caller thread. |
| Dense mixed | At b12 the initial literals and RLE candidate each copy **0.562**, second dense assembly **0.441**, Huffman candidate **0.437**. At b19 assembly is **0.504**, just below the entire archive (**0.504**). | Dense assembly is worth investigating but not the highest amplification. Preserve candidate pricing and byte-identical output when comparing alternatives. |
| Ranges | For a 4 KiB result: **2.002** body at b12 (straddles two blocks) vs **128.001** at b19 (one block); b19 also zeroes **128.001**. For a 512 KiB result: **1.009** vs **2.000**, respectively. Slice and file stage the same lengths. | Borrowing from a slice could remove staging but the full touched block must still be verified. For files, distinguish capacity reuse from bytes a verified read still requires. |
| WIT wide decode | ABI argument 12,819 B (**0.7824**), result 16,384 B (**1.0**), total **1.7824** B/B. | Lengths of lists crossing the API, **not measured physical transfers**. Native one-shot is a fresh-workspace control. A timing difference includes guest code generation, codec execution, allocation and bindings; it does not isolate the cost of copying lists. |

The TSV includes all capacity diagnostics; for example a b12 serial RAW decode grows body capacity by 4,100 bytes once and services 1,045,500 bytes of later requests at existing capacity. The parallel pipeline can keep several such buffers live; different growth totals are not evidence of copying the payload at reorder or commit.

For scale, the uninstrumented 1 MiB b12 dictionary decode took **376–395 µs** serial and **216–222 µs** with four workers. The b19 short range took **33.90–33.94 µs** from a slice and **45.41–46.70 µs** from the warm file. Wide decode took **119.96–132.48 µs** native one-shot versus **140.02–154.64 µs** through WIT. These whole-call intervals do not predict the savings from removing a particular copy.

**Coverage limits:** selected bulk sites, not a census of stores. Checksumming reads, table indexing, headers/seek group metadata, scalar controls, allocator internals, peak RSS and cache misses are outside the counts; their costs are inside timings. No WIT dictionary or range method exists; only the wide fixture is paired across the WIT boundary. Guest internal movement is not directly instrumented. For real workload decisions, vary input distributions, range lengths/alignment, thread lifetime and dictionary hit rates, and repeat timing baselines before attributing a speed gain to any proposed change.
