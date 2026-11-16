#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=release-c7c0571992effba33a50e5b94c3ed4da7145c1e3

[ $# -eq 1 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl>\n' "$0" >&2
    exit 2
}
target=$1
case "$target" in
    aarch64-apple-darwin) sha=8102162b2a90120efac9cac0d14c2f8b747772e4ebe1e6308907d1121a0e5397 ;;
    x86_64-unknown-linux-musl) sha=1d28430818c830b222b2fe8e46c65d76731b43e1724d5d60ee1e7b651f187f26 ;;
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
