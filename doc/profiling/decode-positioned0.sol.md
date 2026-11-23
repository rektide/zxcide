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
- Not claimed: non-Linux execution, filesystems offering coherent mutation or snapshot semantics, detection of every same-length rewrite, truly reserved CPUs, or equivalence outside the measured inputs/affinity. The first matching dictionary remains selected by the shared unchanged slot lookup. Cold pages and other storage/media are out of scope.

## Measurement method

The [diagnostic overlay](decode-positioned-sol-data/diag.sol.patch) builds a separate Linux binary. Its forced-stream and positioned instantiations time the same successful `next` edges with `CLOCK_THREAD_CPUTIME_ID` and monotonic elapsed time. Frame markers, finish (seek extent/footer), prologues, physical EOF and the final positioned stat are separate. Byte counts are **logical successful admission/materialization**, not kernel syscall bytes or a page-cache profile. Workers batch their counters until thread exit and the CLI reports only after its output action has succeeded. The clean `S→DS` and `B→DB` pairs test whether probing shifts whole-command results; diagnostic results never supply the acceptance wall or child CPU samples. The optional interior wait reducer was not implemented.

For clean timing the parent A is unmodified, S is the trial in stream mode, and B is the trial requesting positioned input. Single and concat each use two independently seeded series of 24 balanced adjacent pairs with fresh interleaved noise controls at the same setting. Changes are `100 × (other/base − 1)` per pair, reported as separate series medians and two-sided order-statistic median intervals; noise is not subtracted. Pass requires both files and both series to resolve at least −10% wall and at most +3% decoder CPU/GB, plus less reader work and no pipe regression (+3% upper limit for both metrics). An interval spanning a decision boundary is unresolved, not equivalent.

W8 plus the caller inherit mask `0–7`, one logical CPU per physical core on the Ryzen 7 5800X; SMT siblings `8–15` are unreserved. Boost is on and governors are `powersave`. Warmups are discarded. `/dev/null` wall is launch through the decoder's `wait4`; decoder CPU is that child's user+sys divided by 0.211938580 GB of plaintext. The real pipe uses an independent `cat` child with the same mask and an 8 KiB kernel pipe (raising it to 64 KiB was denied at preflight); feeder status and CPU are separate. Real output uses fresh names and the existing buffered flush/publication on Btrfs, **no `fsync`**; hash and unlink happen after the timing edge. The shared lock covers all fixture/output verification, warmups and timed measurements.

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
