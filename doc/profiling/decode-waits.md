---
type: Measurement
title: Where four Crossbeam decode workers wait
description: Fixed-archive decode timing and a checked decomposition of ordered-head, credit and worker-idle intervals.
resource: /doc/profiling/decode-waits.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e04ef8b0cffeJahvs500cwmoBM', title: Implemented the scratch probe and measured the four pinned archive cases }
  - { resource: /design/decode-waits/assessment0.astra.md, title: Measurement design and decision gates at 4957a14d }
  - { resource: /doc/profiling/pivco-worker.md, title: Earlier reuse measurements and fixed archive identities }
---

# Where four Crossbeam decode workers wait

**Keep eight slots as the four-worker baseline; do not redesign ordered delivery on this evidence.** The `mozilla` window is often full, but the conservative idle intersection from a missing ordered head is only 0.29–0.46 ms per frame at the series medians. Empty-queue idle while the coordinator is in `next` is much larger: 2.02–2.19 ms on `mozilla`, 0.75–0.84 ms on `sao`. These are elapsed intervals in the diagnostic build, not measured speedups. The most useful next discriminator is the coordinator's **on-CPU read/resize versus off-CPU scheduling time**; `sao` does not resolve small wall effects on this host.

## Clean scaling and instrumentation cost

Values separated by `/` are series 1 / 2. Wall and CPU are per decode; CPU-s/GB uses decimal plaintext GB, not compressed bytes. Mean cores includes the coordinator. The clean binary SHA-256 is exactly the prior retained candidate (`14d4a3a3…`), not just a matching source tree.

| Archive | Clean 1 → 4 wall, ms (series 1 / 2) | Paired wall Δ | Clean 1 → 4 CPU-s/GB (series 1 / 2) | Clean 4 mean cores |
| --- | --- | --- | --- | --- |
| `mozilla.c7` | 37.20 → 14.84 / 37.01 → 14.83 | −60.4% / −60.5% | 0.720 → 0.830 / 0.717 → 0.857 | 2.89 / 2.94 |
| `mozilla.dense` | 41.30 → 15.97 / 41.24 → 15.89 | −62.6% / −61.4% | 0.800 → 0.902 / 0.798 → 0.905 | 2.96 / 2.92 |
| `sao.c7` | 8.21 → 5.15 / 8.13 → 5.13 | −38.4% / −36.9% | 1.100 → 1.663 / 1.098 → 1.690 | 2.32 / 2.38 |
| `sao.dense` | 10.52 → 6.17 / 10.56 → 5.98 | −41.0% / −43.7% | 1.433 → 2.026 / 1.431 → 2.025 | 2.42 / 2.51 |

Percent change is probe relative to its adjacent clean reference, except A/A (both clean). Brackets give 95% intervals for capture wall, the least well bounded dimension; the [full intervals](decode-waits-data/main/summary.txt) also cover clean scaling, A/A, dump and CPU.

| Archive | A/A wall | Capture wall [95% interval] | Capture CPU | Dump wall | Dump CPU |
| --- | --- | --- | --- | --- | --- |
| `mozilla.c7` | −0.4% / +0.2% | +0.2 [−3.5,+4.8] / −1.9 [−4.3,+1.7] | +0.6% / −1.4% | +2.0% / +2.6% | +1.0% / +1.5% |
| `mozilla.dense` | −1.4% / −0.6% | +2.4 [−4.6,+6.1] / +0.8 [−2.7,+6.7] | −0.5% / −0.7% | −0.9% / +0.7% | +0.1% / +0.9% |
| `sao.c7` | +3.8% / +0.4% | +1.2 [−3.3,+7.0] / −1.1 [−5.1,+6.5] | +0.5% / −0.6% | +5.8% / +0.2% | +1.7% / −0.5% |
| `sao.dense` | +0.4% / +6.7% | +3.6 [−9.0,+12.5] / −4.2 [−6.7,+1.3] | +0.7% / +0.0% | +3.8% / −0.9% | +2.6% / −0.4% |

