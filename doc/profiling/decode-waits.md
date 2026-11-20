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

## Comparability and measurement limits

All four archives were decoded by the **same retained-buffer Rust codec** (`530db0a02f0559fee5a568698075702153dbff7e`). C7 and Dense name the encoders, not the decoders. No pre-reuse code or timing enters this diagnosis of the present pipeline. Each archive is one frame with 512 KiB blocks: `mozilla` has 98 blocks / 51,220,480 plaintext bytes; `sao` has 14 blocks / 7,251,944 bytes. The short `sao` frame cannot establish steady-state behavior. Pair within an archive, never across producers or files.

Clean and probe came from separate `git archive` exports of the pin, locked `cargo build --release -p zxc-cli` with defaults (`merge-simd`), no extra rustflags, Tracy or ledger. The only codec addition in the diagnostic copy is a wire-metadata accessor; no block kernels or channel control decisions are changed. Rust `Instant` provides one monotonic origin; the coordinator and each spawn ordinal append fixed-size records to separate preallocated Vecs. No events are written or merged at the endpoints; after decoded output and flush the diagnostic CLI either drops the captured lanes (`capture`) or writes them to a buffered binary sidecar (`dump`, no fsync). Dump timing includes creating and writing that sidecar. `capture` still allocates, timestamps, writes lane records, checks bounds and disposes the lanes, so it is the control for in-pipeline perturbation. Keep probe timing separate from clean speedups; never correct by subtracting a mean probe tax.

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
```

The [build script](decode-waits-data/build.sh), [injection script](decode-waits-data/inject.py) and [probe](decode-waits-data/probe.rs) never alter shipping sources. The [runner](decode-waits-data/bench.py) writes the original order, warmups (`pair=0`), member (`0` reference, `1` comparison), raw wall ns, user/sys µs, maximum child RSS in KiB, and sidecar filename. The [reducer](decode-waits-data/reduce.py) writes frame budgets in ns (worker budget fields are summed worker-ns), per-block rows in ns, and per-head rows; [summarize.py](decode-waits-data/summarize.py) aggregates across **runs**, not across blocks as independent samples. Block p50/p95 values are medians of within-trace nearest-rank percentiles. Local full sidecars and build logs are under `.test-agent/decode-waits/`.
