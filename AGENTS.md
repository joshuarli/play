# play — Architecture

Minimal media player for apple silicon only.

## Threading Model

```
Main Thread (AppKit)           Player Thread              Demuxer Thread
─────────────────────          ─────────────               ──────────────
NSApplication.run()            crossbeam select! loop      blocking av_read_frame
NSWindow + layers              video/audio decode          sends DemuxPacket
key events → Command           seek state machine          receives seek/stop/ChangeAudio
video frame display            schedules audio buffers     150MB packet cache (binary search)
OSD/subtitle rendering         subtitle lookup

                               Audio Render Thread (implicit, CoreAudio AudioUnit)
                               lock-free SPSC ring pop, updates Arc<AtomicI64> clock
```

## Channel Layout

```
cmd_tx/rx          bounded(32)   Command        Main → Player     key events
demux_packet_tx/rx bounded(64)   DemuxPacket    Demuxer → Player  video/audio/sub packets
demux_cmd_tx/rx    bounded(4)    DemuxCommand   Player → Demuxer  seek/stop/ChangeAudio
video_frame_tx/rx  bounded(8)    VideoFrame     Player → Main     PixelBuffer (RAII) + PTS
ui_update_tx/rx    unbounded     UiUpdate       Player → Main     OSD text, subtitles, EOF
audio_clock        Arc<AtomicI64>               Audio → Player    current PTS (microseconds)
```

## Data Flow

```
File → Demuxer (PacketCache, binary search) → DemuxPacket
   │
   ├── Video: VideoToolbox GPU decode
   │   → PixelBuffer (RAII, CVPixelBufferRelease on Drop)
   │   → try_send to bounded(8) channel
   │   → Main Thread 240Hz timer
   │   → CMSampleBuffer → AVSampleBufferDisplayLayer
   │
   ├── Audio: ffmpeg CPU decode
   │   → resample to f32 planar (per-channel Vec<Vec<f32>>)
   │   → push_slice into lock-free SPSC ring buffers (65536 samples/channel)
   │   → CoreAudio render callback pops from rings (no locks)
   │   → render callback updates audio_clock via AtomicI64
   │
   └── Periodic (16ms timeout / 1ms when seek queued)
       → SubtitleTrack.text_at(audio_pts)
       → UiUpdate if changed (allocation-free comparison)
```

## Modules

| File | Lines | Responsibility |
|---|---|---|
| `main.rs` | ~320 | Thread spawning, file probe, playlist loop, stream info logging |
| `cmd.rs` | ~330 | Command/DemuxPacket/DemuxCommand/VideoFrame/PixelBuffer(RAII)/UiUpdate/Args types, arg parser |
| `player.rs` | ~985 | State machine: crossbeam select!, decode dispatch, seek coalescing, volume, subtitles, audio track switching |
| `demux.rs` | ~810 | ffmpeg format::input, 150MB packet cache (binary search), seek coalescing, ChangeAudio, stream probe |
| `decode_video.rs` | ~170 | VideoToolbox hwaccel setup, PixelBuffer extraction from AVFrame.data[3] |
| `decode_audio.rs` | ~230 | ffmpeg audio decode + resample to f32 planar, per-channel planes, 8192-sample accumulation, end_us() |
| `video_out.rs` | ~340 | AVSampleBufferDisplayLayer, CMSampleBuffer from PixelBuffer, CMTimebase sync |
| `audio_out.rs` | ~490 | CoreAudio AudioUnit, lock-free SPSC ring buffers (65536 samples/ch), non-interleaved f32 render callback |
| `window.rs` | ~455 | NSWindow via objc2 define_class!, key/mouse monitors, GCD timer (240Hz / 4ms), layer setup |
| `osd.rs` | ~535 | CATextLayer OSD messages, NSAttributedString subtitles (cached shadow/paragraph style), clickable progress bar |
| `sync.rs` | ~100 | Audio-master clock (AtomicI64), pause/resume, seek position |
| `subtitle.rs` | ~265 | SRT parser, binary search lookup, auto-detection of .srt files |
| `input.rs` | ~165 | Virtual key code → Command mapping (Carbon key codes + characters) |
| `time.rs` | ~190 | PTS↔microseconds conversion (i128 overflow-safe), HH:MM:SS format/parse, now_ms() |
| `terminal.rs` | ~180 | Audio-only mode: termion raw terminal, metadata display, keyboard controls |
| `build.rs` | ~16 | Links Apple frameworks (AVFoundation, CoreMedia, CoreVideo, etc.) |

