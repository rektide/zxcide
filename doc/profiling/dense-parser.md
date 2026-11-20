---
type: Measurement
title: Closing the Dense archive-size gap against C7
description: Dense match parsing and Huffman write cost on the pinned Silesia corpus.
resource: /doc/profiling/dense-parser.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-20 }
sources:
  - { resource: 'opencode://ses_e0867fb1fffeYLtmeIXhQF2Q0W', title: Compact Huffman writer, paired timings and independent corpus verification }
  - { resource: 'opencode://ses_e0867c642ffe3csRS8ZuBh5XCw', title: Dense match parser and search tradeoffs }
  - { resource: /doc/profiling/dense-huffman.sol.md, title: Post-package-merge baseline and section-level comparison against C7 }
  - { resource: /doc/profiling/scorecard/README.md, title: Pinned corpus, C build and cross-verification method }
---

# Closing the Dense archive-size gap against C7

On the two inputs in the [Huffman study](dense-huffman.sol.md), the new parser (`02e59646`) shrinks Dense archives by **20.3% on `dickens` and 9.9% on `sao`**. Both are now smaller than pinned upstream C level 7. The tradeoff is encode time: 5.6× and 2.3× the old Dense wall time, respectively.

| Input | Previous Dense bytes | New Dense bytes | C7 bytes | Previous / new / C7 encode ms |
|---|---:|---:|---:|---:|
| `dickens` | 4,679,837 | 3,729,868 | 3,745,574 | 121 / 680 / 812 |
| `sao` | 6,140,199 | 5,531,098 | 5,587,854 | 133 / 308 / 307 |

**Full Silesia:** the sum of the 12 independent archives fell from **82,530,245 to 70,459,885 bytes** (−14.6%). C7 totals **70,113,423 bytes**: the aggregate gap narrowed from 17.7% to **0.49%**. Every file shrank versus previous Dense; five are smaller than C7. The biggest remaining byte gaps are `webster` (+151,370 bytes) and `mozilla` (+129,936); the largest percentage gap is `nci` (+3.43%). The [full scorecard](scorecard/dense-full-smoke-20261120.json) includes exact byte comparisons with both decoders for all 48 archives. Its single-pass times are smoke evidence; use the [paired encode samples](scorecard/dense-paired-20261120.tsv) for the two-file timing comparison.

**Decode was the next gap.** On the same new Dense archives, the Rust/C CLIs decoded `dickens` in 30.4/7.1 ms and `sao` in 31.3/6.8 ms (seven shuffled warm-cache medians, affinity 12–15). The [subsequent decoder changes](dense-decode.md) more than doubled Rust throughput across all 12 frozen archives.

**Where the bytes went.** On `dickens`, the parser leaves 968,736 literal bytes, down from 2,443,017. C7 leaves 1,617,412 but uses fewer sequences (999,495 vs. our 1,190,757); its smaller offset stream nearly cancels our literal saving. On `sao` we now leave 4,270,734 literal bytes against C7's 4,277,930. All 20 and 14 Rust blocks still choose Huffman for both sections. The gain came from what the parser fed the entropy coder.

**How the search is bounded.** The [parser](../../crates/codec/src/encode/dense_match.rs) follows at most 24 predecessors per lookup within the 64 KiB match window. It considers shorter prefixes of the best match, charging for literals, sequences and length overflows. It keeps one best predecessor per endpoint over 32 KiB at a time, and can extend the last match across the span boundary. These are estimated costs; the final serializer still prices the exact entropy sections and can select RAW. The heads, predecessor ring and parse states together use about 896 KiB per Dense worker at capacity. Pure Split/Packed workloads never allocate them.

## Cost of emitting the same Huffman sections

The [bounded-Huffman fix](dense-huffman.sol.md) made Huffman coding viable on both streams of both study files. In six Dense encodes of `dickens` at the resulting baseline (`2096ad98`), `perf record -e cycles:u -F 997` attributed 40.9% of 723 samples to `Tree::emit`. The writer had a separately allocated list of `(run, code, width)` tuples for each symbol. It updated two destination bytes on every step, including single bits that could never cross a byte boundary.

The new writer (`e2db4d67`) keeps a compact route for each symbol. The prefix identifies single-bit runs; only the terminal run can need a multi-bit write. An inline table and its payload now share one allocation. Candidate pricing still uses each run's byte-rounded length. With the parser held fixed, **all 12 Silesia archives were byte-identical**. Median whole-CLI encode time fell from 121.5 to 109.5 ms on `dickens` and 133.7 to 116.3 ms on `sao` (seven shuffled paired samples, one warmup, affinity 12–15). This isolates the writer gain from any change in match quality.

In a scalar Wasmtime component check with the same parser held fixed, Criterion's encode interval went from 142.75–148.86 to 130.02–131.19 ms on `dickens` and 163.39–186.41 to 138.67–141.03 ms on `sao` (10 samples per binary, whole WIT call, sequential runs). These runs do not measure the cost of the new match search.

## Measurement and reproduction

Ryzen 7 5800X, Linux x86-64. Both Rust CLIs used `cargo build --locked --release -p zxc-cli`, default features and portable flags, rustc 1.101.0-nightly (`822182db4`); the previous Dense baseline is `2096ad98`. The C CLI is the [frozen scorecard binary](scorecard/README.md) at `5719a96f` (GCC 16.2, CMake native Release). CLI encode times above are medians from seven shuffled passes after a warmup, affinity 12–15, warm inputs, stdout to `/dev/null`. Process-CPU medians (old/new/C7): **121/679/815 ms** on `dickens`, **133/307/311 ms** on `sao`. C uses `-T1`, Rust is serial; system load and frequency were not isolated. Treat the `sao` elapsed times as comparable rather than a speed ranking.

Use `cargo xtask scorecard` with the pinned binaries to reproduce the archive matrix; `--full --samples 1 --warmups 0` runs the exact-output checks over all 12 files. For the focused encoder comparison use `zxc encode --mode dense FILE` for each Rust revision and `c-zxc -qzc -T1 -7 FILE` for upstream, with checksums enabled and the default 512 KiB blocks. Final serial and four-worker Rust Dense archives matched byte-for-byte on the study pair. The frozen interop gate passed 180 cross-decodes and 72 indexed range checks, plus 24 cross-decodes and 52 range checks on generated cases with each of serial and crossbeam.

For the next **encode-speed** pass, profile match selection first. `perf record -e cycles:u -F 997` on the final binary put 89% of `dickens` and 78% of `sao` self samples in `Parser::parse` (676 and 308 samples respectively, including inlined helpers). A speed comparison should hold archive sizes alongside timings; reducing the probe limit can give back some of the ratio win.
