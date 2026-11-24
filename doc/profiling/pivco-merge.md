---
type: Measurement
title: Scalar and SIMD PivCo bitmap gathers
description: Cursor, mask-table, and safe dynamic byte shuffle with the same merge buffer layout.
resource: /doc/profiling/pivco-merge.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Three release component variants and paired relaxed swizzle comparison }
  - { resource: 'opencode://ses_e0ac424eeffe4kk65VvnMwl4oQ', title: Implemented and measured bitmap gather candidates and bounded-load differential coverage }
  - { resource: '/doc/profiling/pivco-scratch.md', title: Paired workloads and retained copy-back layout }
  - { resource: 'https://docs.rs/fearless_simd/1.2.0/fearless_simd/', title: Runtime dispatch and safe byte shuffle API exercised by the prototype }
---

# Scalar and SIMD PivCo bitmap gathers

**Keep the mask table as the native default and the shuffle as `merge-simd` opt-in. Keep the original cursor in the default WIT component.** The native scalar table cleared the serial gate; the shuffle adds a material win on the measured host and in an explicitly built SIMD component. The unflagged table trial failed the whole-call WIT RAW control even though RAW bypasses the kernel. No release build flag or WIT interface changed.

The [retained layout](pivco-scratch.md) writes every mixed bitmap's gathered bytes into scratch and copies them back. All candidates keep that layout, the same wire parsing and partial-byte rules, and the same output bounds. They replace only the bitmap gather, not flat-subtree decode, LZ reconstruction, or encoder code.

The reference uses a scalar cursor for each child. The mask-table candidate looks up eight selectors and the left child's advance from each bitmap byte. It combines eight readable bytes from each child, then selects the eight outputs with scalar byte lookups. The optional `fearless_simd` 1.2.0 candidate uses the same table, but selects the bytes with `u8x16` dynamic swizzle. The whole-call prototypes take the cursor path for runs shorter than 32 bytes or when either side has fewer than eight bytes initially. The inner loop checks for a full eight-byte chunk from **each child**, not merely the parent buffer; the cursor handles the remainder and ignores unused high bits of the final bitmap byte. SIMD detection and dispatch are outside the inner eight-symbol loop; baseline x86 SSE2 and fallback use scalar.

## Method