Capture CPU medians stay within ±1.5%, and the CPU intervals are nearly all within ±3% (one bound −3.05%). Capture wall medians meet ±3% except on Dense `sao`; **wall equivalence within ±3% is not demonstrated** in any case across both series. `sao` A/A intervals extend to +11–14%. Consequently, the larger, repeated `mozilla` supply shoulders merit a diagnostic follow-up, but these runs cannot certify a few-percent scheduling benefit, exclude rare losses or quantitatively transfer sub-ms exposures to the clean binary. Probe dump and capture comparisons are each paired against clean, not a direct estimate of dump-only cost.

## A full window seldom starves workers behind the head

Pool budgets and intersections are series medians (each field medianed independently). Head **a/b/c** are subdivisions of coordinator result-receive wall-ms; the order and input columns are **interior** measurements, with wall-ms followed by worker-ms in parentheses. Total compute is worker-ms; average compute utilization divides by `4T`. A full window is not the same as empty worker supply.

| Archive | Pool `T`, ms | Interior / drain, ms (S1; S2) | Compute / job receive, worker-ms (S1; S2) | Compute utilization | Full + unread, ms |
| --- | --- | --- | --- | --- | --- |
| `mozilla.c7` | 13.55 / 13.54 | 12.20 / 1.14; 12.24 / 1.06 | 37.37 / 15.52; 37.49 / 15.00 | 68% / 68% | 8.48 / 8.34 |
| `mozilla.dense` | 15.09 / 14.61 | 13.70 / 1.11; 13.36 / 1.04 | 42.07 / 16.73; 40.95 / 16.28 | 70% / 69% | 9.61 / 9.48 |
| `sao.c7` | 4.15 / 3.89 | 3.02 / 0.86; 2.68 / 0.89 | 10.00 / 4.67; 9.80 / 4.30 | 61% / 63% | 0.97 / 0.63 |
| `sao.dense` | 5.02 / 5.02 | 3.43 / 1.38; 3.34 / 1.49 | 13.00 / 5.33; 13.03 / 5.51 | 67% / 65% | 1.14 / 0.83 |

| Archive | Head a/b/c, wall-ms (S1; S2) | Order exposure, later pending, ms (worker-ms) | Empty-queue `next`, ms (worker-ms) | Empty-queue commit, ms |
| --- | --- | --- | --- | --- |
| `mozilla.c7` | 2.84/1.25/4.82; 2.79/1.23/5.02 | 0.29 (0.32) / 0.38 (0.73) | 2.02 (5.12) / 2.12 (5.32) | 0.04 / 0.04 |
| `mozilla.dense` | 3.05/1.44/5.73; 2.77/1.29/5.51 | 0.36 (0.68) / 0.46 (0.66) | 2.19 (5.52) / 2.17 (5.35) | 0.04 / 0.04 |
| `sao.c7` | 0.35/0.31/0.25; 0.35/0.37/0.28 | 0 / 0 | 0.76 (1.46) / 0.82 (1.48) | <0.01 / <0.01 |
| `sao.dense` | 0.61/0.42/0.72; 0.64/0.11/0.76 | 0 / 0 | 0.75 (1.46) / 0.84 (1.43) | <0.01 / <0.01 |

All-worker compute overlap covers only 36–42% of the `mozilla` interior and 50–55% of `sao`. `mozilla` spends about 3.85–3.96 ms in data `next` and only 0.13–0.14 ms in commit. The corresponding `sao` service is 2.12–2.50 ms and 0.01 ms. Much of the `mozilla` receive budget is **(c)**, after the head finishes; long collection/dispatch delay does not establish a particular cause or mean a worker parked in a syscall. The [trace summaries](decode-waits-data/main/summary.txt) include 0–4 compute overlap, frame/pool residuals, failed-pop time, pending depth, queue certainty and per-block p50/p95; the [frame budgets](decode-waits-data/main/budgets.tsv.gz) include exact occupancy time and both interior and drain cuts. Interior order exposure is zero in 9–13 of 32 `mozilla` traces and 21–26 of 32 `sao` traces per series. Even so, in one representative `mozilla.c7` trace, head 8 took 1.38 ms; block 9 was already pending at 0.49 ms, and order held empty-queue receivers for 0.38 ms (0.80 worker-ms, up to three workers). In another representative trace, head 39 had later compute finished but **no** exposed order wait and spent 0.46 ms in collection. See [representative timelines](decode-waits-data/main/timelines.txt) for both, and for each other archive/series.

