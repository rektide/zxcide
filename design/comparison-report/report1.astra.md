---
type: Design
title: Measuring fixed-archive decoder scaling
description: Extend the standard CLI report with worker counts, CPU cost and an optional concatenated-frame control.
resource: /design/comparison-report/report1.astra.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra#medium', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e08492965ffe73Q6gm0qwqKych', title: Original report design and worker-scaling revision }
  - { resource: /design/comparison-report/report0.astra.md, title: Original command and evidence contract }
  - { resource: /xtask/comparison_report.py, title: Shipped staging, exact verification and baseline import }
  - { resource: /xtask/comparison_tables.py, title: Schema 1 jobs, validator and renderer }
  - { resource: /doc/profiling/README.md, title: Shipped comparison commands and limitations }
  - { resource: /crates/cli/src/main.rs, title: Rust thread, slot and backend choices }
---

# Read worker scaling beside its CPU cost

**Shipped (schema 1):** `comparison-report` already writes per-file and aggregate tables to `report.md` and `run.log`. It stages hash-pinned C archives for configurable levels 1–7, measures serial Rust and C `-T1`, records wall time and saves the actual executables. At revision `5c735cb2` the level default was `3,7`; the requested **`1,3,7`** default is a separate configuration change compatible with schema 1. This revision adds measurements that cannot be recovered from old wall times.

## Proposed command contract (schema 2)

```sh
cargo xtask comparison-report                           # full 12, C1/C3/C7, decode workers 1/4/8, separate files
cargo xtask comparison-report --quick --decode-workers 1 --before PATH/TO/V1-RUN
cargo xtask comparison-report --files sao,mozilla --levels 1,7 --decode-workers 1,4,8 --concat
```

`--quick` remains dickens+sao at full sample count. `--files` is mutually exclusive with it; accept a nonempty distinct subset of the 12 manifest names and normalize to manifest order. Label subset totals with their actual members. `--levels` retains its shipped semantics. Accept distinct positive supported worker counts; show no 1-worker speedup if 1 was not requested. Apply workers only to decode: C `-qdc -TN`, Rust `decode --threads N` with default backend and slots (record actual policy); 1 uses today's serial argv. C may have an additional driver thread. Encode Split/Dense and each C level once per file per pass, always at the existing serial/`-T1` settings. C1 has no Rust encode counterpart; compare Split/C3 and Dense/C7 only when selected.

**Show the CPU budget before starting.** Workers are requested concurrency; affinity is the inherited set of logical CPUs eligible to run them. Record mask and count, and effective cgroup CPU quota in CPU equivalents when discoverable (record unknown otherwise). Print and persist a conspicuous `CONSTRAINED` warning if either known limit is below the largest worker count, e.g. eight workers on CPUs 12–15. Suggest changing the launcher's affinity or `--decode-workers`; never silently expand the mask or reduce workers. Retain this label beside aggregate results. Eligible SMT siblings are not dedicated physical cores.

## Two workloads with the same frame bytes

**Default, separate:** one invocation per member. Stage archives once using the existing manifests and pins. Keep schema 1 exact cross-decodes on every generated archive at one worker. For every timed `(fixed C archive, decoder binary, N)` additionally stream-compare its output to the verified plaintext through EOF and require successful exit, reusing identical checks. Changing N never regenerates the input. Store verification keyed by archive digest, binary digest and argv, outside the timer.

**Optional `--concat`, additive:** per selected level append those exact fixed C frames in manifest order into one scratch file. Record ordered member names, byte lengths, component hashes, concatenated byte count and SHA-256. Decode all frames with **one** CLI invocation for each decoder/N; verify exact output against the chained plaintext streams including EOF. A CLI stopping at frame one or rejecting the flags fails verification, not a fast result. Never re-encode the concatenated plaintext. Reject `--concat` for fewer than two members. Shuffle these jobs alongside the separate-file jobs; member order inside the bundle is fixed.

