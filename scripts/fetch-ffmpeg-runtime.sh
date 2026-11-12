#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PIN="$ROOT/ffmpeg-runtime.lock.json"

[ $# -eq 1 ] || [ $# -eq 2 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl> [ARCHIVE]\n' "$0" >&2
    exit 2
}
target=$1
local_archive=${2:-}
DEST="$ROOT/.work/deps/ffmpeg-runtime/$target"
CACHE="$ROOT/.work/tmp/ffmpeg-runtime"
export TMPDIR="$ROOT/.work/tmp"
mkdir -p "$CACHE" "$(dirname "$DEST")"

pin_field() {
    python3 - "$PIN" "$target" "$1" <<'PY'
import json, sys
pin = json.load(open(sys.argv[1]))
target, field = sys.argv[2], sys.argv[3]
if pin.get("contract") != 2 or pin.get("profile") != "runtime":
    sys.exit("error: ffmpeg-runtime.lock.json must be contract 2, profile runtime")
prefix = "release-"
commit = pin["release"].removeprefix(prefix)
if commit == pin["release"] or len(commit) != 40 or commit.strip("0123456789abcdef"):
    sys.exit("error: ffmpeg-runtime.lock.json release must be release-<40 hex commit>")
archive = f"ffmpeg-runtime-{pin['ffmpeg']}-{target}.tar.xz"
if field == "archive":
    print(archive)
elif field == "sha256":
    digest = pin["sha256"].get(target)
    if digest is None:
        sys.exit(f"error: ffmpeg-runtime.lock.json pins no sha256 for {archive}")
    print(digest)
else:
    print(pin[field])
PY
}

check_manifest() {
    python3 - "$PIN" "$target" "$1/share/ffmpeg-build/manifest.json" <<'PY'
import json, sys
pin = json.load(open(sys.argv[1]))
target = sys.argv[2]
try:
    manifest = json.load(open(sys.argv[3]))
except (OSError, json.JSONDecodeError) as error:
    sys.exit(f"runtime manifest unreadable: {error}")
want = {
    ("profile",): "runtime",
    ("target",): target,
    ("sources", "ffmpeg", "version"): pin["ffmpeg"],
    ("producer", "commit"): pin["release"].removeprefix("release-"),
    ("release", "archive"): f"ffmpeg-runtime-{pin['ffmpeg']}-{target}.tar.xz",
}
for keys, value in want.items():
    found = manifest
    for key in keys:
        found = found.get(key) if isinstance(found, dict) else None
    if found != value:
        sys.exit(f"runtime manifest {'.'.join(keys)} is {found!r}, pin wants {value!r}")
missing = [name for name in pin["libraries"] if name not in manifest.get("libraries", {})]
if missing:
    sys.exit(f"runtime manifest lacks pinned libraries {missing}")
PY
}

sha256() {
    if command -v sha256sum >/dev/null; then
        sha256sum "$1" | awk '{ print $1 }'
    else
        shasum -a 256 "$1" | awk '{ print $1 }'
    fi
}

repository=$(pin_field repository)
release=$(pin_field release)
asset=$(pin_field archive)
expected=$(pin_field sha256)

if [ -z "$local_archive" ] && [ -f "$DEST/share/ffmpeg-build/manifest.json" ] && check_manifest "$DEST" 2>/dev/null; then
    printf 'FFmpeg runtime %s for %s already in %s\n' "$release" "$target" "$DEST"
    exit 0
fi

if [ -n "$local_archive" ]; then
    [ "$(basename "$local_archive")" = "$asset" ] || {
        printf 'error: %s is not the pinned archive %s\n' "$local_archive" "$asset" >&2
        exit 1
    }
    archive=$local_archive
else
    archive="$CACHE/$asset"
    if [ ! -f "$archive" ] || [ "$(sha256 "$archive")" != "$expected" ]; then
        url="https://github.com/$repository/releases/download/$release/$asset"
        printf 'download %s\n' "$url"
        curl -fsSL --retry 3 "$url" -o "$archive.part"
        mv "$archive.part" "$archive"
    fi
fi
actual=$(sha256 "$archive")
if [ "$actual" != "$expected" ]; then
    printf 'error: %s has sha256 %s; ffmpeg-runtime.lock.json pins %s\n' "$archive" "$actual" "$expected" >&2
    [ -n "$local_archive" ] || rm -f "$archive"
    exit 1
fi

root=${asset%.tar.xz}
staging="$CACHE/extract-$target"
rm -rf "$staging"
mkdir -p "$staging"
tar -xJf "$archive" -C "$staging"
entries=$(cd "$staging" && printf '%s\n' *)
[ "$entries" = "$root" ] || {
    printf 'error: %s does not contain exactly %s/\n' "$asset" "$root" >&2
    exit 1
}
check_manifest "$staging/$root" || {
    printf 'error: %s disagrees with ffmpeg-runtime.lock.json\n' "$asset" >&2
    exit 1
}
rm -rf "$DEST"
mv "$staging/$root" "$DEST"
rmdir "$staging"
printf 'FFmpeg runtime %s for %s in %s\n' "$release" "$target" "$DEST"
