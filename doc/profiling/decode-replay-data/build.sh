#!/usr/bin/env bash
# At the repository root: build.sh OUT UPSTREAM_REPO clean|stage|perf
set -euo pipefail
set -x
here=$(realpath "$(dirname "$0")")
out=$(realpath -m "$1")
c_repo=$(realpath "$2")
mode=$3
[[ $mode == clean || $mode == stage || $mode == perf ]]
rust=8b3b88974f40b46808ce13e2f5f2306c11a1bc19
c=5719a96f778226868b29dba621a8c4e302b74d7c
mkdir "$out"
mkdir "$out/rs" "$out/c"
git archive "$rust" | tar -x -C "$out/rs"
git -C "$c_repo" archive "$c" | tar -x -C "$out/c"
mkdir "$out/rs/crates/codec/src/bin"
cp "$here/replay.rs" "$out/rs/crates/codec/src/bin/replay.rs"
flags=()
if [[ $mode == stage ]]; then
    python3 -B "$here/instrument.py" "$out/rs" "$out/c"
    flags=(-DREPLAY_STAGES -I"$here")
fi
rustflags=''
cflags='-O3 -DNDEBUG'
if [[ $mode == perf ]]; then
    # Expose the production section boundary only in the archived Rust tree.
    cat "$here/section-entry.rs" >> "$out/rs/crates/codec/src/block.rs"
    printf '\n#[doc(hidden)]\npub use block::replay_huf;\n' >> "$out/rs/crates/codec/src/lib.rs"
    rustflags=$(printf '%s\x1f%s\x1f%s' --cfg replay_perf -Cdebuginfo=line-tables-only)
    export CARGO_PROFILE_RELEASE_STRIP=none
    cflags+=' -g'
    flags=(-DREPLAY_PERF -g)
fi
rustc -Vv
cargo -V
cmake --version
/usr/bin/cc --version
(
    cd "$out/rs"
    CARGO_ENCODED_RUSTFLAGS="$rustflags" CARGO_TARGET_DIR="$out/cargo" \
        cargo build --release --locked -p zxc-codec --features merge-simd --bin replay
    cp "$out/cargo/release/replay" "$out/replay-rust"
    if [[ $mode == clean ]]; then
        CARGO_ENCODED_RUSTFLAGS='' CARGO_TARGET_DIR="$out/cargo" \
            cargo build --release --locked -p zxc-cli
        cp "$out/cargo/release/zxc" "$out/rust-cli"
    fi
    if [[ $mode == perf ]]; then
        CARGO_ENCODED_RUSTFLAGS="$rustflags"$'\x1f-Ctarget-cpu=native' CARGO_TARGET_DIR="$out/cargo-native" \
            cargo build --release --locked -p zxc-codec --features merge-simd --bin replay
        cp "$out/cargo-native/release/replay" "$out/replay-rust-native"
        rustc --print cfg
        rustc --print cfg -Ctarget-cpu=native
    fi
)
cmake -S "$out/c" -B "$out/cmake" -G 'Unix Makefiles' -DCMAKE_BUILD_TYPE=Release \
    -DZXC_BUILD_TESTS=OFF -DZXC_NATIVE_ARCH=ON -DBUILD_SHARED_LIBS=OFF \
    -DCMAKE_EXPORT_COMPILE_COMMANDS=ON -DCMAKE_C_COMPILER=/usr/bin/cc \
    -DCMAKE_C_FLAGS= "-DCMAKE_C_FLAGS_RELEASE=$cflags"
if [[ $mode == clean ]]; then
    cmake --build "$out/cmake" --parallel 4
else
    cmake --build "$out/cmake" --parallel 4 --target zxc_lib
fi
/usr/bin/cc -std=gnu17 -O3 -DNDEBUG -D_POSIX_C_SOURCE=200809L -D_FILE_OFFSET_BITS=64 \
    -DZXC_STATIC_DEFINE -I"$out/c/include" -I"$out/c/src/lib" -I"$out/c/src/lib/vendors" "${flags[@]}" \
    "$here/replay.c" "$out/cmake/libzxc.a" -pthread -o "$out/replay-c"
sha256sum "$out/replay-"*
