Prefix: PLAY
Next: 55

# Backlog

Priorities are assigned by impact and likelihood. Source-confirmed paths are distinguished from hardware checks and historical measurements that remain unproven on the current build.

## P1

## P2

### PLAY-47 · P2 · 10-bit decoded video fails before reaching the renderer

- **Area:** Types
- **Where:** `crates/player-engine/src/video.rs:287`, `crates/player-engine/src/video.rs:340`, `crates/player-output/src/frame.rs:89`, `crates/player-output/src/frame.rs:150`, `tests/diff_mpv.py:143`.
- **Problem:** The normal presentation path accepts only the listed 8-bit formats; a decoded yuv420p10le frame returns unsupported pixel format. No conversion runs on that path. Both renderer upload paths also request 8-bit textures. This affects ordinary H.264 High 10, HEVC Main 10, and VP9 profile 2 inputs even on an SDR display. The mpv-diff gate uses --vo=null, which returns before format validation, so its 10-bit cases cannot establish display support.
- **Recommendation:** Carry storage depth, meaningful component depth, packing, and stride through the frame/upload boundary, with an explicit policy for unsupported layouts. Prefer preserving 10-bit precision into libplacebo; verify an actual rendered High 10/Main 10 gradient and the screenshot path. Keep this separate from Metal HDR output in PLAY-33.
- **Fix with:** `api-surface-and-module-boundaries`
- **Effort:** M (days)
- **Design:** [Draft RFC](docs/rfc/PLAY-47-video-pixel-depth.md)

### PLAY-45 · P2 · Playback speed changes the clock but leaves audio at normal speed

- **Area:** Concurrency
- **Where:** `crates/player-engine/src/sched.rs:440`, `crates/player-engine/src/backend.rs:326`, `crates/player-audio/src/decode.rs:59`, `crates/player-audio/src/sink.rs:7`.
- **Problem:** SetSpeed changes only the media clock. Audio decoding still resamples at the fixed source/device ratio and the sink receives unchanged sample counts at its original rate; no audio speed or tempo path exists. At 2x the clock and video advance faster while audio still takes its normal duration, breaking A/V synchronization and audio-only speed controls. The existing clock simulation does not prove audible playback rate.
- **Recommendation:** Apply speed in the PCM processing path and define how media time maps to samples consumed by the device. Re-anchor speed transitions and handle already-queued samples without claiming a new rate before audio follows it. Verify duration and A/V alignment at 0.5x, 1x, and 2x, including paused and midstream changes. Resolve pitch policy in the linked draft.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** L (weeks)
- **Design:** [Draft RFC](docs/rfc/PLAY-45-audio-speed.md)

### PLAY-46 · P2 · Audio output collapses timestamp offsets and gaps

- **Area:** Concurrency
- **Where:** `crates/player-engine/src/sched.rs:615`, `crates/player-engine/src/sched.rs:701`, `crates/player-engine/src/sched.rs:855`, `crates/player-audio/src/decode.rs:95`, `crates/player-engine/src/backend.rs:326`.
- **Problem:** Decoded chunks retain the first packet's pts, but the decoder coalesces subsequent packets without preserving discontinuities. feed_output then queues chunks solely by the output watermark and EngineOutput::push writes only samples. Startup rebasing from the first video frame does not delay the audio. A track whose first packet starts one second after video is queued as soon as decoded, and later gaps collapse into contiguous PCM. An isolated Matroska fixture probe confirmed video start 0 and audio start 1; the player behavior is established by the scheduling path, not a live A/V run.
- **Recommendation:** Preserve timestamp discontinuities before coalescing decoded packets, then align those spans to the shared media timeline before sink writes: insert bounded silence for positive gaps, handle overlap explicitly, and reset alignment on seek and track changes. Keep device time and media time distinct, including non-unit speed. Add delayed-start and midstream-gap cases that assert the output sample timeline, followed by an audible A/V check.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M (days)

### PLAY-48 · P2 · Presentation ignores sample aspect ratio and display rotation

- **Area:** Types
- **Where:** `crates/player-engine/src/video.rs:20`, `crates/player-output/src/frame.rs:286`, `crates/player-output/src/frame.rs:376`, `crates/player-app/src/properties.rs:758`.
- **Problem:** The presentation arguments passed to the output carry coded width and height but no sample aspect ratio or display transform. Rendering hard-codes square pixels and zero rotation, fit_crop uses coded dimensions, and video-params reports par=1 and rotate=0. Anamorphic video is displayed with the wrong shape, and rotation metadata is ignored. Headless decode-to-end coverage cannot detect either. The existing rotation fixture recipe must be checked with ffprobe before relying on it; an encode-time rotate tag did not produce a display matrix in a disposable system-FFmpeg probe.
- **Recommendation:** Read frame/stream aspect ratio and display-matrix metadata, carry it in the presentation description, and apply it consistently to fitting, libplacebo rendering, screenshots, and reported properties. Verify an anamorphic image and a known-valid remuxed 90-degree display-matrix fixture, checking metadata as well as rendered geometry.
- **Fix with:** `api-surface-and-module-boundaries`
- **Effort:** M (days)

