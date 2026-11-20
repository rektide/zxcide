---
type: Design
title: A standard CLI comparison report
description: One command measures encode speed and size, then decoder speed on pinned upstream archives.
resource: /design/comparison-report/report0.astra.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra#high', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e08492965ffe73Q6gm0qwqKych', title: Comparison report design and implementation handoff }
  - { resource: /xtask/scorecard.py, title: Corpus pin and CLI correctness matrix }
  - { resource: /xtask/src/main.rs, title: Existing xtask names and build path resolution }
  - { resource: /xtask/src/interop.rs, title: Cached upstream build with a separate conformance pin }
  - { resource: /doc/profiling/scorecard/README.md, title: Frozen CLI and buffer API protocols }
  - { resource: /doc/profiling/scorecard/dense-full-smoke-20261120.json, title: Plaintext and C archive identities for all twelve files }
  - { resource: /doc/profiling/dense-decode.md, title: Median of full pass totals on fixed archives }
  - { resource: /xtask/probes/dense_decode.py, title: Hash gate and shuffled decoder passes }
---

# One command, two questions

How much does the current CLI compress, at what encode speed? How fast does it read the **same existing archives** as upstream? Make these two tables the standard report. The existing `scorecard` requires two binaries, build descriptions and an output path. Its frozen results establish archive identities; their historical times do not describe today's machine.

```sh
cargo xtask comparison-report                          # current Rust vs pinned C; all 12 Silesia members
cargo xtask comparison-report --quick                  # dickens + sao, same sampling; label as a subset
cargo xtask comparison-report --before PATH/TO/RUN     # also remeasure the Rust CLI saved by an earlier run
```

First use needs Cargo/Rust, Python 3.11+ stdlib, Git, C compiler, CMake 3.21+, a native build tool and network. Target native Linux first. Resolve the Cargo target directory via xtask metadata. Put disposable caches under it and write each run to `comparison-report/runs/<unique-id>/` there. Stdout: scope, up to four aggregate highlights (Split/C3 and Dense/C7 encode speed **with size gap**; C3 and C7 fixed-archive decode speeds), verification status, report path. Put detailed tables in `report.md`, build output and diagnostics in `run.log`. Chat can quote these highlights and link the files.

## Prepare and verify outside timing

1. Build the working tree with `cargo build --locked --release -p zxc-cli`, default features (currently `merge-simd`). Save a private copy of the executable in the run before measuring; the next Cargo build must not overwrite a baseline. `--before` loads only a completed run's saved Rust executable, checks its recorded SHA-256 and imports its build identity. Remeasure it in today's job list; never subtract two saved timings. Allow `--quick` on either run: the old binary reads today's inputs.
2. Fetch **`hellobertrand/zxc@5719a96f778226868b29dba621a8c4e302b74d7c`**, the performance pin. The `interop` oracle uses `2992c28e…` for conformance. Reuse its detached-fetch/clean-tree check and out-of-tree CMake Release build pattern, with independent cache keys and no change of oracle. Use the upstream native Release configuration of the frozen study. Check the full SHA and clean source on reuse; run the incremental configure/build, key the build dir by toolchain/flags/host for native ISA. Record effective compiler flags. Preserve cache entries on error; name the offending path. Never use upstream HEAD or pick an executable by searching nearby build trees.
3. Reuse `scorecard.py`'s `URL`, `ZIP_SHA`, `FILES` and verified atomic download (`<target>/scorecard/silesia.zip`). Check ZIP digest and exact member set on cache hits too. Extract only named members to run scratch. Check plaintext sizes and hashes against the [frozen full manifest](../../doc/profiling/scorecard/dense-full-smoke-20261120.json). With warm source/ZIP and Cargo dependencies there should be no network fetch. Never cache elapsed times.
4. For each plaintext stage one archive per encoder: C `-qzc -T1 -3/-7 FILE`, current Rust `encode --mode split/dense FILE`, and the same modes for before if present. Defaults: checksum on, ordinary 512 KiB blocks, serial Rust; C has one worker plus the streaming driver. Record each archive's size and digest. **Require C3 and C7 hashes to match their per-file frozen entries** before calling them fixed. Feed these exact staged paths to all timed decoders. A changed Rust encoder is free to emit different bytes. Cross-decode *all* staged archives with every participating decoder (`decode ARCHIVE` or C `-qdc -T1 ARCHIVE`); compare streamed output through EOF to the original bytes and check every exit status. Abort on any mismatch. Remove scratch after the run; rebuilding fixed C archives costs one untimed encode and avoids a second archive cache.

