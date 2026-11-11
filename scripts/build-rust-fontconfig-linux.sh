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
tree="$SRC/rust-fontconfig-${TAG#v}"
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

check_source_path() {
    local directory
    for directory in "$ROOT/.work" "$ROOT/.work/tmp" "$SRC" "$SRC/target" "$tree"; do
        if [ -L "$directory" ]; then
            printf 'error: rust-fontconfig source path contains a symlink: %s\n' "$directory" >&2
            return 1
        fi
    done
    if [ -e "$tree" ] && [ ! -d "$tree" ]; then
        printf 'error: invalid rust-fontconfig source tree: %s\n' "$tree" >&2
        return 1
    fi
}

check_source_path
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
if [[ "$(stat -Lc '%d:%i' -- "$PREFIX")" != "$prefix_id" ||
      "$(stat -Lc '%d:%i' -- "$SRC")" != "$source_id" ]]; then
    printf 'error: rust-fontconfig lock path changed while waiting\n' >&2
    exit 1
fi
check_source_path

if ! python3 "$INSTALL" "$prefix_lock" prepare "$SONAME" "$DEFAULT_PREFIX" "$ALLOW_LEGACY_DEFAULT"; then
    python3 "$INSTALL" "$prefix_lock" invalidate
    exit 1
fi

LOCAL="${RUST_FONTCONFIG_SOURCE:-}"
calculate_source_digest() {
    tar -c -C "$LOCAL" --sort=name --exclude=./target --exclude=./.git --exclude=./.work . | sha256sum | awk '{print $1}'
}
verify_source_digest() {
    local current
    current=$(calculate_source_digest) || {
        printf 'error: cannot fingerprint rust-fontconfig source\n' >&2
        return 1
    }
    [ "$current" = "$source_digest" ]
}
if [ -n "$LOCAL" ]; then
    source_digest=$(calculate_source_digest)
else
    source_digest=$SHA256
fi
source "$FLAG_HELPER"
export RUSTFLAGS="-C target-feature=-crt-static -C link-arg=-static-libgcc -C link-arg=-Wl,--as-needed -C link-arg=-Wl,-soname,$SONAME"
if [[ ! ${CARGO_ENCODED_RUSTFLAGS+x} ]]; then CARGO_ENCODED_RUSTFLAGS=; fi
append_cargo_rustflags -C target-feature=-crt-static -C link-arg=-static-libgcc \
    -C link-arg=-Wl,--as-needed -C "link-arg=-Wl,-soname,$SONAME"
profile_settings() {
    local setting
    while IFS= read -r setting; do
        [[ "$setting" =~ ^CARGO_PROFILE_RELEASE_[A-Z0-9_]+$ ]] || continue
        case "$setting" in
            CARGO_PROFILE_RELEASE_LTO|CARGO_PROFILE_RELEASE_OPT_LEVEL|CARGO_PROFILE_RELEASE_CODEGEN_UNITS) continue ;;
        esac
        printf '%s\0%s\0' "$setting" "${!setting}"
    done < <(compgen -e | LC_ALL=C sort)
}
calculate_input() {
    cat "$0" "$INSTALL" "$FLAG_HELPER" "$LOCK" | cat - <(printf '%s\0%s\0%s\0%s\0' "$source_digest" "$PREFIX" "$CARGO_ENCODED_RUSTFLAGS" "$1") <(profile_settings) | sha256sum | awk '{print $1}'
}
environment_identity=$(python3 "$INSTALL" build-environment-identity "$tree") || environment_identity=
if [ -n "$environment_identity" ] && input=$(calculate_input "$environment_identity") && \
    python3 "$INSTALL" "$prefix_lock" validate "$input" "$SONAME"; then
    if [ -n "$LOCAL" ] && ! verify_source_digest; then
        printf 'error: rust-fontconfig source changed during validation\n' >&2
        exit 1
    fi
    printf 'rust-fontconfig %s already in %s\n' "$TAG" "$PREFIX"
    exit 0
fi

python3 "$INSTALL" check-target "$SRC/target"
python3 "$INSTALL" "$prefix_lock" invalidate
archive="$SRC/$TAG.tar.gz"
if [ -n "$LOCAL" ]; then
    :
elif [ ! -f "$archive" ] || [ "$(sha256sum "$archive" | awk '{print $1}')" != "$SHA256" ]; then
    printf 'download %s\n' "$URL"
    curl -fsSL --retry 3 "$URL" -o "$archive"
fi
rm -rf "$tree"
if [ -n "$LOCAL" ]; then
    mkdir -p "$tree"
    tar -c -C "$LOCAL" --exclude=./target --exclude=./.git --exclude=./.work . | tar -x -C "$tree"
    if ! verify_source_digest; then
        printf 'error: rust-fontconfig source changed during copying\n' >&2
        exit 1
    fi
else
    [ "$(sha256sum "$archive" | awk '{print $1}')" = "$SHA256" ] || {
        printf 'error: %s does not match the pinned sha256\n' "$archive" >&2
        exit 1
    }
    tar -xzf "$archive" -C "$SRC"
fi
if [ -L "$tree/Cargo.toml" ] || [ ! -f "$tree/Cargo.toml" ] || \
    [ -L "$tree/Cargo.lock" ] || { [ -e "$tree/Cargo.lock" ] && [ ! -f "$tree/Cargo.lock" ]; }; then
    printf 'error: invalid rust-fontconfig source manifest or lockfile\n' >&2
    exit 1
fi
cp "$LOCK" "$tree/Cargo.lock"
printf '\n[workspace]\n' >>"$tree/Cargo.toml"
environment_identity=$(python3 "$INSTALL" build-environment-identity "$tree") || {
    printf 'error: configured rust-fontconfig linker cannot be resolved, or compiler cannot be resolved\n' >&2
    exit 1
}
input=$(calculate_input "$environment_identity")

export CARGO_TARGET_DIR="$SRC/target"
(
    cd "$tree"
    cargo clean --locked --workspace --release --target x86_64-unknown-linux-musl \
        --config 'profile.release.lto="fat"' \
        --config 'profile.release.opt-level="s"' \
        --config 'profile.release.codegen-units=1'
    cargo rustc --locked --release --lib --crate-type cdylib \
        --target x86_64-unknown-linux-musl \
        --no-default-features --features ffi \
        --config 'profile.release.lto="fat"' \
        --config 'profile.release.opt-level="s"' \
        --config 'profile.release.codegen-units=1'
)

if [ "$(python3 "$INSTALL" build-environment-identity "$tree")" != "$environment_identity" ]; then
    printf 'error: configured rust-fontconfig linker changed during build, or compiler changed\n' >&2
    exit 1
fi
if [ -n "$LOCAL" ] && ! verify_source_digest; then
    printf 'error: rust-fontconfig source changed during build\n' >&2
    exit 1
fi
python3 "$INSTALL" "$prefix_lock" publish "$input" "$SONAME" \
    "$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release/librust_fontconfig.so" \
    "$tree/ffi/rust_fontconfig.h" "$PREFIX" "$VERSION"
printf 'rust-fontconfig %s in %s\n' "$TAG" "$PREFIX"
