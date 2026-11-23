---
type: Implementation
title: Checked decode Tracy pair capture, v1
description: A local opt-in C fork, exact paired capture checks, and the gate for using Tracy phase magnitudes.
resource: /doc/profiling/decode-tracy0.sol.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_dfa1e7930ffeXdMOeOa7bolZIX', title: Implemented the probes and replay, ran paired exact captures and checked build and error-path evidence }
  - { resource: /design/decode-tracy/design0.astra.md, title: Pin, boundary and acceptance design }
  - { resource: /doc/profiling/decode-pool-data/fixture-manifest.json, title: Fixed C7 archive and plaintext byte identities }
  - { resource: /xtask/probes/decode-tracy/pins.json, title: Fork history, patch hashes and Tracy release pins }
---

# Checked decode Tracy pair capture, v1

The paired runner verifies what was traced, not which decoder is faster. These captures are **uncalibrated; qualitative only**. The [clean comparison and fixed-byte controls](decode-pool.md) and the [separate checked replay](decode-replay0.sol.md) answer different performance questions. Do not infer CPU from durations in a trace.

## Reproduce

Run from the Rust repo root on Linux x86-64. Use Python ≥3.12, local source repositories containing the pins, Cargo's locked dependencies cached, and the pinned Linux x86-64 Tracy release tools (not tools selected by ordinary `PATH`). The scripts only create fresh direct children of `.test-agent/decode-tracy/`. Heavy builds and captures hold `.test-agent/zxc-perf.lock`; traces may disclose build paths and host metadata, so keep these private.

```sh
p=xtask/probes/decode-tracy
s=.test-agent/decode-tracy
f=.test-agent/decode-pool/fixtures
b=$s/build-local                     # must not exist
t=$HOME/.cache/opencode/tmp/zxc-tracy.jBOVlP  # local hint, not trusted by the runner
pin=5719a96f778226868b29dba621a8c4e302b74d7c
cbase=target/comparison-acceptance/comparison-report/cache/$pin/source

python3 -B "$p/build.py" --base-repo "$cbase" --tracy-repo "$HOME/archive/wolfpld/tracy" \
  --tracy-sys-public "$HOME"/.cargo/registry/src/*/tracy-client-sys-0.30.1/tracy \
  --verify-c --out "$b"
python3 -B "$p/build.py" --stock-rust-only --out "$s/stock-local"

pair() {
  program=$1; shift
  python3 -B "$p/$program.py" --build "$b/build.json" --c "$b/c-cap/zxc" --c-off "$b/c-off/zxc" \
    --rust "$b/rust-cap" --rust-off "$b/rust-off" --capture "$t/tracy-capture" \
    --csvexport "$t/tracy-csvexport" --manifest doc/profiling/decode-pool-data/fixture-manifest.json \
    --fixtures "$f" --port 18186 "$@"
}
pair pair --case sao --workers 1 --out "$s/sao-local" --negatives \
  --verify-c "$b/c-verify/zxc" --stock-c "$f/c" --stock-rust "$s/stock-local/stock.json"
pair pair --case single --workers 8 --out "$s/single-local" --null-smoke
```

Use the same regular archives as the pool experiment (see its [generation recipe](decode-pool.md)); `single.zxc` is not `concat.zxc`. Recheck a saved unwrapped export with `python3 -B "$p/events.py" c sao <path-to-events.csv>` (use `rust` for the Rust events). [The boundary map](../../xtask/probes/decode-tracy/boundaries.json) binds label, source site and same-lane parent; unclassified events remain local. The verifier checks per-block payload/hash order and single reconstruction containment, and derives prepare from each `reconstruct.start − payload.start`, with tail separate. Parent, child and multi-worker elapsed durations overlap. Existing `cargo xtask tracy` is still the independent Rust-only smoke.

## What was verified

The local C repo is `~/src/zxc-decode-tracy/`, bookmark `decode-tracy-v1`, no Git remote. Base `5719a96f778226868b29dba621a8c4e302b74d7c`, base tree `5fd04aa19027db94fc20ee6d9bbd624b025237e6`; tip `bfbf3b050e579d643e79ca746cc7e8c34fc8f9b8`, tree `ad615cf857ee32b2569ab0ba555f284bd1ae2a44`. An independent shallow import plus the exact [format-patch](../../xtask/probes/decode-tracy/0001-checked-decode.patch) reproduced that tree at replay commit `24eff51af368b741eede973afc8e5d0da11a6d67`. The Rust span commit is `72127fe966c04080065ef7b9b784ad9f6234dcd0`; there are no dependency or release-profile edits.

C OFF configured with a nonexistent `CXX`, before creating the Tracy export: no CXX cache entries, Tracy compile input, linked symbol, static link-map entry or runtime dependency. Its normally stripped CLI was byte-identical to the bound stock C CLI (SHA-256 `21f4e94add22d98dae206e1c4d9ec003a6bc2bdcda16a8a7c8e70cdb03787589`). C ON compiled one `TracyClient.cpp` archive member, with matching defines on driver, CLI and each `_default`, `_avx2`, `_avx512` decompressor. All three use the upstream per-ISA options and C17; optimized OFF/ON retain native generic flags. Separate GDB breakpoint checks hit `_avx2` for both inputs in the C capture binary (not a sample of timed captures); Rust dispatch was not separately observed. Rust profiling OFF and ordinary release had no Tracy entries in the Cargo feature tree, symbols, or runtime libraries. Neither build used debug-line or remapping flags beyond the respective profiles; C ON retains symbols.

