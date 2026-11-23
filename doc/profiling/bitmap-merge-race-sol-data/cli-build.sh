#!/usr/bin/env bash
set -euxo pipefail
s=$(realpath "$(dirname "$0")")
export TMPDIR="$s/tmp" CARGO_BUILD_JOBS=4 CARGO_ENCODED_RUSTFLAGS='' RUSTFLAGS=''
for arm; do (
  export CARGO_TARGET_DIR="$s/target-$arm"
  cd "$s/src-$arm"
  cargo build --release --locked -p zxc-cli
  cp "$CARGO_TARGET_DIR/release/zxc" "$s/cli-$arm"
  cargo build --release --locked -p zxc-cli --no-default-features
  cp "$CARGO_TARGET_DIR/release/zxc" "$s/cli-$arm-scalar"
  sha256sum "$s/cli-$arm" "$s/cli-$arm-scalar"
); done
