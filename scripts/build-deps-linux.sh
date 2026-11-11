#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX="$ROOT/.work/deps/linux"
SRC="$ROOT/.work/tmp/linux"
LOGS="$SRC/logs"
JOBS=${JOBS:-$(nproc)}
export TMPDIR="$ROOT/.work/tmp"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$PREFIX/lib64/pkgconfig:$PREFIX/share/pkgconfig"
export CFLAGS="-O2 -fPIC -ffile-prefix-map=$ROOT=."
export CXXFLAGS="-O2 -fPIC -ffile-prefix-map=$ROOT=."

mkdir -p "$PREFIX" "$SRC" "$LOGS"
# Release tarballs carry no .git, so a build-time `git describe` (meson's
# vcs_tag in libplacebo) would describe this checkout instead and stamp
# Player's own version into the dependency. Stop git's search at $SRC.
export GIT_CEILING_DIRECTORIES="$SRC"

SHADERC_PREBUILT_TAR="$ROOT/native/prebuilt/shaderc-combined-x86_64-linux-musl-2026.4.tar.gz"
SHADERC_PREBUILT_SHA=cbd1172afd26321bac14ee9389b4981308717d06fdc27f742513b92091d4d1dd

INPUT_HASH=$(cat "$ROOT/scripts/build-deps-linux.sh" "$SHADERC_PREBUILT_TAR" | sha256sum | awk '{print $1}')

MESON=${MESON:-$(command -v muon 2>/dev/null || command -v meson 2>/dev/null || printf meson)}
NINJA=${NINJA:-$(command -v samu 2>/dev/null || command -v samurai 2>/dev/null || command -v ninja 2>/dev/null || printf ninja)}
case "$(basename "$MESON")" in
    muon*) MESON_SETUP=("$MESON" meson setup) ;;
    *) MESON_SETUP=("$MESON" setup) ;;
esac
MESON_SETUP_LIBPLACEBO=("${MESON_SETUP[@]}")
case "$(basename "$MESON")" in
    muon*)
        if command -v meson >/dev/null 2>&1; then
            printf 'note: using meson for libplacebo (muon frontend compatibility fallback); using %s backend\n' "$NINJA" >&2
            MESON_SETUP_LIBPLACEBO=(meson setup)
        fi
        ;;
esac

case "$(uname -sm)/$(ldd --version 2>&1)" in
    "Linux x86_64/"*musl*) ;;
    *)
        printf 'error: requires x86_64 musl Linux; run through scripts/linux-container.sh\n' >&2
        exit 1
        ;;
esac

for tool in curl tar sha256sum "$MESON" "$NINJA" pkg-config python3 make; do
    command -v "$tool" >/dev/null || {
        printf 'error: %s is required (development tool)\n' "$tool" >&2
        exit 1
    }
done

fetch() {
    local url=$1 sha=$2 out=$3
    if [ ! -f "$out" ] || [ "$(sha256sum "$out" | awk '{print $1}')" != "$sha" ]; then
        printf 'download %s\n' "$url"
        curl -fsSL "$url" -o "$out"
    fi
    if [ "$(sha256sum "$out" | awk '{print $1}')" != "$sha" ]; then
        printf 'error: checksum mismatch for %s\n' "$out" >&2
        exit 1
    fi
}

extract() {
    local archive=$1 dir=$2
    rm -rf "$dir"
    tar -xf "$archive" -C "$(dirname "$dir")"
}

stamp() { printf '%s\n' "$INPUT_HASH" >"$PREFIX/.stamp-$1"; }
have() {
    [ -f "$PREFIX/.stamp-$1" ] &&
        [ "$(cat "$PREFIX/.stamp-$1")" = "$INPUT_HASH" ]
}

run_log() {
    local name=$1
    shift
    if ! "$@" >"$LOGS/$name.log" 2>&1; then
        printf 'error: %s failed; tail of %s:\n' "$name" "$LOGS/$name.log" >&2
        tail -40 "$LOGS/$name.log" >&2
        exit 1
    fi
}