### Follow-up: vary input and output separately

Two new balanced series of 24 pairs per case/test, same archive/layout and 2–5 mask. The diagnostic source control uses the same executable for each pair: warm regular file versus a `Cursor<Vec<u8>>` over the same bytes, preloaded before frame/pool entry. The clean sink control pairs ordinary `decode > /dev/null` with shipping `test` (same decode path, `io::sink`, no output hashing possible in the sink-only command; exact decode hashes are established separately). Do not treat preloading as an optimization: it moves work outside `T` and adds an archive-sized allocation; whole-CLI wall **regressed 54–91%**. It also changes buffering and cache state, not just system calls.

| Archive | Cursor minus file `next`, paired ms | Cursor minus file empty-queue `next`, paired ms | Cursor minus file pool `T`, paired ms | Clean sink wall Δ |
| --- | --- | --- | --- | --- |
| `mozilla.c7` | −2.10 / −1.83 | −0.99 / −0.60 | −0.70 / −0.73 | +0.2% / −2.2% |
| `mozilla.dense` | −2.11 / −2.06 | −0.91 / −0.67 | −1.46 / −1.31 | −3.0% / −0.7% |
| `sao.c7` | −0.27 / −0.42 | −0.35 / −0.53 | −0.09 / +0.03 | −3.0% / +8.1% |
| `sao.dense` | −0.45 / −0.21 | −0.16 / −0.14 | +0.19 / −0.14 | +1.5% / −6.3% |

Both intervals for each `mozilla` read-gap column exclude zero, unlike `sao.dense`. Only `mozilla.dense` has both diagnostic pool intervals below zero. All clean sink wall/CPU intervals include zero. This supports prioritizing the input-side service gap on `mozilla`; it does **not** prove where the time is spent within `next`, that the sink never matters, or that an input change will improve clean CLI performance. See [source/sink results and intervals](decode-waits-data/io/summary.txt), [ordered timings](decode-waits-data/io/timings.tsv), [frame budgets](decode-waits-data/io/budgets.tsv.gz), 192 raw sidecars ([series 1](decode-waits-data/io/traces-s1.tar.gz), [series 2](decode-waits-data/io/traces-s2.tar.gz)), [hash/quiet checks](decode-waits-data/io/verification.tsv) and [host snapshot](decode-waits-data/io/environment.txt).

### Ranked disposition

1. **Measure coordinator `next` first.** It is the largest repeated empty-queue overlap with unread work in these traces, not yet a demonstrated optimization. Separate resize/header/copy-on-read from coordinator off-CPU time (e.g. scheduler-switch trace or per-thread CPU clock alongside these existing edges). Validate on clean fixed-archive wall and CPU, independently of the richer probe.
2. **Separate result collection delay from worker skew.** `mozilla` block indices 8 and 10 are persistently among the slowest in both encodings (about 0.60–0.72 ms median against block p50 0.36–0.40 ms); both are kind 1 with different payload sizes. On `sao` early indices dominate and startup/drain take a large share. These are *elapsed* service times; before choosing a codec stage, compare on-CPU entropy/reconstruction/checksum at those indices, with existing `sao` Split and long RAW as optional controls. No four-core saturation claim follows from worker count.
3. **Defer a slots sweep and scheduler changes.** Full-unread occupancy is about 8.3–9.6 ms on `mozilla`, but order-induced no-supply is far smaller than the concurrent `next` gaps and its typical size is near the wall-noise floor. Revisit slots 4/8/16 only if a tighter run shows a repeatable order-idle signature and **clean** wall improvement at acceptable CPU and memory cost. Neither pending residence nor the observed union-bounds are speedup estimates.

