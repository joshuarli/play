#!/bin/sh
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
VK_DRIVER_FILES="$root/.work/deps/release/share/vulkan/icd.d/MoltenVK_icd.json"
export VK_DRIVER_FILES
exec "$@"
