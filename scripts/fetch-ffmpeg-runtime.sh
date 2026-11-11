#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
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
LOCK="$ROOT/.work/deps/ffmpeg-runtime/.lock-$target"
export TMPDIR="$ROOT/.work/tmp"
mkdir -p "$CACHE" "$(dirname "$DEST")"
if [ -z "${PLAYER_FFMPEG_RUNTIME_LOCK_FD:-}" ]; then
    exec python3 - "$LOCK" "$0" "$@" <<'PYLOCK'
import fcntl, os, sys
fd = os.open(sys.argv[1], os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
fcntl.flock(fd, fcntl.LOCK_EX)
os.set_inheritable(fd, True)
os.environ["PLAYER_FFMPEG_RUNTIME_LOCK_FD"] = str(fd)
os.execv(sys.argv[2], sys.argv[2:])
PYLOCK
fi
python3 - "$LOCK" "$PLAYER_FFMPEG_RUNTIME_LOCK_FD" <<'PYLOCK'
import fcntl, os, stat, sys
fd = int(sys.argv[2])
actual = os.fstat(fd)
expected = os.stat(sys.argv[1], follow_symlinks=False)
if not stat.S_ISREG(actual.st_mode) or (actual.st_dev, actual.st_ino) != (expected.st_dev, expected.st_ino):
    sys.exit("error: invalid FFmpeg runtime lock")
fcntl.flock(fd, fcntl.LOCK_EX)
PYLOCK

pin_field() {
    python3 - "$PIN" "$target" "$1" <<'PY'
import json, sys
pin = json.load(open(sys.argv[1]))
target, field = sys.argv[2], sys.argv[3]
if pin.get("contract") != 2 or pin.get("profile") != "runtime":
    sys.exit("error: ffmpeg-runtime.lock.json must be contract 2, profile runtime")
archive = f"ffmpeg-{pin['ffmpeg']}+{pin['release']}-{target}-runtime.tar.gz"
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

repository=$(pin_field repository)
release=$(pin_field release)
asset=$(pin_field archive)
expected=$(pin_field sha256)

if [ -n "$local_archive" ]; then
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
actual=$(sha256 "$archive")
if [ "$actual" != "$expected" ]; then
    printf 'error: %s has sha256 %s; ffmpeg-runtime.lock.json pins %s\n' "$archive" "$actual" "$expected" >&2
    [ -n "$local_archive" ] || rm -f "$archive"
    exit 1
fi

root=${asset%.tar.gz}
if python3 "$ROOT/scripts/verify-ffmpeg-runtime.py" "$archive" "$root" "$DEST"; then
    if [ -z "$local_archive" ]; then
        check_manifest "$DEST"
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
staging="$CACHE/extract-$target"
rm -rf "$staging"
mkdir -p "$staging"
tar -xzf "$archive" -C "$staging"
entries=$(cd "$staging" && printf '%s\n' *)
[ "$entries" = "$root" ] || {
    printf 'error: %s does not contain exactly %s/\n' "$asset" "$root" >&2
    exit 1
}
python3 "$ROOT/scripts/verify-ffmpeg-runtime.py" "$archive" "$root" "$staging/$root" || {
    printf 'error: %s extraction does not match its archive\n' "$asset" >&2
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
