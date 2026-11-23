#!/usr/bin/env bash
set -euxo pipefail
s=$(realpath "$(dirname "$0")")
mkdir -p "$s/tmp"
export TMPDIR="$s/tmp" CARGO_BUILD_JOBS=4 CARGO_ENCODED_RUSTFLAGS='' RUSTFLAGS=''
rustc -Vv
for arm; do (
  src="$s/src-$arm"
  if [[ ! -d $src ]]; then python3 -B "$s/prepare.py" prod "$arm"; fi
  export CARGO_TARGET_DIR="$s/target-$arm"
  if [[ ! -f $src/crates/codec/src/bin/replay.rs ]]; then python3 -B "$s/prepare.py" shim "$arm"; fi
  cd "$src"
  if [[ $arm == capture ]]; then
    cargo build --release --locked -p zxc-codec --features merge-simd --bin capture
    cp "$CARGO_TARGET_DIR/release/capture" "$s/capture"
  else
    export CARGO_PROFILE_RELEASE_STRIP=none CARGO_ENCODED_RUSTFLAGS=$'--cfg\x1freplay_perf\x1f-Cdebuginfo=line-tables-only'
    cargo build --release --locked -p zxc-codec --features merge-simd --bin kernel --bin replay
    for bin in kernel replay; do cp "$CARGO_TARGET_DIR/release/$bin" "$s/$bin-$arm"; done
  fi
); done
