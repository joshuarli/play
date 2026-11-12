Prefix: PLAY
Next: 55

# Backlog

Priorities are assigned by impact and likelihood. Source-confirmed paths are distinguished from hardware checks and historical measurements that remain unproven on the current build.

## P2

### PLAY-32 · P2 · Complete the direct VideoToolbox presentation comparison

- **Area:** Performance
- **Where:** `crates/player-output/src/context.rs`, `crates/player-output/src/iosurface.rs`, `crates/player-display/src/mac.rs`, `scripts/patches/libplacebo-metal.patch`.
- **Implemented:** Metal imports supported VideoToolbox NV12 and P010 IOSurfaces directly. Retained surfaces and use counts survive GPU execution in a four-entry lease queue. Shutdown drains leases. Vulkan, unsupported imports, and explicit `videotoolbox-copy` use software transfer. Import fallback renders into the acquired drawable before its single submission.
- **Verified:** Native NV12 and P010 pixel comparisons pass through decoder teardown, repeated buffer reuse, seek, GPU shutdown, explicit copy conversion, and same-target fallback. The full macOS Rust workspace and affected Linux packages pass.
- **Remaining:** The interleaved windowed CPU/RSS comparison was stopped when the owner asked to use the computer. No performance gain is claimed. Keep this ticket open until a qualified comparison is accepted.
- **Resume:** Run windowed tests only when the owner permits them. The preserved baseline is `.work/tmp/iosurface-baseline/Player.app/Contents/MacOS/player`; use identical `--hwdec=videotoolbox --gpu-api=metal --ao=null` settings for baseline and candidate. The existing frame harness has prepared `video-1080p` and `video-4k` fixtures. Compare at least five interleaved runs per side, including drops and RSS.
- **Fix with:** `make-it-fast`
- **Effort:** S (hours)

### PLAY-21 · P2 · Terminal-launched macOS keyboard focus still needs owner verification

- **Area:** Operability
- **Where:** `crates/player-display/bridge/metal_window.m:68`, `crates/player-display/bridge/metal_window.m:380`.
- **Problem:** Unproven on the current build: the previously reported terminal-launch focus failure has not been checked in a real owner session after the AppKit main-loop change. The bridge now dispatches window creation to the main thread, but that source fact does not establish foreground activation. An agent session typing into another app cannot reproduce the intended launch interaction reliably.
- **Recommendation:** On an Apple Silicon Mac, launch build/mpv FILE from the terminal and verify immediate keyboard control under both Metal and Vulkan. Record the result; if it fails, trace activation/focus at launch before changing policy. Delete this ticket if the current behavior passes.
- **Fix with:** `os-boundaries`
- **Effort:** S (hours)
- **Status:** The owner could not run the interactive check during the 2026-11-12 fix session. No focus-policy change was made without a reproduction.

## P3

### PLAY-51 · P3 · CoreAudio buffer-allocation failure disposes the queue twice

- **Area:** Concurrency
- **Where:** `crates/player-audio/src/backend_coreaudio.rs:198`, `crates/player-audio/src/backend_coreaudio.rs:213`, `crates/player-audio/src/backend_coreaudio.rs:425`.
- **Problem:** Once CoreAudioSink owns the queue, an AudioQueueAllocateBuffer failure manually calls AudioQueueDispose and returns. Dropping the already-constructed sink calls stop_streaming and AudioQueueDispose on the same handle again. The duplicate cleanup is source-confirmed, but the allocation-failure path and its native outcome were not exercised on macOS.
- **Recommendation:** Transfer cleanup responsibility exactly once when constructing the sink and let its owner handle subsequent failures. Add an injected allocation failure after ownership transfer and assert one disposal, covering failure after an earlier buffer allocation as well.
- **Fix with:** `os-boundaries`
- **Effort:** S (hours)

### PLAY-53 · P3 · Large Finder open batches silently lose files

- **Area:** Concurrency
- **Where:** `crates/player-display/bridge/metal_window.m:44`, `crates/player-display/bridge/metal_window.m:57`, `crates/player-display/bridge/metal_window.m:108`, `crates/player-display/bridge/metal_window.m:373`.
- **Problem:** The 64-slot input ring holds only 63 events and silently drops the next event when full. Early Finder opens accumulate in an array, then window creation enqueues the entire batch while the playback caller is blocked in dispatch_sync and cannot drain it. A batch of more than 63 valid local files therefore loses entries, while the Finder delegate reports success. This is deterministic from the queue/control flow; Finder was not exercised here.
- **Recommendation:** Deliver file-open batches through a representation that preserves the complete batch, or surface an explicit failure before claiming success. Keep any coalescing limited to input for which loss is acceptable. Verify a startup batch larger than the ring and repeated batches while ordinary input is pending.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M (days)

