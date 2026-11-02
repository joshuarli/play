#!/bin/sh
tool=${PLAYER_VG_TOOL:-helgrind}
case "$tool" in
    helgrind) opts="--history-level=approx" ;;
    memcheck) opts="--track-origins=yes --leak-check=no" ;;
    *) echo "unknown PLAYER_VG_TOOL: $tool" >&2; exit 2 ;;
esac
mkdir -p /work/.work/tmp
exec valgrind --tool="$tool" $opts --num-callers=24 \
    --error-limit=no --fair-sched=yes \
    --log-file="/work/.work/tmp/vg-$tool.log" /work/build-linux/mpv "$@"
