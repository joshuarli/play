#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PREFIX="$ROOT/.work/deps/release"
FFMPEG_PREFIX="$PREFIX/ffmpeg"
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-27.0}

cd "$ROOT"
rm -rf "$ROOT/dist/Player.app"
scripts/build-deps.sh

export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig:$FFMPEG_PREFIX/lib/pkgconfig"
unset PKG_CONFIG_PATH
export CFLAGS="-I$PREFIX/include -I$FFMPEG_PREFIX/include"
export LDFLAGS="-L$PREFIX/lib -L$FFMPEG_PREFIX/lib -Wl,-dead_strip"

cargo build --profile dist --manifest-path "$ROOT/rust/Cargo.toml" -p player-app

python3 "$ROOT/scripts/bundle.py" "$ROOT"
printf 'release app: %s\n' "$ROOT/dist/Player.app"
