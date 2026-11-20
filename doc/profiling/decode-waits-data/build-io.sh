#!/usr/bin/env bash
set -euo pipefail
tools=$(cd "$(dirname "$0")" && pwd)
scratch=$(realpath "$1")
cd "$scratch"
mkdir io-src
cp -a instrumented/. io-src/
python3 - "$scratch/io-src/crates/cli/src/main.rs" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
old = '        input(path)?,\n        w,\n        DecodeOptions {'
new = '''        if std::env::var_os("ZXC_WAIT_MEMORY_SOURCE").is_some() {
            Box::new(io::Cursor::new(std::fs::read(path)?)) as Box<dyn Read>
        } else {
            input(path)?
        },
        w,
        DecodeOptions {'''
assert text.count(old) == 1
path.write_text(text.replace(old, new))
PY
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER
export CARGO_INCREMENTAL=0 CARGO_TARGET_DIR="$scratch/target"
cd io-src
cargo build --release --locked -p zxc-cli -j 2 > "$scratch/io-build.log" 2>&1
cp "$CARGO_TARGET_DIR/release/zxc" "$scratch/probe-io"
