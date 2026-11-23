#!/usr/bin/env bash
# Run from repo root. build.sh OUT UPSTREAM_REPO [a b c tests]
set -euxo pipefail
here=$(realpath "$(dirname "$0")")
data=$(realpath doc/profiling/decode-replay-data)
out=$(realpath -m "$1")
repo=$(realpath "$2")
shift 2
if [[ $# == 0 ]]; then set -- a b c tests; fi
rust=3f711434ad83b85fa191a209041777c457c6972b
c=5719a96f778226868b29dba621a8c4e302b74d7c
mkdir -p "$out/tmp"
export TMPDIR="$out/tmp" CARGO_BUILD_JOBS=4 CARGO_ENCODED_RUSTFLAGS=''
rustc -Vv
cargo -V
for label in "$@"; do case "$label" in
  a|b) (
    src="$out/src-$label"
    mkdir "$src"
    git archive "$rust" | tar -x -C "$src"
    cd "$src"
    if [[ "$label" == b ]]; then patch -p1 < "$here/trial.patch"; fi
    export CARGO_TARGET_DIR="$out/target-$label"
    cargo build --locked --release -p zxc-cli
    cp "$CARGO_TARGET_DIR/release/zxc" "$out/cli-$label"
    cargo build --locked --release -p zxc-cli --no-default-features
    cp "$CARGO_TARGET_DIR/release/zxc" "$out/cli-$label-scalar"
    mkdir crates/codec/src/bin
    sed 's/h\[0\] == 1 \&\& view.len() >= 16 \&\& view\[11\] == 0/h[0] == 1 \&\& view.len() >= 16/;s@ // SPL16$@ // SPL16 or SPL8@' \
        "$data/replay.rs" > crates/codec/src/bin/replay.rs
    cat "$data/section-entry.rs" >> crates/codec/src/block.rs
    printf '\n#[doc(hidden)]\npub use block::replay_huf;\n' >> crates/codec/src/lib.rs
    export CARGO_ENCODED_RUSTFLAGS=$'--cfg\x1freplay_perf\x1f-Cdebuginfo=line-tables-only'
    export CARGO_PROFILE_RELEASE_STRIP=none
    cargo build --locked --release -p zxc-codec --features merge-simd --bin replay
    cp "$CARGO_TARGET_DIR/release/replay" "$out/replay-$label"
    cargo build --locked --release -p zxc-codec --no-default-features --bin replay
    cp "$CARGO_TARGET_DIR/release/replay" "$out/replay-$label-scalar"
    sha256sum "$out/cli-$label"{,-scalar} "$out/replay-$label"{,-scalar}
  ) ;;
  c) (
    mkdir "$out/src-c"
    git -C "$repo" archive "$c" | tar -x -C "$out/src-c"
    /usr/bin/cc --version
    cmake -S "$out/src-c" -B "$out/cmake" -G 'Unix Makefiles' -DCMAKE_BUILD_TYPE=Release \
      -DZXC_BUILD_TESTS=OFF -DZXC_NATIVE_ARCH=ON -DBUILD_SHARED_LIBS=OFF \
      -DCMAKE_EXPORT_COMPILE_COMMANDS=ON -DCMAKE_C_COMPILER=/usr/bin/cc -DCMAKE_C_FLAGS= \
      '-DCMAKE_C_FLAGS_RELEASE=-O3 -DNDEBUG -g'
    cmake --build "$out/cmake" --parallel 4
    cp "$out/cmake/zxc" "$out/cli-c"
    /usr/bin/cc -std=gnu17 -O3 -DNDEBUG -D_POSIX_C_SOURCE=200809L -D_FILE_OFFSET_BITS=64 \
      -DZXC_STATIC_DEFINE -I"$out/src-c/include" -I"$out/src-c/src/lib" -I"$out/src-c/src/lib/vendors" \
      -DREPLAY_PERF -g "$data/replay.c" "$out/cmake/libzxc.a" -pthread -o "$out/replay-c"
    sha256sum "$out/cli-c" "$out/replay-c"
  ) ;;
  tests) (
    src="$out/test-b"
    mkdir "$src"
    git archive "$rust" | tar -x -C "$src"
    cd "$src"
    patch -p1 < "$here/trial.patch"
    patch -p1 < "$here/tests.patch"
    export CARGO_TARGET_DIR="$out/target-b"
    cargo fmt --check --all
    cargo nextest run --locked --release --workspace
    cargo nextest run --locked --release --workspace --no-default-features
    cargo nextest run --locked --release -p zxc-codec --no-default-features
    cargo clippy --locked --release --workspace --all-targets -- -D warnings -A clippy::new_instead_of_clear
  ) ;;
  *) exit 2 ;;
esac; done
