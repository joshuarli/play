#!/bin/bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMAGE=$(docker build --platform linux/amd64 -q -t player-linux -f "$ROOT/docker/linux/Dockerfile" "$ROOT")
if [[ ! "$IMAGE" =~ ^sha256:[[:xdigit:]]{64}$ ]]; then
    printf 'error: docker build did not return an image ID\n' >&2
    exit 1
fi

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
