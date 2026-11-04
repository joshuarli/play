# Rust Port: Migration Charter

Reference commit: `ee5021c` (tree clean except staged campaign doc at preflight).
Preflight doc: `campaign-rust.md`. This file is the durable resume point.

## Current phase

S5 complete. S6 complete. S7a complete (clipboard removal, suite 337).
S7b complete (Rust engine Lua adapter, property and event pump,
command surface, headless osc.lua). S7c complete (libass wrapper,
subtitle pipeline, OSD composition, pixel-diff gate). S7d complete
(TUI on the Rust engine). S7e complete (Rust GUI host, OSC overlay,
paused-seek subtitle priming). S8a complete (Rust `player`
application core: CLI, IPC, playlists, chapters, track switching,
properties and events, OSC dispatch, traces). Next: S8b (GUI and
TUI modes in the Rust `player` binary, native integration), then S9
(legacy removal, Cargo-authoritative release).

## S8a record

`rust/player-app` builds the `player` executable used only through
`PLAYER_BIN`; the C host remains the default. The app owns argument
parsing, local-only playlist expansion, the Unix-socket JSON IPC
server, property and observer delivery, command dispatch, chapters,
playlist operations, OSC lifecycle, and engine session reloads.

- Engine prerequisites: `SchedulerEngine` now exposes speed, real
  audio-track switching (decoder reconfiguration plus output flush),
  video-only sessions, and reference-shaped track and media metadata.
  The audio sink opens with the decoder's target layout, so 5.1 PCM
  downmixes instead of failing the whole-frame check.
- Command path: `player-core::command` parses `set`, `cycle`, `seek`,
  `add`, `loadfile`, and track commands for keyboard, IPC, and
  `input.conf`; bindings run atomically (a failing command in a
  multi-command binding runs none of them).
- OSC: the vendored `osc.lua` runs headless. `input-bindings` merges
  script sections, `mouse`/`script-binding`/`script-message-to`
  dispatch through `client-message`, and `user-data/osc/*` reads the
  engine backend. OSD composition stays off for null output so libass
  does not emit errors into headless logs.
- Events: `start-file`, `file-loaded`, `end-file`, `seek`,
  `playback-restart`, `idle`, and stream `audio-reconfig` /
  `video-reconfig` follow the reference order; lifecycle events are
  deferred one tick so command replies precede them. Traces match
  34/34 goldens on macOS.
- Verification (macOS): `cli` 23, `input` 9, `playlist` 6, `state` 13,
  `play` 8, `seek` 5, `corpus` 109, `osc` 7, `osc-input` 9,
  `dispatch` 13, `failures` 8, `traces` 2, all green. The same named
  gates run in the Alpine container against `rust/target/debug/player`.
- Build: Cargo profiles match the `ish` workspace (`dev` line tables,
  dependency opt-level 2, thin-LTO release, fat-LTO `dist`); release
  and Linux artifact scripts build `dist`. The Linux image reads the
  Rust channel from `rust-toolchain.toml` and builds with the repo
  root as context.

## S7e record

`player-gui` is a Rust GUI host built on the same `SchedulerEngine`,
`EngineBackend`, and vendored `osc.lua` as the TUI and the OSD gate.
It opens `MacWindow` (AppKit + CAMetalLayer) or the Wayland `Window`,
presents decoded RGBA frames through libplacebo, and renders OSD,
subtitles, and OSC into a transparent window-sized overlay instead of
blending them into the video buffer. The overlay keeps OSD text at
window pixel size while libplacebo scales the video independently.
`EngineBackend::set_overlay_mode` disables per-frame compositing for
this path; `render_overlay` blends libass coverage into straight-alpha
RGBA. The sRGB overlay path was covered by a new libplacebo test that
blends a half-alpha rectangle against a solid base.

Input is native per platform: AppKit key and tracking-area events queue
through `player-display`'s bridge, Wayland keyboard and pointer events
arrive through the existing display pump. Both feed one action table
(space, q/escape, f, arrows, up/down, m, o, j). The macOS menu closes
the window cleanly, and the OSC is shown on pointer motion and hidden
on pointer leave. `player-gui --gate` plays the h264 fixture, keeps the
OSC visible, requires the frame to reach the end plus 24 presented
frames and real OSC ink, then exits. It is part of `linux-gpu-next`
(41 checks) and runs standalone on macOS; both pass.

