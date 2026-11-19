---
type: Measurement
title: Alternating scalar PivCo merge buffers
description: Copy-back, auxiliary capacity, and whole-call performance in native and WIT decode.
resource: /doc/profiling/pivco-scratch.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0ac424eeffe4kk65VvnMwl4oQ', title: Implemented and measured the alternating scalar PivCo buffer experiment }
  - { resource: '/doc/profiling/ledger.md', title: Byte movement definitions and fixture selection }
  - { resource: '/crates/parallel/benches/ledger.rs', title: Native encode and decode ledger driver }
  - { resource: '/crates/component/benches/component.rs', title: Wasmtime typed WIT driver }
---

# Alternating scalar PivCo merge buffers

**Drop the alternate layout (`0b8eaeee`).** Removing copy-back did not produce a repeatable native serial win on either entropy-heavy workload. Four-worker 4 KiB mixed improved in both pairs, but that same input slowed by **11.1–12.8%** through WIT with separated intervals. Leave the baseline decoder in place. This tests buffer placement, not whether a faster scalar or SIMD gather would help.

The original scalar decoder unpacks the leaves and flat subtrees into disjoint intervals, gathers each mixed bitmap into scratch, and copies it back. The experiment assigns each interval to one of two same-size byte vectors while reading the wire. The root is assigned to output; mixed nodes put both children in the opposite vector, and uniform nodes keep the same vector and do not merge. Bottom-up gathers therefore write to their final interval for the next merge. The bitmap cursor loop and flat unpacking are unchanged. Depth alone would not suffice: uniform nodes must not flip the assignment or cause a copy. The low bit of each pending interval's bounded start holds its vector assignment, so queue and order entries do not get larger.

Scratch is allocated only after the first mixed bitmap. Each decode of a Huffman section owns its vectors and traversal state. Worker ownership, wire validation, LZ reconstruction, and verified publication are unchanged. No SIMD and no new dependency.

## Method

Ryzen 7 5800X, Linux 7.1.0-debplus.1, rustc 1.101.0-nightly (`822182db4`), Wasmtime 46.0.3, locked dependencies, default optimized bench/release builds. Compare the existing scalar decode at `18982180` with the alternate layout. Build the native `ledger` bench separately at each revision, and build each release Wasm component with `cargo xtask component`. Save the binaries and components. Use a **single** `cargo bench --locked -p zxc-component --features host-bench --bench component --no-run` host executable for both components so the Wasmtime caller and the native archive generator are identical. Load each saved component via `ZXC_COMPONENT`. The WIT host instantiates once outside timing and times complete buffer API calls, not just the guest kernel.

Four independent processes per backend, baseline/candidate/candidate/baseline order, each with a fresh `CRITERION_HOME`. Pin to CPUs `2-7` (six cores) via `taskset`; use `--bench`, 30 samples, 0.45 s warmup and 1 s measurement target. Filters:

```text
^ledger/(wide/w(1|4)|(mixed/b(12|19)|noise/b12)/w(1|4))/decode$
^component/(upstream-l7|ledger/(mixed/b(12|19)|noise/b12))/decode$
```

Wide is one pinned upstream 16 KiB block. The ledger's 1 MiB dense mixed/dictionary-free workload exercises Huffman heavily at 4 KiB but only lightly at 512 KiB; noise is the 4 KiB RAW control. Both drivers use the same source recipe and archive options. Native times fresh codec state with output capacity reserved; four-worker timings include startup and joins. The wide `w4` case only schedules one block and does not measure scaling. No diagnostic or Tracy counters are present in the timed builds. A separate `--features ledger` run records movement and the maximum auxiliary capacity per PivCo section (merge vector, order and queue, excluding returned symbols, codebook, allocator overhead and other block scratch). That is not RSS or the aggregate concurrency peak.

## Byte movement and capacity

Diagnostic results are identical at one and four workers. B/B values divide by decoded plaintext bytes; peak is an absolute maximum, not total traffic.

| Case | Gather B/B (unchanged) | Copy-back B/B before → after | Explicit vector zeroing B/B (unchanged) | Peak auxiliary bytes before → after |
| --- | ---: | ---: | ---: | ---: |
| Wide | 3.96985 | 3.96985 → 0 | 1.96362 | 34,030 → 34,030 |
| Mixed, 4 KiB | 1.31642 | 1.31642 → 0 | 1.12487 | 22,936 → 22,936 |
| Mixed, 512 KiB | 0.00130 | 0.00130 → 0 | 0.00145 | 1,313 → 1,313 |
| RAW noise, 4 KiB | 0 | 0 → 0 | 0 | 0 → 0 |

