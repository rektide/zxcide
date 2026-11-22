---
type: Guide
title: Profiling the Rust ZXC codec
description: Reproducible throughput benchmarks, direct Tracy captures, and a measured decoder optimization.
resource: /doc/profiling/README.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e04f22460ffeiMpWBTvOxfQzAN', title: Dense literal policy sweep using the same two decoders on candidate archives }
  - { resource: 'opencode://ses_e055e0d34ffe9jui9j3JSrIqMx', title: Fixed-archive timing and retention study for the retained Huffman buffers }
  - { resource: 'opencode://ses_e0867fb1fffeYLtmeIXhQF2Q0W', title: Extended default decoder timing matrix to Rust-produced archives }
  - { resource: 'opencode://ses_e0814f934ffe3jycgQ8eN3iHoB', title: Fixed-archive worker scaling implementation and acceptance }
  - { resource: /design/comparison-report/report1.astra.md, title: Worker scaling and CPU cost measurement contract }
  - { resource: 'opencode://ses_e0846a824ffeA6B7lwD85vbibK', title: Configurable pinned CLI comparison and clean-cache acceptance }
  - { resource: 'opencode://ses_e0acf0f83ffeOu7mBN3OsQ5Dc8', title: Native byte ledger, wide-Huffman WIT comparison and paired Criterion measurements }
  - { resource: 'opencode://ses_e0b145b97ffeZCgqrI2NHSkUTX', title: Rust xtask, paired native and WIT component benchmarks, Linux perf and opt-in DWARF capture verification }
  - { resource: 'https://github.com/bytecodealliance/wasmtime/blob/main/docs/examples-profiling-perf.md', title: Wasmtime perf agents and jitdump injection }
  - { resource: 'opencode://ses_e0b1ee216ffelb33FpbVf2g8SE', title: All-mode Criterion suite and named baseline comparison workflow }
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Codec measurements, interoperability and capture verification }
  - { resource: 'opencode://ses_e0b4c34d4ffeK4VV8rf1hpdri9', title: Tracy protocol compatibility investigation }
  - { resource: 'https://github.com/nagisa/rust_tracy_client', title: Rust client and bundled Tracy sources }
  - { resource: 'opencode://ses_e04ef8b0cffeJahvs500cwmoBM', title: Checked four-worker ordered-head and credit exposure on fixed archives }
  - { resource: 'opencode://ses_e04a90c09ffeJ4gVdSQ4JhIki6', title: Measured the full-corpus worker sweep and made decode throughput the default comparison view }
---

# Profile a codec change

For byte traffic, run `cargo xtask ledger --component`. The [byte-movement ledger](ledger.md) ranks individual sites and pairs each workload with its uninstrumented timing interval.

## Compare compression and fixed-archive CLI decode

The [2026-11-20 status report](silesia-status-20261120.md) compares all 12 Silesia files against pinned C and the saved Rust baseline at `1,2,3,4,6,8,12,16` decode workers, and summarizes which profiling experiments shipped.

The [persistent worker follow-up](decode-pool.md) measures current Crossbeam reuse against that earlier frame-local pipeline, including a single-long-frame control to distinguish boundary costs from sustained decode throughput.

The [Rust/C pipeline comparison](pipeline-comparison.sol.md) maps these end-to-end timings to source stages, worker lifetimes and archive section counts. Use it to choose counters before attributing a CLI gap to a codec routine. The [Huffman worker-buffer study](pivco-worker.md) applies that method to a retained decoder change and reports its per-worker memory cost as well as fixed-archive timings. The [Dense literal savings sweep](dense-literal-savings.sol.md) instead varies the encoder's archives while holding both decoders fixed.

```sh
cargo xtask comparison-report                           # all 12 files; C levels 1,3,7; workers 1,2,3,4,6,8,12,16
cargo xtask comparison-report --quick --decode-workers 1 --before PATH/TO/RUN
cargo xtask comparison-report --files sao,mozilla --levels 1,7 --decode-workers 1,4,8 --concat
```

