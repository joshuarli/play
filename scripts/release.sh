#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX="$ROOT/.work/deps/release"
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-27.0}

cd "$ROOT"
rm -rf "$ROOT/dist/Player.app"
scripts/build-deps.sh
scripts/fetch-ffmpeg-sdk.sh aarch64-apple-darwin

export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
unset PKG_CONFIG_PATH
export CFLAGS="-I$PREFIX/include"
export LDFLAGS="-L$PREFIX/lib -Wl,-dead_strip"

cargo build --profile dist -p player-app

python3 "$ROOT/scripts/bundle.py" "$ROOT"
printf 'release app: %s\n' "$ROOT/dist/Player.app"
