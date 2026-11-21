#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

[ $# -eq 1 ] || [ $# -eq 2 ] || {
    printf 'usage: %s <aarch64-apple-darwin|x86_64-unknown-linux-musl> [ARCHIVE]\n' "$0" >&2
    exit 2
}

exec "$ROOT/bootstrap/buildlib-run" -- "$ROOT/scripts/fetch_ffmpeg_runtime.py" "$ROOT" "$@"
