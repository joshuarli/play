Prefix: PLAY
Next: 23

# Backlog

## P2

- PLAY-20: Wire VideoToolbox and VA-API decode requests through `player-engine` and report `hwdec-current` from the decoder that actually opened. `player-media` already carries the hardware paths and their tests; the scheduler currently requests software decode.
- PLAY-21: Launched from a terminal, the macOS window may never become key, so real keystrokes such as `f` go to the terminal. `80e30e7` now finishes AppKit launching and activates from `applicationDidFinishLaunching`; confirm on a real session that `build/mpv FILE` takes keyboard focus. If it does not, try `TransformProcessType` with `SetFrontProcess`, or click-to-focus.

## P3

## P4