Video startup needed two scheduler fixes. Frame-threaded software
decode held the first frame for one frame per decode thread, so the
audio clock advanced past it and every frame was dropped as late. The
engine now selects slice threading, which decodes in parallel without
that delay, and prerolls video: audio chunks buffer until the first
decoded frame, then playback presents that frame and rebases the clock
before releasing the buffered audio. The drain path switches a
finished audio clock to system time so the video tail plays instead of
waiting forever. `plays_video_through_renderer` now requires more than
24 delivered frames, which fails on the old behavior.

The paused-seek gap is closed. A seek while paused primes the selected
embedded subtitle stream by scanning from 30 seconds before the target,
feeds cues up to the target, then re-seeks for audio/video preroll. A
paused seek also presents the frame at the target instead of leaving
the previous frame on screen. New tests cover both.

Known gaps recorded for S8: the standalone host opens one file and has
no playlist replacement yet; Finder open, drag-and-drop, remote command
center, and touchbar are not wired into the Rust host; the artifact is
still the C engine until S8. The GUI controls cover the supported
keyboard set but not OSC mouse clicking (the OSC is display-only).

## S7d record

The TUI now runs every file the Rust engine can decode, including video
files with audio. `SchedulerEngine::load_audio_only` opens the demux
without a selected video stream, so no GPU context, frame channel, or
presentation worker exists; the C ABI uses it and `player_audio_probe`
no longer rejects a non-attached video stream. Video files therefore
claim into the TUI and never touch the C playloop. The `tui` gate adds
a video case: it checks the engine duration appears, no `VO:` or `AO:`
line is logged, and no GPU output opens. Verified 18/18 on macOS and in
the Linux container; workspace tests remain green.

## S7c record

S7c-1 added `player-ass`: hand-transcribed libass handles for library,
renderer, and track with lifetime guards, a pinned-version gate, and
bitmap render tests. S7c-2 added `EngineSub` and the subtitle track
surface: embedded ASS/SRT/VTT and external files, cue retention across
seeks, per-type track ids, and `sid`/`track-list`/`sub-visibility`/
`sub-add` in the Lua adapter. S7c-3 wires libass into presentation and
adds the fidelity gate.

The OSD path reproduces mpv's `sub/osd_libass.c` styling rather than
approximating it: embedded `osd_font.otf`, `sans-serif` through the
fontconfig provider, complex shaper, Unicode wrap feature, PlayRes
288x384 for OSD text and overlay res (720 default) for external
overlays, `mp_ass_set_style` defaults, and libass's internal alignment
encoding converted back to ASS numpad values. Blending mirrors
`draw_ass_rgba` with the 255*255 denominator. `EngineBackend::pump`
composites subtitles, the `show-text` message, and sorted
`osd-overlay ass-events` data into each fresh `PresentedFrame` before it
reaches the consumer. `show-text` uses the 1000 ms default and
wall-clock expiry. `show-text` and OSD overlays render before external
overlays, matching mpv's object order. The same `compose` entry point
serves the gate.

The `osd-fidelity` gate captures the C engine's `screenshot-raw` video
and OSD images at 320x240 on a generated solid fixture, feeds the video
buffer to the Rust example `osd_frame` (which drives the real vendored
osc.lua through `ScriptHost` and `SharedEngine`), and diffs pixels. It
covers the OSC bar and a plain `show-text` message. macOS uses
`gpu-next` on MoltenVK; Linux uses headless weston with lavapipe.
Result: 10 checks pass on both hosts; OSD coverage agrees exactly and
the largest channel difference is 5 (anti-aliasing rounding). The gate
also runs inside `linux-gpu-next`.

