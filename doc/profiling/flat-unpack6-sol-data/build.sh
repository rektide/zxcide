#!/usr/bin/env bash
# From repository root: build.sh OUT [a b tests]
set -euxo pipefail
d=$(realpath "$(dirname "$0")")
replay=$(realpath doc/profiling/decode-replay-data)
out=$(realpath -m "$1")
shift
mkdir -p "$out/tmp"
export TMPDIR="$out/tmp" CARGO_BUILD_JOBS=4 CARGO_ENCODED_RUSTFLAGS=''
pin=72eaabe18d32d83586624230e2ca87189374f356
rustc -Vv
cargo -V
for label in "$@"; do (
  src="$out/src-$label"
  if [[ ! -d "$src" ]]; then
    mkdir "$src"
    git archive "$pin" | tar -x -C "$src"
    if [[ "$label" != a ]]; then patch -d "$src" -p1 < "$d/trial.patch"; fi
  fi
  cd "$src"
  export CARGO_TARGET_DIR="$out/target-$label"
  if [[ "$label" == tests ]]; then
    cargo fmt --all --check
    cargo nextest run --locked --release --workspace
    cargo nextest run --locked --release --workspace --no-default-features
    cargo nextest run --locked --release -p zxc-codec --no-default-features
    cargo clippy --locked --release --workspace --all-targets -- -D warnings -A clippy::new_instead_of_clear
    export CARGO_TARGET_DIR="$out/target-asan"
    RUSTFLAGS='-Zsanitizer=address' CARGO_ENCODED_RUSTFLAGS=$'-Zsanitizer=address' \
      cargo nextest run --locked --release -p zxc-codec --target x86_64-unknown-linux-gnu \
      -E 'test(flat_six_bit_groups_match_individual_wire_bits)'
    exit
  fi
  cargo build --locked --release -p zxc-cli
  cp "$CARGO_TARGET_DIR/release/zxc" "$out/cli-$label"
  cargo build --locked --release -p zxc-cli --no-default-features
  cp "$CARGO_TARGET_DIR/release/zxc" "$out/cli-$label-scalar"
  mkdir -p crates/codec/src/bin
  sed 's/h\[0\] == 1 \&\& view.len() >= 16 \&\& view\[11\] == 0/h[0] == 1 \&\& view.len() >= 16/;s@ // SPL16$@ // SPL16 or SPL8@' \
      "$replay/replay.rs" > crates/codec/src/bin/replay.rs
  cat "$replay/section-entry.rs" >> crates/codec/src/block.rs
  printf '\n#[doc(hidden)]\npub use block::replay_huf;\n' >> crates/codec/src/lib.rs
  export CARGO_ENCODED_RUSTFLAGS=$'--cfg\x1freplay_perf\x1f-Cdebuginfo=line-tables-only'
  export CARGO_PROFILE_RELEASE_STRIP=none
  cargo build --locked --release -p zxc-codec --features merge-simd --bin replay
  cp "$CARGO_TARGET_DIR/release/replay" "$out/replay-$label"
  cargo build --locked --release -p zxc-codec --no-default-features --bin replay
  cp "$CARGO_TARGET_DIR/release/replay" "$out/replay-$label-scalar"
  sha256sum "$out/cli-$label"{,-scalar} "$out/replay-$label"{,-scalar}
); done
