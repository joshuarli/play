# `play` — Minimal macOS Media Player in Rust

## Context

A clean-room Rust media player for Apple Silicon macOS, inspired by mpv but drastically simpler. Plays local mp4 files (H.264/H.265 video, AAC/ALAC/FLAC/Opus audio) with SRT subtitles. Uses Apple's VideoToolbox for hardware decoding, AVSampleBufferDisplayLayer for zero-copy display, and AVAudioEngine for audio output. Keyboard-driven, no mouse interaction, minimal text OSD.

Project location: `~/dev/play` (overwrite existing)

---

## Decisions Summary

| Area | Choice |
|---|---|
| Demux/Decode | ffmpeg-next + ffmpeg-sys-next vendored build (statically linked) |
| Video decode | VideoToolbox hwaccel via ffmpeg |
| Video display | AVSampleBufferDisplayLayer (zero-copy CVPixelBuffer → screen) |
| Audio output | AVAudioEngine (passthrough to system, handles surround auto-downmix) |
| Windowing | Raw AppKit via objc2 (NSWindow/NSView) |
| OSD | Minimal text, bottom-left, white+black outline, no background, fades after 2s |
| Subtitles | SRT only, Core Text rendering, white text with black outline |
| A/V sync | Audio-master (audio drives clock, video adjusts) |
| Config | CLI flags only, no config file |
| Error handling | anyhow only |
| Rust edition | 2024, latest stable |
| HDR | Pass through to macOS (auto tone-mapping) |
| Surround | Pass through to system (auto downmix if stereo output) |

---

## CLI Interface

```
play [OPTIONS] <FILES>...

Arguments:
  <FILES>...           One or more mp4 file paths

Options:
  --volume <0-100>     Initial volume percentage [default: 100]
  --audio-delay <SEC>  Audio delay in seconds (float, can be negative) [default: 0.0]
  --audio-track <N>    Audio track index (1-based) [default: 1]
  --sub-file <PATH>    External SRT subtitle file
  --start <TIME>       Start position (HH:MM:SS, MM:SS, or seconds)
  --fullscreen         Start in fullscreen
  -v                   Verbose logging (stream info on open)
  -vv                  Debug logging (sync decisions, packet timing)
```

---

## Keyboard Controls

| Key | Action |
|---|---|
| Space | Play/pause (no OSD) |
| Left/Right | Seek ±5s (keyframe) |
| Shift+Left/Right | Seek ±1s (exact) |
| Shift+Up/Down | Seek ±60s (keyframe) |
| Up/Down | Volume ±5% |
| `a` | Cycle audio track |
| `s` | Cycle subtitles: embedded → external .srt → off → repeat |
| `+`/`-` | Audio delay ±100ms |
| `>`/Enter | Next file in playlist |
| `<`/Backspace | Previous file in playlist |
| `f` | Toggle fullscreen |
| `q` | Quit |

Seeking works while paused (frame-step: shows new frame, stays paused).

---

## OSD Behavior

- **Position**: Bottom-left corner, white text with black outline (no background box)
- **Font**: System sans-serif, ~2.5% of video height
- **Duration**: Fades out after 2 seconds
- **Triggers**:
  - Seek → `"00:12:34 / 01:45:00"` (shows immediately, before seek completes)
  - Volume → `"Volume: 75%"`
  - Audio track → `"Audio: 2/3 - English (AAC 5.1)"`
  - Audio delay → `"Audio delay: +100ms"`
  - Subtitle → `"Subtitles: English"` or `"Subtitles: off"`
  - Pause → **no OSD** (frozen video is feedback enough)

---

## Subtitle Behavior

- **SRT only** (hand-rolled parser, ~30 lines)
- **Auto-detection**: Look for `video.srt`, `video.*.srt` in same directory as video
- **Embedded tracks**: Also detect subtitle streams in mp4 container
- **Cycle order** (`s` key): embedded tracks → external .srt → off → repeat
- **--sub-file** flag overrides auto-detection (adds to track list)
- **Rendering**: Core Text → CGImage, white text with 2px black outline, positioned bottom-center
- **Size**: ~4% of video height, scales with window resize

---

## Window Behavior

- **Title**: `"play"`
- **Size**: Auto-fit to video resolution, capped to 80% of screen
- **Resize**: Aspect ratio preserved (letterbox/pillarbox), handled by AVSampleBufferDisplayLayer's `ResizeAspect`
- **Fullscreen**: Native macOS fullscreen via `f` key (no Cmd+Q, just `q` to quit)
- **Audio-only files**: No window created. Terminal-only mode with mpv-style status line
- **No Cmd+Q**: Only `q` key quits

---

## Audio-Only Terminal Mode

When file has no video stream, skip window creation entirely. Use raw terminal (crossterm):
- On open: print file info (codec, duration, channels, sample rate)
- Status line: `"▶ 00:03:21 / 00:05:45  filename.m4a"` (overwritten with `\r`)
- Same keyboard controls (space, arrows, q, a, +/-, etc.) via crossterm raw mode
- No NSApplication run loop

---

## File Error Behavior

- Unsupported codec, corrupt file, missing file → print error to stderr, exit immediately
- No graceful skip even with multiple playlist files

