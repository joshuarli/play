#!/bin/sh
exec valgrind --tool=helgrind --history-level=approx --num-callers=24 \
    --error-limit=no --fair-sched=yes \
    --log-file=/work/.work/tmp/helgrind.log /work/build-linux/mpv "$@"
