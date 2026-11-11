#!/bin/bash
set -euo pipefail

ROOT=$(cd -P "$(dirname "$0")/.." && pwd -P)
PIN="$ROOT/ffmpeg-runtime.lock.json"

[ $# -eq 1 ] || [ $# -eq 2 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl> [ARCHIVE]\n' "$0" >&2
    exit 2
}
target=$1
case "$target" in
    aarch64-apple-darwin|x86_64-unknown-linux-musl) ;;
    *) printf 'error: unsupported FFmpeg runtime target: %s\n' "$target" >&2; exit 2 ;;
esac
local_archive=${2:-}
DEST="$ROOT/.work/deps/ffmpeg-runtime/$target"
CACHE="$ROOT/.work/tmp/ffmpeg-runtime"
FS="$ROOT/scripts/ffmpeg-runtime-fs.py"
export TMPDIR="$ROOT/.work/tmp"
if [ -z "${PLAYER_FFMPEG_RUNTIME_LOCK_FD:-}" ]; then
    exec python3 "$FS" setup "$ROOT" "$target" "$ROOT/scripts/fetch-ffmpeg-runtime.sh" "$@"
fi
python3 "$FS" check "$ROOT" "$target"
CACHE=.

pin_field() {
    python3 - "$PIN" "$target" "$1" <<'PY'
import json, sys
pin = json.load(open(sys.argv[1]))
target, field = sys.argv[2], sys.argv[3]
if pin.get("contract") != 2 or pin.get("profile") != "runtime":
    sys.exit("error: ffmpeg-runtime.lock.json must be contract 2, profile runtime")
archive = f"ffmpeg-{pin['ffmpeg']}+{pin['release']}-{target}-runtime.tar.gz"
if '/' in archive or '\0' in archive:
    sys.exit("error: unsafe FFmpeg runtime archive name")
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
    python3 "$FS" run "$ROOT" "$target" "$1" - python3 - "$PIN" "$target" "$2/share/ffmpeg-build/manifest.json" <<'PY'
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
    ("release", "tag"): pin["release"],
    ("release", "archive"): f"ffmpeg-{pin['ffmpeg']}+{pin['release']}-{target}-runtime.tar.gz",
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

if [ -n "${PLAYER_FFMPEG_RUNTIME_ARCHIVE_FD:-}" ]; then
    release=$PLAYER_FFMPEG_RUNTIME_RELEASE
    asset=$PLAYER_FFMPEG_RUNTIME_ARCHIVE_ASSET
else
    repository=$(pin_field repository)
    release=$(pin_field release)
    asset=$(pin_field archive)
    expected=$(pin_field sha256)
fi

if [ -n "${PLAYER_FFMPEG_RUNTIME_ARCHIVE_FD:-}" ]; then
    archive=@snapshot
elif [ -n "$local_archive" ]; then
    [ "$(basename "$local_archive")" = "$asset" ] || {
        printf 'error: %s is not the pinned archive %s\n' "$local_archive" "$asset" >&2
        exit 1
    }
    archive=$local_archive
else
    archive="$CACHE/$asset"
    if [ ! -f "$archive" ] || [ "$(sha256 "$archive")" != "$expected" ]; then
        url="https://github.com/$repository/releases/download/$release/${asset//+/%2B}"
        printf 'download %s\n' "$url"
        download=$(mktemp "$CACHE/.download-$target.XXXXXXXX")
        trap 'rm -f "$download"' EXIT
        curl -fsSL --retry 3 "$url" -o "$download"
        mv "$download" "$archive"
        trap - EXIT
    fi
fi
if [ -z "${PLAYER_FFMPEG_RUNTIME_ARCHIVE_FD:-}" ]; then
    actual=$(sha256 "$archive")
    if [ "$actual" != "$expected" ]; then
        printf 'error: %s has sha256 %s; ffmpeg-runtime.lock.json pins %s\n' "$archive" "$actual" "$expected" >&2
        if [ -z "$local_archive" ]; then
            rm -f "$archive"
        fi
        exit 1
    fi
    exec python3 "$FS" snapshot "$ROOT" "$target" "$archive" "$expected" "$asset" "$ROOT/scripts/fetch-ffmpeg-runtime.sh" "$@"
fi

root=${asset%.tar.gz}
if python3 "$FS" run "$ROOT" "$target" dest "$archive" python3 "$ROOT/scripts/verify-ffmpeg-runtime.py" @archive "$root" "$target"; then
    if [ -z "$local_archive" ]; then
        check_manifest dest "$target"
        printf 'FFmpeg runtime %s for %s already in %s\n' "$release" "$target" "$DEST"
        exit 0
    fi
else
    verification=$?
    [ "$verification" -eq 1 ] || exit "$verification"
    if [ -e "$DEST" ] || [ -L "$DEST" ]; then
        printf 'FFmpeg runtime cache invalid; reinstalling %s\n' "$target"
    fi
fi
staging=$(python3 "$FS" prepare-stage "$ROOT" "$target")
trap 'rm -rf "$staging"' EXIT
python3 "$FS" run "$ROOT" "$target" "$staging" "$archive" tar -xzf @archive
entries=$(python3 "$FS" run "$ROOT" "$target" "$staging" - python3 -c 'import os; print("\n".join(sorted(os.listdir("."))))')
[ "$entries" = "$root" ] || {
    printf 'error: %s does not contain exactly %s/\n' "$asset" "$root" >&2
    exit 1
}
python3 "$FS" run "$ROOT" "$target" "$staging" "$archive" python3 "$ROOT/scripts/verify-ffmpeg-runtime.py" @archive "$root" "$root" || {
    printf 'error: %s extraction does not match its archive\n' "$asset" >&2
    exit 1
}
check_manifest "$staging" "$root" || {
    printf 'error: %s disagrees with ffmpeg-runtime.lock.json\n' "$asset" >&2
    exit 1
}
python3 "$FS" publish "$ROOT" "$target" "$staging" "$root"
trap - EXIT
printf 'FFmpeg runtime %s for %s in %s\n' "$release" "$target" "$DEST"
