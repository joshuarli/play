# play

A lightweight macOS video and audio player. Single 15 MB binary. No runtime dependencies.

## Why?

mpv is excellent and cross-platform, but on macOS it ships its own Metal rendering
pipeline — uploading every decoded frame to the GPU for shader processing, color
conversion, and compositing. That's powerful and flexible, but it means the GPU is
always busy, even for simple playback.

**play** takes a different approach: hand the compressed bitstream to VideoToolbox and
let Apple's hardware decoder produce frames directly into an `AVSampleBufferDisplayLayer`.
The GPU does almost nothing — decode and display happen in hardware, with zero shader
passes and zero extra copies.

## Architecture

~7K lines of Rust. No GUI framework, no Electron, no Swift — just raw AppKit and
CoreAudio via Rust's objc2 bindings.

```
ffmpeg (libav*)          — demux & decode (statically linked)
VideoToolbox             — hardware video decoding
AVSampleBufferDisplayLayer — zero-copy display
CoreAudio (AUHAL)        — low-latency audio output
AppKit                   — window, input, on-screen display
```

Everything libav is compiled in and statically linked at build time. There is no
ffmpeg CLI dependency — no `brew install ffmpeg`, no `PATH` lookup, no version
mismatch. One binary, done.

## Features

- Hardware-accelerated decode via VideoToolbox
- Gapless A/V sync driven by audio clock
- Keyboard-driven (mpv-like bindings)
- SRT subtitle support with on-screen display
- Multiple audio track switching
- Audio-only mode with terminal UI

## Key bindings

| Key | Action |
|---|---|
| Space | Play / Pause |
| ← / → | Seek ±5s |
| Shift+← / → | Seek ±1s (exact) |
| ↑ / ↓ | Volume |
| Shift+↑ / ↓ | Seek ±60s |
| m | Mute |
| a | Cycle audio track |
| s | Cycle subtitles |
| f | Fullscreen |
| n / p | Next / previous file |
| q / Esc | Quit |

## On ffmpeg

~98% of the binary is statically linked ffmpeg (libavcodec, libavformat, libavutil,
libswresample). Video playback requires it — ffmpeg feeds packets to VideoToolbox for
hardware decode, and libavformat handles container demuxing for every format we support.

For a future audio-only player, [Symphonia](https://github.com/pdeljanov/Symphonia)
could replace ffmpeg entirely — it's a pure Rust audio framework with excellent MP3,
FLAC, Vorbis, AAC, and ALAC decoders, plus demuxers for MP4, MKV, Ogg, and WAV. The
one gap is Opus (not yet implemented). That would mean zero C dependencies and a
sub-1 MB binary. But for now, one program that plays both music and video is worth the
15 MB.
