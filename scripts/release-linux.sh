#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DIST="$ROOT/dist"
NAME=player-x86_64-linux-musl
TARBALL="$DIST/$NAME.tar.gz"
STAGE="$ROOT/.work/tmp/release-linux"

"$ROOT/scripts/build-linux.sh"
"$ROOT/scripts/audit-linux.sh"

runtime_packages=$(apk list -I 2>/dev/null |
    awk '$1 ~ /^(musl|vulkan-loader|wayland-libs-client|alsa-lib|libva|fontconfig|harfbuzz|freetype)-[0-9]/ {print "  " $1}' |
    sort)
[ "$(printf '%s\n' "$runtime_packages" | wc -l)" -eq 8 ] || {
    printf 'error: expected eight runtime packages, found:\n%s\n' "$runtime_packages" >&2
    exit 1
}

rm -rf "$STAGE"
mkdir -p "$STAGE"
install -m 755 "$DIST/player" "$STAGE/player"
cat >"$STAGE/README" <<README
Player for Linux x86-64 on musl

A mostly static executable. It plays local files only, with vo=gpu-next over
Vulkan and Wayland and ALSA audio. It needs a musl system (such as Alpine)
with these packages, built against these versions:

$runtime_packages

It also needs a Vulkan driver for your GPU (for example mesa-vulkan-intel,
mesa-vulkan-ati, or mesa-vulkan-swrast), a Wayland compositor, and a
fontconfig configuration with a font such as font-dejavu for subtitles and
the on-screen controller.
The scheduler decodes video in software. --hwdec is accepted for compatibility
and hwdec-current reports no. VA-API decode paths are compiled in but are not
wired into the scheduler yet.
It does not load a user config or scripts. glibc systems cannot run it, and glibc
support is out of scope.

Run:  ./player FILE
Verify the download with:  sha256sum -c $NAME.tar.gz.sha256
README

mkdir -p "$DIST"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime=@0 \
    -C "$STAGE" -czf "$TARBALL" player README
(cd "$DIST" && sha256sum "$NAME.tar.gz" >"$NAME.tar.gz.sha256")

VERIFY="$ROOT/.work/tmp/release-linux-verify"
rm -rf "$VERIFY"
mkdir -p "$VERIFY"
(cd "$DIST" && sha256sum -c "$NAME.tar.gz.sha256")
tar -xzf "$TARBALL" -C "$VERIFY"
[ "$(ls "$VERIFY" | sort | tr '\n' ' ')" = "README player " ] || {
    printf 'error: unexpected tarball contents: %s\n' "$(ls "$VERIFY")" >&2
    exit 1
}
cmp "$VERIFY/player" "$DIST/player"
"$VERIFY/player" --version | sed -n 1,3p
PLAYER="$VERIFY/player" "$ROOT/scripts/audit-linux.sh" >/dev/null
printf 'release-linux: %s\n' "$TARBALL"
