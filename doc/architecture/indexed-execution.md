---
type: Design
title: Indexed reads and bounded block execution
description: Separate wire validation, archive navigation and parallel scheduling so each has an independent consumer and test boundary.
resource: /doc/architecture/indexed-execution.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Random-access priority and transport experiments }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/docs/FORMAT.md', title: Seek groups and v9 integrity semantics }
  - { resource: 'https://github.com/hellobertrand/zxc/blob/2992c28e6e1df07feec0cb39678a6d4a9665c6bb/src/lib/zxc_driver.c', title: Reference reader, workers and ordered ring writer }
---

# Indexed reads first

`cli → seek → codec`. The existing component and sequential stream consumers depend on `codec` alone. Keep wire/header validation and the verified data-block decoder in the codec; share them across sequential and positioned I/O. Only the new `zxc-seek` crate needs to know how to locate a block. A caller with a block's header, payload, frame settings and frame-local index shouldn't need to synthesize a temporary archive to decode it.

`Archive::open(source)` takes a source with a length and exact positioned reads. Support memory and files; avoid a shared file cursor. Walk minimal-width footers backwards and validate the frame bounds, headers, EOF and SEK layout before exposing metadata. Require every frame to be indexed. Keep O(frames) descriptors, bound the number of frames, and keep group entries on disk. Opening and inspecting descriptors don't require dictionaries. Callers must keep the source contents stable while the archive is in use.

`copy_range(start..end, dictionaries, writer)` takes **uncompressed**, archive-relative, half-open byte offsets, including across concatenated frames. Reject reversed and out-of-bounds ranges; EOF..EOF succeeds. Binary-search frames, load only the touched 64-block groups, check their anchors/sizes against that frame, and compare each accessed block header to its entry. Decode a full touched block with its frame-local dictionary and checksum seed; require the expected decoded size before handing the overlap to the writer. Scratch belongs to a read, not the shared index. No allocation proportional to the requested length. A range read verifies touched checksums but cannot vouch for the whole-frame digest. A named CLI output is published only on success.

CLI: `zxc range ARCHIVE START LENGTH [-D DICT] [-o OUTPUT]` and `zxc info ARCHIVE` (indexed frame metadata, not an integrity test). Prove the data path with upstream fixtures and a counting source that forbids reads of unrelated payloads, including a 63→64 boundary and a range crossing frames. Reject `sek_forged_entry` on access.

## Next landing: interchangeable parallel drivers

The parallel driver is a sibling consumer of `codec`; indexed batch execution can also consume `seek`. Keep channel types out of archive, codec, component and CLI APIs. A proposed `crates/parallel` owns admission, per-worker scratch, cancellation and in-order commit. Start with crossbeam-channel; put comparison adapters beside that driver behind opt-in features (split a backend into a crate if it brings a distinct runtime). Compare flume, kanal or crossfire against that same driver before selecting a default. Rayon is a useful bounded indexed-batch baseline: collect a fixed batch, process independent blocks in parallel, write in order. It doesn't require reimplementing an ordered stream with Rayon tasks waiting on each other.

| Candidate | First configuration to compare |
| --- | --- |
| [crossbeam-channel 0.5](https://docs.rs/crossbeam-channel/0.5.17/crossbeam_channel/fn.bounded.html) | `bounded(K)` job and result queues; blocking worker threads |
| [flume 0.12](https://docs.rs/flume/0.12.0/flume/fn.bounded.html) | `bounded(K)` with default features off; add `select` only if needed by the driver |
| [kanal 0.1](https://docs.rs/kanal/0.1.1/kanal/fn.bounded.html) | `bounded(K)` with default async support off |
| [crossfire 3](https://docs.rs/crossfire/3.1.23/crossfire/mpmc/fn.bounded_blocking.html) | `mpmc::bounded_blocking(K)` for jobs, `mpsc::bounded_blocking(K)` for results; default features are empty |

These are experiment configurations, not dependencies of the indexed reader. Use a positive slot count; zero-capacity rendezvous semantics aren't needed for this comparison. Compare cancellation/disconnect behavior as well as the successful send/receive path.

**Bound the lifetime of a block, not just the channel.** Acquire one of K reusable slots *before reading* a block. Carry its sequence number and permit through execution and out-of-order storage; return it only after the writer commits or cancellation disposes of it. Thus queued + executing + waiting for ordered output ≤ K. Budget the maximum input/output capacities per slot **plus** scratch per worker and any separately requested seek index. Even an unbounded channel carrying only those K unique permits is logically bounded, but bounded queues make accidental extra admission easier to detect. A bounded input queue followed by an unbounded reorder map does not satisfy the contract.

Drain results out of completion order into indexed slots so a full results queue cannot prevent the writer from receiving the missing head. On a source, worker or sink failure stop admission, wake blocked participants, reclaim slots and join workers. Keep `threads=1` on the direct serial path. Validate identical encoded bytes at 1 and N workers, peak in-flight slots with a deliberately slow first block, and cancellation with a failing writer. Compare transports on the same codec, corpus, block size, worker count and slot budget, measuring throughput, peak resident memory and time waiting for a slot or the ordered head. Queue-only benchmarks cannot establish the best archive driver.
