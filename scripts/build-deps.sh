#!/bin/bash
set -euo pipefail

# Builds the release closure from pinned sources. Runtime libraries come only
# from this prefix; Homebrew is used for build tools, never for linked dylibs.
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX="$ROOT/.work/deps/release"
SRC="$ROOT/.work/tmp/release"
LOGS="$SRC/logs"
JOBS=${JOBS:-$(/usr/sbin/sysctl -n hw.ncpu 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}
export TMPDIR="$ROOT/.work/tmp"
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-27.0}
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$PKG_CONFIG_PATH"

mkdir -p "$PREFIX" "$SRC" "$LOGS"
# Release tarballs carry no .git, so a build-time `git describe` (meson's
# vcs_tag in libplacebo) would describe this checkout instead and stamp
# Player's own version into the dependency. Stop git's search at $SRC.
export GIT_CEILING_DIRECTORIES="$SRC"

INPUT_HASH=$(cat "$ROOT/scripts/build-deps.sh" "$ROOT/scripts/patches/libplacebo-metal.patch" |
    shasum -a 256 | awk '{print $1}')

for tool in curl tar shasum xcrun; do
    command -v "$tool" >/dev/null || {
        printf 'error: %s is required (development tool)\n' "$tool" >&2
        exit 1
    }
done

require_version() {
    local tool=$1 minimum=$2 found
    command -v "$tool" >/dev/null || {
        printf 'error: %s is required (development tool)\n' "$tool" >&2
        exit 1
    }
    found=$("$tool" --version 2>&1 | head -1 | rg -o '[0-9]+(\.[0-9]+)+' | head -1)
    if [ -z "$found" ] ||
        [ "$(printf '%s\n%s\n' "$minimum" "$found" | sort -V | head -1)" != "$minimum" ]; then
        printf 'error: %s %s or newer is required, found %s\n' \
            "$tool" "$minimum" "${found:-no version}" >&2
        exit 1
    fi
}

require_version cmake 3.22.1
require_version meson 1.3.0
require_version ninja 1.10
require_version pkg-config 0.29
require_version python3 3.9

