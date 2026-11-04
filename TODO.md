Prefix: PLAY
Next: 26

# Backlog

## P2

- PLAY-20: Wire VideoToolbox and VA-API decode requests through `player-engine` and report `hwdec-current` from the decoder that actually opened. `player-media` already carries the hardware paths and their tests; the scheduler currently requests software decode.
- PLAY-21: Confirm on a real session that `build/mpv FILE` launched from a terminal takes keyboard focus. `0e7e0a2` runs AppKit's event loop on the main thread with playback on a second thread, as mpv does; Homebrew mpv takes focus on the owner's machine, and no agent session can test focus because macOS keeps it on the app being typed in.
- PLAY-23: HE-AAC plays as its AAC-LC core. Symphonia's AAC decoder skips SBR and PS, so HE-AAC loses the band above half its sample rate and HE-AAC v2 loses parametric stereo; mpv decodes both. `tests/run.sh mpv-diff` has `aac-he.m4a` and `aac-he-v2.m4a` but checks only play-to-end.
- PLAY-24: AAC layouts outside channel configurations 1-6 and 7.1 wide fail to open: configuration 12 (7.1 rear, what FFmpeg writes) and program-config-element layouts such as quad. `tests/run.sh mpv-diff` lists them in `KNOWN_GAPS`.
- PLAY-25: Audio files with embedded cover art show a black GUI window. The attached-picture frame never reaches the window (also in the Lua release); the TUI shows the cover through its own decode.

## P3

## P4