### PLAY-52 · P3 · Seek preroll reaches EOF without finishing the video decoder

- **Area:** Concurrency
- **Where:** `crates/player-engine/src/sched.rs:559`, `crates/player-engine/src/sched.rs:810`, `crates/player-engine/src/sched.rs:945`, `crates/player-engine/src/video.rs:183`.
- **Problem:** Normal demux EOF calls video.finish, which signals end-of-input to the decoder. EOF inside command_seek only sets eof_seen, and the subsequent draining path calls video.drain, which only receives available frames. A decoder holding delayed frames can therefore miss its tail or leave a paused seek on an old frame. Unproven with a real delayed-frame file in this environment; the inconsistent EOF control flow is confirmed.
- **Recommendation:** Share an explicit, once-only decoder finish transition between normal playback EOF and seek preroll EOF, then drain its frames before declaring the seek complete. First reproduce with the existing delayed-decoder simulation and a short B-frame fixture near EOF, both paused and playing; avoid broad scheduler changes if the real trigger is refuted.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** S (hours)

### PLAY-54 · P3 · GUI letter bindings use US physical key positions

- **Area:** Operability
- **Where:** `crates/player-display/src/display.rs:203`, `crates/player-app/src/window.rs:103`, `crates/player-app/src/window.rs:157`, `crates/player-app/src/window.rs:177`.
- **Problem:** Wayland discards the keymap and forwards keycodes; macOS drops the key event's character/modifier information. Both gui_key_name tables map fixed physical positions to US letters. A non-US layout therefore does not follow the displayed letter bindings; on AZERTY the US-Q position is labeled A but still invokes quit. TUI text input follows a different path.
- **Recommendation:** Translate printable keys through the active platform layout while retaining explicit handling for navigation and other non-text keys. On Wayland retain and interpret the owned keymap; on macOS use the event's translated character information. Verify quit, mute, and OSC controls with US and a non-US layout, including modifier behavior.
- **Fix with:** `os-boundaries`
- **Effort:** M (days)

### PLAY-41 · P3 · Fixture generation validates the FFmpeg version but not its encoders

- **Area:** Tests
- **Where:** `tests/fixtures/gen.sh:12`, `tests/fixtures/gen.sh:21`, `tests/fixtures/gen.sh:38`.
- **Problem:** Any binary reporting ffmpeg version 9.0.2 passes the preflight, including builds without the corpus's encoders. The generator then removes the previous fixture directory and fails later on options such as -preset, leaving an incomplete corpus. The earlier claim that every later gate necessarily times out was not established and is removed.
- **Recommendation:** Check the exact encoders and required generator capabilities before deleting or replacing the fixture directory, with an actionable message naming what is missing. Generate into a temporary directory and publish only a complete validated corpus. Exercise a same-version encoder-deficient stub and assert that existing fixtures survive.
- **Fix with:** `error-model`
- **Effort:** S (hours)

### PLAY-40 · P3 · The hover benchmark does not establish its intended pointer rate

- **Area:** Performance
- **Where:** `scripts/bench.py:628`, `crates/player-app/src/app.rs:705`, `crates/player-app/src/app.rs:877`.
- **Problem:** The recorded hover run achieved about 6 Hz rather than the intended 60 Hz, with roughly 150 ms per synchronous IPC mouse command. The harness waits for each reply before its 1/60-second sleep; its JSON already reports the achieved pointer_rate_hz. The current bottleneck is unproven: a blocking present and harness waiting are candidates, not established causes. Historical CPU comparisons need this workload qualification.
- **Recommendation:** Measure command round-trip time on a quiet qualified host and use the existing pointer_rate_hz result to establish the workload. Trace the delay through IPC polling and present before changing either the app or harness, and retain the achieved rate in performance comparisons. If app latency is confirmed, fix it separately from benchmark pacing and compare equivalent input workloads.
- **Fix with:** `make-it-fast`
- **Effort:** M (days)

### PLAY-33 · P3 · Metal swapchain output is limited to 8-bit SDR

