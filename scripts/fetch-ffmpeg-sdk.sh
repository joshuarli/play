#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PIN="$ROOT/ffmpeg-sdk.lock.json"

[ $# -eq 1 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl>\n' "$0" >&2
    exit 2
}
target=$1
DEST="$ROOT/.work/deps/ffmpeg-sdk/$target"
CACHE="$ROOT/.work/tmp/ffmpeg-sdk"
export TMPDIR="$ROOT/.work/tmp"
mkdir -p "$CACHE" "$(dirname "$DEST")"

pin_field() {
    python3 - "$PIN" "$target" "$1" <<'PY'
import json, sys
pin = json.load(open(sys.argv[1]))
target, field = sys.argv[2], sys.argv[3]
if pin.get("contract") != 1 or pin.get("profile") != "play-sdk":
    sys.exit("error: ffmpeg-sdk.lock.json must be contract 1, profile play-sdk")
entry = pin["targets"].get(target)
if entry is None:
    sys.exit(f"error: ffmpeg-sdk.lock.json has no SDK for {target}")
print(entry[field] if field in ("asset", "sha256") else pin[field])
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
    sys.exit(f"SDK manifest unreadable: {error}")
want = {
    ("profile",): "play-sdk",
    ("target",): target,
    ("sources", "ffmpeg", "version"): pin["ffmpeg"],
    ("release", "tag"): pin["release"],
    ("release", "archive"): pin["targets"][target]["asset"],
}
for keys, value in want.items():
    found = manifest
    for key in keys:
        found = found.get(key) if isinstance(found, dict) else None
    if found != value:
        sys.exit(f"SDK manifest {'.'.join(keys)} is {found!r}, pin wants {value!r}")
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
asset=$(pin_field asset)
expected=$(pin_field sha256)

if [ -f "$DEST/share/ffmpeg-build/manifest.json" ] && check_manifest "$DEST" 2>/dev/null; then
    printf 'FFmpeg SDK %s for %s already in %s\n' "$release" "$target" "$DEST"
    exit 0
fi

archive="$CACHE/$asset"
if [ ! -f "$archive" ] || [ "$(sha256 "$archive")" != "$expected" ]; then
    url="https://github.com/$repository/releases/download/$release/${asset//+/%2B}"
    printf 'download %s\n' "$url"
    curl -fsSL --retry 3 "$url" -o "$archive.part"
    mv "$archive.part" "$archive"
fi
actual=$(sha256 "$archive")
if [ "$actual" != "$expected" ]; then
    printf 'error: %s has sha256 %s; ffmpeg-sdk.lock.json pins %s\n' "$asset" "$actual" "$expected" >&2
    rm -f "$archive"
    exit 1
fi

root=${asset%.tar.gz}
staging="$CACHE/extract-$target"
rm -rf "$staging"
mkdir -p "$staging"
tar -xzf "$archive" -C "$staging"
entries=$(cd "$staging" && printf '%s\n' *)
[ "$entries" = "$root" ] || {
    printf 'error: %s does not contain exactly %s/\n' "$asset" "$root" >&2
    exit 1
}
check_manifest "$staging/$root" || {
    printf 'error: %s disagrees with ffmpeg-sdk.lock.json\n' "$asset" >&2
    exit 1
}
rm -rf "$DEST"
mv "$staging/$root" "$DEST"
rmdir "$staging"
printf 'FFmpeg SDK %s for %s in %s\n' "$release" "$target" "$DEST"