The Lua native-value surface gained byte arrays so a script can call
`screenshot-raw`; without that, the C oracle could not emit pixels
(the dev FFmpeg allowlist has no image encoder). `player-ffi` repeats
the libass link closure for its example target on Linux, and the ffi
test links with dead-strip so unrelated engine objects do not demand
the whole native closure.

The paused-seek subtitle gap that S7c recorded is fixed in S7e: the
scheduler primes the selected embedded subtitle stream from before the
target before re-seeking for audio/video preroll. External subtitles
already composed correctly.

## S6c record

The scheduler plays video files end to end on the Rust path with no
C video machinery: `EngineDemux` routes the selected video stream
alongside audio, `EngineVideo` decodes through a shared-mode
`VideoSession` borrowed from the engine-owned demux session, and
`EngineVideoOutput` presents due frames as RGBA through libplacebo
into a bounded channel the GUI bridges will consume. Untagged content
follows the mpv convention (SD 601, HD 709, limited range); HW frames
transfer to software until the S6d import path lands. Video-only files
stay on the C path: the engine still requires an audio stream.
`SchedulerEngine` gains `presented` in snapshots and a `drain_frames`
consumer; dropping the engine takes the frame receiver first so a
worker blocked in present unblocks instead of hanging the join.

Decisions with reasons:

- Present-time rendering, not drain-time. Dropped-late frames never
  reach the GPU, and eviction from the bounded pending queue keeps a
  missed present a silent skip instead of a leak.
- `Gpu` declares `renderer` before `vk`: struct fields drop in
  declaration order, and destroying the renderer after its Vulkan
  device calls through a dead dispatch table.
- The shared Linux placebo link closure lives in
  `rust/player-output/linux-link.args`, read by both the output and
  the engine build scripts. Build-script link-args do not propagate
  to dependent crates; the file removes the copy.

Load-bearing bugs found by bisect, both use-after-free with the same
shape (a live handle freed underneath its owner):

- `VideoSession { session: Some(s), ..this }` copied its all-`Copy`
  fields and ran `this`'s destructor, freeing the decode context
  twice. Single construction through `open_impl`/`open_decoder` now;
  `open_shared` borrows the session only for probing.
- `Gpu { vk, renderer }` dropped the device before the renderer.
  Field order is the drop order.

Verified: workspace `cargo test` green on macOS (engine 40,
including a 72-frame h264 end-to-end present-count and pixel-variance
test; media 20; output 7, including YUV420 solid colors against an
independent BT.709 reference within 6 levels); Linux container green
(media 20, output 7, engine 40 via the dynamic-link rerun on
lavapipe); `tests/run.sh build`, `rust-link`, `ffi` green. The
`plays_video_through_renderer`, output frame, and output Vulkan tests
ignore under static-pie with the documented dynamic rerun.

Still open in S6: Wayland client, macOS bridge, VO swap, `osc` and
`linux-gpu-next` gates, video-only engine files.

## S6d-1 record

The Linux window presents swapchain frames on weston headless: `sys.rs`
transcribes `VkInstance`/`VkSurfaceKHR`/`PlSwapchain` and the
`vkCreateWaylandSurfaceKHR`/`pl_swapchain_*` externs, `context.rs` gains
`create_for_surface` plus flipped `target_frame`, and `instance.rs` /
`swapchain.rs` hold the `Instance`/`Surface`/`Swapchain` guards with
GPU-first teardown order (swapchain, surface, renderer, device,
instance, protocol). The window itself lives in the new Linux-only
`player-display` crate (`Window::open/poll/present_planes`, xdg-shell
with configure timeout and poll-time `pl_swapchain_resize`).

Decisions with reasons:

- The weston present test moves with the window and requires the
  `present-gate` feature: a static-pie binary with
  `libwayland-client.so.0` in NEEDED dies in the loader before any
  constructor runs, so the static suite cannot even start it. The
  dynamic rerun (`-C target-feature=-crt-static ... --features
  present-gate -- --include-ignored`) stays the real gate and is
  green. This is environmental, not engine logic; the output lib
  suite links no Wayland and stays green statically.
- No static Wayland archive exists in the container, so
  `docker/linux/musl-cc-wrap.sh` maps `-lwayland-client` to the
  shared object appended where `-Bdynamic` is in effect;
  `scripts/linux-container.sh` wires it as the musl linker.