### PLAY-23 · P2 · HE-AAC is rejected or decoded without SBR and parametric stereo

- **Area:** Dependencies
- **Where:** `vendor/symphonia/symphonia-codec-aac/src/aac/mod.rs:152`, `vendor/symphonia/symphonia-codec-aac/src/aac/mod.rs:243`, `vendor/symphonia/symphonia-common/src/mpeg/audio/mod.rs:246`.
- **Problem:** Explicit SBR/PS in AudioSpecificConfig sets sbr_present and is rejected by the decoder's complexity guard. In-band SBR extensions are recognized but their payloads are skipped, leaving only the AAC-LC core and no parametric-stereo reconstruction. The earlier ticket's claim that every HE-AAC file plays as its core was too broad. Play-to-end alone does not establish full-bandwidth or stereo fidelity.
- **Recommendation:** Add SBR/PS support to the audio decode path; report unsupported signaling accurately until supported. Verify both explicit and implicit signaling with reference PCM, output rate, high-frequency energy, and stereo separation, rather than completion alone.
- **Fix with:** `none`
- **Effort:** L (weeks)

### PLAY-25 · P2 · GUI audio has no embedded-cover presentation path

- **Area:** Architecture
- **Where:** `crates/player-engine/src/backend.rs:40`, `crates/player-engine/src/backend.rs:711`, `crates/player-app/src/app.rs:305`, `crates/player-app/src/app.rs:860`.
- **Problem:** A fresh --gui or bundled window opens for cover-art audio, but EngineDemux excludes attached-picture streams and selects passive video, so no cover frame reaches last_frame. The window has no image to render. Switching from video can instead retain its stale last_frame because unload does not clear it. The TUI decodes cover art independently; the existing cover-art gate checks the no-window audio path.
- **Recommendation:** Provide a one-frame decode/presentation path for attached pictures in GUI audio sessions and clear the previous file's frame when changing content. Verify fresh GUI playback and a video-to-cover-art transition, preserving ordinary audio-only startup and the separate TUI path.
- **Fix with:** `api-surface-and-module-boundaries`
- **Effort:** M (days)

### PLAY-20 · P2 · The playback scheduler never requests hardware video decoding

- **Area:** Performance
- **Where:** `crates/player-engine/src/video.rs:65`, `crates/player-media/src/video.rs:485`.
- **Problem:** EngineVideo::open always passes HwRequest::None, so playback remains software-decoded even though player-media has VideoToolbox and VA-API paths. Accepting --hwdec does not wire those paths into the scheduler or establish which decoder opened.
- **Recommendation:** Thread the validated hardware-decode request through engine startup and decoder selection, retain explicit software fallback, and derive hwdec-current from the actual opened decoder. Verify software behavior without a GPU and VideoToolbox/VA-API on suitable hardware; do not infer success from accepting an option.
- **Fix with:** `api-surface-and-module-boundaries`
- **Effort:** M (days)

### PLAY-32 · P2 · VideoToolbox frames still require a CPU transfer before Metal

- **Area:** Performance
- **Where:** `crates/player-engine/src/video.rs:344`, `scripts/patches/libplacebo-metal.patch:2261`.
- **Problem:** After PLAY-20 enables VideoToolbox, the current presentation boundary would transfer every hardware frame to software and upload its planes again. The carried Metal patch already exposes pl_metal_wrap_iosurface, but no player path uses it. This is dependent follow-on work, not a measured regression in today's software decoder.
- **Recommendation:** After PLAY-20, carry owned CVPixelBuffer/IOSurface lifetime through the frame handoff and import it into the window's existing Metal device. Keep a software fallback for unsupported formats and verify ownership through presentation, seek, and shutdown. Compare equivalent hardware-decoded workloads before claiming a performance gain.
- **Fix with:** `make-it-fast`
- **Effort:** M (days)

### PLAY-21 · P2 · Terminal-launched macOS keyboard focus still needs owner verification

- **Area:** Operability
- **Where:** `crates/player-display/bridge/metal_window.m:68`, `crates/player-display/bridge/metal_window.m:380`.
- **Problem:** Unproven on the current build: the previously reported terminal-launch focus failure has not been checked in a real owner session after the AppKit main-loop change. The bridge now dispatches window creation to the main thread, but that source fact does not establish foreground activation. An agent session typing into another app cannot reproduce the intended launch interaction reliably.
- **Recommendation:** On an Apple Silicon Mac, launch build/mpv FILE from the terminal and verify immediate keyboard control under both Metal and Vulkan. Record the result; if it fails, trace activation/focus at launch before changing policy. Delete this ticket if the current behavior passes.
- **Fix with:** `os-boundaries`
- **Effort:** S (hours)

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
