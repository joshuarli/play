#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=9.0.2
TAG=n$VERSION
URL="https://github.com/FFmpeg/FFmpeg/archive/refs/tags/$TAG.tar.gz"
SHA256=6e374ed621e48faa40639307dff48ba6fe574a509977956d2cce9669b7cc27e9
TARBALL="$ROOT/.work/tmp/ffmpeg-$VERSION.tar.gz"
SRC="$ROOT/.work/tmp/FFmpeg-$TAG"
PREFIX=${PREFIX:-"$ROOT/.work/deps/ffmpeg-full"}
BUILD="$PREFIX/build"
ALLOWLIST="$ROOT/scripts/ffmpeg-components.txt"
case "$(uname -s)" in
    Darwin) PLATFORM=macos; sha256() { shasum -a 256 "$1" | awk '{ print $1 }'; }
            JOBS=$(sysctl -n hw.ncpu) ;;
    Linux) PLATFORM=linux; sha256() { sha256sum "$1" | awk '{ print $1 }'; }
           JOBS=$(nproc) ;;
    *) printf 'error: unsupported platform %s\n' "$(uname -s)" >&2; exit 1 ;;
esac
CONFIG_HASH=$(sha256 "$ALLOWLIST")

mkdir -p "$ROOT/.work/tmp" "$ROOT/.work/deps"
export TMPDIR="$ROOT/.work/tmp"
if [ -n "${DAV1D_PREFIX:-}" ]; then
    export PKG_CONFIG_PATH="$DAV1D_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
fi

if [ -f "$PREFIX/lib/pkgconfig/libavcodec.pc" ] &&
    [ -f "$PREFIX/lib/libavcodec.a" ] &&
    [ "$(awk -F'"' '/FFMPEG_VERSION/ { print $2 }' \
        "$PREFIX/include/libavutil/ffversion.h" 2>/dev/null)" = "$VERSION" ] &&
    [ "$(cat "$PREFIX/ffmpeg-components.sha256" 2>/dev/null)" = "$CONFIG_HASH" ]; then
    printf 'FFmpeg %s already installed in %s\n' "$VERSION" "$PREFIX"
    exit 0
fi

if [ ! -f "$TARBALL" ] ||
    [ "$(sha256 "$TARBALL")" != "$SHA256" ]; then
    printf 'download %s\n' "$URL"
    curl -fsSL "$URL" -o "$TARBALL"
fi

if [ "$(sha256 "$TARBALL")" != "$SHA256" ]; then
    printf 'error: checksum mismatch for %s\n' "$TARBALL" >&2
    exit 1
fi

rm -rf "$SRC" "$PREFIX"
tar -xzf "$TARBALL" -C "$ROOT/.work/tmp"
printf '%s\n' "$VERSION" >"$SRC/VERSION"
mkdir -p "$BUILD"

if [ "$PLATFORM" = linux ]; then
    CONFIGURE_PREFIX=/ffmpeg
else
    CONFIGURE_PREFIX=$PREFIX
fi

configure_args=(
    --prefix="$CONFIGURE_PREFIX"
    --disable-shared --enable-static
    --disable-programs --disable-doc
    --disable-network --disable-avdevice --disable-autodetect
    --disable-everything
    --enable-libdav1d
)
case "$PLATFORM" in
    macos) configure_args+=(--enable-videotoolbox) ;;
    linux) configure_args+=(--enable-pic --arch=x86_64) ;;
esac

while IFS= read -r line; do
    line=${line%%#*}
    [ -n "$line" ] || continue
    category=${line%%:*}
    names=${line#*:}
    case "$category" in
        */*)
            [ "${category%%/*}" = "$PLATFORM" ] || continue
            category=${category#*/}
            ;;
    esac
    [ "$category" != "$line" ] || {
        printf 'error: malformed allowlist line: %s\n' "$line" >&2
        exit 1
    }
    for name in $names; do
        case "$category" in
            demuxers) configure_args+=(--enable-demuxer="$name") ;;
            video_decoders|subtitle_decoders)
                configure_args+=(--enable-decoder="$name") ;;
            parsers) configure_args+=(--enable-parser="$name") ;;
            bsfs) configure_args+=(--enable-bsf="$name") ;;
            filters) configure_args+=(--enable-filter="$name") ;;
            hwaccels) configure_args+=(--enable-hwaccel="$name") ;;
            protocols) configure_args+=(--enable-protocol="$name") ;;
            *)
                printf 'error: unknown allowlist category: %s\n' "$category" >&2
                exit 1
                ;;
        esac
    done
done <"$ALLOWLIST"

printf 'configure FFmpeg %s (video/demux allowlist, static)\n' "$VERSION"
cd "$BUILD"
"$SRC/configure" "${configure_args[@]}"

printf 'build FFmpeg %s\n' "$VERSION"
make -j"$JOBS"
if [ "$PLATFORM" = linux ]; then
    make install DESTDIR="$BUILD/stage"
    cp -R "$BUILD/stage$CONFIGURE_PREFIX/." "$PREFIX/"
    rm -rf "$BUILD/stage"
    sed -i "s|=$CONFIGURE_PREFIX|=$PREFIX|" "$PREFIX"/lib/pkgconfig/*.pc
else
    make install
fi

printf '%s\n' "$CONFIG_HASH" >"$PREFIX/ffmpeg-components.sha256"

if [ -e "$PREFIX/bin/ffmpeg" ] || [ -e "$PREFIX/bin/ffprobe" ]; then
    printf 'error: programs were installed in %s/bin\n' "$PREFIX" >&2
    exit 1
fi

printf 'verify FFmpeg %s static libraries\n' "$VERSION"
PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig" pkg-config --modversion libavcodec
ls -l "$PREFIX/lib"/libavcodec.a "$PREFIX/lib"/libavformat.a \
    "$PREFIX/lib"/libavfilter.a "$PREFIX/lib"/libavutil.a \
    "$PREFIX/lib"/libswresample.a "$PREFIX/lib"/libswscale.a
