#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
scripts/build-component.sh
target=${CARGO_TARGET_DIR:-target}
export ZXC_COMPONENT="$(cd "$target/component" && pwd)/zxc-codec.wasm"
exec cargo bench --locked -p zxc-component --features host-bench --bench component -- "$@"
