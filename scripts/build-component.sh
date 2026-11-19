#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
target=${CARGO_TARGET_DIR:-target}
out="$target/component"
cargo build --locked --release -p zxc-component --target wasm32-unknown-unknown --target-dir "$target"
mkdir -p "$out"
wasm-tools component new "$target/wasm32-unknown-unknown/release/zxc_component.wasm" -o "$out/zxc-codec.wasm"
wasm-tools validate "$out/zxc-codec.wasm"
cp crates/component/wit/codec.wit LICENSE "$out/"