fetch() {
    local url=$1 sha=$2 out=$3
    if [ ! -f "$out" ] || [ "$(shasum -a 256 "$out" | awk '{print $1}')" != "$sha" ]; then
        printf 'download %s\n' "$url"
        curl -fsSL "$url" -o "$out"
    fi
    if [ "$(shasum -a 256 "$out" | awk '{print $1}')" != "$sha" ]; then
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
MOLTENVK_VERSION=1.4.2

export VULKAN_VERSION SHADERC_VERSION GLSLANG_REV
export SPIRV_HEADERS_REV SPIRV_TOOLS_REV LIBPLACEBO_VERSION GLAD_VERSION
export JINJA_VERSION MARKUPSAFE_VERSION FAST_FLOAT_VERSION MOLTENVK_VERSION

if ! have vulkan-headers; then
    VH="Vulkan-Headers-$VULKAN_VERSION.tar.gz"
    fetch "https://github.com/KhronosGroup/Vulkan-Headers/archive/refs/tags/vulkan-sdk-$VULKAN_VERSION.tar.gz" \
        4a078be12bef21cfebc09d878b77a63cff9d68f899254a0b00d0e37ef73e7f7e "$SRC/$VH"
    extract "$SRC/$VH" "$SRC/Vulkan-Headers-vulkan-sdk-$VULKAN_VERSION"
    run_log vulkan-headers cmake -S "$SRC/Vulkan-Headers-vulkan-sdk-$VULKAN_VERSION" \
        -B "$SRC/build-vulkan-headers" -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DCMAKE_BUILD_TYPE=Release
    run_log vulkan-headers-install cmake --build "$SRC/build-vulkan-headers" --target install
    stamp vulkan-headers
fi

if ! have vulkan-loader; then
    VL="Vulkan-Loader-$VULKAN_VERSION.tar.gz"
    fetch "https://github.com/KhronosGroup/Vulkan-Loader/archive/refs/tags/vulkan-sdk-$VULKAN_VERSION.tar.gz" \
        941eff558fc74f248745fb7df367640cf89a8457f216dae498a74ae14b6ba6f7 "$SRC/$VL"
    extract "$SRC/$VL" "$SRC/Vulkan-Loader-vulkan-sdk-$VULKAN_VERSION"
    run_log vulkan-loader cmake -S "$SRC/Vulkan-Loader-vulkan-sdk-$VULKAN_VERSION" \
        -B "$SRC/build-vulkan-loader" -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DCMAKE_BUILD_TYPE=Release -DVULKAN_HEADERS_INSTALL_DIR="$PREFIX" \
        -DBUILD_TESTS=OFF -DUPDATE_DEPS=OFF
    run_log vulkan-loader-build cmake --build "$SRC/build-vulkan-loader" -j"$JOBS"
    run_log vulkan-loader-install cmake --install "$SRC/build-vulkan-loader"
    stamp vulkan-loader
fi

if ! have shaderc; then
    SH="shaderc-$SHADERC_VERSION.tar.gz"
    fetch "https://github.com/google/shaderc/archive/refs/tags/v$SHADERC_VERSION.tar.gz" \
        f06ce5bcca94e5df7f34e115743597d0ad2e13c5fe9213c67dc8c76031241947 "$SRC/$SH"
    extract "$SRC/$SH" "$SRC/shaderc-$SHADERC_VERSION"
    for dep in glslang spirv-headers spirv-tools; do
        case "$dep" in
            glslang) rev=$GLSLANG_REV ;;
            spirv-headers) rev=$SPIRV_HEADERS_REV ;;
            spirv-tools) rev=$SPIRV_TOOLS_REV ;;
        esac
        archive="$SRC/$dep-$rev.tar.gz"
        case "$dep" in
            glslang) dep_sha=907174a24713c6202c146f164bf81783f1fbc79c8cb821a30f18f159eb980312 ;;
            spirv-headers) dep_sha=392f4801409aad9c4f1b77745f179952fd6264e4c8bd0fc1bc45dfa6807bf6d0 ;;
            spirv-tools) dep_sha=82c62146083fd558735a3171cf97cfc47903ca7d368482e87f94bd44883c0f00 ;;
        esac
        fetch "https://github.com/KhronosGroup/$dep/archive/$rev.tar.gz" "$dep_sha" "$archive"
        extract "$archive" "$SRC/$dep-$rev"
        rm -rf "$SRC/shaderc-$SHADERC_VERSION/third_party/$dep"
        cp -R "$SRC/$dep-$rev" "$SRC/shaderc-$SHADERC_VERSION/third_party/$dep"
    done
    run_log shaderc cmake -S "$SRC/shaderc-$SHADERC_VERSION" \
        -B "$SRC/build-shaderc" -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DCMAKE_BUILD_TYPE=MinSizeRel -DSHADERC_SKIP_TESTS=ON \
        -DSKIP_GLSLANG_INSTALL=ON -DSKIP_SPIRV_TOOLS_INSTALL=ON \
        -DSKIP_GOOGLETEST_INSTALL=ON -DBUILD_SHARED_LIBS=ON \
        -DCMAKE_MACOSX_RPATH=ON
    run_log shaderc-build cmake --build "$SRC/build-shaderc" -j"$JOBS"
    run_log shaderc-install cmake --install "$SRC/build-shaderc"
    stamp shaderc
fi