Use the [same native ledger and component cases](pivco-scratch.md#method) as the scratch experiment: pinned upstream wide Huffman, 4 KiB dense mixed, the 512 KiB light-entropy mixed control, and 4 KiB RAW noise. Native timings are serial. WIT uses one saved native host executable and swaps validated components; archive generation and Wasmtime instantiation are outside timing. The CPU is a Ryzen 7 5800X, rustc 1.101.0-nightly (`822182db4`), Linux 7.1.0-debplus.1, Wasmtime library 46.0.3; no platform extrapolation. All builds are optimized defaults apart from the specific experimental feature and target flags. No ledger or Tracy counters are present.

Use `cargo bench --locked -p zxc-parallel --bench ledger --no-run` at baseline `0635580d` and trial `561fe398` for the scalar table build. To enable the native shuffle add `--features zxc-codec/merge-simd`. For WIT use the same `cargo bench --locked -p zxc-component --features host-bench --bench component --no-run` host executable for all guests. Save the scalar components with `cargo xtask component`; at the trial revision build table and shuffle guests also with an explicit `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS='-C target-feature=+simd128'` via `cargo build --locked --release -p zxc-component --target wasm32-unknown-unknown` (shuffle adds `--features zxc-codec/merge-simd`), followed by `wasm-tools component new` and `wasm-tools validate`. Point `ZXC_COMPONENT` at each saved artifact. The `+simd128` builds are separate artifacts and not replacements for the default component. After this experiment, an opt-in shuffle guest still requires both the feature and the explicit target flag.

Use separate `CRITERION_HOME` directories, `taskset -c 2-7`, 30 samples, 0.45 s warmup and 1 s target per case; run each candidate twice with the candidate order reversed in the second pass. Use these filters:

```text
^ledger/(wide/w1|(mixed/b(12|19)|noise/b12)/w1)/decode$
^component/(upstream-l7|ledger/(mixed/b(12|19)|noise/b12))/decode$
```

The kernel benchmark is `cargo bench --locked -p zxcide --features merge-simd --bench pivco_merge`. It uses fixed bounded slices and four repeating balanced or skewed masks across a size sweep; its crossover is not universal across bitmap distributions. Whole-call timings decide adoption. Gate: a repeatable ≥5% serial reduction on both entropy-heavy native cases, with no repeatable control or WIT slowdown of ≥3% with separated intervals. Prefer a scalar candidate that clears the bar unless SIMD adds a material whole-call benefit.

## Whole-call results

Criterion 95% intervals in µs (slope, or mean when there is no slope). Each cell shows the two runs separated by `/`; percentages are changes in point estimates. Native shuffle changes refer to the original cursor Native column, not to the table. Both WIT shuffle and the opt-in table control use the explicit `+simd128` build flag; the two other WIT variants do not. The unflagged components do not require SIMD instructions.

| Native serial decode | Cursor | Scalar table | SIMD shuffle | Table change | Shuffle change |
| --- | ---: | ---: | ---: | ---: | ---: |
| Wide Huffman | 118.00–120.07 / 116.83–120.04 | 72.97–74.20 / 71.82–73.53 | 48.04–51.37 / 48.89–50.24 | −38.2% / −38.7% | −58.5% / −58.2% |
| Dense mixed, 4 KiB | 6363.99–6506.74 / 6300.88–6441.90 | 5395.34–5562.70 / 5405.41–5559.40 | 4907.06–5023.60 / 5058.33–5424.96 | −14.9% / −14.0% | −22.9% / −18.0% |
| Dense mixed, 512 KiB | 117.78–119.97 / 116.76–120.62 | 115.86–118.44 / 116.38–119.54 | 114.73–117.91 / 116.03–119.70 | −1.5% / −0.6% | −2.4% / −0.7% |
| RAW noise, 4 KiB | 78.21–79.33 / 79.06–80.01 | 76.97–78.77 / 77.65–79.36 | 79.93–83.42 / 78.19–79.43 | −1.4% / −1.2% | +3.5% / −0.9% |

| WIT decode | Cursor (default ISA) | Table (default ISA) | Table (`+simd128`) | Shuffle (`+simd128`) | Shuffle vs same-ISA table |
| --- | ---: | ---: | ---: | ---: | ---: |
| Wide Huffman | 143.76–146.90 / 137.80–146.49 | 96.27–99.04 / 94.99–97.52 | 97.91–99.77 / 98.49–101.48 | 62.08–63.66 / 62.48–64.67 | −36.4% / −36.6% |
| Dense mixed, 4 KiB | 8029.25–8599.29 / 7959.44–8283.72 | 8461.04–9339.38 / 8069.43–8168.27 | 8315.98–8496.00 / 8255.16–8458.43 | 7376.32–7468.55 / 7504.99–7716.84 | −11.7% / −8.9% |
| Dense mixed, 512 KiB | 523.65–535.20 / 514.67–553.81 | 509.31–534.83 / 518.04–580.25 | 517.79–571.10 / 519.86–538.74 | 521.77–538.15 / 527.20–556.60 | −2.3% / +2.0% |
| RAW noise, 4 KiB | 490.75–503.51 / 480.24–486.60 | 493.67–508.02 / 498.10–515.71 | 487.21–505.34 / 494.11–506.11 | 487.37–501.84 / 480.21–486.88 | −0.5% / −3.4% |

In native serial, the shuffle adds about 32% less wide latency and 5–9% less 4 KiB mixed latency versus the scalar table. Through WIT, the unflagged scalar table accelerates wide by 32–33%. In a dedicated unflagged table/baseline recheck (40 samples, 2 s), 4 KiB mixed was +2.6% and +0.6%, but 4 KiB RAW was **+3.7% and +7.0%**, with separated intervals in both pairs: 483.71–497.35 → 503.29–514.31 µs and 489.43–497.67 → 522.72–533.53 µs. This does not assign the RAW slowdown to the gather itself: RAW never calls it. It rules out the combined unflagged build as an adoption candidate. Neither the native scalar nor the explicit SIMD path showed a repeatable material control regression.

Component byte sizes are 103,571 (cursor), 109,518 (table), 110,195 (`+simd128` table), and 110,094 (`+simd128` shuffle). The shuffle build contains `i8x16.swizzle` as confirmed by `wasm-tools print`; the final default passes `wasm-tools validate --features=-simd`, while the explicit SIMD artifact requires SIMD.

The restored default cursor component is 103,569 bytes. A final paired check (40 samples, 1.4 s) found no repeatable material regression against the original cursor: wide +3.9% / +0.4%, 4 KiB mixed −1.4% / +4.7%, 512 KiB mixed −1.8% / +1.2%, and RAW −0.9% / −2.0%. The restored component need not be byte-identical to the old executable; it retains the same algorithm and ISA, not a binary reproducibility promise.

## Kernel crossover and verification

Representative kernel intervals in ns per run, 20 samples with a 0.32 s measurement target:

| Bitmap mix | Output bytes | Cursor | Table | Shuffle |
| --- | ---: | ---: | ---: | ---: |
| Balanced | 8 | 13.6–13.8 | 15.1–15.3 | 18.3–18.9 |
| Balanced | 16 | 24.6–24.7 | 20.9–21.1 | 20.5–23.0 |
| Balanced | 32 | 45.7–47.2 | 29.0–29.3 | 22.4–23.4 |
| Balanced | 256 | 332.8–362.8 | 150.8–152.7 | 59.5–66.9 |
| Skewed | 32 | 50.7–52.2 | 55.7–56.1 | 69.8–70.2 |
| Skewed | 256 | 499.9–501.3 | 485.4–485.9 | 489.9–492.0 |
| Skewed | 1024 | 2007.6–2008.5 | 911.7–943.3 | 645.3–649.0 |

The direct benchmark includes dispatch in the shuffle timing but not the 32-byte selector. Balanced runs first win at 16 bytes versus cursor; shuffle clearly leads both at 24. The skewed masks select relatively few bytes from one side, so there is little or no fast-path work on short runs and the first clear shuffle win over both in this sweep is at 1024. Keep the tested 32-byte conservative gate and the two child-length checks; do not infer a universal crossover from either pattern.

Differential tests exercise all 256 bitmap values on the fast path and the same masks with exact-size allocations, partial groups, empty sides, and skewed runs. Original entropy tests also check malformed and truncated wire inputs. Both default and opt-in feature matrices passed 41 nextest tests; the opt-in release entropy suite passed three and codec Clippy passed. The pinned C oracle passed 180 frozen and 72 generated cross-decodes and 228 indexed reads in each native build. The WIT harness verified exact plaintext for all cases and rejects the wide archive's undersized output limit. Results characterize this host and workload set only; there are no ARM, other x86 CPU, or other Wasm runtime performance results.

## Strict vs relaxed Wasm components

`cargo xtask component --all` builds three artifacts with identical exported WIT. The scalar artifact validates with SIMD disabled. The strict artifact validates with relaxed SIMD disabled and contains `i8x16.swizzle`; the relaxed one contains `i8x16.relaxed_swizzle`. Sizes in this cut: 103,571, 110,093 and 110,107 bytes respectively. Both SIMD variants use the same `fearless_simd` gather and buffer layout. The mask table uses only indices 0–15, so the relaxed swizzle's latitude for out-of-range indices cannot change which bytes this gather selects.

Reproduce the comparison with a different Criterion results directory per variant and pass. Run a second pass in reverse order. The WIT harness checks plaintext and output limits, with component instantiation excluded from the timer.

```sh
for variant in scalar simd128 relaxed-simd; do
  CRITERION_HOME="$PWD/target/wasm-isa/run1/$variant" taskset -c 2-7 \
    cargo xtask bench component --variant "$variant" -- \
    '^component/(upstream-l7|ledger/(mixed/b12|noise/b12))/decode$' \
    --sample-size 30 --measurement-time 1.2 --warm-up-time 0.5
done
```

Wasmtime 46.0.3 on the Ryzen 7 5800X; Criterion point estimates per WIT decode call, pass 1 / reverse-order pass 2. All three guests passed the all-mode WIT smoke harness.

| Workload | Scalar | SIMD128 | Relaxed SIMD |
| --- | ---: | ---: | ---: |
| Wide Huffman | 140.7 / 140.2 µs | 63.1 / 61.6 µs | 64.3 / 66.4 µs |
| Dense mixed, 4 KiB blocks | 8.14 / 8.20 ms | 7.48 / 7.52 ms | 7.64 / 7.68 ms |
| RAW control, 4 KiB blocks | 493 / 512 µs | 503 / 493 µs | 493 / 538 µs |

**Prefer SIMD128 where supported.** It took less than half the scalar time on wide Huffman. Relaxed SIMD did not beat it on either entropy workload in either pass; RAW point estimates varied and that path bypasses the shuffle. Re-test on the deployment engine and CPU before preferring the relaxed variant.
