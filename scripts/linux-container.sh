#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMAGE=player-linux

docker build --platform linux/amd64 -q -t "$IMAGE" -f "$ROOT/docker/linux/Dockerfile" "$ROOT" >/dev/null

mkdir -p "$ROOT/.work/home"

devices=()
for node in /dev/dri/renderD*; do
    [ -e "$node" ] && devices+=(--device "$node" --group-add "$(stat -c %g "$node")")
done

exec docker run --rm --init --platform linux/amd64 \
    --security-opt seccomp=unconfined --security-opt apparmor=unconfined \
    --user "$(id -u):$(id -g)" \
    -e HOME=/work/.work/home \
    -e CARGO_HOME=/work/.work/home/cargo \
    -e CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=/work/docker/linux/musl-cc-wrap.sh \
    "${devices[@]+"${devices[@]}"}" \
    -v "$ROOT:/work" -w /work \
    "$IMAGE" "$@"