if ! have spirv-cross; then
    SC="SPIRV-Cross-$VULKAN_VERSION.tar.gz"
    fetch "https://github.com/KhronosGroup/SPIRV-Cross/archive/refs/tags/vulkan-sdk-$VULKAN_VERSION.tar.gz" \
        f708982e88b763ef5b0394ed468fc2c3628f68cdab89f2e7a946cd628c04e721 "$SRC/$SC"
    extract "$SRC/$SC" "$SRC/SPIRV-Cross-vulkan-sdk-$VULKAN_VERSION"
    rm -rf "$SRC/build-spirv-cross"
    run_log spirv-cross cmake -S "$SRC/SPIRV-Cross-vulkan-sdk-$VULKAN_VERSION" \
        -B "$SRC/build-spirv-cross" -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DCMAKE_BUILD_TYPE=MinSizeRel -DSPIRV_CROSS_SHARED=OFF \
        -DSPIRV_CROSS_STATIC=ON -DSPIRV_CROSS_CLI=OFF \
        -DSPIRV_CROSS_ENABLE_TESTS=OFF -DSPIRV_CROSS_ENABLE_HLSL=OFF \
        -DSPIRV_CROSS_ENABLE_CPP=OFF -DSPIRV_CROSS_ENABLE_REFLECT=OFF \
        -DSPIRV_CROSS_ENABLE_UTIL=OFF -DSPIRV_CROSS_FORCE_PIC=ON
    run_log spirv-cross-build cmake --build "$SRC/build-spirv-cross" -j"$JOBS"
    run_log spirv-cross-install cmake --install "$SRC/build-spirv-cross"
    stamp spirv-cross
fi

if ! have libplacebo; then
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
    # Metal backend from https://code.videolan.org/videolan/libplacebo/-/merge_requests/858
    # (head ea7599ef9e71f66930b567f25e2c2de73913b38c), which upstream never merged.
    run_log libplacebo-patch patch -d "$SRC/libplacebo-v$LIBPLACEBO_VERSION" -p1 --forward \
        -i "$ROOT/scripts/patches/libplacebo-metal.patch"
    run_log libplacebo meson setup "$SRC/libplacebo-v$LIBPLACEBO_VERSION/build" \
        "$SRC/libplacebo-v$LIBPLACEBO_VERSION" --prefix="$PREFIX" --buildtype=release \
        --default-library=shared -Dvulkan=enabled -Dmetal=enabled \
        -Dvulkan-registry="$PREFIX/share/vulkan/registry/vk.xml" \
        -Dshaderc=enabled -Dglslang=disabled -Dlcms=disabled -Dopengl=disabled \
        -Dd3d11=disabled -Dlibdovi=disabled -Ddemos=false -Dtests=false -Dbench=false
    run_log libplacebo-build ninja -C "$SRC/libplacebo-v$LIBPLACEBO_VERSION/build" install
    placebo_api=${LIBPLACEBO_VERSION#*.}
    placebo_dylib="libplacebo.${placebo_api%.*}.dylib"
    run_log libplacebo-id install_name_tool -id "@rpath/$placebo_dylib" "$PREFIX/lib/$placebo_dylib"
    stamp libplacebo
fi

if ! have moltenvk; then
    MV="MoltenVK-macos.tar"
    fetch "https://github.com/KhronosGroup/MoltenVK/releases/download/v$MOLTENVK_VERSION/$MV" \
        f95765a6229cb7b915990a2890ce12ebe36a730b021545d3d52ae69ce4c4024e "$SRC/$MV"
    rm -rf "$SRC/MoltenVK"
    tar -xf "$SRC/$MV" -C "$SRC"
    cp "$SRC/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib" "$PREFIX/lib/"
    mkdir -p "$PREFIX/share/vulkan/icd.d"
    sed "s#\"\./libMoltenVK\.dylib\"#\"$PREFIX/lib/libMoltenVK.dylib\"#" \
        "$SRC/MoltenVK/MoltenVK/dynamic/dylib/macOS/MoltenVK_icd.json" \
        >"$PREFIX/share/vulkan/icd.d/MoltenVK_icd.json"
    rg -qF "\"$PREFIX/lib/libMoltenVK.dylib\"" "$PREFIX/share/vulkan/icd.d/MoltenVK_icd.json" || {
        printf 'error: MoltenVK ICD no longer names ./libMoltenVK.dylib\n' >&2
        exit 1
    }
    stamp moltenvk
fi

printf 'release dependencies ready in %s\n' "$PREFIX"
