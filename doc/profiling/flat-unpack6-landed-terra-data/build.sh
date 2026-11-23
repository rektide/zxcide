#!/usr/bin/env bash
set -euxo pipefail
root=$PWD
out=$root/.test-agent/flat6-terra
export TMPDIR="$out/tmp" CARGO_BUILD_JOBS=4 CARGO_ENCODED_RUSTFLAGS='' RUSTFLAGS=''
rustc -Vv
cargo -V
for spec in a:2fd99c21 b:0d68cb82; do
  label=${spec%%:*}; rev=${spec#*:}
  mkdir "$out/src-$label"
  git archive "$rev" | tar -x -C "$out/src-$label"
  cd "$out/src-$label"
  export CARGO_TARGET_DIR="$out/target-$label"
  cargo build --locked --release -p zxc-cli
  cp "$CARGO_TARGET_DIR/release/zxc" "$out/cli-$label"
  if [[ $label == b ]]; then
    cargo build --locked --release -p zxc-cli --no-default-features
    cp "$CARGO_TARGET_DIR/release/zxc" "$out/cli-b-scalar"
  fi
  cd "$root"
done
sha256sum "$out"/cli-*