All success runs had status zero and empty decoder stderr. Attached and both OFF/detached file controls matched exact plaintext for each cell: `sao` 7,251,944 B, SHA-256 `c2d0ea2cc59d4c21b7fe43a71499342a00cbe530a1d5548770e91ecd6214adcc`; `single` 211,938,580 B, SHA-256 `cc3b55db1901a40a0de1c6ad5a3fdd469c487b626a5359279d4e201a552bf45b`. Both clients had an owned `127.0.0.1` listener and no decoder UDP socket at listen time (broadcast disabled at build). Decoders used CPUs `0-5,8-13`, collectors `6,14`; SMT was active. `sao` W1 was serial Rust and one C worker plus writer (four job slots); `single` W8 was default Crossbeam with 16 slots and eight C workers plus writer (32 job slots, positioned-read path eligible).

Counts are checked / payload / reconstruct / hash / literal / token, followed by derived prepare count. Hashes identify the `.tracy` and `-u` CSV in the local run folders `sao-w1-sol-00` and `single-w8-sol-00`; aggregate CSV hashes and commands remain in their `sample.json` / `run.json` receipts.

| Cell | CLI | Counts; prepare | Checked lanes | Trace SHA-256 | Event CSV SHA-256 |
|---|---|---|---:|---|---|
| sao W1 | C | 14/14/14/14/6/14; 14 | 1 | `0b0d68cb3798432e2ae2cc450b449c2a27a0fde6899608018d2c86f22309bf8b` | `d5750c6902ab051f74b6c0fe2386e396b8012b06f0bb1fa9afb26e03cfa9d8e0` |
| sao W1 | Rust | 14/14/14/14/6/14; 14 | 1 | `44df5c805198ead5d2a162912f49fd81d6303063205b7136eeb984b03c6989b6` | `af3d41f9dd49df426d43c56fe4097c56f1702434eb47a183882120314cdac31e` |
| single W8 | C | 405/405/405/405/380/405; 405 | 8 | `25e61ff54a0039c3c451876a8b99e99566c1fa19940f0bba2e0f1d5531859822` | `daed39ff8716f47467297b4ab4558ef5a9fe145fb0398736852d8b84c4b7e992` |
| single W8 | Rust | 405/405/405/405/380/405; 405 | 8 | `1fd8df7095e2a479fba3a96abecfb718178416f6f11c1e3ef90dd30336bb31ab` | `4227504b7bb645d43d971bb558bcfabaa44e51f3b529f576ebef90a1dd8be50c` |

The bounded attached `/dev/null` smoke also passed zone counts and nesting for both cells (`sao-w1-null-sol-00`, `single-w8-null-sol-00`). The null sink does not retain bytes to hash; the runner checks exact regular-file output immediately before those smoke captures. No sink timing comparison was made.

**Negatives** (`sao-w1-negative-sol-01`): flipping bit 0 at archive offset 5,587,827 (the last block's stored checksum) produced status 1 on both CLIs, with the expected checksum diagnostic and the same 6,815,744-byte output prefix in stock, OFF and attached builds. The prefix SHA-256 was `4e9b60102328f20b435867e085ada01287dd9a630bd1b79ac5fce7564282d8c6`. C uses `-dc` rather than `-qdc` on the corrupt inputs to expose its diagnostic. The existing `spl_forged_offset.zxc` was rejected with status 1, zero plaintext, and the stock offset/history error; the C verification-enabled trace and the Rust trace each closed one checked / payload / reconstruction zone, with no hash or entropy zones. These failures are not part of success counts. Without a collector, both CLIs returned 1 after their 30-second wait with zero stdout. A separate one-second watchdog test killed and reaped a waiting C decoder. PATH's 0.15.0 capture and csvexport were rejected independently before any decode. C OFF ignored the wait env switch; ordinary Rust release rejected `--wait-for-tracy` and decoded exactly without it. The unchanged `cargo xtask tracy` also completed with 100 repeated wide-Huffman frames, producing a nonempty 900-zone trace.

## Overhead gate (not run)

If phase magnitudes are needed, use `pair overhead --case single --workers 8 --decoder-cpus 0-5,8-13 --collector-cpus 6,14 --seeds 7 21 --out "$s/overhead-local"` with affinities chosen for the host. This is an opt-in **null-sink** calibration, not certification of the regular-file captures above. Both implementations first decode the N- and 2N-frame archives to exact regular outputs in all three flavors. Then two seeded, balanced series (default N=4, 12 adjacent pairs per contrast per series) warm the inputs and use the same `/dev/null` sink for OFF/OFF, OFF/detached, and OFF/attached. Use long-minus-short wall and decoder child user+sys divided by the extra plaintext GB, not attach-to-exit versus a clean decode. The runner stores raw totals, collector rusage separately, capture logs and an order-statistic interval for the paired median change. Reject nonpositive slopes, unstable attach/shutdown or inadequate coverage; require the intervals in both series to lie within ±3% for OFF/OFF and detached and ±10% for connected, for both wall and CPU. Spawn-to-listen and collector totals are diagnostics, not handshake timestamps. If jitter cannot be bounded on this host, the corpus/sink differs, or the gate fails, keep the trace qualitative. Even a pass does not remove per-zone bias.

On a new C/Tracy pin or changed boundary, create a separate local series, audit the range diff and per-ISA/error paths, revise the map (new major for a changed boundary), then repeat exact, negative, and calibration checks. No ABI, oracle, benchmark baseline, or public fork changes are part of this slice.
