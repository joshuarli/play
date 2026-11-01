#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

"$ROOT/tests/run.sh" ffmpeg-audit
"$ROOT/tests/run.sh" build
"$ROOT/tests/run.sh" local-only

protocols=$("$ROOT/build/mpv" --no-config --list-protocols)
if ! printf '%s\n' "$protocols" | rg -q 'file://'; then
    echo "audit: file:// protocol missing" >&2
    exit 1
fi
if printf '%s\n' "$protocols" | rg -qi 'http|https|ftp|rtsp|rtmp|smb'; then
    echo "audit: network protocol present" >&2
    exit 1
fi

if otool -L "$ROOT/build/mpv" | rg -qi 'libmpv|curl|libssl|libcrypto'; then
    echo "audit: unsupported linked library present" >&2
    exit 1
fi

echo "audit: ok"
