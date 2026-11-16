#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=release-494abcc6d59263aa76c33bb5a22afdd2b7c2718a

[ $# -eq 1 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl>\n' "$0" >&2
    exit 2
}
target=$1
case "$target" in
    aarch64-apple-darwin) sha=75011450589edc0cae64f7c51cd73a6c233c867c9de9335c94efcc1bb24f2b00 ;;
    x86_64-unknown-linux-musl) sha=4c3092e5a6dee719b8118dd33fe15b03cfdeb7b3aeb51012b2e52def5d8d9f95 ;;
    *)
        printf 'error: no gluon release for %s\n' "$target" >&2
        exit 2
        ;;
esac

DEST="$ROOT/.work/tools/gluon/$TAG-$target"
if [ -x "$DEST/meson" ] && [ -x "$DEST/ninja" ]; then
    printf '%s\n' "$DEST"
    exit 0
fi

export TMPDIR="$ROOT/.work/tmp"
mkdir -p "$TMPDIR" "$(dirname "$DEST")"
stage=$(mktemp -d "$TMPDIR/gluon.XXXXXX")
trap 'rm -rf "$stage"' EXIT

archive="$stage/gluon-$target.tar.xz"
curl -fsSL "https://github.com/laputa-systems/gluon/releases/download/$TAG/gluon-$target.tar.xz" -o "$archive"
if command -v sha256sum >/dev/null; then
    got=$(sha256sum "$archive" | awk '{print $1}')
else
    got=$(shasum -a 256 "$archive" | awk '{print $1}')
fi
[ "$got" = "$sha" ] || {
    printf 'error: gluon-%s.tar.xz sha256 %s, pinned %s\n' "$target" "$got" "$sha" >&2
    exit 1
}
mkdir "$stage/out"
tar -xJf "$archive" -C "$stage/out"
chmod 755 "$stage/out/meson" "$stage/out/ninja"
rm -rf "$DEST"
mv "$stage/out" "$DEST"
printf '%s\n' "$DEST"