`--quick` selects dickens and sao without reducing the sample count. Alternatively, `--files` takes a distinct comma-separated subset of the twelve Silesia members, in manifest order regardless of argument order. `--levels` takes distinct upstream levels from 1–7; `--decode-workers` takes distinct counts from 1–256. **Both decoders read identical archive bytes at each worker count.** Decode tables report pinned upstream archives first, then the current Rust Split and Dense archives (`current.split`, `current.dense`). Each speed ratio compares decoders on the same source. Encoding remains serial for Rust and `-T1` for C; each selected C level and each Rust mode is encoded once per file/pass. Split pairs with C3 and Dense with C7 for encode comparisons when selected; these are reference points, not numeric equivalents.

Parallel decode uses C `-TN` (which may use an additional driver thread) and Rust `decode --threads N` (current CLI defaults to Crossbeam with `min(2*N, 256)` slots). One worker uses the original CLI arguments and Rust's serial path. With `--before`, supply a trusted **completed** run directory (schema 1–3). The runner checks `COMPLETE`, validates `raw.json`, and imports only the hash-checked `bin/current` and its build identity. All three decoders also read the saved build's Split and Dense archives, generated in this run. If the saved CLI rejects the worker flags or fails to decode all concatenated frames, try `--decode-workers 1` or omit `--concat` as appropriate.

