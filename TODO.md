Prefix: PLAY
Next: 23

# Backlog

## P2

- PLAY-20: Wire VideoToolbox and VA-API decode requests through `player-engine` and report `hwdec-current` from the decoder that actually opened. `player-media` already carries the hardware paths and their tests; the scheduler currently requests software decode.
- PLAY-21: Launched from a terminal, the macOS window never becomes key, so real keystrokes such as `f` go to the terminal. IPC `keypress f` and `CGEventPostToPid` both toggle fullscreen, so the key path works and `lsappinfo front` stays on the terminal. Re-running `finishLaunching`, `makeKeyAndOrderFront:`, `activateIgnoringOtherApps:`, and `NSRunningApplication activateWithOptions:` did not help. Next: try `TransformProcessType` with `SetFrontProcess`, click-to-focus, or launching through the bundle.
- PLAY-22: CoreAudio output sounds "peaky" on a VP9 1080p / Opus 48 kHz file. The decoded samples match FFmpeg to about -98 dB and the samples written to `CoreAudioSink` are clean with no write-side underrun, so the suspect is starvation: the scheduler queues only `AUDIO_HIGH_WATER = 0.05` s and the sink holds three 2048-frame buffers while one worker decodes and presents video. Next: compare audio-only against video playback on the device, measure callback gaps, raise the high-water mark and buffer count or feed audio off the video worker, and try s16 output.

## P3

## P4
