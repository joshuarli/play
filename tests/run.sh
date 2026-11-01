#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

case "${1:-}" in
    ffmpeg-audit)
        shift
        exec "$ROOT/scripts/ffmpeg-manifest.sh" --check "$@"
        ;;
    linux-audit|linux-mostly-static)
        exec "$ROOT/scripts/audit-linux.sh"
        ;;
    *)
        exec python3 "$ROOT/tests/harness.py" "$@"
        ;;
esac
