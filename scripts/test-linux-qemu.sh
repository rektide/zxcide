#!/usr/bin/env bash
set -euo pipefail

case "${1:-}" in
    aarch64-unknown-linux-gnu)
        triplet=aarch64-linux-gnu
        : "${CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER:=$triplet-gcc}"
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER
        export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER='qemu-aarch64 -cpu cortex-a57'
        ;;
    riscv64gc-unknown-linux-gnu)
        triplet=riscv64-linux-gnu
        : "${CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER:=$triplet-gcc}"
        export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER
        export CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_RUNNER='qemu-riscv64 -cpu rv64,v=false'
        ;;
    *)
        printf 'Usage: %s {aarch64-unknown-linux-gnu|riscv64gc-unknown-linux-gnu}\n' "$0" >&2
        exit 2
        ;;
esac
export QEMU_LD_PREFIX="${QEMU_LD_PREFIX:-/usr/$triplet}"

cd -- "$(dirname -- "$0")/.."
sha256sum --quiet -c tests/conformance/v9/vectors.sha256
args=(--locked --release --target "$1" -p zxcide --lib --tests --no-default-features)
cargo test "${args[@]}" --features merge-simd
cargo test "${args[@]}"
