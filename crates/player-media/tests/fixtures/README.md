# Presentation geometry fixture

`geometry-rotated.mp4` is a one-second MPEG-4 test pattern with 32 frames, coded at 64×32 with sample aspect ratio 2:1. A copy remux adds a 90-degree counterclockwise display matrix. Its presentation size is 32×128.

Generate with a full FFmpeg 9.0.2 build:

```sh
ffmpeg -f lavfi -i 'testsrc2=s=64x32:r=32:d=1,setsar=2' -c:v mpeg4 -bf 0 .work/tmp/geometry-base.mp4
ffmpeg -display_rotation:v:0 90 -i .work/tmp/geometry-base.mp4 -c copy crates/player-media/tests/fixtures/geometry-rotated.mp4
```

The frame count permits the threaded decoder to produce a frame during paused seek preroll. A single-frame clip instead exercises the separate delayed-decoder EOF limitation in PLAY-52.

`gradient-high10.mp4` is a lossless H.264 High 10 horizontal luma ramp at 64×32, with 32 frames. Generate it with a full FFmpeg build:

```sh
ffmpeg -f lavfi -i 'nullsrc=s=64x32:r=32:d=1,format=yuv420p10le,geq=lum=X/W*1023:cb=512:cr=512' -c:v libx264 -preset ultrafast -crf 0 -pix_fmt yuv420p10le crates/player-media/tests/fixtures/gradient-high10.mp4
```

Verify both files with `ffprobe` before replacing them. Encode-time rotation tags alone do not establish a display matrix.