## Key Dependencies

- **ffmpeg-next 7** + **ffmpeg-sys-next 7** (vendored build, statically linked, VideoToolbox hwaccel)
- **objc2 0.6** ecosystem for Apple framework bindings (raw `msg_send!` with `AnyObject` for AVFoundation/CoreMedia APIs that lack typed bindings)
- **block2 0.6** for Objective-C blocks (key monitor, timer handler)
- **dispatch2 0.3** for GCD timer on main queue
- **crossbeam-channel 0.5** for inter-thread communication
- **termion 4** for terminal raw mode (audio-only mode), **anyhow** for errors

## A/V Sync

Audio-master: audio plays at device rate, video adjusts. The CoreAudio render callback
writes the current PTS to `Arc<AtomicI64>` as it consumes samples. The display layer's
CMTimebase handles video frame pacing — the player simply pushes frames via `try_send()`
and the layer presents them at the correct PTS. Every ~1s the main thread corrects
timebase drift vs audio clock (threshold: 5ms). If the video channel is full, frames
are implicitly dropped (try_send returns Full).

## Seek Coalescing

Holding an arrow key generates ~30 `SeekRelative` commands/sec. Without coalescing, each
would trigger a full ffmpeg seek + decode flush round-trip. Two layers prevent this:

1. **Player buffering** (`player.rs`): At most one seek is in flight to the demuxer. While
   `pending_seeks > 0`, new seeks accumulate into a `queued_seek` field instead of dispatching.
   A minimum display time (`SEEK_MIN_DISPLAY` = 4ms) ensures each seek-flush frame survives
   at least one VSync before the next flush clears it. A safety timeout (`SEEK_COALESCE_TIMEOUT`
   = 50ms) prevents stuck decodes from freezing scrubbing.

2. **Demuxer coalescing** (`demux.rs`): The demuxer drains all pending seeks from `cmd_rx`
   at the top of each loop iteration (keeps only the last). The packet send uses
   `crossbeam::select!` to race `send` vs `recv`, so a seek command is processed immediately
   even when the packet channel is full (stale packet is dropped). Seeks are served from the
   150MB in-memory packet cache when possible (instant replay, no file I/O).

## OSD Rendering

Two `CATextLayer` sublayers on the content view's layer, above the display layer:
- **Message** (bottom-left, 16pt): seek position, volume, audio delay. Fades after 2s.
- **Subtitle** (bottom-center, dynamic size): NSAttributedString with shadow outline. Shown/hidden by PTS lookup.
- **Progress bar** (bottom, 36pt): clickable track with fill + timestamps. Auto-hides after 2s. Seek-hold prevents snap-back during click-drag.

Subtitle font size scales with window height (`height * 22/720`). Shadow outline via
NSShadow attribute plus CATextLayer shadow for double-pass crispness. Frame and margins
recalculated on each update. `CATransaction` disables implicit animations.

## Gotchas

- `OpaqueCMSampleBuffer` needs a custom `RefEncode` impl (`Pointer → Struct`) to satisfy
  objc2's runtime type checking for `enqueueSampleBuffer:`.
- ffmpeg-sys-next `build-audiotoolbox` feature is broken on macOS (adds iOS flags). Omit it;
  audio output uses CoreAudio AudioUnit via C FFI instead.
- `NSWindow` is not `Send`/`Sync` — stored in a `thread_local! RefCell`. Display layer pointer
  wrapped in `SendPtr` newtype for `OnceLock`.
- `define_class!` in objc2 0.6 requires unit structs (no inline ivars). State goes in globals.
- Opus/AAC `ch_layout.nb_channels` must be read from the modern API — the legacy
  `channel_layout` bitmask is often unset, giving 0 channels.
- `SpscRing::clear()` is only safe after `AudioOutputUnitStop` returns (guarantees callback
  is not running). Always call `flush()` which stops first, then clears.
- `PixelBuffer` RAII owns a retained CVPixelBufferRef. Use `.take()` to transfer ownership
  to CoreMedia without triggering Drop. Forgetting to take leaks the buffer.
- Audio track switching (`CycleAudioTrack`) re-opens the file with `format::input` to get
  new stream parameters. The `Input` context is dropped immediately after decoder creation.
