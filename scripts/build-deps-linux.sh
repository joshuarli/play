#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX="$ROOT/.work/deps/linux"
SRC="$ROOT/.work/tmp/linux"
LOGS="$SRC/logs"
JOBS=${JOBS:-$(nproc)}
export TMPDIR="$ROOT/.work/tmp"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$PREFIX/lib64/pkgconfig:$PREFIX/share/pkgconfig"
export CFLAGS="-O2 -fPIC"
export CXXFLAGS="-O2 -fPIC"

mkdir -p "$PREFIX" "$SRC" "$LOGS"

INPUT_HASH=$(sha256sum "$ROOT/scripts/build-deps-linux.sh" | awk '{print $1}')

case "$(uname -sm)/$(ldd --version 2>&1)" in
    "Linux x86_64/"*musl*) ;;
    *)
        printf 'error: requires x86_64 musl Linux; run through scripts/linux-container.sh\n' >&2
        exit 1
        ;;
esac

for tool in curl tar sha256sum cmake meson ninja pkg-config python3 gperf make; do
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

meson_static() {
    local name=$1 dir=$2
    shift 2
    run_log "$name" meson setup "$dir/build" "$dir" --prefix="$PREFIX" --libdir=lib \
        --buildtype=release --default-library=static "$@"
    run_log "$name-build" ninja -C "$dir/build" install
}

VULKAN_VERSION=1.4.363.0
ZLIB_VERSION=1.3.2
EXPAT_VERSION=2.9.0
DAV1D_VERSION=1.5.4
FREETYPE_VERSION=2.15.0
FRIBIDI_VERSION=1.0.17
HARFBUZZ_VERSION=15.0.0
FONTCONFIG_VERSION=2.16.0
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

if ! have zlib; then
    Z="zlib-$ZLIB_VERSION.tar.xz"
    fetch "https://github.com/madler/zlib/releases/download/v$ZLIB_VERSION/$Z" \
        d7a0654783a4da529d1bb793b7ad9c3318020af77667bcae35f95d0e42a792f3 "$SRC/$Z"
    extract "$SRC/$Z" "$SRC/zlib-$ZLIB_VERSION"
    run_log zlib-configure bash -c "cd '$SRC/zlib-$ZLIB_VERSION' && ./configure --prefix='$PREFIX' --static"
    run_log zlib-build make -C "$SRC/zlib-$ZLIB_VERSION" -j"$JOBS" install
    stamp zlib
fi

if ! have expat; then
    E="expat-$EXPAT_VERSION.tar.xz"
    fetch "https://github.com/libexpat/libexpat/releases/download/R_${EXPAT_VERSION//./_}/$E" \
        1e6371862cc31999b368c3b89b49994f0677e1bab5f1b2b85ae3741f5d803051 "$SRC/$E"
    extract "$SRC/$E" "$SRC/expat-$EXPAT_VERSION"
    run_log expat cmake -S "$SRC/expat-$EXPAT_VERSION" -B "$SRC/build-expat" \
        -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_INSTALL_LIBDIR=lib \
        -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DEXPAT_BUILD_TOOLS=OFF \
        -DEXPAT_BUILD_EXAMPLES=OFF -DEXPAT_BUILD_TESTS=OFF -DEXPAT_BUILD_DOCS=OFF \
        -DCMAKE_POSITION_INDEPENDENT_CODE=ON
    run_log expat-build cmake --build "$SRC/build-expat" --target install -j"$JOBS"
    stamp expat
fi

if ! have dav1d; then
    DAV1D="dav1d-$DAV1D_VERSION.tar.bz2"
    fetch "https://code.videolan.org/videolan/dav1d/-/archive/$DAV1D_VERSION/dav1d-$DAV1D_VERSION.tar.bz2" \
        2abfb0c89212e6e4733a54e0ae509ec00a5b845a6360946f918806e14aedb011 "$SRC/$DAV1D"
    extract "$SRC/$DAV1D" "$SRC/dav1d-$DAV1D_VERSION"
    meson_static dav1d "$SRC/dav1d-$DAV1D_VERSION" -Denable_tools=false -Denable_tests=false
    stamp dav1d
