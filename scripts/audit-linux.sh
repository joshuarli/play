#!/bin/bash
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PLAYER=${PLAYER:-$ROOT/dist/player}
UNSTRIPPED=${UNSTRIPPED:-$ROOT/build-linux/mpv}
ALLOWED_NEEDED=(
    libvulkan.so.1 libwayland-client.so.0 libgcc_s.so.1
    libasound.so.2 libc.musl-x86_64.so.1 libva.so.2 libva-drm.so.2
)
LICENSED=(FFmpeg libass libplacebo shaderc dav1d FreeType FriBiDi HarfBuzz zlib expat fontconfig Lua)
FORBIDDEN_IMPORTS='^(getaddrinfo|freeaddrinfo|getnameinfo|gethostbyname2?|gethostbyaddr|SSL_.*|TLS_.*|curl_.*)$'

failed=0
check() {
    local name=$1 ok=$2 detail=${3:-}
    if [ "$ok" = 1 ]; then
        printf 'PASS audit-linux: %s\n' "$name"
    else
        printf 'FAIL audit-linux: %s%s\n' "$name" "${detail:+: $detail}"
        failed=$((failed + 1))
    fi
}

for tool in file readelf nm ldd strings rg; do
    command -v "$tool" >/dev/null || { printf 'error: %s is required\n' "$tool" >&2; exit 2; }
done
[ -f "$PLAYER" ] || { printf 'error: missing %s\n' "$PLAYER" >&2; exit 2; }

description=$(file "$PLAYER")
case "$description" in
    *"ELF 64-bit"*"x86-64"*stripped*) check "stripped x86-64 ELF" 1 ;;
    *) check "stripped x86-64 ELF" 0 "$description" ;;
esac

mapfile -t needed < <(readelf -dW "$PLAYER" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
extra=()
for lib in "${needed[@]}"; do
    found=0
    for allowed in "${ALLOWED_NEEDED[@]}"; do [ "$lib" = "$allowed" ] && found=1; done
    [ "$found" = 1 ] || extra+=("$lib")
done
check "NEEDED inside the allowed set" "$([ ${#extra[@]} -eq 0 ] && echo 1 || echo 0)" "${extra[*]:-}"

rpath=$(readelf -dW "$PLAYER" | rg 'RPATH|RUNPATH' || true)
check "no RPATH or RUNPATH" "$([ -z "$rpath" ] && echo 1 || echo 0)" "$rpath"

unresolved=$(ldd "$PLAYER" 2>&1 | rg 'not found' || true)
check "ldd resolves every library" "$([ -z "$unresolved" ] && echo 1 || echo 0)" "$unresolved"

paths=$(strings -a "$PLAYER" | rg -F -e "$ROOT/" -e "/work/" | head -3 || true)
check "no build paths" "$([ -z "$paths" ] && echo 1 || echo 0)" "$paths"

imports=$(nm -D --undefined-only "$PLAYER" | awk '{print $NF}' | rg "$FORBIDDEN_IMPORTS" || true)
check "no network imports" "$([ -z "$imports" ] && echo 1 || echo 0)" "$imports"

tls=$(strings -a "$PLAYER" | rg -i 'libssl\.so|libcrypto\.so|libgnutls|libcurl|ca-certificates\.crt' | head -3 || true)
check "no TLS or curl libraries" "$([ -z "$tls" ] && echo 1 || echo 0)" "$tls"

protocols=$("$PLAYER" --no-config --list-protocols 2>&1)
listed=$(printf '%s\n' "$protocols" | rg -o '^\s*[A-Za-z][A-Za-z0-9+.-]*://' | tr -d ' \t' | sort -u)
check "one file protocol" "$([ "$listed" = "file://" ] && echo 1 || echo 0)" "$(printf '%s' "$listed" | tr '\n' ' ')"

network=""
while IFS= read -r scheme; do
    [ -z "$scheme" ] && continue
    case "$scheme" in \#*) continue ;; esac
    printf '%s\n' "$listed" | rg -qix "${scheme}://" && network+="$scheme "
done <"$ROOT/scripts/network-schemes.txt"
check "no network schemes" "$([ -z "$network" ] && echo 1 || echo 0)" "$network"

stale_c=0
for directory in audio common demux filters include input misc options osdep \
    player stream sub ta video; do
    [ -e "$ROOT/$directory" ] && stale_c=1
done
check "C engine sources removed" "$([ "$stale_c" = 0 ] && echo 1 || echo 0)"
check "Meson player build removed" "$([ ! -e "$ROOT/meson.build" ] && [ ! -e "$ROOT/meson.options" ] && echo 1 || echo 0)"
symbols=$(nm "$UNSTRIPPED" 2>/dev/null || true)
case "$symbols" in
    *_mpv_create*|*_mpv_playloop*|*_mp_dispatch_create*) c_engine=1 ;;
    *) c_engine=0 ;;
esac
case "$symbols" in
    *player_engine*) rust_engine=1 ;;
    *) rust_engine=0 ;;
esac
check "no C playback symbols" "$([ "$c_engine" = 0 ] && echo 1 || echo 0)"
check "Rust engine symbols" "$([ "$rust_engine" = 1 ] && echo 1 || echo 0)"

if [ -f "$UNSTRIPPED" ]; then
    lua=$(nm "$UNSTRIPPED" | rg -c 'luaL_newstate' || true)
    check "one Lua interpreter" "$([ "${lua:-0}" = 1 ] && echo 1 || echo 0)" "found ${lua:-0}"
else
    check "one Lua interpreter" 0 "missing $UNSTRIPPED"
fi

missing=()
for name in "${LICENSED[@]}"; do
    rg -qi -F "$name" "$ROOT/docs/licenses.md" || missing+=("$name")
done
check "licenses listed" "$([ ${#missing[@]} -eq 0 ] && echo 1 || echo 0)" "${missing[*]:-}"

if [ "$failed" -ne 0 ]; then
    printf 'audit-linux: %d failed\n' "$failed"
    exit 1
fi
printf 'audit-linux: ok\n'
