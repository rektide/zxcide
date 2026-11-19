---
type: Measurement
title: Reusing the dictionary match-table seed
description: Stage attribution, warm and first-use encode timings, and a byte-identity check against the old parser.
resource: /doc/profiling/dictionary-encode.md
status: draft
generated: { by: 'model:openai/gpt-6.2-sol', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0ac424eeffe4kk65VvnMwl4oQ', title: Attributed encoder stages and measured and checked the cached prefix seed }
  - { resource: '/doc/profiling/ledger.md', title: Baseline traffic and workload definitions }
  - { resource: '/scripts/profile-dict-encode-stages.rs', title: Isolated attribution probe }
  - { resource: '/crates/parallel/benches/ledger.rs', title: Warm and fresh-dictionary benchmark driver }
---

# Reusing the dictionary match-table seed

**Keep `e5e13f60`.** For warmed 4 KiB dictionary encode, latency fell **90.1–90.4%** at one worker and **88.8–89.1%** at four. No control had a repeatable material regression (≥3% slowdown with separated intervals in both pairs). This is a large-dictionary, high-hit fixture, not an estimate for all archives. The change does not remove the history copy; see the separate [decode optimization](dictionary-decode.md) for that side.

## What to remove first

The [baseline ledger](ledger.md) counts logical writes, not CPU time. Run the isolated, optimized [stage probe](../../scripts/profile-dict-encode-stages.rs) to rank the three stages on the ledger's 65,535-byte dictionary:

```sh
rustc --edition 2024 -O scripts/profile-dict-encode-stages.rs -o .test-agent/dict-encode-stages
taskset -c 2 .test-agent/dict-encode-stages
```

Two runs, per-block medians in µs, reusing history and table allocations:

| Block | Copy `[prefix \| plain]` | Fill 65,536 slots | Seed prefix keys |
| --- | ---: | ---: | ---: |
| 4 KiB | 1.32–1.51 | 2.49–2.51 | 117.73–119.34 |
| 512 KiB | 13.51–13.59 | 3.25–3.52 | 117.41–126.38 |

The probe runs the three baseline operations in order, without matching or packing. Its different surrounding code and cache behavior make these magnitudes unsuitable for subtracting from a whole-call interval. The ranking is clear: repeated four-byte hashing and scattered table updates cost far more than a contiguous copy or fill on this fixture.

## Candidate and memory bound

For dictionary content of at least 16 KiB and configured blocks of at most 16 KiB, keep an immutable match-table snapshot on the `Dictionary` object. Build it lazily starting at zero-based block index 16; a fresh dictionary on shorter frames takes the ordinary path. Reuse a warmed snapshot even on the first block of later frames. Workers keep their original private mutable tables. One worker builds the snapshot directly from its scratch; subsequent workers copy the snapshot rather than reset and seed from scratch. There is no cache keyed by dictionary IDs or pointers.

The snapshot holds the last position for each hash wholly within the dictionary. Replay the three keys that straddle the boundary in position order; they must overwrite any earlier collision exactly as before. Match extension still uses contiguous history. On this host a `Dictionary` grows from **56 to 80 bytes inline**, with **256 KiB additional heap payload per warmed dictionary** (not per worker), retained until drop. The table allocation is not made when parsing a dictionary, decoding, or using an ineligible encoder path. These are object sizes, not a peak RSS measurement. Workloads that retain many warmed dictionaries pay the heap cost for each one.

The diagnostic ledger sees the warmed 4 KiB case replace a **64 B/B** table fill and 65,535 indexed positions per block with a **64 B/B** contiguous table copy and three indexed positions. `index:dictionary prefix` counts four logical destination bytes per indexed position, not a payload copy. The `[prefix | plain]` copies remain **16 + 1 B/B**. At 512 KiB, no template is used; the regular path still fills and indexes. Any timing difference there or in short cold streams is not a cache hit: the ordinary prefix loop was also factored onto a mutable slice.

## Whole-call timing

Ryzen 7 5800X, Linux 7.1.0-debplus.1, rustc 1.101.0-nightly (`822182db4`), default optimized bench settings, lockfile, no `ledger` or `tracy` features. Build the same `cargo bench --locked -p zxc-parallel --bench ledger --no-run` at baseline `c8426136` and candidate `e5e13f60`. Run A/B/B/A on CPUs `2-7` using an empty `CRITERION_HOME` for each executable invocation:

```sh
taskset -c 2-7 "$bench" --bench '^ledger/((dict|mixed|noise)/b(12|19)/w(1|4)/encode|dict-first/.*)$' \
  --sample-size 30 --warm-up-time 0.45 --measurement-time 1 --noplot
```

Intervals are Criterion 95% for the full call (slope, or mean if no slope). Warm rows encode 1 MiB checksummed, with dataset construction and initial archive encode excluded; the dictionary snapshot is warmed by that archive's construction. `w4` uses crossbeam and eight slots and includes startup and joins. `dict-first/bN` means **N blocks of 4 KiB**, not a block-size logarithm. Each cold encode gets a newly parsed dictionary and a fresh preallocated destination outside the timer. Cold timings include creation of a seed if the frame reaches the threshold.

| Case | A1 (µs) | B1 (µs) | A2 (µs) | B2 (µs) | Change 1 | Change 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `dict/b12/w1` | 20749.94–21002.34 | 2052.27–2074.90 | 21394.85–21774.93 | 2058.03–2083.50 | −90.1% | −90.4% |
| `dict/b12/w4` | 5727.26–6058.42 | 633.28–675.70 | 5837.27–6912.96 | 660.60–736.71 | −88.8% | −89.1% |
| `dict/b19/w1` | 356.79–363.63 | 305.45–311.16 | 361.27–391.67 | 296.48–306.99 | −14.4% | −19.3% |
| `dict/b19/w4` | 288.73–296.75 | 271.92–298.06 | 294.78–303.85 | 260.68–267.59 | −3.0% | −11.8% |
| `mixed/b12/w1` | 20666.33–21471.79 | 21046.20–21529.92 | 21629.13–22529.36 | 21686.02–22242.74 | +1.3% | −0.5% |
| `mixed/b12/w4` | 5715.04–5845.88 | 5811.16–5944.50 | 6216.88–7360.40 | 5816.97–6058.99 | +1.7% | −11.7% |
| `mixed/b19/w1` | 4167.92–4259.92 | 4104.29–4225.89 | 4178.65–4246.01 | 4080.60–4181.07 | −1.0% | −2.0% |
| `mixed/b19/w4` | 2665.11–2761.44 | 2477.70–2602.99 | 2892.28–3239.35 | 2415.91–2578.02 | −5.9% | −18.4% |
| `noise/b12/w1` | 2562.34–2983.72 | 2134.80–2172.16 | 2322.59–2449.02 | 2193.05–2243.47 | −21.3% | −7.1% |
| `noise/b12/w4` | 725.23–771.04 | 706.79–846.96 | 762.34–793.73 | 737.01–768.40 | +4.7% | −3.2% |
| `noise/b19/w1` | 5699.75–5815.39 | 5665.12–5815.82 | 5957.00–6096.98 | 5505.64–5602.80 | −0.3% | −7.8% |
| `noise/b19/w4` | 3376.70–3497.85 | 3312.83–3483.10 | 3626.87–3909.22 | 3319.79–3462.80 | −1.2% | −10.5% |
| `dict-first/b1/w1` | 81.84–83.23 | 49.24–49.63 | 84.32–85.86 | 50.14–51.45 | −40.1% | −40.3% |
| `dict-first/b2/w1` | 165.17–167.66 | 95.51–95.68 | 167.90–170.87 | 96.92–99.25 | −42.6% | −42.1% |
| `dict-first/b17/w1` | 1385.15–1423.83 | 877.18–909.41 | 1410.05–1434.76 | 880.34–992.14 | −36.6% | −35.2% |
| `dict-first/b32/w1` | 2639.10–2676.23 | 966.22–1034.12 | 2663.25–2712.50 | 961.08–976.39 | −62.6% | −63.9% |
| `dict-first/b32/w4` | 803.85–819.88 | 386.57–429.80 | 809.74–837.28 | 394.98–477.22 | −49.7% | −47.0% |

### First-use cost versus the refactored unseeded loop

Compare the candidate to a build of the same source with `FIRST_SEEDED_INDEX = u64::MAX` (no seed can warm in these cases). Use the same run order, 40 samples and a 1.5 s measurement target. This tests the snapshot independently of the faster ordinary path:

| Case | Disabled 1 (µs) | Seeded 1 (µs) | Disabled 2 (µs) | Seeded 2 (µs) | Change 1 | Change 2 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `dict-first/b17/w1` | 858.81–874.83 | 856.74–872.21 | 837.49–854.87 | 857.70–880.79 | −0.2% | +2.7% |
| `dict-first/b32/w1` | 1685.68–1873.97 | 974.35–994.74 | 1558.02–1573.90 | 988.65–1010.81 | −44.5% | −36.1% |
| `dict/b12/w1` | 13071.91–13228.87 | 2015.23–2053.17 | 12887.79–13151.60 | 2029.09–2061.29 | −84.5% | −84.3% |

The fresh 17-block frame builds the snapshot once and gets no reuse. It shows up to about 23 µs net first-use cost in these point estimates, not a separately timed allocation. The 32-block frame pays that setup and benefits from reusing the snapshot for subsequent blocks. The warmed improvement is mostly attributable to the snapshot, not the refactoring alone.

## Wire identity and interoperability

- Tests compare the entire latest-position table to the original ascending index loop with changing boundary bytes; compare packed, split, and dense against the ordinary path across dictionary switches (even two dictionary objects with the same ID), a no-dictionary block, and short/fast-return tails. 38 workspace nextest tests and two focused release tests passed.
- A separate release driver linked against `c8426136` and the candidate reused an `Encoder` across six checksummed, seekable frames. It switched between 65,535- and 36,013-byte xorshift dictionaries (seeds 42 and 314), no dictionary, and the original dictionary again, with varying first three bytes and dictionary-tail motifs in each block. It exercised block logs 12, 14 and 19, all three modes, and 17/44/45/63/819/4095-byte tails. The individual frames and concatenation were byte-identical; concatenated SHA-256 `be4440451a1973c0cf73d2c40753dede2bb662ba93e4b7fa2305bd452d17d575`. Four-worker crossbeam regenerated identical frame bytes with independent cold dictionaries and decoded the concatenation. Each frame passed both directions against upstream C `2992c28e6e1df07feec0cb39678a6d4a9665c6bb`.
- The ordinary C oracle passed 180 frozen cross-decodes and 72 indexed reads, 72 generated cross-decodes and 156 indexed reads (`cargo xtask interop --cases 36`), plus the three parallel dictionary recipe replays (`--parallel-backend crossbeam --case {11,23,35}`).

## Remaining tradeoffs

The encoder still copies contiguous history and copies a 256 KiB table on every warmed block. Neither requires a more complex representation on the strength of these timings alone. Measure realistic large-dictionary streams, hit rates, and active dictionary counts before broadening the eligibility policy or budgeting a denser/sparser table representation. No SIMD or 512 KiB caching was added.