- `player-build` owns the single `linux-link.args` reader that
  player-output, player-engine, and player-display build scripts
  call; the closure stays edited in one place.

## S6d-2 record

`Window` owns the full per-frame pump: flush, non-blocking socket read
(`prepare_read` + `read`), then `dispatch_pending`. The earlier
dispatch-only poll never read the socket, so no configure or input
event past the initial handshake could arrive; the fullscreen request
likewise never left the socket before the flush. `set_fullscreen`
roundtrips on weston headless both ways once the client commits
fullscreen-sized content after the configure, which the protocol
requires. Resize is the same configure path: any configure size change
resizes the swapchain, and the roundtrip test pins the 320x240 to
1024x640 transition.

`drain_input` returns `InputEvent` (key code + press, pointer button +
press, scroll axis + value, pointer motion) with raw evdev codes; the
keymap fd is dropped unread. The seat binds opportunistically: headless
weston advertises none, so open succeeds seatless and drain yields
nothing. Translation helpers (`translate_key_state`,
`translate_button_state`, `translate_axis`, `is_fullscreen`) are pure
and unit-tested; the weston gate adds fullscreen roundtrip,
seatless-input grace, and an fd-count stability test.

## S6d-3 record

`sys.rs` transcribes `PlOverlay`/`PlOverlayPart` with the C probe
values as the authority (identical on macOS and musl). `frame.rs`
accepts `Option<&Overlay>` on `render_frame`/`render_to_target`: the
RGBA bitmap uploads to a sampleable texture and composites via
`frame.overlays` in `PL_OVERLAY_COORDS_DST_FRAME` with sRGB
full-range styling; the decode path in `player-engine` passes `None`
so video stays pure and OSD composites at presentation time.
`Window::present_planes` forwards the overlay to the swapchain target.

Tests: `overlay_covers_only_its_rect` renders the same base with and
without a white box and asserts covered pixels move while two corners
stay byte-equal; the weston gate adds `window_presents_with_overlay`.
The bitmap producer (libass rendering of `osd-overlay ass-events`
from upstream `osc.lua`) is S7; S6d-3 is the upload primitive only.

## S6d-4 record

`sys.rs` transcribes `VkMetalSurfaceCreateInfoEXT` (C probe:
sType 1000217000, 32 bytes, offsets 0/8/16/24) with the extern
mac-gated: the container loader does not export the metal entry, so an
unconditional declaration would break static-pie startup on Linux.
`Instance::new` requests `VK_EXT_metal_surface` plus the portability
enumeration extension and flag on macOS, and
`create_metal_surface` builds the surface on a caller-provided
`CAMetalLayer`.

The layer comes from a narrow v1 ObjC bridge
(`rust/player-display/bridge/metal_window.m`, compiled via the `cc`
build-dep on macOS only): open, layer pointer, event pump with close
detection, pixel size, fullscreen flag, fullscreen set, close.
`MacWindow` mirrors the Linux `Window` API (open/present/poll/resize/
fullscreen) and drops bridge-last so the layer outlives
`vkDestroySurfaceKHR`. `player-output`'s mac build links the Vulkan
loader now that Rust code references its symbols directly.

Verified for real on a Retina display with
`cargo run -p player-display --example mac_window_check`: 320x240
window reports 640x480 pixels, 30 frames present, fullscreen engages
and resizes to 3024x1898, exits cleanly, PASS. Two lessons recorded:
AppKit throws when a window is created off the main thread, so
`MacWindow` must be driven from the main thread (S7 GUI owns the
runloop; the struct is accordingly `!Send`), and the exit toggle must
wait for the enter animation to finish or macOS swallows it. No cargo
test can satisfy the main-thread rule under libtest, so the example
runner is the durable gate; headless CI cannot run it.

## S6d-5 record

