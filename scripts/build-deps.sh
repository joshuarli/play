#!/bin/bash
set -euo pipefail

# Builds the release closure from pinned sources. Runtime libraries come only
# from this prefix; Homebrew is used for build tools, never for linked dylibs.
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX="$ROOT/.work/deps/release"
FFMPEG_PREFIX="$PREFIX/ffmpeg"
SRC="$ROOT/.work/tmp/release"
LOGS="$SRC/logs"
JOBS=${JOBS:-$(sysctl -n hw.ncpu)}
export TMPDIR="$ROOT/.work/tmp"
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-27.0}
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$PREFIX/ffmpeg/lib/pkgconfig"
export PKG_CONFIG_LIBDIR="$PKG_CONFIG_PATH"

mkdir -p "$PREFIX" "$SRC" "$LOGS"

INPUT_HASH=$(cat "$ROOT/scripts/build-deps.sh" "$ROOT/scripts/build-ffmpeg.sh" \
    "$ROOT/scripts/ffmpeg-components.txt" | shasum -a 256 | awk '{print $1}')

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
DAV1D_VERSION=1.5.4
FREETYPE_VERSION=2.15.0
FRIBIDI_VERSION=1.0.17
HARFBUZZ_VERSION=15.0.0
SHADERC_VERSION=2026.4
GLSLANG_REV=e1b562a8bed273a02f30b59b66a5d499793cede5
SPIRV_HEADERS_REV=04fd3caa1e8267e4d95c806cad901181728e1006
SPIRV_TOOLS_REV=ef96ed763b43b59b33b31b362f09a02b729fa1c9
LIBPLACEBO_VERSION=7.387.0
LIBASS_VERSION=0.17.5
GLAD_VERSION=2.0.8
JINJA_VERSION=3.1.6
MARKUPSAFE_VERSION=3.0.4
FAST_FLOAT_VERSION=8.3.2
MOLTENVK_VERSION=1.4.2

export VULKAN_VERSION DAV1D_VERSION FREETYPE_VERSION FRIBIDI_VERSION
export HARFBUZZ_VERSION SHADERC_VERSION GLSLANG_REV SPIRV_HEADERS_REV
export SPIRV_TOOLS_REV LIBPLACEBO_VERSION LIBASS_VERSION GLAD_VERSION
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

if ! have dav1d; then
    DAV1D="dav1d-$DAV1D_VERSION.tar.bz2"
    fetch "https://code.videolan.org/videolan/dav1d/-/archive/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.bz2" \
        2abfb0c89212e6e4733a54e0ae509ec00a5b845a6360946f918806e14aedb011 "$SRC/$DAV1D"
    extract "$SRC/$DAV1D" "$SRC/dav1d-$DAV1D_VERSION"
    run_log dav1d meson setup "$SRC/dav1d-$DAV1D_VERSION/build" "$SRC/dav1d-$DAV1D_VERSION" \
        --prefix="$PREFIX" --buildtype=release --default-library=shared \
        -Denable_tools=false -Denable_tests=false
    run_log dav1d-build ninja -C "$SRC/dav1d-$DAV1D_VERSION/build" install
    stamp dav1d
fi

SDK_ZLIB_VERSION=$(rg -o 'define ZLIB_VERSION "[0-9.]+"' "$(xcrun --show-sdk-path)/usr/include/zlib.h" |
    rg -o '[0-9]+(\.[0-9]+)+')
[ -n "$SDK_ZLIB_VERSION" ] || {
    printf 'error: cannot read ZLIB_VERSION from the SDK zlib.h\n' >&2
    exit 1
}
mkdir -p "$PREFIX/lib/pkgconfig"
cat >"$PREFIX/lib/pkgconfig/zlib.pc" <<EOF
prefix=/usr
exec_prefix=\${prefix}
libdir=/usr/lib
includedir=\${prefix}/include

Name: zlib
Description: macOS SDK zlib
Version: $SDK_ZLIB_VERSION
Libs: -lz
Cflags:
EOF

if ! have freetype; then
    FT="freetype-$FREETYPE_VERSION.tar.xz"
    fetch "https://downloads.sourceforge.net/project/freetype/freetype2/$FREETYPE_VERSION/freetype-$FREETYPE_VERSION.tar.xz" \
        93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5 "$SRC/$FT"
    extract "$SRC/$FT" "$SRC/freetype-$FREETYPE_VERSION"
    run_log freetype meson setup "$SRC/freetype-$FREETYPE_VERSION/build" \
        "$SRC/freetype-$FREETYPE_VERSION" --prefix="$PREFIX" --buildtype=release \
        --default-library=shared -Dzlib=system -Dpng=disabled -Dbrotli=disabled \
        -Dharfbuzz=disabled -Dbzip2=disabled -Dtests=disabled
    run_log freetype-build ninja -C "$SRC/freetype-$FREETYPE_VERSION/build" install
    stamp freetype
fi