fi

if ! have freetype; then
    FT="freetype-$FREETYPE_VERSION.tar.xz"
    fetch "https://downloads.sourceforge.net/project/freetype/freetype2/$FREETYPE_VERSION/freetype-$FREETYPE_VERSION.tar.xz" \
        93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5 "$SRC/$FT"
    extract "$SRC/$FT" "$SRC/freetype-$FREETYPE_VERSION"
    meson_static freetype "$SRC/freetype-$FREETYPE_VERSION" -Dzlib=system -Dpng=disabled \
        -Dbrotli=disabled -Dharfbuzz=disabled -Dbzip2=disabled -Dtests=disabled
    stamp freetype
fi

if ! have fribidi; then
    FB="fribidi-$FRIBIDI_VERSION.tar.xz"
    fetch "https://github.com/fribidi/fribidi/releases/download/v$FRIBIDI_VERSION/fribidi-$FRIBIDI_VERSION.tar.xz" \
        6949dcde27d41cebad1fd741fcafc36d55a1020d2d872d4a6eb3914caabbada2 "$SRC/$FB"
    extract "$SRC/$FB" "$SRC/fribidi-$FRIBIDI_VERSION"
    meson_static fribidi "$SRC/fribidi-$FRIBIDI_VERSION" -Ddocs=false -Dbin=false -Dtests=false
    stamp fribidi
fi

if ! have harfbuzz; then
    HB="harfbuzz-$HARFBUZZ_VERSION.tar.xz"
    fetch "https://github.com/harfbuzz/harfbuzz/releases/download/$HARFBUZZ_VERSION/harfbuzz-$HARFBUZZ_VERSION.tar.xz" \
        968536e9abd782f07f7954044c0c4020199fcdd39b41d221ec42d2a3be4a44df "$SRC/$HB"
    extract "$SRC/$HB" "$SRC/harfbuzz-$HARFBUZZ_VERSION"
    meson_static harfbuzz "$SRC/harfbuzz-$HARFBUZZ_VERSION" -Dglib=disabled -Dgobject=disabled \
        -Dcairo=disabled -Dfreetype=disabled -Dicu=disabled -Dgraphite2=disabled \
        -Dtests=disabled -Ddocs=disabled -Dutilities=disabled
    stamp harfbuzz
fi

if ! have fontconfig; then
    FC="fontconfig-$FONTCONFIG_VERSION.tar.xz"
    fetch "https://www.freedesktop.org/software/fontconfig/release/$FC" \
        6a33dc555cc9ba8b10caf7695878ef134eeb36d0af366041f639b1da9b6ed220 "$SRC/$FC"
    extract "$SRC/$FC" "$SRC/fontconfig-$FONTCONFIG_VERSION"
    fc="$SRC/fontconfig-$FONTCONFIG_VERSION"
    run_log fontconfig meson setup "$fc/build" "$fc" --prefix="$PREFIX" --libdir=lib \
        --buildtype=release --default-library=static -Dtests=disabled -Dtools=disabled \
        -Dnls=disabled -Ddoc=disabled -Dcache-build=disabled -Dxml-backend=expat \
        -Dsysconfdir=/etc -Dlocalstatedir=/var
    run_log fontconfig-build ninja -C "$fc/build"
    rm -rf "$SRC/fontconfig-dest"
    run_log fontconfig-install env DESTDIR="$SRC/fontconfig-dest" meson install -C "$fc/build" --no-rebuild
    cp -R "$SRC/fontconfig-dest$PREFIX/lib/." "$PREFIX/lib/"
    cp -R "$SRC/fontconfig-dest$PREFIX/include/." "$PREFIX/include/"
    stamp fontconfig
fi

