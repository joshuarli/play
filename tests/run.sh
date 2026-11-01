#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

case "${1:-}" in
    ffmpeg-audit)
        shift
        exec "$ROOT/scripts/ffmpeg-manifest.sh" --check "$@"
        ;;
    *)
        exec python3 "$ROOT/tests/harness.py" "$@"
        ;;
esac
