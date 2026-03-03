# play — Architecture

Minimal macOS media player in Rust. ~2600 lines across 15 source files.

## Threading Model

```
Main Thread (AppKit)           Player Thread              Demuxer Thread
─────────────────────          ─────────────               ──────────────
NSApplication.run()            crossbeam select! loop      blocking av_read_frame
NSWindow + layers              video/audio decode          sends DemuxPacket
key events → Command           A/V sync decisions          receives seek/stop
video frame display            schedules audio buffers
OSD/subtitle rendering         subtitle lookup

                               Audio Render Thread (implicit, AVAudioEngine)
                               pulls PCM, updates Arc<AtomicI64> clock
```

## Channel Layout

```
cmd_tx/rx          bounded(32)   Command        Main → Player     key events
demux_packet_tx/rx bounded(64)   DemuxPacket    Demuxer → Player  video/audio/sub packets
demux_cmd_tx/rx    bounded(4)    DemuxCommand   Player → Demuxer  seek/stop
video_frame_tx/rx  bounded(2)    VideoFrame     Player → Main     CVPixelBuffer + PTS
ui_update_tx/rx    unbounded     UiUpdate       Player → Main     OSD text, subtitles, EOF
audio_clock        Arc<AtomicI64>               Audio → Player    current PTS (microseconds)
```

## Data Flow

```
File → Demuxer → DemuxPacket ──┬── Video: VideoToolbox GPU decode
                               │   → CVPixelBuffer (retained)
                               │   → SyncClock.decide() (display/drop/wait)
                               │   → VideoFrame → Main Thread
                               │   → CMSampleBuffer → AVSampleBufferDisplayLayer
                               │
                               ├── Audio: ffmpeg CPU decode
                               │   → resample to f32 packed
                               │   → deinterleave to planar
                               │   → AVAudioPCMBuffer → AVAudioPlayerNode
                               │   → completion handler updates audio_clock
                               │
                               └── Periodic (16ms timeout)
                                   → SubtitleTrack.text_at(audio_pts)
                                   → UiUpdate if changed
```

## Modules

| File | Lines | Responsibility |
|---|---|---|
| `main.rs` | 258 | CLI parsing (clap), thread spawning, file probe, playlist entry |
| `cmd.rs` | 99 | Command/DemuxPacket/DemuxCommand/VideoFrame/UiUpdate/Args types |
| `player.rs` | 403 | State machine: crossbeam select! loop, decode dispatch, seek, volume, subtitles |
| `demux.rs` | 214 | ffmpeg format::input, packet reading loop, seek handling, stream probe |
| `decode_video.rs` | 176 | VideoToolbox hwaccel setup, CVPixelBuffer extraction from AVFrame.data[3] |
| `decode_audio.rs` | 122 | ffmpeg audio decode + resample to f32 packed via software::resampling |
| `video_out.rs` | 186 | AVSampleBufferDisplayLayer, CMSampleBuffer creation from CVPixelBuffer |
| `audio_out.rs` | 187 | AVAudioEngine + AVAudioPlayerNode, buffer scheduling, clock reporting |
| `window.rs` | 291 | NSWindow via objc2 define_class!, key monitor, GCD timer (~120Hz), layer setup |
| `osd.rs` | 191 | CATextLayer for OSD messages + subtitles, CGColor helpers, fade timer |
| `sync.rs` | 86 | Audio-master clock, SyncAction decisions (±50ms thresholds) |
| `subtitle.rs` | 134 | SRT parser, auto-detection of .srt files alongside video |
| `input.rs` | 63 | Virtual key code → Command mapping (Carbon key codes + characters) |
| `time.rs` | 56 | PTS↔microseconds conversion, HH:MM:SS formatting/parsing |
| `terminal.rs` | 79 | Audio-only mode: crossterm raw terminal, keyboard controls |
| `build.rs` | 17 | Links Apple frameworks (AVFoundation, CoreMedia, CoreVideo, etc.) |

## Key Dependencies

- **ffmpeg-next 7** + **ffmpeg-sys-next 7** (vendored build, statically linked, VideoToolbox hwaccel)
- **objc2 0.6** ecosystem for Apple framework bindings (raw `msg_send!` with `AnyObject` for AVFoundation/AVFAudio/CoreMedia APIs that lack typed bindings)
- **block2 0.6** for Objective-C blocks (audio completion handlers, key monitor, timer)
- **dispatch2 0.3** for GCD timer on main queue
- **crossbeam-channel 0.5** for inter-thread communication
- **clap 4** for CLI, **crossterm 0.28** for terminal mode, **anyhow** for errors

## A/V Sync

Audio-master: audio plays at device rate, video adjusts. The audio completion handler
writes the current PTS to `Arc<AtomicI64>`. The player reads it to decide per-frame:
- **> 50ms late**: drop frame (release CVPixelBuffer, don't enqueue)
- **> 50ms early**: display anyway (layer handles timing in practice)
- **within ±50ms**: display immediately

## OSD Rendering

Two `CATextLayer` sublayers on the content view's layer, above the display layer:
- **Message** (bottom-left, 16pt): seek position, volume, audio delay. Fades after 2s.
- **Subtitle** (bottom-center, 22pt, wrapped): SRT text. Shown/hidden by PTS lookup.

White text + black shadow (radius 2-3px) for contrast. `CATransaction` disables implicit
animations. Font sizing is fixed (not scaled to video height).

## Gotchas

- `initWithCommonFormat:sampleRate:channels:interleaved:` on AVAudioFormat crashes with
  a C++ exception from CoreAudio. Use `initStandardFormatWithSampleRate:channels:` instead
  (non-interleaved float32). Audio data must be deinterleaved before scheduling.
- `OpaqueCMSampleBuffer` needs a custom `RefEncode` impl (`Pointer → Struct`) to satisfy
  objc2's runtime type checking for `enqueueSampleBuffer:`.
- ffmpeg-sys-next `build-audiotoolbox` feature is broken on macOS (adds iOS flags). Omit it;
  AVAudioEngine is used via objc2 instead.
- `NSWindow` is not `Send`/`Sync` — stored in a `thread_local! RefCell`. Display layer pointer
  wrapped in `SendPtr` newtype for `OnceLock`.
- `define_class!` in objc2 0.6 requires unit structs (no inline ivars). State goes in globals.
