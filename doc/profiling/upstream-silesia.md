---
type: Measurement
title: Native CLI versus current upstream on Silesia text and binary
description: Same-archive decode, encode size and time, and first profile of the common raw-literal SPL path.
resource: /doc/profiling/upstream-silesia.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0a84f44affeaEMU9gc5TU02Bd', title: Follow-on reproducible scorecard }
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Head-to-head benchmarks and perf sample }
  - { resource: 'https://github.com/hellobertrand/zxc/tree/5719a96f778226868b29dba621a8c4e302b74d7c', title: Upstream HEAD at measurement }
  - { resource: 'https://github.com/DataCompression/corpus-collection/tree/d59143a41cc7d65063a70e86063a3c39aeae6d9f/Silesia-Corpus', title: Pinned corpus ZIP }
---

# Two real inputs change the order of work

For the runnable pinned-corpus successor and a separately measured buffer-API
comparison, see the [Silesia scorecard](scorecard/README.md). The results below
remain the original study, not measurements of later codec changes.

Ryzen 7 5800X, Linux x86-64, rustc 1.101.0-nightly, GCC 16.2.0, upstream CMake Release (native ISA), Rust `7dbfd727` release (ordinary portable flags), plus `--features zxc-codec/merge-simd`. Timed the CLIs reading a warm local file, stdout to `/dev/null`, CPU affinity 2–7, checksums enabled, default 512 KiB blocks. Rust serial, C `-T1` (one worker plus its streaming driver). Seven samples after a warmup, shuffled command order per pass. Median ms below; process startup and I/O are included. Both decoders reproduced the exact original plaintext from all eight archives.

Inputs from `Silesia-Corpus/silesia.zip` at the corpus revision above, unpacked without transformations:

| File | Plain bytes | SHA-256 |
| --- | ---: | --- |
| dickens | 10,192,446 | `b24c37886142e11d0ee687db6ab06f936207aa7f2ea1fd1d9a36763c7a507e6a` |
| sao | 7,251,944 | `c2d0ea2cc59d4c21b7fe43a71499342a00cbe530a1d5548770e91ecd6214adcc` |

## Encode

Each cell is archive bytes / median ms. Commands: C `-qzc -T1 -3 FILE` or `-7`; Rust `encode --mode split FILE` or `dense`. Modes are operating points, not equivalent algorithms.

| Input | C level 3 | Rust split | C level 7 | Rust dense |
| --- | ---: | ---: | ---: | ---: |
| dickens | 5,831,415 / 81.13 | 6,103,312 / 68.59 | 3,745,574 / 883.04 | 6,103,312 / 70.91 |
| sao | 7,247,411 / 5.66 | 6,527,281 / 49.15 | 5,587,854 / 340.61 | 6,386,860 / 132.77 |

## Decode identical input

Generate the archive with C as above, then compare C `-qdc -T1 ARCHIVE` against Rust `decode ARCHIVE`. Parentheses are the observed min–max for opt-in SIMD, not confidence intervals.

| Archive | C | Rust default | Rust SIMD |
| --- | ---: | ---: | ---: |
| dickens C3 | 5.87 | 21.96 | 21.83 (20.70–26.91) |
| dickens C7 | 6.92 | 31.28 | 29.35 (28.59–30.37) |
| sao C3 | 5.87 | 2.80 | 2.65 (2.45–2.78) |
| sao C7 | 6.30 | 20.62 | 17.55 (16.73–21.98) |

For continuity with the [1,000-frame wide fixture](README.md#first-trace-wide-huffman-decode), `test`/`-qt -T1` medians were **30.40 / 79.14 / 51.46 ms** (C/default/SIMD).

All 20 C3 dickens blocks use SPL with unencoded literals and tokens. Across 24 repeated Rust SIMD CLI decodes, `perf record -e cycles:u -F 997` took 533 samples: 58.9% self in `BlockDecoder::decode`, 18.8% in libc `memmove`, 18.5% in `Vec::extend_from_within`. This attributes the next investigation to sequence reconstruction; it does not give any one code change a guaranteed saving. C7 dickens instead Huffman-encodes both literals and tokens in every block. Rust dense encodes neither in any dickens block: inspect candidate costs, including depth-limit rejection, before choosing between changing table construction and changing match parsing. C3 sao contains eight RAW and six SPL blocks, explaining why it is a different decoder workload.
