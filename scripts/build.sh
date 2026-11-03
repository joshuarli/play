#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

if [ -d "$ROOT/.work/deps/ffmpeg-full/lib/pkgconfig" ]; then
    export PKG_CONFIG_PATH="$ROOT/.work/deps/ffmpeg-full/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi

RUST_TARGET="$ROOT/.work/deps/player-audio/target"
mkdir -p "$RUST_TARGET"
CARGO_TARGET_DIR="$RUST_TARGET" cargo build --release \
    --manifest-path "$ROOT/rust/Cargo.toml" -p player-audio -p player-ffi -p player-engine
PLAYER_AUDIO_LIB="$RUST_TARGET/release/libplayer_audio.a"
PLAYER_FFI_LIB="$RUST_TARGET/release/libplayer_ffi.a"
PLAYER_ENGINE_LIB="$RUST_TARGET/release/libplayer_engine.a"

options=(
    -Dcplayer=true -Dlibmpv=false -Dgpl=true
    -Dtests=false -Dfuzzers=false -Dbuild-date=false
    -Ddisable-packet-pool=false
    -Dmacos-bundle-category=video -Dswift-flags=
    -Dplayer-audio-lib="$PLAYER_AUDIO_LIB"
    -Dplayer-ffi-lib="$PLAYER_FFI_LIB"
    -Dplayer-engine-lib="$PLAYER_ENGINE_LIB"
)

enabled_features=(
    cocoa coreaudio vulkan videotoolbox-pl swift-build
    iconv zlib
    macos-10-15-4-features macos-11-features macos-11-3-features
    macos-12-features
)

disabled_features=(
    cdda cplugins dvbin dvdnav javascript jpeg lcms2 libarchive
    libavdevice libbluray pthread-debug rubberband sdl2-gamepad
    uchardet vapoursynth vector x11-clipboard zimg
    alsa audiounit avfoundation jack openal audiotrack aaudio opensles
    oss-audio pipewire pulse sdl2-audio sndio
    caca dmabuf-wayland drm egl egl-android
    egl-drm egl-wayland egl-x11 gbm
    gl gl-cocoa gl-x11 sdl2-video sixel
    plain-gl vdpau vdpau-gl-x11 vaapi vaapi-drm
    vaapi-wayland vaapi-x11 wayland x11 xv
    android-media-ndk cuda-hwaccel cuda-interop
    ios-gl videotoolbox-gl
    macos-cocoa-cb macos-media-player macos-touchbar
    html-build manpage-build pdf-build
)

for feature in "${disabled_features[@]}"; do
    options+=("-D$feature=disabled")
done
for feature in "${enabled_features[@]}"; do
    options+=("-D$feature=enabled")
done

if [ -d build ]; then
    meson setup --reconfigure build . --buildtype=release "${options[@]}"
else
    meson setup build . --buildtype=release "${options[@]}"
fi

meson compile -C build