if [ "$(cat "$PREFIX/.stamp-input" 2>/dev/null)" != "$INPUT_HASH" ]; then
    find "$PREFIX" -mindepth 1 -maxdepth 1 -exec rm -rf {} +
    printf '%s\n' "$INPUT_HASH" >"$PREFIX/.stamp-input"
fi

meson_static() {
    local name=$1 dir=$2
    shift 2
    run_log "$name" "${MESON_SETUP[@]}" "$dir/build" "$dir" --prefix="$PREFIX" --libdir=lib \
        --buildtype=release --default-library=static "$@"
    run_log "$name-build" "$NINJA" -C "$dir/build" install
}

VULKAN_VERSION=1.4.363.0
SHADERC_VERSION=2026.4
GLSLANG_REV=e1b562a8bed273a02f30b59b66a5d499793cede5
SPIRV_HEADERS_REV=04fd3caa1e8267e4d95c806cad901181728e1006
SPIRV_TOOLS_REV=ef96ed763b43b59b33b31b362f09a02b729fa1c9
LIBPLACEBO_VERSION=7.387.0
GLAD_VERSION=2.0.8
JINJA_VERSION=3.1.6
MARKUPSAFE_VERSION=3.0.4
FAST_FLOAT_VERSION=8.3.2

if ! have vulkan-headers; then
    VH="Vulkan-Headers-$VULKAN_VERSION.tar.gz"
    fetch "https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/vulkan-sdk-$VULKAN_VERSION.tar.gz" \
        4a078be12bef21cfebc09d878b77a63cff9d68f899254a0b00d0e37ef73e7f7e "$SRC/$VH"
    extract "$SRC/$VH" "$SRC/Vulkan-Headers-vulkan-sdk-$VULKAN_VERSION"
    vh="$SRC/Vulkan-Headers-vulkan-sdk-$VULKAN_VERSION"
    mkdir -p "$PREFIX/include" "$PREFIX/share/vulkan"
    cp -R "$vh/include/vulkan" "$PREFIX/include/"
    cp -R "$vh/include/vk_video" "$PREFIX/include/"
    cp -R "$vh/registry" "$PREFIX/share/vulkan/"
    stamp vulkan-headers
fi

if ! have shaderc; then
    [ -f "$SHADERC_PREBUILT_TAR" ] || {
        printf 'error: missing %s\n' "$SHADERC_PREBUILT_TAR" >&2
        exit 1
    }
    [ "$(sha256sum "$SHADERC_PREBUILT_TAR" | awk '{print $1}')" = "$SHADERC_PREBUILT_SHA" ] || {
        printf 'error: checksum mismatch for %s\n' "$SHADERC_PREBUILT_TAR" >&2
        exit 1
    }
    mkdir -p "$PREFIX/include" "$PREFIX/lib/pkgconfig"
    tar -xzf "$SHADERC_PREBUILT_TAR" -C "$PREFIX"
    cat >"$PREFIX/lib/pkgconfig/shaderc_combined.pc" <<PC
prefix=$PREFIX
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include

Name: shaderc
Description: Tools and libraries for Vulkan shader compilation
Version: $SHADERC_VERSION
Libs: -L\${libdir} -l:libshaderc_combined.a -l:libstdc++.a
Cflags: -I\${includedir}
PC
    cp "$PREFIX/lib/pkgconfig/shaderc_combined.pc" "$PREFIX/lib/pkgconfig/shaderc.pc"
    stamp shaderc
fi