The swap is proved by `player-swap`, a gate-only crate whose
`swap-gate` test plays `h264.mp4` through `SchedulerEngine` and
presents every drained `PresentedFrame` into a real weston window:
`window.presented() == delivered > 0`, no C video code in the path.
The test lives in its own crate because a dev-dependency on
`player-display` poisons the engine's static-pie lib test binary at
startup; the `present-gate` feature pattern keeps static suites green
and the dynamic rerun
(`-C target-feature=-crt-static ... -p player-swap --features swap-gate -- --include-ignored`)
is the real gate. The swap binary repeats the per-crate native closure
(ALSA, VA-API, placebo) in its own build script.

C VO removal audit (delete procedure): `video/out/vo_gpu_next.c`
stays registered in `video/out/vo.c`, is asserted by
`tests/wayland.py` (`current-vo == gpu-next`), and remains the dev
binary's only video path — no Rust player integration exists yet, so
nothing is dead. Removal defers to S8 after GUI integration lands.

S6 renderer integration complete.

## S5 record

Rust owns playback scheduling end to end for audio-only files. The
new `player-engine` crate holds the state machine, the master clock,
the scheduler, and a scripted simulator. `Machine` encodes the
campaign diagram with typed resume targets: pausing from draining
remembers draining, seeking from pause remembers the paused return,
and illegal edges are rejected without state change. `MasterClock`
anchors content position to device time in audio mode and to
monotonic time in video-only mode, so seeks rebase the anchor
without rewinding device time; speed, pause, latency, and
discontinuities all run on fake time in tests. `Scheduler` pumps
demuxed packets through decoder traits into output traits against
the clock, presents due video frames, drops late ones deliberately,
finishes decoders at EOF to release held-back frames, and routes
seek decode-ahead into a bounded preroll queue instead of the sink.
Sixteen simulator scenarios cover A/V, video-only, audio-only,
VFR, reordered DTS, forward/backward/rapid/paused seeks, pause and
resume, speed, track switch, missing timestamps, drain, decode,
demux, output, and device failures, shutdown, and stale commands.

The real backend drives the scheduler on a worker thread behind the
unchanged C symbols: `EngineDemux` wraps one media session for the
selected audio stream, `EngineAudio` decodes through the
`PacketDecoder` core extracted from `Pipeline`, and `EngineOutput`
wraps the null or native sink with software gain. The old
`AudioEngine` pump is deleted; the C ABI now lives in
`player-engine`. Audio-only files play, pause, seek, and report
through the scheduler with no TUI changes; video files stay on the
C path until the renderer arrives in S6.

Decisions with reasons:

- Progress-bounded pump steps, not packet counts. A 64-packet step
  blocks seconds on a paced sink and starves commands; each step now
  stops after the first pushed chunk or decoded frame, matching the
  old per-chunk command granularity.
- Seek decode-ahead stashes into preroll. Pushing to a paused sink
  errors loudly, which caught the first implementation; the queue is
  bounded and drained by the pumps, paused or playing.
- Reordered-DTS simulation uses windowed disorder with a matching
  decoder holdback. Fully reversed input is not incrementally
  decodable by anyone; the windowed fake proves pts-order
  presentation with bounded delay, and EOF finish releases the tail.
- The dev build links a third archive for the engine symbols; the
  release bundles already flow through `player-release`, which
  re-exports the engine. The engine repeats the Linux-only native
  link args that do not propagate from dependency build scripts.

Verified: workspace `cargo test` green on macOS (179 across seven
crates) and container engine/media/audio; `tests/run.sh play`,
`seek`, `audio` (15/15 through the scheduler), `tui`, `corpus`,
`hwdec`, `failures` green; full suites green on macOS (337) and
Linux (329); `scripts/audit.sh` ok. Track switching is proven in
the simulator only; product wiring arrives with S7 properties.
Ordered-chapter segment linking stays open and moves to S7 with
track management.

## S4 record

