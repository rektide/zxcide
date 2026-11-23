---
type: Report
title: Positioned reads on the Crossbeam decode workers
description: Verification and warm-file performance for the opt-in positioned decode trial.
resource: /doc/profiling/decode-positioned0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df861f945ffeedoo6kewpdocm2', title: Built and verified the scratch prototype and ran the paired gates }
  - { resource: /design/decode-throughput/positioned0.astra.md, title: Source selection, worker revalidation and three-step trial design }
  - { resource: /design/decode-throughput/next0.sol.md, title: Trial ranking and decision rule }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Fixed independently encoded single, concat and plaintext identities }
---

# Positioned reads on the Crossbeam decode workers

**Keep trial #2 out of production.** Positioned reads removed about 96% of coordinator CPU inside successful block admissions. They did not earn the required 10% lower end-to-end W8 wall: the long single frame was about 2–5% slower in the two clean paired series, and concat was inconsistent. Parity passed on the tested Linux paths. Pipe-wall nonregression and the primary child CPU ceiling were not established.

## Implementation and verification

The [trial patch](decode-positioned-sol-data/trial.sol.patch) is for a scratch export of `3f711434ad83b85fa191a209041777c457c6972b`. `ZXC_TRIAL_INPUT=positioned` means the caller opts in and asserts a stable ordinary named file. On Unix with Crossbeam and W>1, the decoder uses the existing open handle if it is regular and has positive length. All other inputs use the streaming path; invalid config still errors, and errors after dispatch never trigger a restart. Unset or `stream` uses the original buffered source. The worker re-reads and validates the complete header before exact-reading the body into the slot Vec. Final same-fd length and mtime/ctime checks do not provide snapshot semantics. Both modes use one epoch/commit/finish loop, the same codec and writer, and the original credit window. No production Rust or config was changed.

| Locked Release nextest in `trial` | Passed |
| --- | ---: |
| `--workspace` | 75 |
| `--workspace --features zxc-cli/transports` | 75 |
| `--workspace --no-default-features` | 75 |
| `-p zxc-codec --no-default-features` | 35 |

- The added differential tests use the same file with stream and positioned in W2/K1, W5/K3 and W8/K16. They assert actual RAW/SPL/PAK across both checksum and index flags; mix nonempty and empty equal-size epochs with a size-changing empty frame; switch dictionaries including ID zero; and reject a missing ID-zero dictionary on an empty frame before a subsequent frame. Both paths match the cumulative limit exactly and reject one byte below it with the same block prefix.
- Error kind and committed-prefix parity includes empty input, partial magic/prologue, trailing bytes, malformed or oversized blocks, truncated block headers/payload/checksum trailers, bad intermediate checksum, bad EOF fields, truncated/wrong seek headers, short seek extent, and truncated or incorrect footer/digest. A bad nonfinal frame never commits the following frame. Seek tables retain stream validation semantics, not per-entry authentication.
- Deterministic worker hooks run after admission but before materialization: a second wire-valid header of identical extent is rejected on mismatch, and truncation inside the body produces `UnexpectedEof`. A writer hook appends past captured EOF before the same-fd stability check, which rejects after verified output. Unit tests cover near-`u64::MAX` arithmetic and first writer error ahead of later worker checksum/admission errors. The existing blocked-head, EOF-credit, epoch, panic/join, and first-error tests also passed.
- The CLI preflight recorded 85 checks and 16 selection traces. On both long archives, A/S/B produced the manifest length and SHA-256 through EOF, with status zero and quiet stderr, including actual pipes and `cmp` on fresh named outputs. All 18 valid and 35 invalid-category conformance fixtures matched the independent parent status and output prefix; the one expected success under `invalid/` was `sek_forged_entry.zxc` (the stream decoder validates the seek extent, not entries). `test` accepted valid input and rejected a late bad checksum. A/S/B left existing sentinel files unchanged and published no fresh destination on a late failure.
- On named long files, archive-FD `pread64` on nine distinct TIDs (caller plus eight workers) appeared only in B. A/S used `read`. W1, each of Flume/Kanal/Crossfire/Rayon, actual pipe, redirected regular-file stdin, a named FIFO with bounded wait, `/dev/null`, and zero-length-stat `/proc/self/cmdline` all stayed on `read`. The traced named FIFO and regular inputs opened once; stdin was not reopened.
- Not claimed: non-Linux execution, filesystems offering coherent mutation or snapshot semantics, detection of every same-length rewrite, truly reserved CPUs, or equivalence outside the measured inputs/affinity. The first matching dictionary remains selected by the shared unchanged slot lookup. Cold pages and other storage/media are out of scope.

