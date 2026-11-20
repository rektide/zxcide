#!/usr/bin/env bash
set -eu -o pipefail
# Run from the repository root; SCRATCH must not exist.
scratch=$(realpath -m "${1:?usage: build.sh SCRATCH PINNED_C_CLI}")
c=$(realpath "${2:?missing C CLI}")
data=$(dirname "$(realpath "$0")")
base=530db0a02f0559fee5a568698075702153dbff7e
mkdir "$scratch"
mkdir "$scratch/src" "$scratch/bin"
git archive "$base" | tar -x -C "$scratch/src"
cp "$c" "$scratch/bin/c"
cd "$scratch/src"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
export CARGO_TARGET_DIR="$scratch/target"
export CARGO_BUILD_JOBS=2
{
    echo "Rust source: $base (git archive)"
    echo "Cargo command: cargo build --locked --release -p zxc-cli"
    rustc -vV
    cargo -V
    uname -srvmo
} > "$scratch/build-info.txt"
pack=crates/codec/src/entropy/pack.rs
for pct in 0 1 2 4; do
    if [[ $pct == 1 ]]; then
        patch -p1 --fuzz=0 < "$data/threshold.diff"
    elif [[ $pct != 0 ]]; then
        sed -i -E "s/(const LITERAL_HUFFMAN_MIN_SAVINGS_PERCENT: usize = )[0-9]+;/\1$pct;/" "$pack"
    fi
    cargo build --locked --release -p zxc-cli > "$scratch/build-$pct.log" 2>&1
    cp "$CARGO_TARGET_DIR/release/zxc" "$scratch/bin/rust-$pct"
done
(cd "$scratch" && sha256sum bin/*) >> "$scratch/build-info.txt"
