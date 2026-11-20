#!/usr/bin/env bash
set -euo pipefail
tools=$(cd "$(dirname "$0")" && pwd)
repo=$(git -C "$tools" rev-parse --show-toplevel)
pin=530db0a02f0559fee5a568698075702153dbff7e
scratch=${1:-$(mktemp -d "$repo/.test-agent/decode-waits.XXXXXX")}
mode=${2:-both}
mkdir -p "$scratch"
scratch=$(realpath "$scratch")
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
export CARGO_INCREMENTAL=0
export CARGO_TARGET_DIR="$scratch/target"

if [[ $mode != probe ]]; then
    mkdir "$scratch/src"
    git -C "$repo" archive "$pin" | tar -x -C "$scratch/src"
    (
        cd "$scratch/src"
        rustc -vV
        cargo -V
        cargo build --release --locked -p zxc-cli -j 2
    ) > "$scratch/clean-build.log" 2>&1
    cp "$CARGO_TARGET_DIR/release/zxc" "$scratch/clean"
fi
if [[ $mode != clean ]]; then
    mkdir "$scratch/instrumented"
    git -C "$repo" archive "$pin" | tar -x -C "$scratch/instrumented"
    python3 "$tools/inject.py" "$scratch/instrumented"
    (
        cd "$scratch/instrumented"
        cargo build --release --locked -p zxc-cli -j 2
    ) > "$scratch/probe-build.log" 2>&1
    cp "$CARGO_TARGET_DIR/release/zxc" "$scratch/probe"
fi
printf '%s\n' "$scratch"
