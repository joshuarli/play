#!/bin/sh
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
BUILD_DIR="$ROOT/build-linux"

fail() {
    printf 'linux-check: %s\n' "$1" >&2
    exit 1
}

[ "$(uname -s)" = "Linux" ] || fail "requires Linux, found $(uname -s)"
[ "$(uname -m)" = "x86_64" ] || fail "requires x86_64, found $(uname -m)"

for tool in gcc meson ninja pkg-config python3 wayland-scanner; do
    command -v "$tool" >/dev/null 2>&1 || fail "missing build tool: $tool"
done

for mod in wayland-client wayland-cursor xkbcommon alsa vulkan; do
    pkg-config --exists "$mod" || fail "missing pkg-config module: $mod"
done

[ -f "$ROOT/meson.build" ] || fail "missing meson.build"

if [ -d "$BUILD_DIR" ]; then
    meson setup --reconfigure "$BUILD_DIR" "$ROOT" \
        -Dcplayer=true -Dlibmpv=false -Dgpl=true \
        -Dtests=false -Dfuzzers=false -Dbuild-date=false \
        -Dwayland=enabled -Dvulkan=enabled -Dalsa=enabled \
        -Dgl=disabled -Dx11=disabled -Ddrm=disabled \
        -Ddmabuf-wayland=disabled 2>&1 ||
        fail "meson reconfigure failed; see output above"
else
    meson setup "$BUILD_DIR" "$ROOT" \
        -Dcplayer=true -Dlibmpv=false -Dgpl=true \
        -Dtests=false -Dfuzzers=false -Dbuild-date=false \
        -Dwayland=enabled -Dvulkan=enabled -Dalsa=enabled \
        -Dgl=disabled -Dx11=disabled -Ddrm=disabled \
        -Ddmabuf-wayland=disabled 2>&1 ||
        fail "meson setup failed; see output above"
fi

printf 'linux-check: Alpine x86_64 container has the build tools, Vulkan, Wayland, xkbcommon, and ALSA.\n'
