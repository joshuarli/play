#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
DEPS="$ROOT/.work/deps"

mkdir -p "$DEPS"

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
pin meson 1.12.1
pin ninja 1.13.2
pin pkgconf 3.0.10
