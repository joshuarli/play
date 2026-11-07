#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

case "$(uname -s)" in
    Darwin) "$ROOT/scripts/fetch-ffmpeg-runtime.sh" aarch64-apple-darwin ;;
    Linux) "$ROOT/scripts/fetch-ffmpeg-runtime.sh" x86_64-unknown-linux-musl ;;
esac

if [ "$(uname -s)" = "Linux" ]; then
    # A development binary that links Wayland must use the dynamic musl
    # loader; the default static-pie dies before main.
    export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static"
fi

cargo build -p player-app

BINARY="${CARGO_TARGET_DIR:-$ROOT/target}/debug/player"
if [ "$(uname -s)" = "Linux" ]; then
    "$ROOT/scripts/install-layout.sh" "$BINARY" "$ROOT/build-linux" mpv
else
    mkdir -p "$ROOT/build"
    install -m 755 "$BINARY" "$ROOT/build/mpv"
fi
if [ "$(uname -s)" = "Darwin" ]; then
    mkdir -p "$ROOT/build/vulkan/icd.d"
    cp "$ROOT/.work/deps/release/share/vulkan/icd.d/MoltenVK_icd.json" "$ROOT/build/vulkan/icd.d/"
fi