---

## Architecture

### Threading Model (4 threads)

```
┌─────────────────────────────────────────────────────┐
│ Main Thread (AppKit)                                │
│ - NSApplication.run() event loop                    │
│ - Owns: NSWindow, NSView, display layer, OSD layers │
│ - Receives: VideoFrame, UiUpdate from Player        │
│ - Sends: Command (key events) to Player             │
└──────────────────┬──────────────────────────────────┘
                   │ Command channel
                   ▼
┌─────────────────────────────────────────────────────┐
│ Player Thread (orchestrator)                        │
│ - Owns: Player state, both decoders, sync clock     │
│ - Runs: crossbeam select! on cmd_rx + demux_rx      │
│ - Decodes video (VideoToolbox, near-instant)        │
│ - Decodes audio (CPU, fast for supported codecs)    │
│ - Schedules audio buffers on AVAudioEngine          │
│ - Sends video frames + UI updates to Main Thread    │
└──────────────────┬──────────────────────────────────┘
                   │ DemuxCommand channel
                   ▼
┌─────────────────────────────────────────────────────┐
│ Demuxer Thread (read-ahead)                         │
│ - Owns: ffmpeg format::context::Input               │
│ - Blocking av_read_frame loop                       │
│ - Sends: DemuxPacket to Player (bounded chan, 64)   │
│ - Receives: seek/flush/stop from Player             │
└─────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────┐
│ Audio Render Thread (implicit, AVAudioEngine)       │
│ - Pulls PCM from scheduled buffers                  │
│ - Reports playback PTS via Arc<AtomicI64>           │
└─────────────────────────────────────────────────────┘
```

### Channel Layout

```rust
// User input → Player
cmd_tx/rx: crossbeam::bounded<Command>(32)

// Demuxer → Player
demux_packet_tx/rx: crossbeam::bounded<DemuxPacket>(64)

// Player → Demuxer
demux_cmd_tx/rx: crossbeam::bounded<DemuxCommand>(4)

// Player → Main thread (video frames)
video_frame_tx/rx: crossbeam::bounded<VideoFrame>(2)

// Player → Main thread (OSD/subtitle/sync updates)
ui_update_tx/rx: crossbeam::unbounded<UiUpdate>()

// Audio clock (lock-free)
audio_clock: Arc<AtomicI64>  // current PTS in microseconds
```

### Data Flow

```
File → [Demuxer] → DemuxPacket → [Player] ─┬─ Video Packet
                                             │   → VideoDecoder (VideoToolbox GPU)
                                             │   → CVPixelBuffer
                                             │   → SyncClock decision (display/drop/wait)
                                             │   → VideoFrame → [Main Thread]
                                             │     → CMSampleBuffer
                                             │     → AVSampleBufferDisplayLayer.enqueue()
                                             │
                                             ├─ Audio Packet
                                             │   → AudioDecoder (ffmpeg CPU)
                                             │   → f32 PCM (resampled if needed)
                                             │   → AVAudioPCMBuffer
                                             │   → AVAudioPlayerNode.scheduleBuffer()
                                             │   → [Audio Render Thread] → speakers
                                             │   → AtomicI64 audio_clock update
                                             │
                                             └─ Periodic tick (every ~16ms)
                                                 → SubtitleTrack.text_at(media_time)
                                                 → CoreText render if changed → UiUpdate
                                                 → Sync video timebase to audio clock
```

### A/V Sync