Label these rows **N file launches** versus **1 concatenated launch** for the same byte total. Their difference includes process and pool startup, input opens, cache state, frame boundaries and scheduler effects. It neither isolates startup cost nor proves that output ordering is the bottleneck. Random job order helps with drift; it does not randomize frame order or attribute waiting. Use instrumentation for causal claims.

## Tables and arithmetic

Keep stdout bounded: scope, levels/workers, CPU budget/warnings, a Split/C3 and Dense/C7 size/speed highlight when available, an aggregate decode line for the highest selected level at each endpoint worker count, and artifact paths. In both detailed outputs, lead with aggregate rows; then group by file, fixed level and implementation with adjacent ascending worker rows. A reader looking up `mozilla` or `sao` should see these columns without looking up a denominator:

| Section | Columns |
|---|---|
| Encode / compression | file or scope, encoder/mode, plain B, archive B, archive/plain %, size Δ % vs paired C, ms, MB/s, CPU s/GB, speed × vs paired C |
| Decode / scaling | file or scope + layout, level, plain B, fixed archive B, decoder, workers, ms, MB/s, CPU s/GB, × vs own 1 worker, × vs C at same N |

With `--before`, add current/before speed × on matching workload and worker count, and encode size Δ vs before. Keep observed wall min–max in detail. Print byte counts as integers, elapsed ms to three decimals, rates/costs to three decimals; raw evidence retains integer elapsed ns and child user/system CPU time in µs (Linux `getrusage(RUSAGE_CHILDREN)` deltas around each **single awaited** subprocess, no other children during sampling). CPU includes all CLI threads, excludes the Python runner, and can exceed elapsed time; don't estimate it as wall × workers.

One shuffled warmup and seven measured passes as shipped. For each `(operation, implementation, operating point, workers, layout)`, let `B` be selected plaintext bytes counted once. Per file or concat use its invocation; for the separate-file aggregate **sum within each pass**. Take medians of the seven wall totals `W` (seconds) and, independently, seven user+system totals `U` (seconds). Display `1000W` ms, `B/(10^6 W)` MB/s and `U/(B/10^9)` CPU seconds per decimal plaintext GB. Speed × is reference `W` / candidate `W`; archive/plain and size delta use sums of bytes before division. No averaging rates, pooling levels, or adding concat observations to separate-file totals. Render only at the end from unrounded inputs.

## Implementation and acceptance

Extend `comparison_report.py` staging and sampling, `comparison_tables.py` job identities/validation/rendering and the typed xtask options. Dispatch reader and validator on schema. Preserve v1 rendering without inventing CPU data. For trusted `--before` v1 or v2 runs verify `COMPLETE`, raw schema and `bin/current` digest as today; **only import the saved binary and its build identity**, then verify and time it under the newly requested files/levels/workers/layout. For unsupported options fail with the binary and offending argv and suggest `--decode-workers 1` or omitting concat. Never silently substitute serial measurements.

Print matrix size up front. With F files, L levels, R Rust binaries, W worker settings and K=1 when concat is enabled (else 0), jobs/pass = `F(L+2R) + (F+K)L(1+R)W`. Standard full: 276 jobs/pass, 2,208 launches including warmup, plus verification. Narrow files, levels or workers for iteration. Store observations by job ID, not duplicated commands; keep concat files and generated archives in removable scratch and use streaming verification. Retain the schema 1 publication contract (binaries, `raw.json`, report, log, `COMPLETE`; incomplete on failure).

Acceptance: re-render an actual v1 run, use it as `--before` on a quick serial run, then run the targeted `sao,mozilla` example and reproduce its tables offline. Test a narrow inherited mask, a decoder truncating the second frame, missing worker/pass evidence and corrupt old binaries. Use hand-calculated multithread CPU and asymmetric per-pass times to check units, grouping and both scaling ratios; assert each fixed digest is identical across worker rows. Keep [the previous design](report0.astra.md) as the original scope.
