#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

if [ -d "$ROOT/.work/deps/ffmpeg-full/lib/pkgconfig" ]; then
    export PKG_CONFIG_PATH="$ROOT/.work/deps/ffmpeg-full/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi

if [ "$(uname -s)" = "Linux" ]; then
    # A development binary that links Wayland must use the dynamic musl
    # loader; the default static-pie dies before main.
    export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static"
fi

cargo build --manifest-path "$ROOT/rust/Cargo.toml" -p player-app

if [ "$(uname -s)" = "Linux" ]; then
    OUTPUT="$ROOT/build-linux/mpv"
else
    OUTPUT="$ROOT/build/mpv"
fi
mkdir -p "$(dirname "$OUTPUT")"
install -m 755 "${CARGO_TARGET_DIR:-$ROOT/rust/target}/debug/player" "$OUTPUT"
