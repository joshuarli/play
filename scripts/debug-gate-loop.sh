#!/bin/bash
set -uo pipefail

count=${1:-20}
parallel=${2:-2}
faildir=$(mktemp -d)

worker() {
    local tag=$1
    for i in $(seq 1 "$count"); do
        if [ -e "$faildir/failed" ]; then
            return 0
        fi
        printf 'gate %s run %d\n' "$tag" "$i"
        if ! tests/run.sh linux-gpu-next; then
            : >"$faildir/failed"
            return 1
        fi
    done
}

pids=()
for tag in $(seq 1 "$parallel"); do
    worker "$tag" &
    pids+=($!)
done
status=0
for pid in "${pids[@]}"; do
    wait "$pid" || status=1
done
rm -rf "$faildir"
exit "$status"
