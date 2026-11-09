#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=5.0.2
TAG=v5.0.2-laputa.1
SHA256=2cbbc3f8ef457252a5ba822de0b5ac99138a4defc2ea04affaecf7ac90a16ace
URL="https://github.com/laputa-systems/rust-fontconfig/archive/refs/tags/$TAG.tar.gz"
LOCK="$ROOT/docker/linux/rust-fontconfig.Cargo.lock"
PREFIX="${RUST_FONTCONFIG_PREFIX:-$ROOT/.work/deps/rust-fontconfig/x86_64-unknown-linux-musl}"
SRC="$ROOT/.work/tmp/rust-fontconfig"
SONAME="librust_fontconfig.so.$VERSION"
export TMPDIR="$ROOT/.work/tmp"

case "$(uname -sm)/$(ldd --version 2>&1)" in
    "Linux x86_64/"*musl*) ;;
    *)
        printf 'error: requires x86_64 musl Linux; run through scripts/linux-container.sh\n' >&2
        exit 1
        ;;
esac

input=$(cat "$0" "$LOCK" | sha256sum | awk '{print $1}')
if [ -f "$PREFIX/lib/$SONAME" ] && [ "$(cat "$PREFIX/.input" 2>/dev/null)" = "$input" ]; then
    printf 'rust-fontconfig %s already in %s\n' "$TAG" "$PREFIX"
    exit 0
fi

mkdir -p "$SRC" "$PREFIX/lib/pkgconfig" "$PREFIX/include/rust-fontconfig"
archive="$SRC/$TAG.tar.gz"
if [ ! -f "$archive" ] || [ "$(sha256sum "$archive" | awk '{print $1}')" != "$SHA256" ]; then
    printf 'download %s\n' "$URL"
    curl -fsSL --retry 3 "$URL" -o "$archive"
fi
[ "$(sha256sum "$archive" | awk '{print $1}')" = "$SHA256" ] || {
    printf 'error: %s does not match the pinned sha256\n' "$archive" >&2
    exit 1
}

tree="$SRC/rust-fontconfig-${TAG#v}"
rm -rf "$tree"
tar -xzf "$archive" -C "$SRC"
cp "$LOCK" "$tree/Cargo.lock"
printf '\n[workspace]\n' >>"$tree/Cargo.toml"

export CARGO_TARGET_DIR="$SRC/target"
export RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-static-libgcc -C link-arg=-Wl,--as-needed -C link-arg=-Wl,-soname,$SONAME"
(
    cd "$tree"
    cargo rustc --locked --release --lib --crate-type cdylib \
        --target x86_64-unknown-linux-musl \
        --no-default-features --features ffi \
        --config 'profile.release.lto="fat"' \
        --config 'profile.release.opt-level="s"' \
        --config 'profile.release.codegen-units=1'
)

install -m 755 "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so" "$PREFIX/lib/$SONAME"
ln -sfn "$SONAME" "$PREFIX/lib/librust_fontconfig.so"
install -m 644 "$tree/ffi/rust_fontconfig.h" "$PREFIX/include/rust-fontconfig/rust_fontconfig.h"
cat >"$PREFIX/lib/pkgconfig/rust-fontconfig.pc" <<PC
prefix=$PREFIX
libdir=\${prefix}/lib
includedir=\${prefix}/include/rust-fontconfig

Name: rust-fontconfig
Description: Pure-Rust font discovery, matching, and fallback with a C API
Version: $VERSION
Libs: -L\${libdir} -lrust_fontconfig
Cflags: -I\${includedir}
PC
printf '%s\n' "$input" >"$PREFIX/.input"
printf 'rust-fontconfig %s in %s\n' "$TAG" "$PREFIX"