- **Area:** Architecture
- **Where:** `scripts/patches/libplacebo-metal.patch:2308`.
- **Problem:** The carried Metal swapchain fixes CAMetalLayer to BGRA8Unorm and does not establish an EDR/HDR output contract. Supporting a 10-bit input alone (PLAY-47) does not provide wide-precision or HDR display output.
- **Recommendation:** Define and implement the layer pixel format, extended-range content setting, EDR color space, and matching libplacebo swapchain color metadata together. Preserve an explicit SDR fallback and verify tone mapping and output range on a suitable display before claiming HDR support.
- **Fix with:** `api-surface-and-module-boundaries`
- **Effort:** L (weeks)

### PLAY-30 · P3 · Windowed OSC input checks lack an agreed coordinate contract

- **Area:** Tests
- **Where:** `crates/player-display/bridge/metal_window.m:237`, `crates/player-app/src/app.rs:811`, `crates/player-app/src/app.rs:1430`, `tests/osc_input.py:61`.
- **Problem:** The earlier Retina run reported osc-areas in 2560x1440 backing pixels where the gate expected a 1280x720 layout, plus IPC pointer motion that did not reveal or hide the OSC. The current bridge converts native motion to backing pixels and real/injected input reach the app handlers; the current windowed failure remains unproven on hardware. Changing API units without a contract could break clients while merely hiding a test mismatch.
- **Recommendation:** Resolve the coordinate and injected-input contract in the linked draft, then reproduce on a real Retina window for both backends. Derive expected hit regions from the actual output size and separately test motion-driven reveal/hide; keep headless coverage and verify 1x as well as 2x scaling.
- **Fix with:** `test-audit`
- **Effort:** M (days)
- **Design:** [Draft RFC](docs/rfc/PLAY-30-osc-coordinates.md)

### PLAY-26 · P3 · SubRip formatting is discarded instead of rendered

- **Area:** Types
- **Where:** `crates/player-engine/src/sub.rs:208`, `crates/player-overlay/src/text.rs:35`.
- **Problem:** markup_free strips SubRip italic, bold, underline, strike, and font-color tags. TextBlock carries one style for the whole block, so the native text backends cannot preserve per-span formatting. The current limitation is deliberate in the parser but loses presentation information from supported subtitle files.
- **Recommendation:** Parse the supported SubRip tags into typed text spans and carry attributes through Core Text ranges and HarfBuzz runs, retaining plain-text fallback for malformed markup. Verify mixed styles and color within one line without broadening the SubRip-only format boundary.
- **Fix with:** `typing-audit`
- **Effort:** M (days)

### PLAY-27 · P3 · Start alignment does not follow a paragraph's base direction

- **Area:** Types
- **Where:** `crates/player-overlay/src/display.rs:74`, `crates/player-overlay/src/text/mac.rs:328`, `crates/player-overlay/src/text/linux.rs:1071`.
- **Problem:** Align::Start always resolves to zero offset and both backends position lines with that result, so an RTL paragraph starts at the left edge. Centered subtitles avoid the issue, but start-aligned OSD messages and OSC titles do not.
- **Recommendation:** Resolve logical Start/End against each paragraph's base direction before placing its lines, consistently on both text backends. Verify mixed LTR/RTL paragraphs, wrapped lines, and the unchanged centered-subtitle path.
- **Fix with:** `make-invalid-states-unrepresentable`
- **Effort:** M (days)

### PLAY-28 · P3 · One tall fallback glyph changes spacing throughout a text block

- **Area:** Types
- **Where:** `crates/player-overlay/src/text/mac.rs:235`, `crates/player-overlay/src/text/mac.rs:286`, `crates/player-overlay/src/text/linux.rs:1009`, `crates/player-overlay/src/text/linux.rs:1062`.
- **Problem:** Both text backends compute one block-wide maximum ascent/descent across fallback fonts, then use it as every line's spacing. One tall CJK or Thai fallback glyph therefore spreads otherwise unchanged lines apart; the size depends on the system fallback fonts.
- **Recommendation:** Base line spacing on the primary font's metrics while allocating enough ink bounds/inset for tall fallback glyphs, so layout stays stable without clipping. Verify a multi-line block with a single tall fallback glyph on each backend and preserve wrapping and outline bounds.
- **Fix with:** `typing-audit`
- **Effort:** M (days)