## Measurement method

The [diagnostic overlay](decode-positioned-sol-data/diag.sol.patch) builds a separate Linux binary. Its forced-stream and positioned instantiations time the same successful `next` edges with `CLOCK_THREAD_CPUTIME_ID` and monotonic elapsed time. Frame markers, finish (seek extent/footer), prologues, physical EOF and the final positioned stat are separate. Byte counts are **logical successful admission/materialization**, not kernel syscall bytes or a page-cache profile. Workers batch their counters until thread exit and the CLI reports only after its output action has succeeded. The clean `S→DS` and `B→DB` pairs test whether probing shifts whole-command results; diagnostic results never supply the acceptance wall or child CPU samples. The optional interior wait reducer was not implemented.

For clean timing the parent A is unmodified, S is the trial in stream mode, and B is the trial requesting positioned input. Single and concat each use two independently seeded series of 24 balanced adjacent pairs with fresh interleaved noise controls at the same setting. Changes are `100 × (other/base − 1)` per pair, reported as separate series medians and two-sided order-statistic median intervals; noise is not subtracted. Pass requires both files and both series to resolve at least −10% wall and at most +3% decoder CPU/GB, plus less reader work and no pipe regression (+3% upper limit for both metrics). An interval spanning a decision boundary is unresolved, not equivalent.

W8 plus the caller inherit mask `0–7`, one logical CPU per physical core on the Ryzen 7 5800X; SMT siblings `8–15` are unreserved. Boost is on and governors are `powersave`. Warmups are discarded. `/dev/null` wall is launch through the decoder's `wait4`; decoder CPU is that child's user+sys divided by 0.211938580 GB of plaintext. The real pipe uses an independent `cat` child with the same mask and an 8 KiB kernel pipe (raising it to 64 KiB was denied at preflight); feeder status and CPU are separate. Real output uses fresh names and the existing buffered flush/publication on Btrfs, **no `fsync`**; hash and unlink happen after the timing edge. The shared lock covers all fixture/output verification, warmups and timed measurements.

## Diagnostic gate: less coordinator work

| Archive | Jobs | DS→DB coordinator successful `next` CPU (series 1 / 2) | Median paired CPU change [≥95% interval] (series 1 / 2) |
| --- | ---: | --- | --- |
| Single | 405 | 12.163→0.477 / 11.714→0.481 ms | −96.10% [−96.25, −95.95] / −96.07% [−96.25, −95.69] |
| Concat | 411 | 12.162→0.522 / 12.227→0.510 ms | −95.75% [−95.99, −95.55] / −95.81% [−96.12, −95.54] |

Single moved 70,135,738 body bytes off the caller; concat moved 70,109,662. The caller still read 3,240 / 3,288 data-header bytes, plus 8 / 96 EOF bytes respectively; workers in DB read another 3,240 / 3,288 data-header bytes for validation. Each mode admitted and handled the same number of jobs. Elapsed `next` time also fell about 95–96%. All capture-vs-clean intervals for total wall and child CPU included zero, with median changes within about 2.1%. The repeatedly lower coordinator CPU is far larger than that noise. It is not evidence that workers were starved for these entire intervals or that the saving was removed from the process rather than moved into workers.

## Clean gate: no qualifying speedup

A→B below is clean parent vs clean trial with positioned requested, W8/K16. S1 is seed 56813; S2 is 88241. Entries are medians of paired percent changes with two-sided order-statistic intervals; positive means slower or more CPU.

