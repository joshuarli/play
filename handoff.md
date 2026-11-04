# Handoff

Paused during playback debugging after the Rust port cleanup. Source tree is clean at
`fb5ea5c`; the temporary fullscreen and audio instrumentation changes were reverted.

## State

- `HEAD`: `fb5ea5c docs: finish the post-cleanup description`
- Commits are local; nothing is pushed.
- Final artifacts: `dist/Player.app` and `dist/player-x86_64-linux-musl.tar.gz`.
- Last verified gates: macOS suite 340/340, Linux suite 334/334, `scripts/audit.sh` ok,
  `scripts/audit-linux.sh` ok, `scripts/release.sh` ok, `make linux` ok,
  `scripts/measure.sh final` ok.
- macOS release: 24,123,793 B app, 7,600,640 B executable, 15,127,360 B closure.
- Linux release: 24,471,888 B stripped, seven `NEEDED` libraries.

## Reported symptoms

Running `dist/Player.app/Contents/MacOS/player ~/Downloads/'The Science of Roast Chicken [dHdrFjpBYsk].mp4'`:

1. `f` does not toggle fullscreen.
2. Audio sounds "peaky", "sparkly", or like it is blowing out the speakers.

The file is VP9 1080p 29.97 fps with Opus 48 kHz stereo audio, 8:05 long.

## What was verified

### Fullscreen

- IPC `keypress f` toggles the `fullscreen` property and the native window:
  `osd-dimensions` went from 640x480 to 3024x1898 and back.
- A real macOS key event posted directly to the player PID with `CGEventPostToPid`
  also toggled it. The Rust key path and the AppKit `toggleFullScreen:` bridge work.
- A System Events keystroke did not reach the player because
  `lsappinfo front` stayed on Ghostty. The window is not becoming the frontmost/key
  window in the current session when launched from a terminal.
- Tried, then reverted:
  - `[NSApp finishLaunching]`
  - calling `makeKeyAndOrderFront:` and `activateIgnoringOtherApps:` again after the
    window is created
  - `NSRunningApplication activateWithOptions:`
- Next step: reproduce on an interactive GUI session and confirm `lsappinfo front`
  and the window's first responder. Consider `TransformProcessType` +
  `SetFrontProcess`, an explicit click-to-focus path, or launching through the app
  bundle. Do not assume the fullscreen toggle itself is broken.

### Audio

- Source peak is -0.5 dBFS and mean volume is -21.6 dB; this is a normal, loud
  YouTube-style master.
- Decoded with the in-tree Symphonia pipeline:
  - full file peak 0.9456, RMS 0.0827
  - first 5.7M frames (about 60 s) peak 0.715, RMS 0.0887
  - no DC offset, no samples above 1.0
- Compared against FFmpeg's decode of the first 10 s:
  - difference RMS 1.2e-5, difference peak 6.6e-4 (about -98 dB)
  - our output is not louder than FFmpeg's decode.
- Captured the exact f32 samples handed to `CoreAudioSink` for the first 9.5 s:
  - peak 0.640, RMS 0.097
  - max adjacent-sample delta 0.224, zero samples near full scale
  - no write-side CoreAudio underrun was counted in a 10 s video run.
- Microphone recordings at a fixed system volume showed app, `afplay`, and `ffplay`
  at broadly similar levels; `--volume=0` silenced the app. Gain and volume plumbing
  are working.

The decoded audio and the samples written to the audio backend are clean. Remaining
suspects are the CoreAudio playback path on the user's device, AudioQueue format
handling, or audio starvation while the same worker thread decodes/renders the
1080p VP9 video. The A/V sync suspicion points at the latter: the scheduler keeps
only `AUDIO_HIGH_WATER = 0.05` seconds queued, and the CoreAudio sink holds three
2048-frame buffers (about 128 ms). A slow video step can starve that queue.

Suggested next experiments:

- Switch CoreAudio output from f32 to interleaved s16 (the baseline/mpv path used
  s16) and compare on the real device.
- Test the same file audio-only versus video to separate decoder load from the sink.
- Measure real-device underruns or callback gaps, not only the write-side queue.
- Increase `AUDIO_HIGH_WATER` and the CoreAudio buffer count, or move audio feeding
  off the video-presenting worker, then re-check the manual audio checklist.
- If the device path is ruled out, test whether the user's output device resamples
  48 kHz and whether a different device reproduces the sparkle.

## Commands

```sh
scripts/build.sh
tests/run.sh suite
scripts/release.sh
python3 tests/trace.py --selftest
scripts/linux-container.sh tests/run.sh suite
scripts/linux-container.sh scripts/audit-linux.sh
scripts/measure.sh final
```

Scratch is under `.work/`; do not commit it. The reverted activation/instrumentation
patches are described above rather than left in the tree.
