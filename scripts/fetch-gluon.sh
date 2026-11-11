#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=release-4f7e194610f8c3e2bb5d7faa7b98a2b4e1893370

[ $# -eq 1 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl>\n' "$0" >&2
    exit 2
}
target=$1
case "$target" in
    aarch64-apple-darwin) sha=31c1e7490c52875035150d836527d1aad08a2b4d4f311b3bf02013e2890958ad ;;
    x86_64-unknown-linux-musl) sha=707e1ee2bcc3f9fdedaa32513304687d99e7eb7510d9467fd9330dfafd1412f0 ;;
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

archive="$stage/gluon-$target.zip"
curl -fsSL "https://github.com/laputa-systems/gluon/releases/download/$TAG/gluon-$target.zip" -o "$archive"
if command -v sha256sum >/dev/null; then
    got=$(sha256sum "$archive" | awk '{print $1}')
else
    got=$(shasum -a 256 "$archive" | awk '{print $1}')
fi
[ "$got" = "$sha" ] || {
    printf 'error: gluon-%s.zip sha256 %s, pinned %s\n' "$target" "$got" "$sha" >&2
    exit 1
}
mkdir "$stage/out"
unzip -q "$archive" -d "$stage/out"
chmod 755 "$stage/out/meson" "$stage/out/ninja"
rm -rf "$DEST"
mv "$stage/out" "$DEST"
printf '%s\n' "$DEST"
