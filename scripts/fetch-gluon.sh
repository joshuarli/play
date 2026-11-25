#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=release-dd4b9664d3319e7dc9d01bd895c72d57aa298244

[ $# -eq 1 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl>\n' "$0" >&2
    exit 2
}
target=$1
case "$target" in
    aarch64-apple-darwin) sha=f7383222f00d8d580d47c543e99628046c2b2691df9059524838b829ef4cee76 ;;
    x86_64-unknown-linux-musl) sha=abef15f7c04213c219e97eae794e55544e5e508204c06c79db8275b1868849fd ;;
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
got=$(sha256sum "$archive" | awk '{print $1}')
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
