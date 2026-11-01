#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEPS="$ROOT/.work/deps"
TMP="$ROOT/.work/tmp"
LUA_PREFIX="$DEPS/lua52"
LUA_VERSION=5.2.4
LUA_SHA256=b9e2e4aad6789b3b63a056d442f7b39f0ecfca3ae0f1fc0ae4e9614401b69f4b
LUA_URL="https://www.lua.org/ftp/lua-$LUA_VERSION.tar.gz"

mkdir -p "$DEPS" "$TMP"

pin() {
    formula=$1
    version=$2
    installed=$(brew list --versions "$formula" 2>/dev/null | awk 'NR == 1 { print $2 }')
    if [ "$installed" = "$version" ]; then
        printf 'ok      %-16s %s\n' "$formula" "$version"
        return
    fi
    printf 'install %-16s %s (found %s)\n' "$formula" "$version" "${installed:-none}"
    brew install "$formula"
    installed=$(brew list --versions "$formula" | awk 'NR == 1 { print $2 }')
    if [ "$installed" != "$version" ]; then
        printf 'error: %s resolved to %s, pinned to %s\n' \
            "$formula" "$installed" "$version" >&2
        exit 1
    fi
}

command -v brew >/dev/null || {
    printf 'error: Homebrew is required\n' >&2
    exit 1
}

pin ffmpeg 9.0.2_1
pin libplacebo 7.387.0
pin libass 0.17.5_1
pin molten-vk 1.4.2
pin vulkan-loader 1.4.363.0
pin vulkan-headers 1.4.363.0
pin shaderc 2026.4
pin dav1d 1.5.4
pin meson 1.12.1
pin ninja 1.13.2
pin pkgconf 3.0.10

lua_ready() {
    test -f "$LUA_PREFIX/lib/liblua.a" &&
        test -f "$LUA_PREFIX/lib/pkgconfig/lua52.pc" &&
        test -f "$LUA_PREFIX/include/lua.h" &&
        test -f "$LUA_PREFIX/include/luaconf.h" &&
        test -f "$LUA_PREFIX/include/lualib.h" &&
        test -f "$LUA_PREFIX/include/lauxlib.h"
}

if lua_ready &&
    [ "$(awk '/^Version:/ { print $2 }' "$LUA_PREFIX/lib/pkgconfig/lua52.pc")" = "$LUA_VERSION" ]; then
    printf 'ok      %-16s %s (source build)\n' lua "$LUA_VERSION"
    exit 0
fi

tarball="$TMP/lua-$LUA_VERSION.tar.gz"
if [ ! -f "$tarball" ] ||
    [ "$(shasum -a 256 "$tarball" | awk '{ print $1 }')" != "$LUA_SHA256" ]; then
    printf 'download lua-%s\n' "$LUA_VERSION"
    curl -fsSL "$LUA_URL" -o "$tarball"
fi

if [ "$(shasum -a 256 "$tarball" | awk '{ print $1 }')" != "$LUA_SHA256" ]; then
    printf 'error: checksum mismatch for %s\n' "$tarball" >&2
    exit 1
fi

rm -rf "$TMP/lua-$LUA_VERSION"
tar -xzf "$tarball" -C "$TMP"

printf 'build    %-16s %s (source build)\n' lua "$LUA_VERSION"
make -C "$TMP/lua-$LUA_VERSION/src" liblua.a CC=cc SYSCFLAGS="-DLUA_USE_POSIX"

mkdir -p "$LUA_PREFIX/lib/pkgconfig" "$LUA_PREFIX/include"
install -m 644 "$TMP/lua-$LUA_VERSION/src/liblua.a" "$LUA_PREFIX/lib/liblua.a"
install -m 644 \
    "$TMP/lua-$LUA_VERSION/src/lua.h" \
    "$TMP/lua-$LUA_VERSION/src/luaconf.h" \
    "$TMP/lua-$LUA_VERSION/src/lualib.h" \
    "$TMP/lua-$LUA_VERSION/src/lauxlib.h" \
    "$LUA_PREFIX/include/"

cat >"$LUA_PREFIX/lib/pkgconfig/lua52.pc" <<EOF
prefix=$LUA_PREFIX
exec_prefix=\${prefix}
libdir=\${exec_prefix}/lib
includedir=\${prefix}/include

Name: lua52
Description: Lua $LUA_VERSION interpreter library
Version: $LUA_VERSION
Libs: -L\${libdir} -llua
Libs.private: -lm
Cflags: -I\${includedir}
EOF
