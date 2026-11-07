#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)

strip_binary=
if [ "${1:-}" = --strip ]; then
    strip_binary=1
    shift
fi
[ $# -eq 3 ] || {
    printf 'usage: %s [--strip] BINARY PREFIX NAME\n' "$0" >&2
    exit 2
}
binary=$1
prefix=$2
name=$3

case "$(uname -s)" in
    Linux) target=x86_64-unknown-linux-musl ;;
    *) printf 'error: the bin/lib layout is the Linux player layout\n' >&2; exit 2 ;;
esac
runtime_lib="${FFMPEG_PREFIX:-$ROOT/.work/deps/ffmpeg-runtime/$target}/lib"
[ -d "$runtime_lib" ] || {
    printf 'error: no FFmpeg runtime libraries at %s; run scripts/fetch-ffmpeg-runtime.sh %s\n' "$runtime_lib" "$target" >&2
    exit 1
}

mkdir -p "$prefix/bin"
if [ -n "$strip_binary" ]; then
    strip -o "$prefix/bin/$name" "$binary"
else
    install -m 755 "$binary" "$prefix/bin/$name"
fi
ln -sfn "bin/$name" "$prefix/$name"

if [ -e "$prefix/lib" ] && [ ! -L "$prefix/lib" ]; then
    printf 'error: %s/lib exists and is not the runtime link\n' "$prefix" >&2
    exit 1
fi
relative=$(python3 -c 'import os, sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$runtime_lib" "$prefix")
ln -sfn "$relative" "$prefix/lib"
