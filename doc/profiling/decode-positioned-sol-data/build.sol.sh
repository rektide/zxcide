#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname -- "$0")" && pwd)
root=$(git -C "$here" rev-parse --show-toplevel)
pin=3f711434ad83b85fa191a209041777c457c6972b
scratch=${1:-"$root/.test-agent/decode-positioned-sol/repro"}
# Need a fresh destination; never overwrite prior build/evidence.
mkdir -- "$scratch"
scratch=$(cd "$scratch" && pwd)
mkdir "$scratch"/{parent,trial,diag,bin,logs,tmp}
export CARGO_ENCODED_RUSTFLAGS='' TMPDIR="$scratch/tmp"
{
    git -C "$root" rev-parse "$pin"
    rustc -Vv
    cargo -V
    cargo nextest --version
    printf 'CARGO_ENCODED_RUSTFLAGS=%q\n' "$CARGO_ENCODED_RUSTFLAGS"
} > "$scratch/logs/versions.sol.txt"
for variant in parent trial diag; do
    git -C "$root" archive "$pin" | tar -x -C "$scratch/$variant"
    (
        cd "$scratch/$variant"
        export CARGO_TARGET_DIR="$PWD/target"
        if [[ $variant != parent ]]; then
            patch -p1 < "$here/trial.sol.patch"
        fi
        if [[ $variant == diag ]]; then
            patch -p1 < "$here/diag.sol.patch"
        fi
        cargo build --release --locked -p zxc-cli
        cp target/release/zxc "$scratch/bin/zxc-$variant"
    ) > "$scratch/logs/build-$variant.log" 2>&1
done
(
    cd "$scratch/trial"
    export CARGO_TARGET_DIR="$PWD/target"
    cargo nextest run --release --locked --workspace > "$scratch/logs/nextest-workspace.log" 2>&1
    cargo nextest run --release --locked --workspace --features zxc-cli/transports > "$scratch/logs/nextest-transports.log" 2>&1
    cp target/release/zxc "$scratch/bin/zxc-transports"
    cargo nextest run --release --locked --workspace --no-default-features > "$scratch/logs/nextest-no-defaults.log" 2>&1
    cargo nextest run --release --locked -p zxc-codec --no-default-features > "$scratch/logs/nextest-codec-no-defaults.log" 2>&1
)
sha256sum "$scratch"/bin/zxc-* "$here"/*\.patch > "$scratch/logs/sha256.sol.txt"