## Comparability and measurement limits

All four archives were decoded by the **same retained-buffer Rust codec** (`530db0a02f0559fee5a568698075702153dbff7e`). C7 and Dense name the encoders, not the decoders. No pre-reuse code or timing enters this diagnosis of the present pipeline. Each archive is one frame with 512 KiB blocks: `mozilla` has 98 blocks / 51,220,480 plaintext bytes; `sao` has 14 blocks / 7,251,944 bytes. The short `sao` frame cannot establish steady-state behavior. Pair within an archive, never across producers or files.

Clean and probe came from separate `git archive` exports of the pin, locked `cargo build --release -p zxc-cli` with defaults (`merge-simd`), no extra rustflags, Tracy or ledger, rustc `1.101.0-nightly (822182db4)` / LLVM 23.1.3. The only codec addition in the diagnostic copy is a wire-metadata accessor; no block kernels or channel control decisions are changed. Rust `Instant` provides one monotonic origin; the coordinator and each spawn ordinal append fixed-size records to separate preallocated Vecs. No events are written or merged at the endpoints; after decoded output and flush the diagnostic CLI either drops the captured lanes (`capture`) or writes them to a buffered binary sidecar (`dump`, no fsync). Dump timing includes creating and writing that sidecar. `capture` still allocates, timestamps, writes lane records, checks bounds and disposes the lanes, so it is the control for in-pipeline perturbation. Keep probe timing separate from clean speedups; never correct by subtracting a mean probe tax.

The driver takes an exclusive `flock` on `/home/rektide/src/zxc-rs/.test-agent/zxc-perf.lock` before verification, warmup and timing. Verify fixed archives and plaintext against the [existing SHA manifest](pivco-worker-data/sha256.txt); read each mode's decoded stdout through EOF, require exact digest and length, exit zero and empty stderr. Verification also requires the shipping binary to ignore `ZXC_WAIT_TRACE`; the probe must write a sidecar only in dump mode. All timed stderr streams were checked. Timed runs use warm regular-file input and stdout redirected to `/dev/null`. Four Crossbeam workers **and the coordinator** share logical CPUs 2–5 (four distinct physical cores); `--threads 1` runs the serial codec on CPU 2. SMT remained on, with siblings 10–13 not in the affinity mask but not reserved. The lock serializes cooperating measurements, not all host work. The absolute scenario is Ryzen 7 5800X / Linux 7.1 with AMD P-State active, `powersave`; do not extrapolate to other topology, sinks, or filesystem/cache states.

The default batch uses two independently seeded, balanced, shuffled series of 32 adjacent pairs per case/test, plus one discarded warmup pair per case/test per series. Wall is monotonic process launch to `wait4`; CPU is `wait4` user+sys for all CLI threads. Report paired median percentage changes (negative is less) and 95% binomial order-statistic intervals *per series*; intervals assume independent pairs and do not account for persistent host drift. A/A uses the same clean binary and settings for both members. The predeclared overhead gate was capture without dump within ±3% wall **and** CPU in both series, with uncertainty small compared with the proposed effect; where a trace mechanism is smaller than overhead or unstable, remain inconclusive. Raw sidecar measurements do not reveal on-core time or actual kernel parks.

## What the reducer counts

`T` is pool entry through completed join, not elapsed CLI time. First compute-start splits off startup; last successful data send-return starts drain whether or not EOF has been read (the interior may be empty). Frame prologue, footer and launch/flush/dump residual are preserved separately. Each worker has a disjoint budget: `compute` (including first scratch setup, owned decode/reconstruction and verification), job receive, result send, other live, outside its entry-to-exit span. Integrating compute concurrency partitions `T` into 0–4 compute spans. Mean overlapping compute workers is `compute worker-ms / T`; compute utilization is `compute worker-ms / (4T)`, not child CPU utilization. Coordinator service and worker service overlap; never sum them into a serial cost. `next` includes buffered body input and resize; `commit` includes ordered validation/accounting and buffered output, not just a write syscall.

