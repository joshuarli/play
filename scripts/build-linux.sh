#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BUILD_DIR="$ROOT/build-linux"
DEPS="$ROOT/.work/deps/linux"
RUST_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/.work/deps/player-linux/target}"
RUST_TRIPLE=x86_64-unknown-linux-musl
DIST="$ROOT/dist"

"$ROOT/scripts/linux-check.sh"

[ -f "$DEPS/.stamp-libplacebo" ] || {
    printf 'error: missing dependency prefix; run scripts/build-deps-linux.sh\n' >&2
    exit 1
}

"$ROOT/scripts/fetch-ffmpeg-runtime.sh" "$RUST_TRIPLE"

mkdir -p "$RUST_TARGET_DIR" "$ROOT/.work/tmp" "$BUILD_DIR" "$DIST"
export CARGO_TARGET_DIR="$RUST_TARGET_DIR"
export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig"
# The Wayland-linked binary must use the dynamic musl loader. A static-pie
# with Wayland in NEEDED dies in the loader before main.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$ROOT=. -C target-feature=-crt-static -C link-arg=-static-libgcc"
cargo build --profile dist --target "$RUST_TRIPLE" \
    --manifest-path "$ROOT/Cargo.toml" -p player-app

BINARY="$RUST_TARGET_DIR/$RUST_TRIPLE/dist/player"
"$ROOT/scripts/install-layout.sh" "$BINARY" "$BUILD_DIR" mpv
"$ROOT/scripts/install-layout.sh" --strip "$BINARY" "$DIST" player
