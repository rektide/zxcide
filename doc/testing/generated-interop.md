---
type: Design
title: Generated valid-input cross-decoding
description: A replayable two-implementation oracle for generated frames and indexed ranges.
resource: /doc/testing/generated-interop.md
status: draft
generated: { by: 'model:openai/gpt-6.2-astra', at: 2026-11-19 }
sources:
  - { resource: 'opencode://ses_e0b4ca850ffe4VaTbUv0wsGqWz', title: Generated interoperability gate design }
  - { resource: /xtask/src/interop.rs, title: Existing fixture oracle and pinned C build }
---

# Generated valid-input cross-decoding

The current C oracle runs many codec options on 18 fixed plaintexts. Generate **plaintext plus frame recipes**, then ask real encoders to build archives. This reaches combinations of match lengths, block boundaries, and frame settings that fixed fixtures don't cover. The expected answer always exists before either encoder runs.

## Case contract

Identify a case by `(generator revision, seed: u64, case index)`. Derive each case independently with fixed-width, specified integer arithmetic, so filtering, process order and platform don't change its bytes. A bounded schedule assigns required shapes; use randomness *within* each shape rather than hoping a random choice covers it. The default run should fit alongside today's fixture check: roughly a dozen cases, no more than a few MiB of plaintext per case, release binaries, one thread. Larger local runs use the same generator.

| Family | Behavior to exercise |
| --- | --- |
| Empty and short | Empty frame, one byte, sizes next to a footer width transition |
| Block edges | Inputs just below, exactly at and just above 4 KiB; long periodic or overlapping matches |
| Mixed and distant | Alternate noise and repeated records across blocks; place repeated bytes about 64 KiB apart **inside a larger block** |
| Seek groups | More than 64 blocks at 4 KiB, distinguishable neighboring blocks; ask for bytes straddling blocks 63 and 64 |
| Concatenation | Multiple independently encoded frames, including an empty frame; change block size and checksum policy; cross a nonempty frame boundary in a range read |
| Dictionary | Use a known vendored `.zxd` with generated related records. Respect the reference CLI's one-dictionary-per-invocation interface |

Vary Rust split/packed/dense, representative C levels (including 1 and 7), checksum and index settings across the schedule. Do not require encoders to select a particular internal representation: the existing conformance corpus owns exact wire examples. For an indexed case every frame must be seekable.

## Execution and verdict

1. Materialize the original bytes and per-frame options. Encode each frame independently with both implementations and concatenate the respective results in order.
2. Decode the **Rust-produced** archive with C and the **C-produced** archive with Rust. Compare each full output directly to concatenated plaintext. A nonzero exit or mismatch fails the case, including for empty output.
3. For indexed cases, run Rust range reads against both producers. Check exact plaintext slices: zero length at EOF, a seeded interior slice, and a slice crossing each applicable structural boundary above. Deduplicate ranges and keep their lengths small. No slice or expected offset may be derived by asking a decoder.

## CI and replay

Extend `cargo xtask interop`: by default run the existing frozen matrix and the fixed-seed generated gate. Preserve the optional executable paths. Provide `--seed N`, `--cases N` for generated coverage, and `--case I` to replay just one generated case without running the frozen matrix. Report counts separately. On failure identify generator revision, seed/index, direction, recipe, range (if any), and first differing offset or length; print the replay command. Preserve expected input and available archive bytes under a fresh directory inside Cargo's target directory. Clean successful cases. Include a test locking at least one known recipe/output fingerprint and verify replay, including a controlled failure.

Keep this under `xtask`: production codec and fuzz-target dependencies need no changes. The [existing compatibility checks](../../README.md#compatibility-checks) remain the entry point. Later a parallel Rust encoder can consume identical recipes and compare its archive bytes with serial Rust before cross-decoding.
