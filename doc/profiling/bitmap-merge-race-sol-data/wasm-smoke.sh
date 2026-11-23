#!/usr/bin/env bash
set -euxo pipefail
s=$(realpath "$(dirname "$0")")
root=$PWD
dir="$s/wasm"
mkdir -p "$dir/bin"
ln -sfn "$root/.test-agent/pivco-queue/fixtures" "$dir/fixtures"
export TMPDIR="$s/tmp" CARGO_BUILD_JOBS=4 RUSTFLAGS=''
runner="$root/.test-agent/pivco-queue/bin/wit"
for mode in scalar simd128 relaxed; do
  case "$mode" in
    scalar) flags='-simd128,-relaxed-simd'; validation='-simd,-relaxed-simd'; features=() ;;
    simd128) flags='+simd128,-relaxed-simd'; validation='simd,-relaxed-simd'; features=(--features zxc-codec/merge-simd) ;;
    relaxed) flags='+simd128,+relaxed-simd'; validation='simd,relaxed-simd'; features=(--features zxc-codec/merge-simd) ;;
  esac
  for arm in a avx2; do (
    cd "$s/src-$arm"
    export CARGO_TARGET_DIR="$s/wasm-target-$arm" CARGO_ENCODED_RUSTFLAGS="-Ctarget-feature=$flags"
    cargo +nightly-2026-10-14 build --locked --release -p zxc-component --target wasm32-unknown-unknown "${features[@]}"
    component="$dir/$arm-$mode.wasm"
    wasm-tools component new "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/zxc_component.wasm" -o "$component"
    wasm-tools validate "--features=$validation" "$component"
    sha256sum "$component"
  ); done
  ln -sfn "$dir/a-$mode.wasm" "$dir/bin/base.wasm"
  ln -sfn "$dir/avx2-$mode.wasm" "$dir/bin/trial.wasm"
  taskset -c 3 "$runner" "$dir" --verify-only
done