First use on native Linux needs Cargo/Rust, Python 3.11+ (stdlib), Git, CMake 3.21+, Make, a native C compiler and network. The runner downloads the pinned Silesia ZIP, checks the digest and member set, fetches the exact [performance reference](https://github.com/hellobertrand/zxc/tree/5719a96f778226868b29dba621a8c4e302b74d7c), and builds it in native Release mode. This pin is separate from `cargo xtask interop`. Warm runs recheck source, ZIP, plaintext and [archive identities](scorecard/upstream-level-hashes.json). Rust uses release, default features and empty rustflags; both builds keep default block size and checksums. Before timing, every CLI cross-decodes each staged archive at one worker, and every scheduled decoder/worker/input combination must produce exact plaintext through EOF and exit successfully. Timed launches discard stdout and require successful exit; they do not compare plaintext again.

`--concat` adds one decode launch per archive source, decoder and worker count by appending that source's frames in manifest order (at least two files). It does not combine plaintext for encoding, nor replace separate-file jobs. Comparing *N file launches* with *1 concatenated launch* uses the same byte totals, but the difference includes process/pool setup, input opens, cache state, frame boundaries and scheduler effects. It does not isolate startup cost or demonstrate an ordering bottleneck. The runner shuffles jobs, not frame order.

The runner prints the matrix size and CPU budget before acquiring inputs. The default is 1,020 jobs per pass, or 8,160 launches including warmup, plus verification. To iterate faster, narrow `--files`, `--levels`, or `--decode-workers` (e.g. `1,4,8`). Worker settings are not a count of available cores: the runner reports inherited logical CPU affinity and the tightest discoverable cgroup CPU quota, warning `CONSTRAINED` if either is below the largest requested count. Neither the affinity nor the worker count changes automatically. Use a quiet host; an affinity mask can be set with `taskset`, and logical CPUs may be SMT siblings.

### Read or export the results

| Output | Use it for |
| --- | --- |
| Terminal | Side-by-side Rust and C decode throughput, one row per worker count, on the highest selected C level's archives. Separate files and concat are shown side by side when both are measured. |
| `report.md` | Decode tables for all measured archive sources and worker counts, then the aggregate and **per-file** decode detail, including CPU, timing ranges and saved Rust comparisons. Also in `run.log`. |
| `--format encode` | Optional encode and archive-size Markdown report, regenerated from `raw.json`. |
| `summary.tsv` | Spreadsheet or script input: one row per measured configuration and scope, with numeric size and timing fields. |
| `raw.json` | Full evidence: archive hashes, binary/build identities, exact output verification, timing order and individual samples. |

Size Δ is Rust archive bytes minus reference archive bytes; negative is smaller, and percentages divide by the reference size. A zero size delta does not certify identical bytes (check SHA-256). Throughput uses decimal MB of **plaintext** per second; speed ratios over 1 favor the candidate. Wall time includes CLI startup, file input and output to `/dev/null`. The separate-file aggregate sums matching samples within each pass before taking the median. CPU cost is the independently medianed child user+system CPU time per decimal plaintext GB (not wall × workers). Min–max is observed wall spread, not a confidence interval. See the [worker scaling design](../../design/comparison-report/report1.astra.md) for arithmetic, or the [separate scorecard study](scorecard/README.md) for preloaded buffer API timings.

**TSV contract:** UTF-8 with a header, decimal numeric cells without display rounding or thousands separators, empty cells for unavailable or inapplicable values. The row key is `scope`, `file`, `kind`, `layout`, `point`, `implementation`, `workers`. `scope` is `aggregate` (empty `file`) or `file`; `layout` is `separate` or `concat`; `implementation` is `c`, `current` or `before`. For encode, `point` names a mode or C level; for decode, it names the fixed archive source. `archive_pct_of_plain` is `100 * archive_bytes / plain_bytes`. The normalized encode rows use C3 for Split and C7 for Dense when present (`c_reference_point`); compare other levels by joining encode rows on the same scope and file. Decode compares only the same source and worker count across implementations; it has no size delta. `speed_vs_before` compares current to saved Rust when present; `speed_vs_one_worker` applies only to decode with worker 1 measured. `wall_*_ns` are integer nanoseconds and `cpu_median_us` is integer child CPU microseconds.

Each completed `<Cargo target>/comparison-report/runs/<id>/` contains these outputs and the saved executables. Copy the directory to keep it across `cargo clean`. Re-render any supported evidence schema without running either codec; schema 1 omits CPU measurements:

```sh
python3 -B xtask/comparison_tables.py PATH/TO/RUN/raw.json
python3 -B xtask/comparison_tables.py --format tsv PATH/TO/RUN/raw.json > summary.tsv
python3 -B xtask/comparison_tables.py --format encode PATH/TO/RUN/raw.json > encode.md
```

Failures leave `incomplete-*/run.log`, never a published report. If a cached checkout or ZIP is corrupt, inspect the named path and move it aside before retrying.

## Compare parallel transports

```sh
ZXC_BENCH_THREADS=4 ZXC_BENCH_SLOTS=8 cargo bench --locked -p zxc-parallel \
  --features flume,kanal,crossfire,rayon --bench transports
```

Each backend processes the same 4 MiB input alternating repetitive and noisy 4 KiB spans, at 4 KiB and 512 KiB block sizes. The harness checks encoded archive identity and decoded bytes before starting timers. Timings include stream and worker setup, input reads and output copies into reused destination capacity. Compare encode and decode separately. For longer-lived workloads include larger files before selecting a default backend. `--features` here deliberately omits Tracy; enable `tracy` on the CLI to inspect where workers or ordered commits wait.

Initial Criterion quick run on the Ryzen 7 5800X: at 4 KiB blocks, crossbeam with four workers and eight slots encoded the input in 2.08 ms versus 6.59 ms serial. Decode took 605 µs versus 358 µs serial. At 512 KiB blocks, encode took 3.82 ms versus 13.34 ms serial, and decode took 257 µs versus 390 µs serial. All five backends passed the generated C oracle. These small synthetic runs support keeping serial as the default and exposing parallelism as a workload choice; rank transports only after measuring real corpora, wait times and peak memory.

The [four-worker wait study](decode-waits.md) holds the Rust decoder and four real archives fixed. Its scratch-only probe distinguishes a coordinator waiting for an ordered result from workers actually running out of jobs while unread input remains.

Use Criterion for speed and ratio comparisons; use Tracy to find which phase is worth changing. Production builds omit Tracy, including the dependency. [Architecture](../architecture/README.md) explains the instrumentation boundaries.

## Measure throughput

```sh
cargo bench --locked --bench codec
# Compare a real corpus with the same block size and mode each time:
ZXC_BENCH_INPUT=/path/to/silesia/dickens ZXC_BENCH_BLOCK_LOG=19 ZXC_BENCH_MODE=dense \
  cargo bench --locked --bench codec -- dickens
```

The default workloads are 1 MiB of repetitive corpus text, 1 MiB of deterministic xorshift noise, the 64 KiB mixed conformance plaintext, and a separate upstream level‑7 PivCo decode. Inputs are in memory and verified before timing. The benchmark reuses contexts and destination capacity but includes frame setup, section scratch, checksums when present, and writing the actual output bytes. Throughput is measured in **uncompressed** bytes; source/archive byte counts are printed separately. For results worth retaining, record corpus SHA-256, CPU, toolchain, revision, options and Criterion intervals. Compare a fixed workload with Tracy disabled. Do not treat the highly repetitive fixture as typical text performance.

A baseline on the 5800X below (30 samples, checksums on for `split`, 512 KiB blocks):

| Input | Archive bytes | Encode | Decode |
| --- | ---: | ---: | ---: |
| Repeated text, 1 MiB | 788 | 5.61 GiB/s | 12.86 GiB/s |
| Noise, 1 MiB | 1,048,639 | 177 MiB/s | 12.26 GiB/s |
| Upstream wide Huffman, 16 KiB | reference fixture | — | 132 MiB/s |

## Compare a change

On a quiet machine, save a baseline before editing, then compare against it in the same target directory. For example, to tune dense encoding on mixed input:

```sh
cargo nextest run --locked --workspace
export ZXC_BENCH_MODE=dense
cargo bench --locked --bench codec -- mixed/Dense/b19/encode --save-baseline before
# Apply the change, run the correctness tests again, then:
cargo bench --locked --bench codec -- mixed/Dense/b19/encode --baseline before
```

Criterion reports the estimated change and its confidence interval. Repeat the comparison on unchanged code to see how much the machine varies. Keep CPU power settings, build flags, and corpus fixed; check archive size alongside throughput so a speedup from writing a larger archive is visible. For decode-only changes the fixed upstream archive also isolates decoder speed from changes in encoder output.

For a full sweep, set `ZXC_BENCH_MODE=all` and omit the filter in both commands. For a block-size experiment set `ZXC_BENCH_BLOCK_LOG` (12–21); the size is part of the benchmark ID. CI uses Criterion's `--test` mode to exercise every case once. Use a quiet, consistent host for timing decisions; nextest's parallel test durations are for diagnosing slow tests.

## Compare native and component

Use the same unprefixed case ID to measure three API paths sequentially. The `upstream-l7/decode` fixture holds compressed bytes fixed across builds. For encode, select a corpus, mode and block size, e.g. `ZXC_BENCH_MODE=dense cargo xtask compare mixed/Dense/b19/encode`.

```sh
cargo xtask compare upstream-l7/decode -- --save-baseline before
# After a codec change, compare each path to its own saved result:
cargo xtask compare upstream-l7/decode -- --baseline before
```

The bare ID measures native Rust with reused workspace and output capacity. `oneshot/` measures a direct native call with fresh workspace and output, matching the guest's allocation policy. `component/` adds the WIT byte-list transfers and uses Wasmtime's compiled guest code. Compare the latter two for the end-to-end cost of choosing the component API. All use plaintext bytes for throughput. On a Ryzen 7 5800X with Wasmtime 46.0.3, the 16 KiB upstream decode measured 132.1, 132.5 and 113.5 MiB/s respectively (100 samples each). `cargo xtask bench native` runs just the native suite and needs no Wasm tools.

## Benchmark the component

Install the [component build prerequisites](../../README.md#webassembly-component) first. The wrapper rebuilds the release component and runs Criterion on the host using Wasmtime's typed bindings to the shipped WIT interface. For example:

```sh
ZXC_BENCH_MODE=all cargo xtask bench component -- --test
export ZXC_BENCH_MODE=dense ZXC_BENCH_BLOCK_LOG=19
cargo xtask bench component -- component/mixed/Dense/b19/encode --save-baseline wasm-before
# Change the codec or guest build flags, then repeat with the same workload:
cargo xtask bench component -- component/mixed/Dense/b19/encode --baseline wasm-before
```

The component and native suites use the same inputs (including `ZXC_BENCH_INPUT`), mode selection and uncompressed-byte throughput. Each component case verifies the output before measurement. The timer covers one call on an existing instance, including byte-list transfers, per-call codec workspaces, and output allocation and disposal. Component loading, JIT compilation, instantiation, and preparing the decode archive are outside the loop. The native suite reuses workspaces and buffers, so the difference between suites is not solely the cost of WebAssembly execution.

Keep the host runtime and input fixed while tuning the guest. For example, set `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS='-C target-feature=+simd128'` for a comparison on SIMD-capable runtimes. Record flags, component size, archive size, and component SHA-256 with the Criterion result. To measure an already distributed artifact, set `ZXC_COMPONENT` to its **absolute path** and run `cargo bench --locked -p zxc-component --features host-bench --bench component` directly.

### Locate component hotspots with Linux perf

Use the [Rust xtask](../../xtask/src/main.rs) to sample one exact Criterion case on either backend. Both require Linux `perf` and access to user-space cycle counters (`perf stat -e cycles:u -- true`); the component also needs `perf inject --jit`. The native capture uses `perf.data` directly. Both commands print the report path.

```sh
cargo xtask perf native upstream-l7/decode
cargo xtask perf component upstream-l7/decode
ZXC_BENCH_MODE=dense cargo xtask perf component mixed/Dense/b19/encode --seconds 10
# In the upstream decode component capture directory:
perf report --stdio --percent-limit 1 -i perf.jit.data
perf annotate --stdio -i perf.jit.data --symbol '<zxc_codec::entropy::Tree>::decode'
```

Following [Wasmtime's perf guide](https://github.com/bytecodealliance/wasmtime/blob/main/docs/examples-profiling-perf.md), the helper enables `ProfilingStrategy::JitDump` in the Rust host (`host-perf` enables `wasmtime/profiling` in 46.0.3). It builds first, then runs the benchmark executable under `perf record -k mono -e cycles:u` for a Criterion `--profile-time` loop. Startup, JIT and correctness checks are also sampled; increase `--seconds` when those obscure the loop. `perf inject --jit` resolves guest code addresses into named functions for reporting and assembly annotation. Keep `perf.jit.data` and the generated `jitted-*.so` files in their capture directory. Use ordinary Criterion baselines to measure whether a change helps.

**First captures:** On a Ryzen 7 5800X (Wasmtime 46.0.3, perf 7.2.11, five seconds per case), `component/upstream-l7/decode` put **83.3% of sampled user cycles in `Tree::decode`** and 7.3% in `Tree::new`. For that wide-Huffman fixture, inspect the [decode loop](../../crates/codec/src/entropy.rs) first. Dense mixed encode had a different profile: `__multi3` (15.9%), `Tree::new` (14.4%) and heap `pop` (12.1%). Those are self costs; this capture does not identify callers of `__multi3`.

For call stacks and source lines, add `--dwarf`:

```sh
cargo xtask perf native upstream-l7/decode --dwarf
cargo xtask perf component upstream-l7/decode --dwarf
```

This uses the optimized `profiling` profile with line tables and asks `perf` to record DWARF call stacks. The component path also requests JIT debug info from Wasmtime and puts the debug component under `target/profiling/component/`, separate from the shipping bundle. In a native capture, `perf report --stdio --no-children -g none -s srcline,symbol` resolved individual `entropy.rs` lines. With Wasmtime 46.0.3 the guest contained `.debug_line`, but the injected perf report showed guest function names and instruction offsets rather than Rust lines. Use the default capture for lighter sampling.

## Capture phases

`tracy-client 0.19.0` and the locked `tracy-client-sys 0.30.1` use **Tracy 0.14.1 (protocol 82)**. Use the [matching capture tool](https://github.com/wolfpld/tracy/releases/tag/v0.14.1); 0.15.0's live protocol is incompatible. The build enables on-demand, localhost-only spans with static labels and no call stacks. Building this feature needs a C++ compiler. The profiling profile retains line tables.

For the repeatable wide-Huffman workload, run `cargo xtask tracy --capture /path/to/tracy-capture`. This builds the profiled CLI, writes 1,000 concatenated reference frames, attaches the capture process and prints the trace path. `repeated.zxc` and `decode.tracy` stay together under `target/tracy-*/`. Override `--repeats` or `--port` when needed. Use `tracy-csvexport` on the trace to compare phases.

For an arbitrary archive or encoder workload, run the client and capture manually:

```sh
cargo build --locked --profile profiling -p zxc-cli --features tracy
# Terminal A: wait for attachment before running even a tiny job.
TRACY_PORT=18086 target/profiling/zxc --wait-for-tracy test archive.zxc
# Terminal B (within 30 seconds):
tracy-capture -a 127.0.0.1 -p 18086 -o decode.tracy
tracy-csvexport decode.tracy > zones.csv
```

Use `encode --mode dense input -o new.zxc` in place of `test` for match finding, packing and entropy pricing. The frame zone includes I/O; use block and nested codec zones to separate it. Nested zone totals overlap. To sample native code without Tracy, build with `cargo build --profile profiling -p zxc-cli` and use `perf record -e cycles:u --call-graph dwarf -- target/profiling/zxc test archive.zxc`.

## First trace: wide Huffman decode

On a Ryzen 7 5800X, Linux x86‑64, rustc 1.101.0-nightly (`822182db4`), a capture of 1,000 concatenated `spl_pivco_wide_l7.zxc` fixtures showed **574 ms of 598 ms** of frame time in PivCo. The initial scalar loop traversed the tree for every symbol. Switching to bottom-up list merging removed that traversal without unsafe or SIMD: the same in-memory Criterion quick test went from **29.7 to 134.6 MiB/s**. The follow-up capture put 140 ms in PivCo out of 163 ms of frame time. This result concerns the pinned 16 KiB wide-Huffman fixture, not all archives.

To recreate the CLI comparison (release binaries, no attached profiler):

```sh
# Use repeated.zxc from the capture directory printed by `cargo xtask tracy`.
hyperfine --warmup 2 --runs 5 -N 'target/release/zxc test /path/to/repeated.zxc' '/path/to/c-zxc -qt -T1 /path/to/repeated.zxc'
```

After the merge change, the Rust CLI took **122 ± 2 ms**, versus **31 ± 1 ms** for the pinned upstream native C release build. Both read the same archive and discard output. This end-to-end gap includes frame setup and I/O; the trace points to entropy decoding as the next place to look on this workload.

**Explicit SIMD comparison (same fixture, Ryzen 7 5800X):** rebuild the pinned upstream checkout in a second CMake build directory with `-DZXC_DISABLE_SIMD=ON`, keeping the Release settings, and compare both C CLIs (`-qt -T1`) with the Rust release CLI (`test`). Across nine warm-cache runs on the 1,000-frame input: upstream default **28.6 ± 0.8 ms**, upstream explicit SIMD disabled **82.8 ± 9.5 ms**, Rust **123.3 ± 3.2 ms**. Disabling explicit kernels does not disable compiler autovectorization. This measures an end-to-end effect of upstream's kernel selection, not the speedup a Rust port of any individual loop will obtain.