Rust owns video decode from packets to software frames. `VideoSession`
opens any FFmpeg decoder by codec name from a probed `StreamInfo` with
the packet timebase, rejects image codecs and dimension-less streams,
and pumps packets to drained frames with optional seeking and monotonic
`u64` decode ids. `VideoFrame` exposes plane slices sized by the real
stride and transfers hardware frames to software with a pixel-format
note. Hardware goes through `HwRequest` with a format-picking callback:
VideoToolbox on macOS, VA-API on Linux, and software fallback on any
init failure. Headless proof: VideoToolbox engages on this Mac (no
window, no display needed); VA-API fast-fails before touching libva
when no DRM render node exists, which is the same static-pie class as
the ALSA issue and is by design headless-safe.

Decisions with reasons:

- Transcription over function-only access. No lavc field getters exist,
  so the S2 precedent (hand-transcribed structs, runtime major-version
  gate, C enum-dump as authority) extends to `AVCodecContext`,
  `AVFrame`, hwcontext, and codec/frame functions. Hand-parsed enum
  values were wrong more than once; the compiler plus drift tests are
  now the rule.
- Stride assertions tolerate CPU alignment padding. lavc aligns luma
  stride to the host CPU preference (320 on macOS, 384 in the Linux
  container for the same 320-wide fixture); tests assert stride at
  least the width and exact buffer sizing, not exact stride.

Verified: `player-media` 19/19 green on macOS and Linux (container);
`tests/run.sh corpus` 109, `hwdec` 8, `failures` 8. The decoded frames
are not wired to any renderer yet; that is S5/S6 work. Ordered-chapter
segment linking still defers to S5.

## S3 record

Rust owns audio playback end to end for local files. `DecoderSession`
opens any Symphonia-closure codec by FFmpeg codec name, takes extradata
and the native channel mask from the probed stream, synthesizes missing
packet timestamps from a cursor, and resets both decoder and cursor on
seek. The mixer maps Symphonia layouts onto mono, stereo, and 5.1 with
standard downmix gains and fans mono out to every channel. The
streaming resampler interpolates with Catmull-Rom from an integer
output count, so split pushes are bit-identical to one whole push and
long streams cannot drift; same-rate conversion is a bit-exact
passthrough and one scratch buffer is reused across pushes. `Pipeline`
pulls packets through decode, mix, and resample into caller-owned
chunks with no per-chunk allocation, fails loudly on mid-stream layout
changes, and verifies the decoded layout against the container hint.
`AudioClock` derives position from frames handed to the sink minus
frames still queued in it, freezes on pause, and re-anchors on seek
and resume. `AudioEngine` runs the pump on a worker thread behind a
command channel, applies software gain, parks at EOF for restart by
seek, and records worker death loudly. The C ABI exposes load, pause,
seek, volume, mute, snapshot, destroy, and a metadata probe with
case-insensitive key match.

The TUI claims audio-only files into the engine and leaves the C
playloop idle: startup sets idle mode with no current entry when the
first playlist file probes engine-ready, and the TUI reads track state
from engine snapshots. Video files keep the client path, engine load
failures degrade to C playback, and a dead engine falls back to
`loadfile`. The null sink paces in real time like the C null AO, so
headless runs keep real playback dynamics. The TUI gate proves the
bypass: no `AO: [` or `VO: [` line appears in the log while metadata,
cover art, and redraws work.

Decisions with reasons:

- The resampler is pure Rust, not libswresample. The campaign permits
  libswresample where justified, and no justification survived: the
  pinned prefix already ships it for the C chain, but the Rust path
  needs no new native linkage, stays deterministic, and is covered by
  unit tests for tone fidelity, zero-crossing preservation, silence,
  and stable scratch capacity.
- The CoreAudio channel bitmap passes the FFmpeg mask through. The SDK
  headers prove the bitmap bits match FFmpeg for mono, stereo, and 5.1;
  the engine only emits those three layouts, and a compile-time size
  assert plus a runtime capacity check guard the transcribed structs.
- Build-script link-args do not propagate to dependent crates the way
  link-lib and link-search do, so `player-audio/build.rs` repeats the
  Linux VA-API pair that `player-media` emits for its own targets.
- Static-pie test binaries cannot call into the container's shared
  libasound: the first call segfaults. The production link is dynamic
  musl and unaffected. The two ALSA tests are ignored with the
  dynamic-link rerun command and both pass under
  `RUSTFLAGS="-C target-feature=-crt-static"`. The CoreAudio device
  test is ignored for the owner manual checklist.

