#!/bin/bash
set -uo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PLAYER=$(readlink -f "${PLAYER:-$ROOT/dist/player}")
UNSTRIPPED=${UNSTRIPPED:-$ROOT/build-linux/mpv}
TARGET=x86_64-unknown-linux-musl
RUNTIME=${FFMPEG_PREFIX:-$ROOT/.work/deps/ffmpeg-runtime/$TARGET}
ALLOWED_NEEDED=(
    libvulkan.so.1 libwayland-client.so.0
    libasound.so.2 libc.musl-x86_64.so.1 libz.so.1
    librust_fontconfig.so.5.0.2 libharfbuzz.so.0 libfreetype.so.6
)
FFMPEG_NEEDED_PATTERN='^lib(av[a-z]*|sw[a-z]*|postproc|dav1d)\.so'
FFMPEG_SYMBOLS='^(av|avio|avformat|avcodec|avfilter|avutil|sws|swr|ff|dav1d)_'
EXPECTED_RUNPATH=${PLAYER_FFMPEG_RPATH-'$ORIGIN/../lib'}
[ "$EXPECTED_RUNPATH" = none ] && EXPECTED_RUNPATH=
RETIRED_TEXT_STACK='libass|fribidi|pango|cairo|pixman'
RETIRED_TEXT_SYMBOLS='^(ass|fribidi|pango|cairo|pixman)_'
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

for tool in file readelf nm ldd strings rg python3; do
    command -v "$tool" >/dev/null || { printf 'error: %s is required\n' "$tool" >&2; exit 2; }
done
[ -f "$PLAYER" ] || { printf 'error: missing %s\n' "$PLAYER" >&2; exit 2; }

mapfile -t ffmpeg_sonames < <(python3 - "$ROOT/ffmpeg-runtime.lock.json" "$RUNTIME/share/ffmpeg-build/manifest.json" <<'PY'
import json, sys
pin = json.load(open(sys.argv[1]))
libraries = json.load(open(sys.argv[2]))["libraries"]
for name in sorted(pin["libraries"]):
    print(libraries[name]["soname"])
PY
)
[ ${#ffmpeg_sonames[@]} -gt 0 ] || { printf 'error: no pinned FFmpeg runtime at %s\n' "$RUNTIME" >&2; exit 2; }
run_env=()
[ -n "$EXPECTED_RUNPATH" ] || run_env=(env "LD_LIBRARY_PATH=$RUNTIME/lib:${RUST_FONTCONFIG_PREFIX:-$ROOT/.work/deps/rust-fontconfig/$TARGET}/lib")

description=$(file -L "$PLAYER")
case "$description" in
    *"ELF 64-bit"*"x86-64"*stripped*) check "stripped x86-64 ELF" 1 ;;
    *) check "stripped x86-64 ELF" 0 "$description" ;;
esac

