#!/usr/bin/env bash
# From repository root, after build.sh OUT a b.
set -euxo pipefail
d=$(realpath "$(dirname "$0")")
out=$(realpath "$1")
replay=$(realpath doc/profiling/decode-replay-data/replay.rs)
export TMPDIR="$out/tmp" CARGO_BUILD_JOBS=4 CARGO_ENCODED_RUSTFLAGS=$'-Cdebuginfo=line-tables-only'
for label in a b; do
  (
    cd "$out/src-$label"
    sed -n '3p;9,27p;31,62p' "$replay" > crates/codec/src/bin/flat6.rs
    cat "$d/saved.rs" >> crates/codec/src/bin/flat6.rs
    export CARGO_TARGET_DIR="$out/target-$label"
    cargo build --locked --release -p zxc-codec --features merge-simd --bin flat6
    cp "$CARGO_TARGET_DIR/release/flat6" "$out/flat6-$label"
  )
done