| Archive | Sink / input | Wall S1 | Wall S2 | Child CPU/GB S1 | Child CPU/GB S2 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Single | `/dev/null` / named | +5.11% [+0.02, +8.04] | +1.99% [−6.62, +14.03] | +2.27% [−0.22, +4.35] | +1.08% [−0.90, +4.15] |
| Concat | `/dev/null` / named | +1.02% [−5.33, +5.37] | −4.41% [−10.43, +4.36] | +1.81% [−1.20, +3.09] | +2.24% [−0.94, +3.61] |
| Single | `/dev/null` / pipe | +2.00% [−8.65, +11.79] | +1.63% [−0.69, +6.76] | +0.43% [−1.42, +1.48] | −1.01% [−3.01, +3.40] |
| Concat | `/dev/null` / pipe | +2.68% [+0.18, +7.24] | −0.16% [−3.64, +2.44] | +0.71% [−1.72, +2.38] | −0.13% [−1.74, +1.03] |
| Single | Fresh file / named | −4.25% [−12.87, +2.28] | −7.30% [−10.92, +2.52] | +4.63% [−1.72, +11.32] | +1.19% [−2.05, +7.51] |
| Concat | Fresh file / named | −3.43% [−9.89, +6.34] | −1.79% [−5.69, +5.57] | +1.59% [−2.00, +6.14] | +3.24% [+1.70, +6.03] |

Same-binary S→B wall medians were positive in both series on both layouts (+4.10% / +0.50% single, +5.64% / +1.93% concat), with intervals excluding a 10% win. A→S wall intervals all included zero, so the stream refactor is not a demonstrated cause. The clean 1-minute load moved from 10.56 to 11.97 in S1 and from 11.97 to 23.83 in S2; some A→A wall intervals were wide. Concat S2 alone crossed −10%, but both single intervals and concat S1 exclude the target. The +3% CPU ceiling is unresolved for the primary rows. Pipe wall has a small positive median on both single series but does not resolve either equivalence or a repeatable regression; concat's first-series positive shift did not repeat. Buffered real-file output did not confirm a wall gain. No quieter repeat was spent: it could clarify a marginal or fallback effect, but the single-file primary gate already missed in both series. Do not adopt the selector, alter the queue, or infer anything about the serial codec from this result.

See [all per-series tables](decode-positioned-sol-data/results.sol.md), including each A/A, capture-noise contrasts, per-block admission CPU and elapsed time, and separate frame tails. The raw [clean](decode-positioned-sol-data/clean.sol.json.gz) and [diagnostic](decode-positioned-sol-data/diag.sol.json.gz) data record pair order, warmups, builds and fixtures, wall, user/sys, feeder, status and stderr. [Preflight results](decode-positioned-sol-data/verification.sol.json), [raw selection traces](decode-positioned-sol-data/selection.sol.tar.gz), [build and test logs](decode-positioned-sol-data/build-and-tests.sol.tar.gz), and [build/input hashes](decode-positioned-sol-data/build-and-inputs.sol.txt) are preserved alongside the patches.

## Reproduction

Use the existing prepared files named `plain`, `single.zxc`, `concat.zxc` from the [pool run](decode-pool.md) without re-encoding either archive. The harness checks archive and plaintext SHA-256 and lengths against the committed manifest. `zxc-parent` (A), `zxc-trial` (S/B), `zxc-diag` (DS/DB) and the selection-only `zxc-transports` are in `.test-agent/decode-positioned-sol/bin/`.

```sh
s=.test-agent/decode-positioned-sol
d=doc/profiling/decode-positioned-sol-data
# Optional fresh build; use $s instead of $s/repro to reuse the measured binaries.
bash "$d/build.sol.sh" "$s/repro"
taskset -c 0-7 python3 "$d/bench.sol.py" \
  "$s/repro" .test-agent/decode-pool/fixtures "$s/repro-run"
# Diagnostic stdout is still plaintext; the probe record is stderr after flush.
ZXC_TRIAL_INPUT=positioned "$s/bin/zxc-diag" decode --backend crossbeam \
  --threads 8 --slots 16 .test-agent/decode-pool/fixtures/single.zxc >/dev/null
```
