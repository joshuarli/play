#!/bin/sh
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BUILD_DIR="$ROOT/build-linux"
RUST_TARGET="$ROOT/.work/deps/player-linux/target"

"$ROOT/scripts/linux-check.sh"

mkdir -p "$RUST_TARGET"
CARGO_TARGET_DIR="$RUST_TARGET" cargo build --release \
    --manifest-path "$ROOT/rust/Cargo.toml" -p player-audio -p player-ffi

if [ -d "$BUILD_DIR" ]; then
    setup="meson setup --reconfigure"
else
    setup="meson setup"
fi

$setup "$BUILD_DIR" "$ROOT" --buildtype=release \
    -Dcplayer=true -Dlibmpv=false -Dgpl=true \
    -Dtests=false -Dfuzzers=false -Dbuild-date=false \
    -Dplayer-audio-lib="$RUST_TARGET/release/libplayer_audio.a" \
    -Dplayer-ffi-lib="$RUST_TARGET/release/libplayer_ffi.a" \
    -Dwayland=enabled -Dvulkan=enabled -Dalsa=enabled \
    -Dgl=disabled -Dx11=disabled -Ddrm=disabled \
    -Ddmabuf-wayland=disabled -Dlibavdevice=disabled

meson compile -C "$BUILD_DIR"
