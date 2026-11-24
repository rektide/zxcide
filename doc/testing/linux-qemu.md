---
type: Guide
title: Linux codec correctness under QEMU
description: Run the ARM64 NEON and RISC-V portable codec tests as Linux target binaries.
resource: /doc/testing/linux-qemu.md
status: active
generated: { by: 'model:openai/gpt-6.2-sol#high', at: 2026-11-23 }
sources:
  - { resource: 'opencode://ses_df81fbff2ffekImRUlwIuD1pCH', title: Linux QEMU harness and target validation }
  - { resource: 'opencode://ses_df0cead5cffe4OzCqZ1UFJVHJJ', title: 'Why the QEMU job pins Ubuntu 24.04' }
  - { resource: /crates/codec/tests/conformance.rs, title: Frozen archive plaintext and rejection tests }
  - { resource: /crates/codec/tests/streaming.rs, title: Streaming plaintext and checksum tests }
---

# Linux codec correctness under QEMU

The [Codec checks workflow](../../.github/workflows/check.yml) cross-builds the codec tests and runs the Linux ELF binaries under QEMU user-mode. AArch64 uses a `cortex-a57` with NEON; RISC-V uses the `riscv64gc` target with `v=false` on the emulated CPU, so the SIMD-enabled build exercises the portable dispatch. Don't override the target CPU or enable RVV in `RUSTFLAGS` for this check.

The workflow pins this job to `ubuntu-24.04`. On Ubuntu 26.04 the riscv64 runtime from `libc6-riscv64-cross` 2.43 ships a dynamic loader that executes RVV instructions on its startup path (a `vsetivli` at offset `0x17b50` of `ld-linux-riscv64-lp64d.so.1`), so under `v=false` the process dies with SIGILL before the first test runs, whatever the Rust build contains. The 24.04 loader (glibc 2.39) has no vector code. Keep the CPU without V rather than enabling it to work around a newer loader: the portable dispatch on a CPU without V is what this check exists to exercise.

On Ubuntu or Debian, install the cross compiler and libc sysroot for each target you want to test. From the repository root, using a current Rust toolchain:

```sh
sudo apt-get update
sudo apt-get install qemu-user gcc-aarch64-linux-gnu libc6-dev-arm64-cross gcc-riscv64-linux-gnu libc6-dev-riscv64-cross
rustup target add aarch64-unknown-linux-gnu riscv64gc-unknown-linux-gnu
scripts/test-linux-qemu.sh aarch64-unknown-linux-gnu
scripts/test-linux-qemu.sh riscv64gc-unknown-linux-gnu
```

If you already have cross toolchains in a container or a user-owned directory, no host-wide install is needed. Put the cross GCC, binutils and QEMU on `PATH`. To use a different linker driver or wrapper set `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER` or `CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER`. If needed, use `CC_<target>` and `AR_<target>` for C test dependencies as well (e.g. `CC_aarch64_unknown_linux_gnu`). Set `QEMU_LD_PREFIX` per invocation to the matching target libc directory (the parent of `lib/`), if not using `/usr/aarch64-linux-gnu` or `/usr/riscv64-linux-gnu`.

Each run verifies the vendored fixture hashes and runs `zxcide` unit and integration tests in locked release builds: first with `merge-simd` (the release CLI's default codec feature), then without it. The tests compare decoded bytes to the reference plaintext, reject malformed frames including bad checksums, and check streaming and entropy primitives. This check covers Linux/glibc codec correctness, not CLI behavior on other operating systems or performance on real hardware.
