#!/bin/sh
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMAGE=player-linux

docker build -q -t "$IMAGE" -f "$ROOT/docker/linux/Dockerfile" "$ROOT/docker/linux" >/dev/null

mkdir -p "$ROOT/.work/home"
exec docker run --rm --init \
    --security-opt seccomp=unconfined \
    --user "$(id -u):$(id -g)" \
    -e HOME=/work/.work/home \
    -e CARGO_HOME=/work/.work/home/cargo \
    -v "$ROOT:/work" -w /work \
    "$IMAGE" "$@"
