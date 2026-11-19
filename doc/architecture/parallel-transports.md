---
type: Design
title: Bounded parallel block transports
description: One frame driver, four queue adapters, and a fixed-batch Rayon comparison with common correctness and performance tests.
resource: /doc/architecture/parallel-transports.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Parallel execution and testing requirements }
  - { resource: /doc/architecture/indexed-execution.md, title: Verified block boundary, admission invariant and candidate libraries }
  - { resource: /doc/testing/generated-interop.md, title: Independent reference and replay contract }
---

# Parallel block execution

`zxc-parallel` depends on `zxc-codec`; the component, seek reader and serial codec need no thread runtime. A parallel operation takes the existing stream options and a separate `Config { workers, slots, backend }`. Expose `encode(reader, writer, options, config)` and `decode(...)`; the caller retains flushing and output-file policy. One worker takes the direct serial path. Crossbeam is the initial default; optional Cargo features add flume, kanal, crossfire, and Rayon. Reject zero or excessive settings before starting workers. Keep pools scoped to the call or frame, not the process-global Rayon pool.

## Data path

Keep frame metadata in the codec. Extract small shared helpers for serial/parallel frame prologue, ordered counters/digest/seek entries and footer. Workers get a block and its frame-local index, immutable settings/dictionary, and private reusable scratch. Compression returns the same payload and checksum as the serial block encoder. Decompression calls the existing verified `BlockDecoder`. A worker never writes directly to the sink. Drain the current frame before processing its footer or changing dictionaries. On corrupt data the sink may contain preceding verified blocks, but nothing from the failed block or beyond it.

Use a bounded sliding window and a sequenced result buffer. The invariant is `admitted - committed ≤ K`; take a credit **before** reading a data block and keep it until ordered commit. Out-of-order completion is not a credit. Use bounded queues for jobs and results; carry unique sequence numbers, never duplicate payloads just to reorder. An unfilled final window must drain too. With all K jobs live the coordinator must be able to receive completions, including one not yet at the head. Do not wait for workers to join while they are blocked sending to a live receiver. Worker panic, source error or writer error must close channel endpoints and join all spawned workers. Synchronous `Read` and `Write` themselves cannot be forcibly interrupted; distinguish external I/O stalls from internal deadlocks.

Crossbeam, flume, kanal and crossfire provide blocking adapters to the same scheduling loop. Pick the adapter once per operation, not per byte/token. Use a local Rayon pool and at most K jobs per ordered batch as a different scheduler under the same outer contract. Keep the baseline's batch barrier visible in measurements. CLI stream commands accept `--threads`, `--slots`, `--backend` (backend names reported by the compiled binary); default to serial until the performance evidence supports changing it. Do not add threading semantics to range/info in this landing.

## Gates

- In a deterministic blocked-head test, two workers and two slots complete job 1 before job 0. Before releasing 0, verify that job 2 was not admitted and no result committed. Exercise endpoint shutdown and failing sinks with a bounded test timeout. Cover K=1, K less than worker count, empty/partial windows and single-frame/multiframe EOF.
- Run one suite for every enabled backend: serial and parallel archives byte-identical for all encode modes, checksums, seekable tables and dictionaries; parallel decode equal to reference bytes with cumulative `max_output` and ordered corruption errors. Use the [generated gate](../testing/generated-interop.md) for independent C checks on generated parallel CLI jobs.
- Expose Criterion encode/decode comparisons with identical inputs, worker and slot budgets, including 4 KiB and default blocks. Record throughput in source bytes, keep setup scope explicit, and use Tracy on demand for worker compute and queue/ordered waits. Measure before recommending a backend; a timing table for one machine is not a universal ranking.
