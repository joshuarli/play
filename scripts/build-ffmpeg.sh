#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=9.0.2
TAG=n$VERSION
URL="https://github.com/FFmpeg/FFmpeg/archive/refs/tags/$TAG.tar.gz"
SHA256=6e374ed621e48faa40639307dff48ba6fe574a509977956d2cce9669b7cc27e9
TARBALL="$ROOT/.work/tmp/ffmpeg-$VERSION.tar.gz"
SRC="$ROOT/.work/tmp/FFmpeg-$TAG"
PREFIX="$ROOT/.work/deps/ffmpeg-full"
BUILD="$PREFIX/build"
ALLOWLIST="$ROOT/scripts/ffmpeg-components.txt"
CONFIG_HASH=$(shasum -a 256 "$ALLOWLIST" | awk '{ print $1 }')

mkdir -p "$ROOT/.work/tmp" "$ROOT/.work/deps"
export TMPDIR="$ROOT/.work/tmp"

if [ -f "$PREFIX/lib/pkgconfig/libavcodec.pc" ] &&
    [ -f "$PREFIX/lib/libavcodec.a" ] &&
    [ "$(awk -F'"' '/FFMPEG_VERSION/ { print $2 }' \
        "$PREFIX/include/libavutil/ffversion.h" 2>/dev/null)" = "$VERSION" ] &&
    [ "$(cat "$PREFIX/ffmpeg-components.sha256" 2>/dev/null)" = "$CONFIG_HASH" ]; then
    printf 'FFmpeg %s already installed in %s\n' "$VERSION" "$PREFIX"
    exit 0
fi

if [ ! -f "$TARBALL" ] ||
    [ "$(shasum -a 256 "$TARBALL" | awk '{ print $1 }')" != "$SHA256" ]; then
    printf 'download %s\n' "$URL"
    curl -fsSL "$URL" -o "$TARBALL"
fi

if [ "$(shasum -a 256 "$TARBALL" | awk '{ print $1 }')" != "$SHA256" ]; then
    printf 'error: checksum mismatch for %s\n' "$TARBALL" >&2
    exit 1
fi

rm -rf "$SRC" "$PREFIX"
tar -xzf "$TARBALL" -C "$ROOT/.work/tmp"
printf '%s\n' "$VERSION" >"$SRC/VERSION"
mkdir -p "$BUILD"

configure_args=(
    --prefix="$PREFIX"
    --disable-shared --enable-static
    --disable-programs --disable-doc
    --disable-network --disable-avdevice --disable-autodetect
    --disable-everything
    --enable-libdav1d
    --enable-videotoolbox
)

while IFS= read -r line; do
    line=${line%%#*}
    [ -n "$line" ] || continue
    category=${line%%:*}
    names=${line#*:}
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
make -j"$(sysctl -n hw.ncpu)"
make install

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