## Time and render

Use one untimed warmup pass, then seven measured passes. For each pass shuffle the complete list of `(file, encode implementation/mode)` and `(file, fixed C3/C7 archive, decoder)` jobs with one seeded RNG (42). Run sequentially; inherit CPU affinity (operator may use `taskset`). Time each CLI invocation with a monotonic nanosecond clock through successful exit, warm file input and stdout to `/dev/null`; no hashing, extraction, compilation or verification inside that interval. These are elapsed CLI times including startup and streaming. Record the native-C/ordinary-Rust ISA settings, affinity and uncontrolled host load as limitations. The standalone buffer API probes even differ on checksum work; leave those numbers in their [own study](../../doc/profiling/scorecard/README.md#single-thread-buffer-api-probe).

Render aggregate rows first, then the same tables by filename. Separate C3/Split and C7/Dense operating points; paired labels are useful reference points, not equivalent algorithms. Column contracts:

| Table | Row key | Columns after key |
|---|---|---|
| Encode + compression | scope, reference pair, encoder (C/current/before) | plain B, archive B, archive/plain %, size Δ vs C %, encode median ms, encode MB/s, speed × vs C |
| Fixed-archive decode | scope, source (C3/C7), decoder (C/current/before) | plain B, archive B, decode median ms, decode MB/s, speed × vs C |

With `--before`, also show current/before speed × and encode size Δ vs before for each matching mode. MB/s is **decimal plaintext bytes per second / 1,000,000**, for both directions. For one file use median elapsed time. For a scope total, sum elapsed ns across its files **within each pass for the same job kind**, then take the median of those seven sums; divide total plaintext by that time. Never average throughputs or sum file medians. Size uses exact sums: `100 × Σarchive/Σplain`; size Δ is `100 × (candidate_bytes/reference_bytes − 1)` (negative is smaller); speed × is `reference_median_time/candidate_median_time` (above one is faster). Round only for display. Show observed min–max pass times alongside medians in the log; no significance claims. No combined score across levels or archive producers.

## Evidence and build handoff

`raw.json` (versioned) must suffice to regenerate `report.md` without invoking a codec. Include run argv/time, scope, seed, selected files, host/OS/CPU/affinity, source revisions plus Rust worktree dirty state, build commands/tool versions/flags/features, saved binary hashes, corpus URL and ZIP/manifest hashes, plain and archive bytes/hashes, verification matrix, and job definitions with argv and archive identity. Record pass, phase (warmup/sample), actual order, job ID and elapsed ns. Validate completeness and uniqueness before rendering. Stage results under a unique incomplete name; publish the completed directory only after verification, sampling and rendering succeed. Failed runs retain an explicitly incomplete diagnostic log and exit nonzero.

**Implementation slice:** add a typed `ComparisonReport` task next to `Scorecard` (`compare` already means Criterion native vs component). A small Python stdlib runner/renderer can reuse the scorecard's corpus/check helpers and the fixed-hash check from the [decode probe](../../xtask/probes/dense_decode.py). Keep the existing explicit-binary scorecard and frozen artifacts usable. Add the three command examples to the maintained profiling docs.

**Acceptance:** demonstrate clean-cache bootstrap and warm-cache reuse; run a verified standard report and a paired subset using its saved CLI. Full scope is 12 files / 211,938,580 plaintext B; without before expect 48 archives, 96 exact decodes and 96 timed jobs per pass. Check both C-level hashes for every file. Test wrong cache or archive hash, wrong baseline binary, bad decoder output and missing/duplicate samples: none may yield a complete report. Use a tiny synthetic sample set where the median of pass totals differs from the sum of medians to test the aggregate and ratio directions. Deliver the command, report path and aggregate highlights to the operator; preserve all raw samples in the run directory.