if ! have fribidi; then
    FB="fribidi-$FRIBIDI_VERSION.tar.xz"
    fetch "https://github.com/fribidi/fribidi/releases/download/v$FRIBIDI_VERSION/fribidi-$FRIBIDI_VERSION.tar.xz" \
        6949dcde27d41cebad1fd741fcafc36d55a1020d2d872d4a6eb3914caabbada2 "$SRC/$FB"
    extract "$SRC/$FB" "$SRC/fribidi-$FRIBIDI_VERSION"
    run_log fribidi-configure bash -c "cd '$SRC/fribidi-$FRIBIDI_VERSION' && ./configure --prefix='$PREFIX' --disable-static --enable-shared --disable-docs"
    run_log fribidi-build make -C "$SRC/fribidi-$FRIBIDI_VERSION/lib" -j"$JOBS"
    run_log fribidi-install make -C "$SRC/fribidi-$FRIBIDI_VERSION/lib" install
    install -d "$PREFIX/lib/pkgconfig"
    install -m 644 "$SRC/fribidi-$FRIBIDI_VERSION/fribidi.pc" "$PREFIX/lib/pkgconfig/"
    stamp fribidi
fi

if ! have harfbuzz; then
    HB="harfbuzz-$HARFBUZZ_VERSION.tar.xz"
    fetch "https://github.com/harfbuzz/harfbuzz/releases/download/$HARFBUZZ_VERSION/harfbuzz-$HARFBUZZ_VERSION.tar.xz" \
        968536e9abd782f07f7954044c0c4020199fcdd39b41d221ec42d2a3be4a44df "$SRC/$HB"
    extract "$SRC/$HB" "$SRC/harfbuzz-$HARFBUZZ_VERSION"
    run_log harfbuzz meson setup "$SRC/harfbuzz-$HARFBUZZ_VERSION/build" \
        "$SRC/harfbuzz-$HARFBUZZ_VERSION" --prefix="$PREFIX" --buildtype=release \
        --default-library=shared -Dglib=disabled -Dgobject=disabled -Dcairo=disabled \
        -Dfreetype=disabled -Dicu=disabled -Dgraphite2=disabled -Dtests=disabled \
        -Ddocs=disabled -Dutilities=disabled
    run_log harfbuzz-build ninja -C "$SRC/harfbuzz-$HARFBUZZ_VERSION/build" install
    stamp harfbuzz
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
    run_log libplacebo meson setup "$SRC/libplacebo-v$LIBPLACEBO_VERSION/build" \
        "$SRC/libplacebo-v$LIBPLACEBO_VERSION" --prefix="$PREFIX" --buildtype=release \
        --default-library=shared -Dvulkan=enabled \
        -Dvulkan-registry="$PREFIX/share/vulkan/registry/vk.xml" \
        -Dshaderc=enabled -Dglslang=disabled -Dlcms=disabled -Dopengl=disabled \
        -Dd3d11=disabled -Dlibdovi=disabled -Ddemos=false -Dtests=false -Dbench=false
    run_log libplacebo-build ninja -C "$SRC/libplacebo-v$LIBPLACEBO_VERSION/build" install
    stamp libplacebo
fi

if ! have libass; then
    ASS="libass-$LIBASS_VERSION.tar.xz"
    fetch "https://github.com/libass/libass/releases/download/$LIBASS_VERSION/libass-$LIBASS_VERSION.tar.xz" \
        2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc "$SRC/$ASS"
    extract "$SRC/$ASS" "$SRC/libass-$LIBASS_VERSION"
    run_log libass meson setup "$SRC/libass-$LIBASS_VERSION/build" \
        "$SRC/libass-$LIBASS_VERSION" --prefix="$PREFIX" --buildtype=release \
        --default-library=shared -Dfontconfig=disabled -Dcoretext=enabled \
        -Dlibunibreak=disabled -Dtest=disabled -Dcompare=disabled \
        -Dprofile=disabled -Dfuzz=disabled -Dcheckasm=disabled
    run_log libass-build ninja -C "$SRC/libass-$LIBASS_VERSION/build" install
    stamp libass
fi

if ! have moltenvk; then
    MV="MoltenVK-macos.tar"
    fetch "https://github.com/KhronosGroup/MoltenVK/releases/download/v$MOLTENVK_VERSION/$MV" \
        f95765a6229cb7b915990a2890ce12ebe36a730b021545d3d52ae69ce4c4024e "$SRC/$MV"
    rm -rf "$SRC/MoltenVK"
    tar -xf "$SRC/$MV" -C "$SRC"
    cp "$SRC/MoltenVK/MoltenVK/dynamic/dylib/macOS/libMoltenVK.dylib" "$PREFIX/lib/"
    mkdir -p "$PREFIX/share/vulkan/icd.d"
    cp "$SRC/MoltenVK/MoltenVK/dynamic/dylib/macOS/MoltenVK_icd.json" \
        "$PREFIX/share/vulkan/icd.d/"
    stamp moltenvk
fi

if ! have ffmpeg; then
    PREFIX="$FFMPEG_PREFIX" DAV1D_PREFIX="$PREFIX" \
        "$ROOT/scripts/build-ffmpeg.sh" >"$LOGS/ffmpeg.log" 2>&1 || {
        tail -60 "$LOGS/ffmpeg.log" >&2
        exit 1
    }
    stamp ffmpeg
fi

printf 'release dependencies ready in %s\n' "$PREFIX"