The reducer verifies successful job/result bijections, monotone lanes, contiguous commits, credit occupancy and separate in-flight reservation, EOF, matching archive metadata and committed plaintext bytes, and exact coordinator/worker budgets in integer nanoseconds. Per-block handoff spans begin at send invocation; the `max(0, recv-return − send-return)` fields are lower bounds on post-send latency. Pending residence is receive-return to commit-begin, while pending depth-time starts at insertion. Neither is elapsed stall. Coordinator missing-head receives are split at compute-end: **(a)** head unfinished, no later job complete; **(b)** head unfinished, at least one later job complete; **(c)** head computed, delivery/collection not finished. A later pending result gives a conservative subset of (b). A full window with unread data is not enough: exposed order pressure requires (b), full credits, further data, and a worker in job receive while no begun job send lacks a matched receive-return. Otherwise queue state is definitely nonempty only when a send has returned before its matched receive even begins, or ambiguous. This observed loss of supply is **not a speedup estimate**: the slow head must still finish.

## Reproduce

From the checkout root, with the [fixed cache](pivco-worker.md#fixed-archive-timing) present:

```sh
d=doc/profiling/decode-waits-data
scratch=$("$d/build.sh")
python3 -B "$d/test_reduce.py"
"$d/bench.py" "$scratch" "$scratch/main"
"$d/reduce.py" "$scratch/main" "$scratch/main"
"$d/summarize.py" "$scratch/main"
# Separate source/sink sensitivity:
"$d/build-io.sh" "$scratch"
"$d/bench.py" "$scratch" "$scratch/io" --io --pairs 24 --seeds 7201 13741
"$d/reduce.py" "$scratch/io" "$scratch/io"
"$d/summarize.py" "$scratch/io"
```

The [build script](decode-waits-data/build.sh), [injection script](decode-waits-data/inject.py) and [probe](decode-waits-data/probe.rs) never alter shipping sources. The [runner](decode-waits-data/bench.py) writes the original order, warmups (`pair=0`), member (`0` reference, `1` comparison), raw wall ns, user/sys µs, maximum child RSS in KiB, and sidecar filename. The [reducer](decode-waits-data/reduce.py) writes frame budgets in ns (worker budget fields are summed worker-ns), per-block rows in ns, and per-head rows; [summarize.py](decode-waits-data/summarize.py) aggregates across **runs**, not across blocks as independent samples. Block p50/p95 values are medians of within-trace nearest-rank percentiles. Local full sidecars and build logs are under `.test-agent/decode-waits/`.

Main batch evidence: [ordered raw timings](decode-waits-data/main/timings.tsv), 256 raw sidecars ([series 1](decode-waits-data/main/traces-s1.tar.gz), [series 2](decode-waits-data/main/traces-s2.tar.gz)), [hash/quiet checks](decode-waits-data/main/verification.tsv), [host snapshot](decode-waits-data/main/environment.txt). To replay without timing, copy the timing TSV to a fresh directory, extract both `traces-s*.tar.gz` archives for that batch there, and run `reduce.py` on that directory with the fixed fixtures present. [timeline.py](decode-waits-data/timeline.py) accepts a named sidecar. The [controlled reducer tests](decode-waits-data/test_reduce.py) cover a held head with two slots and a later pending job, in-order wait, retention behind a write, drain after last admission, and rejection of premature credit reuse. `cargo nextest run --release --locked -p zxc-cli -p zxc-parallel --features zxc-cli/transports` passed 13 tests each on the [clean](decode-waits-data/main/src-tests.txt) and [instrumented](decode-waits-data/main/instrumented-tests.txt) trees, including the existing blocked-head, first-error, and limit tests.