mapfile -t needed < <(readelf -dW "$PLAYER" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')
extra=()
for lib in "${needed[@]}"; do
    found=0
    for allowed in "${ALLOWED_NEEDED[@]}" "${ffmpeg_sonames[@]}"; do [ "$lib" = "$allowed" ] && found=1; done
    [ "$found" = 1 ] || extra+=("$lib")
done
check "NEEDED inside the allowed set" "$([ ${#extra[@]} -eq 0 ] && echo 1 || echo 0)" "${extra[*]:-}"

linked_ffmpeg=$(printf '%s\n' "${needed[@]}" | rg "$FFMPEG_NEEDED_PATTERN" | sort | tr '\n' ' ')
want_ffmpeg=$(printf '%s\n' "${ffmpeg_sonames[@]}" | sort | tr '\n' ' ')
check "FFmpeg NEEDED is exactly the pinned runtime libraries" \
    "$([ "$linked_ffmpeg" = "$want_ffmpeg" ] && echo 1 || echo 0)" "have: $linked_ffmpeg want: $want_ffmpeg"

retired=$(printf '%s\n' "${needed[@]}" | rg -i "$RETIRED_TEXT_STACK" || true)
c_fontconfig=$(printf '%s\n' "${needed[@]}" | rg '^libfontconfig\.' || true)
check "no C Fontconfig in NEEDED" "$([ -z "$c_fontconfig" ] && echo 1 || echo 0)" "$c_fontconfig"
check "no retired text stack in NEEDED" "$([ -z "$retired" ] && echo 1 || echo 0)" "$retired"

rpath=$(readelf -dW "$PLAYER" | rg '\(RPATH\)' || true)
check "no RPATH" "$([ -z "$rpath" ] && echo 1 || echo 0)" "$rpath"
runpath=$(readelf -dW "$PLAYER" | sed -n 's/.*(RUNPATH).*\[\(.*\)\]/\1/p')
check "RUNPATH is ${EXPECTED_RUNPATH:-absent}" \
    "$([ "$runpath" = "$EXPECTED_RUNPATH" ] && echo 1 || echo 0)" "${runpath:-absent}"

unresolved=$("${run_env[@]}" ldd "$PLAYER" 2>&1 | rg 'not found|Error' || true)
check "ldd resolves every library" "$([ -z "$unresolved" ] && echo 1 || echo 0)" "$unresolved"

paths=$(strings -a "$PLAYER" | rg -F -e "$ROOT/" -e "/work/" | head -3 || true)
check "no build paths" "$([ -z "$paths" ] && echo 1 || echo 0)" "$paths"

gnu_runtime=$(nm -D --undefined-only "$PLAYER" | awk '{print $NF}' | rg '@GCC_|^_Unwind_|^__popcountdi2|^__(de)?register_frame_info' || true)
check "no GNU runtime imports" "$([ -z "$gnu_runtime" ] && echo 1 || echo 0)" "$(printf '%s' "$gnu_runtime" | tr '\n' ' ')"

imports=$(nm -D --undefined-only "$PLAYER" | awk '{print $NF}' | rg "$FORBIDDEN_IMPORTS" || true)
check "no network imports" "$([ -z "$imports" ] && echo 1 || echo 0)" "$imports"

tls=$(strings -a "$PLAYER" | rg -i 'libssl\.so|libcrypto\.so|libgnutls|libcurl|ca-certificates\.crt' | head -3 || true)
check "no TLS or curl libraries" "$([ -z "$tls" ] && echo 1 || echo 0)" "$tls"

imported=$(nm -D --undefined-only "$PLAYER" | awk '{print $NF}' | rg -c '^avformat_open_input(@|$)' || true)
check "FFmpeg imported from the shared runtime" "$([ "${imported:-0}" = 1 ] && echo 1 || echo 0)"
exported=$(nm -D --defined-only "$PLAYER" | awk '{print $NF}' | rg "$FFMPEG_SYMBOLS" | head -3 || true)
check "no FFmpeg symbols exported" "$([ -z "$exported" ] && echo 1 || echo 0)" "$exported"

protocols=$("${run_env[@]}" "$PLAYER" --no-config --list-protocols 2>&1)
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
    lua=$(nm "$UNSTRIPPED" | rg -c '\blua(L|_)[A-Za-z]' || true)
    check "no Lua interpreter" "$([ "${lua:-0}" = 0 ] && echo 1 || echo 0)" "found ${lua:-0} Lua symbols"
else
    check "no Lua interpreter" 0 "missing $UNSTRIPPED"
fi

if [ -f "$UNSTRIPPED" ]; then
    static_ffmpeg=$(nm --defined-only "$UNSTRIPPED" 2>/dev/null | awk '{print $NF}' | rg "$FFMPEG_SYMBOLS" | head -3 || true)
    check "no FFmpeg linked statically" "$([ -z "$static_ffmpeg" ] && echo 1 || echo 0)" "$static_ffmpeg"
else
    check "no FFmpeg linked statically" 0 "missing $UNSTRIPPED"
fi

if [ -f "$UNSTRIPPED" ]; then
    linked=$(nm "$UNSTRIPPED" 2>/dev/null | awk '{print $NF}' | rg "$RETIRED_TEXT_SYMBOLS" | head -3 || true)
    check "no retired text stack linked statically" "$([ -z "$linked" ] && echo 1 || echo 0)" "$linked"
fi
archives=$(printf '%s\n' "$ROOT"/.work/deps/linux/lib/*.a | rg -i "/lib($RETIRED_TEXT_STACK)[^/]*\.a$" || true)
check "no retired text stack archives in the prefix" "$([ -z "$archives" ] && echo 1 || echo 0)" "$archives"

if [ "$failed" -ne 0 ]; then
    printf 'audit-linux: %d failed\n' "$failed"
    exit 1
fi
printf 'audit-linux: ok\n'
