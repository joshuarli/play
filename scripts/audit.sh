#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

"$ROOT/tests/run.sh" ffmpeg-audit
"$ROOT/tests/run.sh" build
"$ROOT/tests/run.sh" local-only
"$ROOT/tests/run.sh" release

protocols=$("$ROOT/build/mpv" --no-config --list-protocols)
if ! printf '%s\n' "$protocols" | rg -q 'file://'; then
    echo "audit: file:// protocol missing" >&2
    exit 1
fi
if printf '%s\n' "$protocols" | rg -qi 'http|https|ftp|rtsp|rtmp|smb'; then
    echo "audit: network protocol present" >&2
    exit 1
fi

if otool -L "$ROOT/build/mpv" | rg -qi 'libmpv|curl|libssl|libcrypto|liblua|mlua'; then
    echo "audit: unsupported linked library present" >&2
    exit 1
fi

if rg -n 'lua_State|luaL_newstate' \
    "$ROOT/player" "$ROOT/include" "$ROOT/common" "$ROOT/input" \
    "$ROOT/options" "$ROOT/stream" "$ROOT/demux" "$ROOT/sub" \
    "$ROOT/video" "$ROOT/audio" "$ROOT/filters" "$ROOT/osdep" \
    "$ROOT/misc" "$ROOT/ta" "$ROOT/rust/player-ffi" \
    "$ROOT/rust/player-core" "$ROOT/rust/player-audio" >/dev/null; then
    echo "audit: Lua API remnants outside player-lua" >&2
    exit 1
fi

interpreters=$(nm "$ROOT/build/mpv" | rg -c 'luaL_newstate' || true)
if [ "$interpreters" != "1" ]; then
    echo "audit: expected one Lua interpreter, found ${interpreters:-0}" >&2
    exit 1
fi

release_dylib="$ROOT/.work/deps/player-audio/target/release/libplayer_release.dylib"
if [ -f "$release_dylib" ]; then
    release_interpreters=$(nm "$release_dylib" | rg -c 'luaL_newstate' || true)
    if [ "$release_interpreters" != "1" ]; then
        echo "audit: release expected one Lua interpreter, found ${release_interpreters:-0}" >&2
        exit 1
    fi
fi

if command -v cargo-audit >/dev/null; then
    (cd "$ROOT/rust" && cargo audit)
else
    echo "audit: cargo-audit not installed; advisory scan skipped" >&2
fi

echo "audit: ok"