if ! have libplacebo; then
    unset PYTHONPATH
    export PYTHONNOUSERSITE=1
    LP="libplacebo-v$LIBPLACEBO_VERSION.tar.bz2"
    fetch "https://code.videolan.org/videolan/libplacebo/-/archive/v$LIBPLACEBO_VERSION/libplacebo-v$LIBPLACEBO_VERSION.tar.bz2" \
        14ceeeb14d1c08fcdb1a782a06129da11f842c6fddbf3a43c1c4061406ad03d2 "$SRC/$LP"
    extract "$SRC/$LP" "$SRC/libplacebo-v$LIBPLACEBO_VERSION"
    fetch "https://github.com/fastfloat/fast_float/archive/refs/tags/v$FAST_FLOAT_VERSION.tar.gz" \
        71bb9e865b888df01351558db5a39586d13ea97ada162156b3a07784d8c1b26b "$SRC/fast_float-$FAST_FLOAT_VERSION.tar.gz"
    fetch "https://files.pythonhosted.org/packages/6e/5a/d62b24fe1c7c2f34e15c2aa4418a5327a8550fdc272999a59e0dddebc3ee/glad2-$GLAD_VERSION.tar.gz" \
        b84079b9fa404f37171b961bdd1d8da21370e6c818defb8481c5b3fe3d6436da "$SRC/glad2-$GLAD_VERSION.tar.gz"
    fetch "https://files.pythonhosted.org/packages/df/bf/f7da0350254c0ed7c72f3e33cef02e048281fec7ecec5f032d4aac52226b/jinja2-$JINJA_VERSION.tar.gz" \
        0137fb05990d35f1275a587e9aee6d56da821fc83491a0fb838183be43f66d6d "$SRC/jinja2-$JINJA_VERSION.tar.gz"
    fetch "https://files.pythonhosted.org/packages/38/9b/e422a865e1d5d57d0e509b4e0bf1c1a70a7f6382c29a5aa428df994c8bc8/markupsafe-$MARKUPSAFE_VERSION.tar.gz" \
        2e9ad7dd851bf45fab9f75cbff4cb493fee9979e8d8c7c9c3ee119022518edd6 "$SRC/markupsafe-$MARKUPSAFE_VERSION.tar.gz"
    for item in "fast_float-$FAST_FLOAT_VERSION" "glad2-$GLAD_VERSION" "jinja2-$JINJA_VERSION" "markupsafe-$MARKUPSAFE_VERSION"; do
        extract "$SRC/$item.tar.gz" "$SRC/$item"
    done
    third="$SRC/libplacebo-v$LIBPLACEBO_VERSION/3rdparty"
    rm -rf "$third/fast_float" "$third/glad" "$third/jinja" "$third/markupsafe"
    cp -R "$SRC/fast_float-$FAST_FLOAT_VERSION" "$third/fast_float"
    mkdir -p "$third/glad" "$third/jinja/src" "$third/markupsafe/src"
    cp -R "$SRC/glad2-$GLAD_VERSION/glad" "$third/glad/"
    cp -R "$SRC/jinja2-$JINJA_VERSION/src/jinja2" "$third/jinja/src/"
    cp -R "$SRC/markupsafe-$MARKUPSAFE_VERSION/src/markupsafe" "$third/markupsafe/src/"
    run_log libplacebo "${MESON_SETUP_LIBPLACEBO[@]}" "$SRC/libplacebo-v$LIBPLACEBO_VERSION/build" \
        "$SRC/libplacebo-v$LIBPLACEBO_VERSION" --prefix="$PREFIX" --libdir=lib \
        --buildtype=release --default-library=static -Dvulkan=enabled \
        -Dvulkan-registry="$PREFIX/share/vulkan/registry/vk.xml" \
        -Dshaderc=enabled -Dglslang=disabled -Dlcms=disabled -Dopengl=disabled \
        -Dd3d11=disabled -Dlibdovi=disabled -Ddemos=false -Dtests=false -Dbench=false
    run_log libplacebo-build "$NINJA" -C "$SRC/libplacebo-v$LIBPLACEBO_VERSION/build" install
    [ -f "$PREFIX/lib/libplacebo.a" ] || [ -f "$PREFIX/lib/libplacebo.so" ] || {
        printf 'error: libplacebo library missing from %s/lib after install\n' "$PREFIX" >&2
        exit 1
    }
    stamp libplacebo
fi

printf 'linux dependencies ready in %s\n' "$PREFIX"
