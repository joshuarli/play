#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

"$ROOT/tests/run.sh" ffmpeg-audit
"$ROOT/tests/run.sh" build
"$ROOT/tests/run.sh" local-only
"$ROOT/tests/run.sh" release

protocols=$("$ROOT/build/mpv" --no-config --list-protocols)
if ! printf '%s\n' "$protocols" | rg -q '^file://$'; then
    echo "audit: file:// is not the only protocol" >&2
    exit 1
fi
while IFS= read -r scheme; do
    [ -z "$scheme" ] && continue
    case "$scheme" in \#*) continue ;; esac
    if printf '%s\n' "$protocols" | rg -qi "${scheme}://"; then
        echo "audit: network protocol present: $scheme" >&2
        exit 1
    fi
done < "$ROOT/scripts/network-schemes.txt"

if otool -L "$ROOT/build/mpv" | rg -qi 'libmpv|curl|libssl|libcrypto|liblua|mlua'; then
    echo "audit: unsupported linked library present" >&2
    exit 1
fi

for directory in audio common demux filters include input misc options osdep \
    player stream sub ta video; do
    if [ -e "$ROOT/$directory" ]; then
        echo "audit: C engine source tree remains: $directory" >&2
        exit 1
    fi
done
if [ -e "$ROOT/meson.build" ] || [ -e "$ROOT/meson.options" ]; then
    echo "audit: Meson player build remains" >&2
    exit 1
fi

if nm "$ROOT/build/mpv" | rg '_(mpv_create|mpv_playloop|mp_dispatch_create)$' >/dev/null; then
    echo "audit: C playback symbols present" >&2
    exit 1
fi
if ! nm "$ROOT/build/mpv" | rg 'player_engine' >/dev/null; then
    echo "audit: Rust engine symbols missing" >&2
    exit 1
fi

if [ -e "$ROOT/rust/player-lua" ] || rg -n -i 'mlua|lua-src|luajit' "$ROOT/rust/Cargo.lock" >/dev/null; then
    echo "audit: the Lua crate or its dependencies remain" >&2
    exit 1
fi
if rg -n 'lua_State|luaL_newstate' "$ROOT/rust" >/dev/null; then
    echo "audit: Lua API references remain in the Rust tree" >&2
    exit 1
fi

release_binary="$ROOT/dist/Player.app/Contents/MacOS/player"
for binary in "$ROOT/build/mpv" "$release_binary"; do
    if nm "$binary" | rg -q '\b_?lua(L|_)[A-Za-z]'; then
        echo "audit: Lua symbols linked into $binary" >&2
        exit 1
    fi
done
if [ -e "$ROOT/dist/Player.app/Contents/Frameworks/libplayer_release.dylib" ]; then
    echo "audit: release carries the retired C ABI dylib" >&2
    exit 1
fi

if command -v cargo-audit >/dev/null; then
    (cd "$ROOT/rust" && cargo audit)
else
    echo "audit: cargo-audit not installed; advisory scan skipped" >&2
fi

echo "audit: ok"