The avoided writes are 65,042 bytes in the wide block and 1,380,371 bytes over the 4 KiB mixed stream. The measured Huffman sections do not show a reduction in zeroing or peak capacity. In a section with no mixed bitmap, the candidate needs no second byte vector; that is a property of the layout, not a measured gain on this matrix. No buffer survives an entropy call or is shared across workers. A conservative bound from the 4 KiB mixed data is four times the per-section peak, **91,744 bytes**, for concurrently live auxiliary PivCo state at `w4`; the true concurrent maximum was not measured. Tree metadata, returned symbols, and codec output add to that bound for process memory.

## Whole-call results

Criterion 95% intervals in µs (slope, or mean when there is no slope). Negative point-estimate changes mean less time. The second pair ran candidate before baseline.

| Decode case | Baseline 1 | Candidate 1 | Baseline 2 | Candidate 2 | Change 1 | Change 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `ledger/wide/w1` | 118.03–120.54 | 119.34–122.17 | 117.83–119.61 | 117.20–119.23 | +1.1% | −0.6% |
| `ledger/wide/w4` | 202.17–209.73 | 221.48–236.60 | 199.73–206.41 | 199.32–209.94 | +10.8% | +0.5% |
| `ledger/mixed/b12/w1` | 6265.71–6394.09 | 6550.96–6677.01 | 6387.50–6706.31 | 6315.13–6449.57 | +4.5% | −2.4% |
| `ledger/mixed/b12/w4` | 1965.66–2272.69 | 1860.59–1972.55 | 1941.92–2105.95 | 1864.75–1922.98 | −9.6% | −5.8% |
| `ledger/mixed/b19/w1` | 117.02–118.88 | 115.15–116.74 | 115.40–117.50 | 115.24–117.20 | −1.8% | −0.2% |
| `ledger/mixed/b19/w4` | 144.27–149.27 | 148.50–161.68 | 145.15–152.05 | 140.17–143.89 | +5.0% | −4.2% |
| `ledger/noise/b12/w1` | 77.69–79.51 | 79.95–81.63 | 78.72–81.25 | 76.12–79.64 | +2.8% | −3.0% |
| `ledger/noise/b12/w4` | 271.11–287.21 | 280.68–310.78 | 257.34–265.50 | 287.16–315.13 | +5.0% | +15.2% |
| `component/upstream-l7` | 142.97–160.42 | 149.86–151.39 | 142.41–147.07 | 149.34–151.86 | +0.1% | +4.1% |
| `component/ledger/mixed/b12` | 8031.48–8372.07 | 9074.27–9136.91 | 8209.91–8460.31 | 9279.66–9520.06 | +11.1% | +12.8% |
| `component/ledger/mixed/b19` | 503.17–514.99 | 509.69–527.35 | 520.28–549.05 | 515.35–526.29 | +1.9% | −2.5% |
| `component/ledger/noise/b12` | 511.93–573.33 | 493.22–501.81 | 499.68–513.58 | 493.07–506.16 | −7.9% | −1.3% |

The native four-worker RAW control is noisy and never executes the changed loop. Its movement cautions against ascribing all parallel differences to the buffer change. This single-host comparison does not identify whether the WIT regression comes from traversal bookkeeping, access locality, or different generated code in the scalar gather; it does not measure the cost of list transfer separately. Same-size buffers and a source-identical gather loop are not a guarantee of identical machine code. The baseline and candidate release components were 103,570 and 104,001 bytes, respectively (SHA-256 `7a0e29e3283530ff3ddfdc32acccdb1c1331da490d11a4734165acb331310188` and `6f03563013e9f43fc7e06937e790ac937e9070a386229f398fe3fa26c64555ee`).

## Verification

Both benchmark drivers check plaintext exactly. The WIT driver also verifies the wide fixture's output-limit rejection. The candidate passed `cargo nextest run --locked --workspace --features zxc-cli/transports` (40 tests), two focused release tests for uniform ancestors, bitmap tails, truncated/trailing runs, and missing children, and codec Clippy with warnings denied. Pinned `cargo xtask interop --cases 36` passed 180 frozen and 72 generated cross-decodes and 228 indexed reads in total against upstream C `2992c28e6e1df07feec0cb39678a6d4a9665c6bb`. The decision is performance-based, not a correctness rejection. The reusable fixtures, metric, and regression tests stay; the alternate layout does not.