Verified: `cargo test` green on macOS (45 audio, 36 media, 13 ffi, 19
core, 9 lua, 14 tui) and Linux (44 audio with 2 ALSA ignored, rest
identical); `tests/run.sh audio` 15/15 and `play` 8/8 on macOS; full
suites green on macOS (337) and Linux (329); `scripts/audit.sh` ok;
`scripts/measure.sh final` ok with no growth (executable 9,440,304 B,
distributable 26,774,400 B, first frame 139.61 ms, idle RSS
83.94 MB). Non-Symphonia codecs (for example AC-3) fail loudly at
probe time and stay on the C engine; that split is recorded for the
S7/S8 scope decision.

## S2 record

New `player-media` crate: `MediaSession` opens local files through
`classify_input` plus `LocalFile`, feeds libavformat through a custom AVIO
context that never receives a path, and exposes streams, container
metadata, duration, chapters, attachments, packets, and seeking. FFmpeg
bindings are hand-transcribed from the pinned n9.0.2 headers
(`player-media/src/sys.rs`) with a runtime major-version gate; the static
closure links libavformat, libavcodec, libavutil, zlib, libm, dav1d, and on
Linux the container libva, mirroring the C link. Nine session tests cover
stream discovery, chapters, multiaudio, packet flow to EOF (proving
`AVERROR_EOF`), seeking, cover art, local-only rejection, malformed input,
and tagged metadata. The C engine is untouched; no demux C is deleted yet.

Proven against the pinned build (C replicas in `.work/tmp/s2probe`):
libavformat consumes the context on failed open (nulls the pointer, never
touch it) and may reallocate the AVIO buffer during probing (always free
the current pointer). Both rules are encoded in `MediaSession::close` and
the open failure path.

Open items, explicitly deferred:

- Ordered-chapter segment linking stays open for S5. libavformat does not
  do it (mpv used `demux_mkv_timeline`); no ordered-chapter fixture exists
  to prove it either way. S2 added a simple-chapter fixture
  (`chapters.mkv`) and chapter enumeration; ordered chapters need a fixture
  plus a linking design.
- The Rust session is not wired to any decoder yet. That is S3/S4 work;
  S2 proves demuxing alone.
- `cargo fmt --check` drift is unchanged (pre-existing, no gate).

## S1 record

Rust owns the application entry (`player_ffi_run` sequences
create/init/play/finish/destroy; `player/main.c` is a thin host), one
command dispatcher (parse-time checks plus a `run_command` gate that sees
every executed command including node input; multi-command strings validate
per part and never partially execute), scalar playback state plus
aid/sid/vid, and the property event bus. C getters read the Rust store;
C reports facts on option changes and each playloop pass. Verified:
`cargo test` green, `tests/run.sh dispatch` 13/13 on both platforms, full
suites green on macOS (331) and Linux (323), budgets and audits pass.

Scoped deviations from `campaign-rust.md`:

- Property-change *delivery* still runs through the C client path; the Rust
  bus is the authoritative log in S1 and becomes the delivery source in S7.
  Replacing `send_client_property_changes` now would risk the observe paths
  that S7 owns.
- Full playlist convergence stays with S5 (scheduler owns playlist
  transitions). S1 classifies playlist commands at the dispatcher and keeps
  the existing order/ID sync.
- `cargo fmt --check` reports drift across the repo (including untouched
  files) under the pinned toolchain; no fmt gate exists in CI or audits, so
  S1 leaves formatting alone. New code follows neighboring style.
- The Linux suite runs against `build-linux/mpv`, which no suite rebuilds.
  Rebuild it with `scripts/linux-container.sh scripts/build-linux.sh`
  before every Linux suite run; S1 caught a stale binary this way.

## Rust-owned components

- Startup parsing and validation: `player-core::config`, `command` (subset), `state`, `playlist`, `input`, `local_file`.
- FFI surface: `rust/player-ffi` (command, state, playlist, local file, startup, TUI, Lua, mpv handles).
- Audio pipeline: `rust/player-audio` (Symphonia sessions, channel
  mixer, streaming resampler, clock, CoreAudio and ALSA sinks,
  threaded engine with a C ABI, metadata probe).