if ! have shaderc; then
    SH="shaderc-$SHADERC_VERSION.tar.gz"
    fetch "https://github.com/google/shaderc/archive/refs/tags/v$SHADERC_VERSION.tar.gz" \
        f06ce5bcca94e5df7f34e115743597d0ad2e13c5fe9213c67dc8c76031241947 "$SRC/$SH"
    extract "$SRC/$SH" "$SRC/shaderc-$SHADERC_VERSION"
    for dep in glslang spirv-headers spirv-tools; do
        case "$dep" in
            glslang)
                rev=$GLSLANG_REV
                dep_sha=907174a24713c6202c146f164bf81783f1fbc79c8cb821a30f18f159eb980312 ;;
            spirv-headers)
                rev=$SPIRV_HEADERS_REV
                dep_sha=392f4801409aad9c4f1b77745f179952fd6264e4c8bd0fc1bc45dfa6807bf6d0 ;;
            spirv-tools)
                rev=$SPIRV_TOOLS_REV
                dep_sha=82c62146083fd558735a3171cf97cfc47903ca7d368482e87f94bd44883c0f00 ;;
        esac
        archive="$SRC/$dep-$rev.tar.gz"
        fetch "https://github.com/KhronosGroup/$dep/archive/$rev.tar.gz" "$dep_sha" "$archive"
        rm -rf "$SRC/shaderc-$SHADERC_VERSION/third_party/$dep"
        mkdir -p "$SRC/shaderc-$SHADERC_VERSION/third_party/$dep"
        tar -xf "$archive" -C "$SRC/shaderc-$SHADERC_VERSION/third_party/$dep" --strip-components=1
    done
    run_log shaderc cmake -S "$SRC/shaderc-$SHADERC_VERSION" \
        -B "$SRC/build-shaderc" -DCMAKE_INSTALL_PREFIX="$PREFIX" \
        -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_BUILD_TYPE=MinSizeRel \
        -DSHADERC_SKIP_TESTS=ON -DSHADERC_SKIP_EXAMPLES=ON \
        -DSHADERC_SKIP_COPYRIGHT_CHECK=ON -DSKIP_GLSLANG_INSTALL=ON \
        -DSKIP_SPIRV_TOOLS_INSTALL=ON -DSKIP_GOOGLETEST_INSTALL=ON \
        -DBUILD_SHARED_LIBS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON
    run_log shaderc-build cmake --build "$SRC/build-shaderc" -j"$JOBS"
    run_log shaderc-install cmake --install "$SRC/build-shaderc"
    rm -f "$PREFIX"/lib/libshaderc_shared.so* "$PREFIX"/lib/libSPIRV-Tools-shared.so \
        "$PREFIX/lib/pkgconfig/shaderc.pc"
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
    meson_static libplacebo "$SRC/libplacebo-v$LIBPLACEBO_VERSION" -Dvulkan=enabled \
        -Dvulkan-registry="$PREFIX/share/vulkan/registry/vk.xml" \
        -Dshaderc=enabled -Dglslang=disabled -Dlcms=disabled -Dopengl=disabled \
        -Dd3d11=disabled -Dlibdovi=disabled -Ddemos=false -Dtests=false -Dbench=false
    stamp libplacebo
fi

if ! have libass; then
    ASS="libass-$LIBASS_VERSION.tar.xz"
    fetch "https://github.com/libass/libass/releases/download/$LIBASS_VERSION/libass-$LIBASS_VERSION.tar.xz" \
        2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc "$SRC/$ASS"
    extract "$SRC/$ASS" "$SRC/libass-$LIBASS_VERSION"
    meson_static libass "$SRC/libass-$LIBASS_VERSION" -Dfontconfig=enabled -Dcoretext=disabled \
        -Dlibunibreak=disabled -Dtest=disabled -Dcompare=disabled -Dprofile=disabled \
        -Dfuzz=disabled -Dcheckasm=disabled
    stamp libass
fi

PREFIX="$PREFIX/ffmpeg" DAV1D_PREFIX="$PREFIX" "$ROOT/scripts/build-ffmpeg.sh" \
    >"$LOGS/ffmpeg.log" 2>&1 || {
    tail -60 "$LOGS/ffmpeg.log" >&2
    exit 1
}

sed -i 's/^Libs: .*/& -l:libstdc++.a/' "$PREFIX/lib/pkgconfig/shaderc_combined.pc"

printf 'linux dependencies ready in %s\n' "$PREFIX"