- **Audio-master**: Audio plays at device's native rate, video adjusts
- **AVSampleBufferDisplayLayer** has a `controlTimebase` (CMTimebase) synced to the audio clock
- Player periodically reads `audio_clock` AtomicI64 and updates the CMTimebase
- Frames enqueued with correct PTS; the layer handles display timing automatically
- Player drops frames that are >50ms behind audio clock (don't even enqueue them)
- Player waits (select! with timeout) for frames >50ms ahead

### Seek Implementation

1. Compute target PTS from current position + delta
2. Send `DemuxCommand::Seek` to demuxer thread
3. Flush video decoder, flush audio (stop player node, clear buffers)
4. OSD immediately shows target timestamp
5. Demuxer seeks to keyframe, resumes sending packets
6. For exact seek: decode+discard frames until target PTS reached
7. Resume normal playback from new position

### Pause

- Set `audio_clock` to paused state (stop updating)
- `AVAudioPlayerNode.pause()`
- `CMTimebaseSetRate(timebase, 0.0)` to freeze video layer
- Seeking while paused: decode to target frame, display it, stay paused

---

## Module Structure

```
play/
  Cargo.toml
  build.rs                 # Link Apple frameworks
  src/
    main.rs                # CLI parsing (clap), app bootstrap
    player.rs              # Player struct, select! loop, state machine
    demux.rs               # Demuxer thread, packet reading
    decode_video.rs        # VideoToolbox hwaccel setup, CVPixelBuffer extraction
    decode_audio.rs        # Audio decode + resample to f32
    video_out.rs           # AVSampleBufferDisplayLayer, CMSampleBuffer creation
    audio_out.rs           # AVAudioEngine, buffer scheduling, clock reporting
    sync.rs                # Audio-master clock, video timing decisions
    subtitle.rs            # SRT parser + Core Text renderer
    osd.rs                 # CATextLayer transient messages
    window.rs              # NSWindow/NSView, key events, layer hierarchy
    input.rs               # Key code → Command mapping
    cmd.rs                 # Command enum, shared types
    time.rs                # PTS helpers, HH:MM:SS formatting/parsing
```

14 source files. Each has a single responsibility.

---

## Key Dependencies

```toml
[package]
name = "play"
version = "0.1.0"
edition = "2024"

[dependencies]
# FFmpeg (vendored, statically compiled during cargo build)
ffmpeg-next = "7"
ffmpeg-sys-next = { version = "7", features = [
    "build",
    "build-videotoolbox",
    "build-audiotoolbox",
] }

# Apple frameworks (objc2 ecosystem)
objc2 = "0.6"
objc2-foundation = "0.3"
objc2-app-kit = "0.3"
objc2-quartz-core = "0.3"
objc2-av-foundation = "0.3"
objc2-avf-audio = "0.3"
objc2-core-media = "0.3"
objc2-core-video = "0.3"
objc2-core-graphics = "0.3"
objc2-core-text = "0.3"
objc2-core-foundation = "0.3"
block2 = "0.6"
dispatch2 = "0.3"

# Concurrency
crossbeam-channel = "0.5"

# CLI
clap = { version = "4", features = ["derive"] }

# Error handling
anyhow = "1"

# Terminal (audio-only mode)
crossterm = "0.28"

# Logging
log = "0.4"
env_logger = "0.11"
```

Feature flags for the objc2-* crates will be refined during implementation (they use granular per-class features).

### Build Configuration

```rust
// build.rs — link Apple frameworks
fn main() {
    for fw in [
        "AVFoundation", "CoreMedia", "CoreVideo", "CoreGraphics",
        "CoreText", "QuartzCore", "AppKit", "VideoToolbox",
        "AudioToolbox", "AVFAudio", "CoreFoundation",
    ] {
        println!("cargo:rustc-link-lib=framework={fw}");
    }
}
```

FFmpeg is compiled from source by ffmpeg-sys-next during `cargo build`. First build takes ~5 minutes. Subsequent builds are cached.

---

## Implementation Phases

### Phase 1: Skeleton — Get a Window
1. `cargo init` at ~/dev/play
2. `main.rs` — clap CLI parsing, file validation
3. `cmd.rs` — Command enum
4. `window.rs` — NSWindow + layer-backed NSView on screen
5. `input.rs` — key events → print to console
6. **Milestone**: Window appears, key presses logged

### Phase 2: Video Playback (No Audio)
7. `demux.rs` — open file, spawn read thread, stream discovery
8. `decode_video.rs` — VideoToolbox hwaccel, CVPixelBuffer decode
9. `video_out.rs` — AVSampleBufferDisplayLayer, enqueue frames
10. `time.rs` — PTS conversion helpers
11. Wire demuxer → decoder → display on main thread
12. **Milestone**: Video plays (no audio, no sync, probably too fast)

### Phase 3: Audio
13. `decode_audio.rs` — decode to f32 PCM, resample if needed
14. `audio_out.rs` — AVAudioEngine setup, schedule buffers
15. **Milestone**: Audio plays alongside video (rough timing)

### Phase 4: Sync & Controls
16. `sync.rs` — audio-master clock, video timing decisions
17. `player.rs` — full state machine with select! loop
18. Seek, pause, volume, playlist next/prev
19. **Milestone**: Proper A/V sync, all keyboard controls work

### Phase 5: Subtitles & OSD
20. `subtitle.rs` — SRT parser + auto-detection + Core Text render
21. `osd.rs` — CATextLayer with fade-out timer
22. **Milestone**: Subtitles render, OSD shows on actions

### Phase 6: Polish
23. Audio-only terminal mode (crossterm)
24. Audio track cycling + audio delay adjustment
25. Fullscreen toggle
26. Seek-while-paused (frame step)
27. File info logging on open
28. Drain audio before exit
29. Edge cases: multi video stream (pick highest res), error handling

---

## Verification Plan

1. **Build**: `cargo build` succeeds (ffmpeg compiles, frameworks link)
2. **Window**: Run with an mp4 → window appears at correct size
3. **Video**: Frames display in correct order and speed
4. **Audio**: Audio plays in sync with video
5. **Seek**: Left/right arrows seek correctly, OSD shows timestamp
6. **Pause**: Space pauses/resumes, seeking while paused shows correct frame
7. **Subtitles**: External .srt file renders at correct times
8. **Audio tracks**: `a` key cycles tracks in multi-audio file
9. **Audio delay**: `+`/`-` adjusts delay, OSD confirms
10. **Playlist**: Multiple files play in sequence, `>`/`<` navigate
11. **Fullscreen**: `f` toggles native fullscreen
12. **Audio-only**: m4a file plays with terminal status, no window
13. **Error**: Invalid file → error message, exit code 1
14. **HDR**: HEVC HDR content displays correctly (macOS tone maps)
