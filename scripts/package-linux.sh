#!/bin/bash
set -euo pipefail

tar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -cJf "$2" -C "$1" README player
