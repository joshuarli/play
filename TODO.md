Prefix: PLAY
Next: 42

# Backlog

## P2

- PLAY-20: Wire VideoToolbox and VA-API decode requests through `player-engine` and report `hwdec-current` from the decoder that actually opened. `player-media` already carries the hardware paths and their tests; the scheduler currently requests software decode.
- PLAY-21: Confirm on a real session that `build/mpv FILE` launched from a terminal takes keyboard focus. `0e7e0a2` runs AppKit's event loop on the main thread with playback on a second thread, as mpv does; Homebrew mpv takes focus on the owner's machine, and no agent session can test focus because macOS keeps it on the app being typed in.
- PLAY-23: HE-AAC plays as its AAC-LC core. Symphonia's AAC decoder skips SBR and PS, so HE-AAC loses the band above half its sample rate and HE-AAC v2 loses parametric stereo; mpv decodes both. `tests/run.sh mpv-diff` has `aac-he.m4a` and `aac-he-v2.m4a` but checks only play-to-end.
- PLAY-24: AAC layouts outside channel configurations 1-6 and 7.1 wide fail to open: configuration 12 (7.1 rear, what FFmpeg writes) and program-config-element layouts such as quad. `tests/run.sh mpv-diff` lists them in `KNOWN_GAPS`.
- PLAY-25: With `--gui` or in the bundled app, audio files with embedded cover art show a black window: the attached-picture frame never reaches it. Without `--gui` such files play audio-only and open no window. The TUI shows the cover through its own decode.

- PLAY-32: VideoToolbox zero-copy on Metal: once PLAY-20 decodes through VideoToolbox, hand its `CVPixelBuffer` IOSurfaces to libplacebo with `pl_metal_wrap_iosurface` (in the carried Metal patch) instead of copying planes to the CPU. Depends on PLAY-20.

## P3

- PLAY-41: `tests/fixtures/gen.sh` accepts any `ffmpeg version 9.0.2`, including Play's decode-only build at `~/.local/bin/ffmpeg`, which has no encoders; `tests/run.sh suite` then fails with `Unrecognized option 'preset'`, leaves `.work/fixtures` half-written, and every later gate times out. Check for the encoders the corpus needs (libx264 and the rest) and fail with that message instead.
- PLAY-40: `scripts/bench.py gui-cpu` hover sends pointer moves at about 6 Hz, not 60 Hz: each synchronous IPC `mouse` command takes roughly 150 ms to answer while the GUI player sits near 13% CPU, so the time is waiting, not work. Find whether the app answers IPC only after a blocking present or the harness waits on something else; if it is the app, it is pointer-input latency users feel. Then make the hover workload reach its intended rate and report the achieved rate.

- PLAY-26: SubRip `<i>`, `<b>`, `<u>`, and `<font color>` are stripped, not styled. `player-overlay::TextBlock` carries one style; styled spans need per-run attributes in both text backends (Core Text attributed ranges; HarfBuzz runs per span on Linux).
- PLAY-27: A right-to-left paragraph with `Align::Start` starts at the left edge. Subtitles centre, so only OSD messages and the OSC title see it; base direction should pick the start edge.
- PLAY-28: Line height is the tallest font's ascent plus descent across every fallback font on the block, on both platforms, so one CJK or Thai character spreads all lines apart; on Linux Noto Sans CJK makes it visibly wider than macOS. Line spacing should come from the primary font, with tall glyphs kept inside the inset.
- PLAY-33: `--gpu-api=metal` presents SDR only. The carried patch's swapchain fixes the layer at `BGRA8Unorm`, so 10-bit and EDR/HDR output need a wider pixel format, `wantsExtendedDynamicRangeContent`, and an EDR color space on the `CAMetalLayer`, reported through the swapchain's color space.
- PLAY-30: `tests/run.sh osc-input` passes only headless. Given a window (`tests/run.sh osc-input --gpu-api=vulkan --fs=no --geometry=1280x720`, either backend), four checks fail on a Retina display: `osc-areas` comes back in 2560x1440 backing pixels where the gate expects the 720p layout, and IPC pointer motion neither shows the OSC nor starts the hide timer. Decide whether `osc-areas` should report logical points and whether injected pointer input should reach a windowed OSC, then make the gate hold windowed.
## P4
