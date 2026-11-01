#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BUILD_DIR="$ROOT/build-linux"
DEPS="$ROOT/.work/deps/linux"
RUST_TARGET_DIR="$ROOT/.work/deps/player-linux/target"
RUST_TRIPLE=x86_64-unknown-linux-musl
DIST="$ROOT/dist"

"$ROOT/scripts/linux-check.sh"

[ -f "$DEPS/.stamp-libass" ] || {
    printf 'error: missing dependency prefix; run scripts/build-deps-linux.sh\n' >&2
    exit 1
}

mkdir -p "$RUST_TARGET_DIR" "$ROOT/.work/tmp" "$DIST"
RUSTFLAGS="--remap-path-prefix=$ROOT=." CARGO_TARGET_DIR="$RUST_TARGET_DIR" cargo rustc --release --target "$RUST_TRIPLE" \
    --manifest-path "$ROOT/rust/Cargo.toml" -p player-release --crate-type staticlib
RUST_LIB_DIR="../.work/deps/player-linux/target/$RUST_TRIPLE/release"

PKG_CONFIG_STATIC="$ROOT/.work/tmp/pkg-config-static"
printf '#!/bin/sh\nexec pkg-config --static "$@"\n' >"$PKG_CONFIG_STATIC"
chmod +x "$PKG_CONFIG_STATIC"
export PKG_CONFIG="$PKG_CONFIG_STATIC"
export PKG_CONFIG_PATH="$DEPS/lib/pkgconfig:$DEPS/ffmpeg/lib/pkgconfig"
export CFLAGS="-ffile-prefix-map=$ROOT=. -I$DEPS/include -I$DEPS/ffmpeg/include"
export LDFLAGS="-static-libgcc -Wl,--gc-sections"

options=(
    -Dcplayer=true -Dlibmpv=false -Dgpl=true
    -Dtests=false -Dfuzzers=false -Dbuild-date=false
    -Dplayer-ffi-lib="$RUST_LIB_DIR/libplayer_release.a"
)

enabled_features=(alsa iconv vaapi vaapi-drm vaapi-wayland vulkan wayland zlib)

disabled_features=(
    cdda cocoa coreaudio cplugins dvbin dvdnav javascript jpeg lcms2 libarchive
    libavdevice libbluray pthread-debug rubberband sdl2-gamepad swift-build
    uchardet uwp vapoursynth vector win32-threads x11-clipboard zimg
    audiounit avfoundation jack openal audiotrack aaudio opensles
    oss-audio pipewire pulse sdl2-audio sndio wasapi
    caca d3d11 direct3d dmabuf-wayland drm egl egl-android egl-angle
    egl-angle-lib egl-angle-win32 egl-drm egl-wayland egl-x11 gbm
    gl gl-cocoa gl-dxinterop gl-win32 gl-x11 sdl2-video shaderc sixel
    spirv-cross plain-gl vdpau vdpau-gl-x11
    vaapi-win32 vaapi-x11 x11 xv
    android-media-ndk cuda-hwaccel cuda-interop d3d-hwaccel d3d9-hwaccel
    gl-dxinterop-d3d9 ios-gl videotoolbox-gl videotoolbox-pl
    macos-cocoa-cb macos-media-player macos-touchbar
    win32-smtc html-build manpage-build pdf-build
)

for feature in "${disabled_features[@]}"; do
    options+=("-D$feature=disabled")
done
for feature in "${enabled_features[@]}"; do
    options+=("-D$feature=enabled")
done

rm -rf "$BUILD_DIR"
meson setup "$BUILD_DIR" "$ROOT" --buildtype=release "${options[@]}"

meson compile -C "$BUILD_DIR"
strip -o "$DIST/player" "$BUILD_DIR/mpv"
