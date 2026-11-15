#!/bin/bash
set -euo pipefail

app=$1
archive=$2
archive=$(cd "$(dirname "$archive")" && pwd)/$(basename "$archive")
cd "$(dirname "$app")"
app=$(basename "$app")
export TZ=UTC COPYFILE_DISABLE=1
find "$app" -exec touch -h -t 197001010000.00 {} +
find "$app" -print0 | LC_ALL=C sort -z | tar --uid 0 --gid 0 --uname '' --gname '' \
    --no-xattrs --no-acls --no-mac-metadata --no-recursion --null -T - -cJf "$archive"