- Subtitles and OSD: `rust/player-ass` (libass handles and bitmaps),
  `rust/player-engine/src/sub.rs` (embedded and external cues), and the
  `player-ffi` OSD composer plus Lua overlay adapter.
- Script host: `rust/player-lua` (mlua, Lua 5.2, vendored).
- Terminal UI: `rust/player-tui` (renderer) with `player-ffi` routing
  audio-only files to the Rust engine and video to the C engine.
- GUI host: `rust/player-gui` on `player-display`, `player-engine`,
  `player-ffi`, `player-lua`, and `player-output`; native window and
  input, OSC overlay presentation, macOS menu close.

## Remaining mpv C dependencies

Everything else: `player/` playloop, audio, video, command, demux orchestration,
`demux/`, `stream/`, `sub/`, `video/`, `audio/`, `filters/`, `osdep/` platform
glue, client API, IPC, OSD/OSC delivery.

## S0 baseline (reference: legacy engine at `ee5021c`)

Host: macOS 27.0.1 arm64, Xcode 27.2, SDK 27.0, Meson 1.12.1, Python 3.15.1,
Rust nightly-1.100.0 (`nightly-2026-09-15`). `perf-gate-ready` exit 0.

- `cargo test --manifest-path rust/Cargo.toml`: pass (~1 s warm).
- `tests/run.sh suite`: 316 passed, 0 failed (~2m20s).
- `scripts/audit.sh`: ok (~27 s).
- `scripts/measure.sh final`: ok (~1m17s).
  - cold 68.31 ms (spread 10.38), warm 66.83 ms (spread 2.51)
  - first frame 1080p 154.31 ms (spread 11.16)
  - idle RSS 83.84 MB, 1080p RSS 213.31 MB, CPU 0.26 s (spread 0.13)
  - seek 28.99 ms, exit 6.37 ms, audio underruns 0
  - executable 9,440,304 B, distributable 26,774,400 B, budgets pass
  - protocols 1 (`file`), network 0, one Lua interpreter
- Linux (Alpine x86-64 container): `make linux` ok (~39 s, deps cached),
  `tests/run.sh suite` 310 passed 0 failed (~2m4s),
  `scripts/audit-linux.sh` ok (~2 s).
- `tests/run.sh ffi`: 3 passed (~16 s).
- `tests/trace.py --check`: 33/33 goldens match, stable over 6 consecutive
  macOS runs and green in the Linux container.

Oracle traces: `tests/trace.py` drives the reference binary over all 34
fixtures (21 corpus, 3 audio, 3 subtitle via companions, 2 cover, 3 failure;
2 local-path fixtures stay with the `local-paths` gate). Goldens in
`tests/traces/` (33 files, committed). `traces` gate runs in both suites.
Known reference nondeterminism, normalized in `canon_order` with a
`--selftest`: seek/playback-restart adjacency, doubled playback-restart,
post-EOF teardown events, macOS-only startup `idle`, duplicate pause
property-change delivery. Pause behavior asserts the exact True-to-False
transition; event order asserts kinds and pre-EOF order.

Fixture corpus: reproducible from `tests/fixtures/gen.sh` plus
`tests/fixtures/manifest.json`. Measurement fixtures under `.work/fixtures/`.

## Known platform gaps

- Real-GPU VA-API (`linux-vaapi` gate, needs render node), VideoToolbox
  hardware paths, audible output, HDR/color, manual-checklist owner runs:
  all on the Needs environment list, none blocking.
- Non-UTF-8 path arguments rejected before the classifier (open, unchanged).
- The S7e GUI host opens one file; Finder open, drag-and-drop, playlist
  replacement, remote command center, and touchbar stay for S8 artifact
  integration.
- Clipboard removal landed in S7a.

## perf-gate-ready

Exit 0 before the S0 measurements above. A busy host defers measurement;
a deferred run never counts as a pass.
