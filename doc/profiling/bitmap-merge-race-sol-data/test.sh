#!/usr/bin/env bash
set -euxo pipefail
s=$(realpath "$(dirname "$0")")
export TMPDIR="$s/tmp" CARGO_BUILD_JOBS=4 CARGO_ENCODED_RUSTFLAGS='' RUSTFLAGS=''
for arm; do (
  export CARGO_TARGET_DIR="$s/target-$arm"
  cd "$s/src-$arm"
  cargo nextest run --release --locked -p zxc-codec --lib --features merge-simd
  cargo nextest run --release --locked -p zxc-codec --lib --no-default-features
  # Explicit build so the two CPU models run the same test executable.
  test_exe=$(cargo test --release --locked -p zxc-codec --lib --features merge-simd --no-run --message-format=json \
    | python3 -c 'import sys,json; print(next(d["executable"] for d in map(json.loads,sys.stdin) if d.get("executable") and d["target"]["name"]=="zxc_codec"))')
  export RUST_TEST_THREADS=1
  for cpu in Conroe Nehalem; do qemu-x86_64 -cpu "$cpu" "$test_exe" entropy::tests::; done
  export CARGO_TARGET_DIR="$s/target-asan-$arm" CARGO_ENCODED_RUSTFLAGS=-Zsanitizer=address
  cargo nextest run --release --locked -p zxc-codec --lib --features merge-simd --target x86_64-unknown-linux-gnu -E 'test(race_tests)'
); done
