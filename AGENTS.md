# play — Architecture

Minimal media player for apple silicon only.

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
                               │   → resample to f32 planar (matches AVAudioEngine)
                               │   → memcpy per-plane → AVAudioPCMBuffer
                               │   → AVAudioPlayerNode
                               │   → completion handler updates audio_clock
                               │
                               └── Periodic (16ms timeout)
                                   → SubtitleTrack.text_at(audio_pts)
                                   → UiUpdate if changed
```

## Modules

| File | Lines | Responsibility |
|---|---|---|
| `main.rs` | ~265 | Thread spawning, file probe, stream info logging |
| `cmd.rs` | ~260 | Command/DemuxPacket/DemuxCommand/VideoFrame/UiUpdate/Args types, arg parser |
| `player.rs` | ~540 | State machine: crossbeam select!, decode dispatch, seek buffering, volume, subtitles |
| `demux.rs` | ~310 | ffmpeg format::input, packet reading, seek coalescing, stream probe |
| `decode_video.rs` | ~170 | VideoToolbox hwaccel setup, CVPixelBuffer extraction from AVFrame.data[3] |
| `decode_audio.rs` | ~130 | ffmpeg audio decode + resample to f32 planar via software::resampling |
| `video_out.rs` | ~300 | AVSampleBufferDisplayLayer, CMSampleBuffer from CVPixelBuffer, timebase sync |
| `audio_out.rs` | ~415 | CoreAudio AudioUnit, non-interleaved f32 render callback, planar buffer scheduling, clock reporting |
| `window.rs` | ~325 | NSWindow via objc2 define_class!, key monitor, GCD timer (~120Hz), layer setup |
| `osd.rs` | ~265 | CATextLayer OSD messages, NSAttributedString subtitles with outline |
| `sync.rs` | ~105 | Audio-master clock (AtomicI64), pause/resume, seek position |
| `subtitle.rs` | ~260 | SRT parser, binary search lookup, auto-detection of .srt files |
| `input.rs` | ~165 | Virtual key code → Command mapping (Carbon key codes + characters) |
| `time.rs` | ~180 | PTS↔microseconds conversion (i128 overflow-safe), HH:MM:SS format/parse |
| `terminal.rs` | ~80 | Audio-only mode: termion raw terminal, keyboard controls |
| `build.rs` | ~17 | Links Apple frameworks (AVFoundation, CoreMedia, CoreVideo, etc.) |

## Key Dependencies

- **ffmpeg-next 7** + **ffmpeg-sys-next 7** (vendored build, statically linked, VideoToolbox hwaccel)
- **objc2 0.6** ecosystem for Apple framework bindings (raw `msg_send!` with `AnyObject` for AVFoundation/AVFAudio/CoreMedia APIs that lack typed bindings)
- **block2 0.6** for Objective-C blocks (audio completion handlers, key monitor, timer)
- **dispatch2 0.3** for GCD timer on main queue
- **crossbeam-channel 0.5** for inter-thread communication
- **termion 4** for terminal raw mode (audio-only mode), **anyhow** for errors

## A/V Sync

Audio-master: audio plays at device rate, video adjusts. The audio completion handler
writes the current PTS to `Arc<AtomicI64>`. The player reads it to decide per-frame:
- **> 50ms late**: drop frame (release CVPixelBuffer, don't enqueue)
- **> 50ms early**: display anyway (layer handles timing in practice)
- **within ±50ms**: display immediately

## Seek Coalescing

Holding an arrow key generates ~30 `SeekRelative` commands/sec. Without coalescing, each
would trigger a full ffmpeg seek + decode flush round-trip. Two layers prevent this:

1. **Player buffering** (`player.rs`): At most one seek is in flight to the demuxer. While
   `pending_seeks > 0`, new seeks overwrite a `buffered_seek` field instead of dispatching.
   When the Flush returns, the buffered seek is dispatched without redundant decoder flushes
   (decoders are still clean — no packets were fed since the last flush).

2. **Demuxer coalescing** (`demux.rs`): The demuxer drains all pending seeks from `cmd_rx`
   at the top of each loop iteration (keeps only the last). The packet send uses
   `crossbeam::select!` to race `send` vs `recv`, so a seek command is processed immediately
   even when the packet channel is full (stale packet is dropped).

## OSD Rendering

Two `CATextLayer` sublayers on the content view's layer, above the display layer:
- **Message** (bottom-left, 16pt): seek position, volume, audio delay. Fades after 2s.
- **Subtitle** (bottom-center, dynamic size): NSAttributedString with stroke outline. Shown/hidden by PTS lookup.

Subtitle font size scales with window height (`height * 22/720`). Black stroke outline via
negative `NSStrokeWidth` plus subtle shadow for readability. Frame and margins recalculated
on each update. `CATransaction` disables implicit animations.

## Gotchas

- `initWithCommonFormat:sampleRate:channels:interleaved:` on AVAudioFormat crashes with
  a C++ exception from CoreAudio. Use `initStandardFormatWithSampleRate:channels:` instead
  (non-interleaved float32). Resampler outputs planar f32 to match; memcpy per-plane.
- `OpaqueCMSampleBuffer` needs a custom `RefEncode` impl (`Pointer → Struct`) to satisfy
  objc2's runtime type checking for `enqueueSampleBuffer:`.
- ffmpeg-sys-next `build-audiotoolbox` feature is broken on macOS (adds iOS flags). Omit it;
  AVAudioEngine is used via objc2 instead.
- `NSWindow` is not `Send`/`Sync` — stored in a `thread_local! RefCell`. Display layer pointer
  wrapped in `SendPtr` newtype for `OnceLock`.
- `define_class!` in objc2 0.6 requires unit structs (no inline ivars). State goes in globals.
