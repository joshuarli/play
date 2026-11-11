#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
VERSION=5.0.2
TAG=v5.0.2-laputa.2
SHA256=f8ae910d451336d2241821af5748abcbf547559fd083b73eefd097eb7558f593
URL="https://github.com/laputa-systems/rust-fontconfig/archive/refs/tags/$TAG.tar.gz"
LOCK="$ROOT/docker/linux/rust-fontconfig.Cargo.lock"
DEFAULT_PREFIX="$ROOT/.work/deps/rust-fontconfig/x86_64-unknown-linux-musl"
PREFIX="${RUST_FONTCONFIG_PREFIX:-$DEFAULT_PREFIX}"
ALLOW_LEGACY_DEFAULT=0
if [ -z "${RUST_FONTCONFIG_PREFIX:-}" ]; then ALLOW_LEGACY_DEFAULT=1; fi
SRC="$ROOT/.work/tmp/rust-fontconfig"
SONAME="librust_fontconfig.so.$VERSION"
INSTALL="$ROOT/scripts/rust_fontconfig_install.py"
FLAG_HELPER="$ROOT/scripts/cargo-rustflags.sh"
export TMPDIR="$ROOT/.work/tmp"

case "$(uname -sm)/$(ldd --version 2>&1)" in
    "Linux x86_64/"*musl*) ;;
    *)
        printf 'error: requires x86_64 musl Linux; run through scripts/linux-container.sh\n' >&2
        exit 1
        ;;
esac

python3 "$INSTALL" check-prefix "$PREFIX" "$ALLOW_LEGACY_DEFAULT"
mkdir -p -- "$SRC" "$PREFIX"
exec {prefix_lock}<"$PREFIX"
exec {source_lock}<"$SRC"
prefix_id=$(stat -Lc '%d:%i' "/proc/$$/fd/$prefix_lock")
source_id=$(stat -Lc '%d:%i' "/proc/$$/fd/$source_lock")
IFS=: read -r prefix_device prefix_inode <<<"$prefix_id"
IFS=: read -r source_device source_inode <<<"$source_id"
if [[ "$prefix_id" == "$source_id" ]]; then
    flock -x "$prefix_lock"
elif (( prefix_device < source_device || (prefix_device == source_device && prefix_inode < source_inode) )); then
    flock -x "$prefix_lock"
    flock -x "$source_lock"
else
    flock -x "$source_lock"
    flock -x "$prefix_lock"
fi

if ! python3 "$INSTALL" "$prefix_lock" prepare "$SONAME" "$DEFAULT_PREFIX" "$ALLOW_LEGACY_DEFAULT"; then
    python3 "$INSTALL" "$prefix_lock" invalidate
    exit 1
fi

LOCAL="${RUST_FONTCONFIG_SOURCE:-}"
if [ -n "$LOCAL" ]; then
    source_digest=$(tar -c -C "$LOCAL" --sort=name --exclude=./target --exclude=./.git --exclude=./.work . | sha256sum | awk '{print $1}')
else
    source_digest=$SHA256
fi
source "$FLAG_HELPER"
export RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-static-libgcc -C link-arg=-Wl,--as-needed -C link-arg=-Wl,-soname,$SONAME"
if [[ ! ${CARGO_ENCODED_RUSTFLAGS+x} ]]; then CARGO_ENCODED_RUSTFLAGS=; fi
append_cargo_rustflags -C target-feature=-crt-static -C link-arg=-static-libgcc \
    -C link-arg=-Wl,--as-needed -C "link-arg=-Wl,-soname,$SONAME"
input=$(cat "$0" "$INSTALL" "$FLAG_HELPER" "$LOCK" | cat - <(printf '%s\0%s\0%s\0' "$source_digest" "$PREFIX" "$CARGO_ENCODED_RUSTFLAGS") | sha256sum | awk '{print $1}')
if python3 "$INSTALL" "$prefix_lock" validate "$input" "$SONAME"; then
    printf 'rust-fontconfig %s already in %s\n' "$TAG" "$PREFIX"
    exit 0
fi

python3 "$INSTALL" "$prefix_lock" invalidate
archive="$SRC/$TAG.tar.gz"
if [ -n "$LOCAL" ]; then
    :
elif [ ! -f "$archive" ] || [ "$(sha256sum "$archive" | awk '{print $1}')" != "$SHA256" ]; then
    printf 'download %s\n' "$URL"
    curl -fsSL --retry 3 "$URL" -o "$archive"
fi
tree="$SRC/rust-fontconfig-${TAG#v}"
rm -rf "$tree"
if [ -n "$LOCAL" ]; then
    mkdir -p "$tree"
    tar -c -C "$LOCAL" --exclude=./target --exclude=./.git --exclude=./.work . | tar -x -C "$tree"
else
    [ "$(sha256sum "$archive" | awk '{print $1}')" = "$SHA256" ] || {
        printf 'error: %s does not match the pinned sha256\n' "$archive" >&2
        exit 1
    }
    tar -xzf "$archive" -C "$SRC"
fi
cp "$LOCK" "$tree/Cargo.lock"
printf '\n[workspace]\n' >>"$tree/Cargo.toml"

export CARGO_TARGET_DIR="$SRC/target"
(
    cd "$tree"
    cargo rustc --locked --release --lib --crate-type cdylib \
        --target x86_64-unknown-linux-musl \
        --no-default-features --features ffi \
        --config 'profile.release.lto="fat"' \
        --config 'profile.release.opt-level="s"' \
        --config 'profile.release.codegen-units=1'
)

python3 "$INSTALL" "$prefix_lock" publish "$input" "$SONAME" \
    "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so" \
    "$tree/ffi/rust_fontconfig.h" "$PREFIX" "$VERSION"
printf 'rust-fontconfig %s in %s\n' "$TAG" "$PREFIX"
