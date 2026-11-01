#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

LUA_PC="$ROOT/.work/deps/lua52/lib/pkgconfig/lua52.pc"
if [ ! -f "$LUA_PC" ]; then
    printf 'error: Lua 5.2.4 is not installed; run scripts/dev-deps.sh\n' >&2
    exit 1
fi
PKG_CONFIG_PATH="$ROOT/.work/deps/lua52/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
if [ -d "$ROOT/.work/deps/ffmpeg-full/lib/pkgconfig" ]; then
    PKG_CONFIG_PATH="$ROOT/.work/deps/ffmpeg-full/lib/pkgconfig:$PKG_CONFIG_PATH"
fi
export PKG_CONFIG_PATH

RUST_TARGET="$ROOT/.work/deps/player-audio/target"
mkdir -p "$RUST_TARGET"
CARGO_TARGET_DIR="$RUST_TARGET" cargo build --release \
    --manifest-path "$ROOT/rust/Cargo.toml" -p player-audio
PLAYER_AUDIO_LIB="$RUST_TARGET/release/libplayer_audio.a"

options=(
    -Dcplayer=true -Dlibmpv=false -Dgpl=true -Dlua=lua52
    -Dcocoa=enabled -Dswift-build=enabled -Dcoreaudio=enabled
    -Dvulkan=enabled -Dvideotoolbox-pl=enabled
    -Dgl=disabled -Dgl-cocoa=disabled -Dvideotoolbox-gl=disabled
    -Dmacos-cocoa-cb=disabled
    -Djavascript=disabled -Dlibarchive=disabled -Duchardet=disabled
    -Dzimg=disabled
    -Drubberband=disabled -Dcdda=disabled -Ddvdnav=disabled
    -Dlibbluray=disabled
    -Dlibavdevice=disabled -Dcplugins=disabled -Dvapoursynth=disabled
    -Djpeg=disabled -Dlcms2=disabled -Dplain-gl=disabled
    -Dx11=disabled -Dwayland=disabled -Ddrm=disabled -Dvaapi=disabled
    -Dvdpau=disabled
    -Daudiounit=disabled -Davfoundation=disabled -Dmacos-touchbar=disabled
    -Dmacos-media-player=disabled -Dtests=false -Dbuild-date=false
    -Dplayer-audio-lib="$PLAYER_AUDIO_LIB"
)

if [ -d build ]; then
    meson setup --reconfigure build . --buildtype=release "${options[@]}"
else
    meson setup build . --buildtype=release "${options[@]}"
fi

meson compile -C build
