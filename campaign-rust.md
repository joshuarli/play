# Coding Agent Campaign: Complete the Rust Port of `play`

## Status

Goal: replace the remaining mpv-derived C playback engine with a small Rust-owned engine. No loss of supported local playback. Both release artifacts keep building.

- Last accepted: S2. Preflight complete 2026-11-03.
- Red: none. S2 acceptance green on macOS and Linux.
- Next: S3 (Rust-owned decoding and audio playback). Then S4.
- Ordered chapters stay open for S5 (no fixture, needs linking design).
- Resume: read this doc and `docs/RUST-PORT.md`, then run the next open slice. If the tree moved, adapt. Never overwrite newer work.

## Charter

You are the executor. This doc is your only order. It stays in place. Git history is the log.

- Land one slice at a time on the integration branch. A slice is done when its acceptance command passes on the integrated tree.
- Run the parent steering checkpoint after each accepted slice: name what landed, name the one bottleneck, then continue, narrow, serialize, fix one prerequisite, or park. Rewrite Status when the ruling changes.
- Decide inside the bounds in Decisions and keep going. Log a decision only when a later slice depends on it.
- Keep every migrated slice buildable. Test and commit each slice. Never leave the tree red.
- Validate Linux continuously, not only at the end.
- Record verified progress in `docs/RUST-PORT.md` at checkpoints only: current phase, Rust-owned components, remaining mpv C, test results, perf comparison, milestone commits, platform gaps, next task.
- Do not port mpv line for line. Build the smallest correct engine that meets the scope below.
- Do not declare victory on a green compile. Runtime behavior and integration must work.
- Stop when every local deliverable meets its proof or every remaining item is parked behind a hard stop. Environment-bound proof never holds the campaign open. It moves to Needs environment.

## Decisions

Owner-decided:

- Remove clipboard support entirely (owner, 2026-11-03). The clipboard command, clipboard property, macOS bridge, and default `Ctrl+v` binding go away in S7. No alias, no fallback. Tests that assert the clipboard contract go away with them. `docs/scope.md` still lists this as open. The executor updates that doc in S7.

Prescribed (settled here, with the reason; a reversal needs the stated evidence):

- No pushes. Commit locally to the integration branch. The repo rule allows pushes only when asked, and preflight forbids pushing. The campaign closes on accepted local commits. The owner pushes.
- Keep the pinned Rust nightly in `rust-toolchain.toml`. The repo forbids routine nightly changes.
- Keep Symphonia as the only general audio decoder. It already decodes packets from the FFmpeg demuxer in-tree. A second decoder needs measured evidence, not suspicion.
- Keep libplacebo for rendering. Reimplementing compositing or tone mapping is out of scope.
- No Tokio, no large async runtime. Threads plus bounded channels already cover the design. A lightweight event mechanism is allowed only with a measurement that justifies it.
- The FFmpeg component allowlist stays frozen. Broaden it only with a fixture-backed justification recorded in `docs/RUST-PORT.md`.
- Budgets gate S0 and S8, not the middle. Between them they are measured and logged. Two engines coexist mid-campaign, so transient growth is expected. Uncontrolled growth (over 25% above baseline without a recorded cause) stops the slice.
- `unsafe` stays in `player-ffi` and `player-audio` for production code. Each block states its safety argument. Test-only `unsafe` (for example `env::set_var` in `rust/player-tui`) is allowed.
- Keep the UTF-8 path boundary. Startup parsing and the classifier take UTF-8. This matches `docs/scope.md`. Widening it is a separate campaign.
- No second Lua interpreter. The vendored Lua 5.2 through `mlua` is the only one.
- Follow-on work stays out: Linux aarch64, HDR output, fully static musl `wlshm`, and any network capability. `docs/architecture.md` already lists these as follow-on.

Delegated (the executor decides inside the bound, uses the smallest reversible assumption, logs it in `docs/RUST-PORT.md` only when a later slice depends on it):

- Crate names and splits inside the Section 4 layout, as long as boundaries stay cohesive. Consolidate when a split adds no boundary.
- Which single pitch-preserving resampling algorithm to port, if any. Port one only when a fixture proves the need. Never keep two scaletempo variants without a measured reason.
- Whether libavformat covers Matroska ordered chapters. Use the existing fixture corpus to decide. Add a narrow Matroska extension only for a demonstrated gap.
- Tolerance values for differential comparison, as long as semantic event order is exact and timing uses stated bounds.
- New crates.io dependencies, within the repo dependency rules: vet maintenance, weight, license, and stdlib alternatives, and record the reason in the commit body.
- Slice order tweaks and acceptance-command narrowing, as long as dependency order holds and proof stays equivalent or stronger.

## Baseline

Pins (from `docs/baseline.md` and `docs/architecture.md`):

- mpv v0.41.0 at `41f6a645068483470267271e1d09966ca3b9f413`. FFmpeg n9.0.2 at `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`. Symphonia at `b6336209bb40d57e4ed71079cf7c6fc6a5e`, vendored under `vendor/symphonia`.
- Prior simplification is done and stays done: no Windows, no legacy GPU renderer, no encoding or recording paths, no SPDIF, no FFmpeg audio fallback, no networking, no libavfilter. Do not repeat it.

Known state at preflight (executor re-verifies in S0; HEAD has moved three commits past the last verified run):

- `docs/status.md` records 315 passed and 0 failed for `tests/run.sh suite` at `96331a9`, plus a passing Linux suite of 307 checks and a passing `scripts/audit-linux.sh`.
- Reference perf at `96331a9` (`docs/metrics.md`): 65.47 ms cold start, 150.12 ms headless first frame, 83.92 MB idle RSS, 215.83 MB 1080p RSS, 23.68 ms seek, 27,041,616 B app total. These are history, not the new baseline. Re-measure in S0.
- Loop timings measured at preflight on a quiet host (`perf-gate-ready` exit 0): `cargo test --manifest-path rust/Cargo.toml` about 1 s warm; `tests/run.sh ffi` about 16 s; `scripts/audit.sh` about 27 s. Full `suite` and `scripts/measure.sh final` are unmeasured. S0 times and records them.

## Scope and engineering rules

These bind every slice. They restate the repo docs; the docs win on conflict.

Supported systems:

- macOS: arm64 only, macOS 27 target, AppKit/Swift UI, CoreAudio, VideoToolbox, Vulkan through MoltenVK, libplacebo, self-contained `Player.app`.
- Linux: x86-64 musl only, Alpine-compatible, Wayland only, ALSA only, Vulkan plus libplacebo, VA-API hardware decode with software fallback, minimal dynamic closure.
- Never add Windows, glibc, X11, PipeWire, PulseAudio, Electron, Qt, SDL, or GStreamer.

Retained multimedia dependencies:

| Task | Implementation |
|---|---|
| Container demuxing | Pinned FFmpeg libavformat |
| Video decoding | Pinned FFmpeg libavcodec, dav1d, hardware accelerators |
| Audio decoding | Vendored Symphonia |
| Audio conversion | libswresample where justified |
| Video rendering | libplacebo |
| Subtitles and shaping | libass with existing font and shaping stack |
| Script execution | Existing `mlua` Lua 5.2 host |
| macOS integration | Existing Swift and AppKit with the Rust engine |
| Linux integration | Minimal Wayland, Vulkan, and ALSA integration in Rust |

Third-party C libraries are permitted. Never reimplement codecs, GPU rendering algorithms, or subtitle shaping.

Behavior to preserve (`docs/scope.md`, `docs/architecture.md`, `README.md`, `docs/lua-api.md`, current tests):

- Local audio and video playback, existing codecs and containers, M3U/M3U8/PLS playlists, Matroska chapters and ordered chapters, embedded and external subtitles, external audio tracks, embedded album artwork.
- GUI and terminal UI, controls and key bindings, seeking, pausing, track selection, playback speed, native fullscreen, drag-and-drop, file opening, screenshots.
- Supported Lua, OSC, IPC, and property-observation behavior. Hardware decoding with software fallback. Clean shutdown and terminal restoration.
- The historical mpv API is not the contract. Inventory actual command, property, and script usage. Drop historical baggage only when the scope and verified tests establish its absence.
- Clipboard is the one exception: it is removed, per the owner decision above.

Security invariants:

- Local input only. No network schemes, no TLS, no network protocol handlers, no remote fetching.
- No implicit user config loading. No automatic third-party script discovery.
- Explicit Lua scripts are trusted code, not sandboxed code.
- No new background services, telemetry, or daemon dependencies. No subprocess-based playback.
- No new unbounded queues and no unbounded metadata parsing.
- Apply the local-only policy on every file-loading path: playlist entries, nested containers, external subtitle and audio paths, and FFmpeg custom I/O. Prefer Rust-owned file handles and FFmpeg custom I/O callbacks so FFmpeg never resolves a path on its own.

Target shape (logical, not a crate mandate):

```text
player executable (Rust)
├── player-core (typed config, command dispatcher, property and event registry,
│                playback state machine, playlists and tracks, local file policy)
├── player-media (FFmpeg demux adapter, video decoder adapter, Symphonia audio,
│                 subtitles and chapters, packet and frame ownership, sessions)
├── player-engine (clocks, scheduler, bounded queues, seek and flush,
│                  playback-rate handling, output coordination)
├── player-output (libplacebo renderer, CoreAudio and ALSA,
│                  VideoToolbox and VA-API, macOS and Wayland integration)
├── player-lua
├── player-tui
└── player-sys (tightly scoped native FFI)
```

Reuse `player-core`, `player-ffi`, `player-audio`, `player-lua`, and `player-tui` where practical. Consolidate when a split adds no boundary.

Ownership rules:

- Rust owns application and session lifetimes, commands, events, mutable playback state, playlist and track metadata, packet and frame queues, clock state, decoder orchestration, session creation and teardown, error handling, and resource cleanup.
- Third-party libraries own their native objects, managed through Rust RAII wrappers.
- One authoritative playback state. No competing C and Rust copies.

FFI rules:

- Centralize `unsafe` in native integration modules. Prefer explicit minimal bindings against pinned library versions.
- Use `NonNull`, typed handles, ownership wrappers, and `Drop`. Model refcounted packets, frames, hardware surfaces, and GPU resources explicitly.
- No Rust panic crosses an external callback ABI. No native callback outlives the Rust state it references.
- Audit destruction order and thread affinity. Avoid opaque global callback tables.
- Never add C glue only because an old mpv wrapper exists.
- A narrow platform ABI shim is allowed only when native interop genuinely needs it. Never keep mpv-derived C as disguised glue.

Concurrency rules:

- Threads and bounded channels. Bounded demux packet queues and bounded decoded-frame queues with explicit backpressure.
- Cancellation and generation counters for seek and session changes. No stale frame survives a seek.
- Deterministic shutdown. No blocking external calls while holding engine locks.
- No heavyweight scripting work on decode or presentation threads. No allocations, blocking mutexes, filesystem access, logging, or panics in real-time audio callbacks.
- Keep the execution model short enough to explain in one document.

## Sequence

One writer by default. Slices share core types and run serially. The single allowed parallel pair is S6-Linux (new `player-output` Linux files only) with S7-Lua (`rust/player-lua` only), after S5 freezes engine core types and only while the integration branch is green. A parallel writer gets a parent-owned worktree at `.work/wt/<lane>`. Never use `/tmp` for worktrees or scratch.

Every slice: implement, run its acceptance command on the integrated tree, commit with the slice ID in the body. A slice gets three attempts. After the third failure, park it on its branch with the cause and the retry condition, and pick the next slice. Red work never lands.

Full `suite` runs at S0, S5, and S8 on macOS, plus the Linux suite in the Alpine container at the same milestones. Other slices run their named gates. A failing slice first checks S0: is this new, or baseline red.

### S0 Ground and oracle

Owns: `docs/RUST-PORT.md`, `tests/fixtures/`, `.work/fixtures/`, harness trace helpers. Touches nothing else except new tests.

- Record commit, platforms, compiler versions, libraries, and build configuration.
- Run `cargo test --manifest-path rust/Cargo.toml`, `scripts/build.sh`, `tests/run.sh suite`, `scripts/audit.sh`, `scripts/measure.sh final`, `make linux`, `scripts/linux-container.sh tests/run.sh suite`, `scripts/linux-container.sh scripts/audit-linux.sh`. Record exact results. Preexisting environment failures go in the charter file with cause. They never count as slice failures later.
- Time the full suite and `measure.sh final`. Record both in the charter file.
- Freeze the fixture corpus: generated fixtures stay reproducible from `tests/fixtures/gen.sh` plus `manifest.json`. Measurement fixtures stay under `.work/fixtures/measure`.
- Capture deterministic behavioral traces from the current engine: media metadata and duration, track enumeration, chapter enumeration, command results and errors, property changes and event order, seek targets and completion, EOF behavior, playlist transitions, stream initialization, cleanup after malformed media.
- Normalize traces for async order. Never compare timestamps exactly. Define tolerances and compare semantic event order.
- Add tests for behavior now covered only by broad integration gates.
- Keep the legacy engine as the development-only reference. Production keeps using it until the Rust engine meets its contract.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh baseline-docs && scripts/measure.sh final
```

`docs/RUST-PORT.md` exists with the baseline numbers, trace locations, loop timings, and the S1 task list.

### S1 Rust owns the control path

Owns: `rust/player-core/`, `rust/player-ffi/` command, state, playlist, startup, and input paths, `player/main.c` startup, key-binding dispatch. Prerequisite: S0.

The split ends here: today the Rust checker validates a subset of commands, C executes them, and node commands bypass the checker.

- Real Rust entry point, typed CLI parsing, engine init and shutdown, native logging and error handling.
- One Rust dispatcher for every entry point: CLI, keyboard, GUI, TUI, Lua, IPC. Unified typed commands, unified property registry, unified event delivery, Rust-owned playlists, input dispatch, IPC dispatch, Lua host adapter, GUI and TUI event subscriptions.
- All command paths converge:

```text
CLI / keyboard / GUI / TUI / Lua / IPC
                  ↓
        Rust command dispatcher
                  ↓
          Engine state changes
                  ↓
        Rust property/event bus
```

- Preserve the IPC wire format and Lua behavior. No generalized mpv option framework. Typed options for supported features only.
- The old engine may run behind an internal adapter, but Rust owns application commands and observable state. Shrink the C-to-Rust callback interface with every slice. Never expand it.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh cli && tests/run.sh input && tests/run.sh playlist && tests/run.sh state
```

The Rust entry point runs existing media. Identical commands through different interfaces give identical results. No validation bypass remains. No duplicated authoritative state. All legacy tests pass.

### S2 Rust-owned local demuxing

Owns: new `rust/player-media/` demux paths, `rust/player-core/src/local_file.rs` extensions, FFmpeg libavformat bindings. Prerequisite: S1.

- One Rust media-session abstraction. Rust-owned local file access. Minimal libavformat bindings.
- Packet reading, stream discovery, container metadata and attachments, stream selection, chapters and timelines, playlist loading, cover-image extraction, seeking with demux state reset.
- Keep supported Matroska ordered chapters. Check libavformat coverage against the fixture corpus first. Add a narrow Matroska extension only for a demonstrated gap. Never port `demux_mkv.c` line for line.
- Malformed or truncated packets never panic. Bound metadata, allocation sizes, and queue growth. Respect file size and seek limits. Keep path semantics. Handle audio-only, video-only, mixed media, multiple audio and subtitle streams, time bases, and timestamp discontinuities. Enforce local-only on nested references and playlists.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh corpus && tests/run.sh local-only && tests/run.sh local-paths && tests/run.sh failures
```

The Rust demux path passes container, playlist, metadata, chapter, attachment, local-only, malformed-input, and seek tests. Supported media needs no mpv stream or demux framework. Delete demux C only after its last consumer is gone.

### S3 Rust-owned decoding and audio playback

Owns: `rust/player-audio/`, Symphonia bridge, CoreAudio and ALSA backends, resampling path. Prerequisite: S2.

- Rust-managed Symphonia sessions: codec init and extradata, packet-to-PCM flow, flush and reset, end-of-stream drain, channel layouts, PCM conversion and resampling, output buffering.
- Remove the extra cross-language frame ownership and copies where practical. No new per-frame allocation on the audio path.
- Small explicit pipeline: demux → Symphonia → PCM conversion and resampler → bounded audio ring → CoreAudio or ALSA.
- The output clock counts frames already queued to the device, not only decoded frames. Handle underflow, device reset, pause, resume, seek, and rate changes without losing sync.
- Pitch-preserving speed changes: port one well-defined algorithm into Rust and test it alone, only when a fixture proves the need. Never keep two scaletempo variants without a measured reason.
- Keep the CoreAudio channel-map workaround or replace it with verified-correct native code. Test mono, stereo, and multichannel order.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh audio && tests/run.sh play
```

Audio-only playback runs without the legacy engine. Symphonia is the only general audio decoder. PCM reaches native outputs through Rust-owned buffers. Seek, pause, resume, EOF, and track switching work. No persistent underruns or deadlocks. Fixtures pass on macOS and Linux. The TUI drives the Rust engine directly.

### S4 Rust-owned video decoding

Owns: `rust/player-media/` video paths, VideoToolbox and VA-API interop. Prerequisite: S2.

- Typed decoder configuration. FFmpeg codec init. Packet send and receive. Drain and flush. Presentation-timestamp reordering. Pixel-format changes. Color space and HDR metadata propagation. Frame reference counting. Software decode. Hardware init and fallback.
- Design hardware-frame ownership explicitly before coding. For each hardware-decoded frame state: owning thread, surface retention, transfer to the renderer, lifetime fences for presentation, release after GPU use, behavior on render failure, behavior on seek or shutdown, fallback condition.
- Keep VideoToolbox with CoreVideo interop on macOS. Keep VA-API with dmabuf zero-copy on Linux where available. A software-copy fallback is allowed only under the existing player's conditions. Never relabel all-CPU copies as hardware acceleration.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh corpus && tests/run.sh hwdec && tests/run.sh failures
```

Supported codecs play through Rust-managed decoding. Hardware decode works or falls back. No stale frame survives seek or track change. Resolution, pixel-format, and color-metadata changes are handled. Decoder and frame memory stays bounded. Malformed-input, repeated-init, hardware, and software paths each have tests.

### S5 Rust playback scheduler

Owns: new `rust/player-engine/`, replacing `player/playloop.c`, `player/audio.c`, `player/video.c`, and remaining filter coordination. Prerequisite: S4. Full `suite` milestone on both platforms.

Build the simplest correct scheduler for supported playback, not a reproduction of mpv's.

Required state machine:

```text
Idle → Opening → Buffering → Playing ⇄ Paused
                                  ↓    ↑
                               Seeking ┘
                                  ↓
                         Draining → Ended
Any state → Error. Any state → Closing → Idle.
```

Multiple files and playlist transitions leak no state.

Clock design: audio-master clock for ordinary A/V. Model device playback time, monotonic system time, video PTS, stream time-base conversion, resampling and buffering latency, drift correction, playback speed, discontinuities and seek rebasing, video-only playback, audio-only playback. Schedule frames against presentation time, never decode-completion time. Drop late frames deliberately. Never render a stale frame to satisfy FIFO order.

Required scenarios: normal A/V, video-only, audio-only, variable frame rate, reordered PTS and DTS, forward and backward seek, repeated rapid seeks, seek while paused, seek across chapters, pause and resume, speed changes, audio and subtitle track switches, missing timestamps, EOF and draining, decode failure mid-playback, device and output failure, shutdown during decode or GPU presentation.

Build a deterministic scheduler simulator with fake clocks and frame sources. Clock behavior must test without a window or sound device.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh play && tests/run.sh seek && tests/run.sh suite
```

The Rust scheduler controls playback end to end. No legacy playloop remains on the normal path. Timing, behavior, memory, and CPU meet the S0 baseline tolerances.

### S6 Rust video rendering integration

Owns: new `rust/player-output/`, Vulkan context, swapchain, libplacebo wrapper, macOS Swift bridge files, Linux Wayland files. Prerequisite: S5. The `gpu-next` implementation is the behavioral reference, not the target architecture.

- Rust ownership of the libplacebo renderer: Vulkan device and context init, swapchain and presentation, frame upload and import, hardware surface mapping, color conversion and tone mapping, HDR metadata, scaling and aspect ratio, subtitle overlays, OSD and OSC overlay integration, fullscreen and resize coordination, presentation timing and diagnostics, teardown and recreation.
- Keep the pinned libplacebo and shader toolchain. No custom shader renderer.
- macOS: reuse Swift and AppKit. Keep AppKit calls on the required thread. Narrow versioned Rust-to-Swift ABI. Bridging-header changes invalidate dependent Swift objects. Preserve Finder file-open events, menu bar, fullscreen, window teardown, and bundled MoltenVK configuration.
- Linux: minimal Wayland client. Preserve Vulkan presentation, xkbcommon input, resizing, pointer interaction, and supported compositor protocols. No second interchangeable backend.
- Capture frames submitted and presented, dropped and repeated frames, presentation-time errors, A/V drift, GPU import success and failure, resource usage, HDR and color metadata, resize and fullscreen transitions. Headless lavapipe covers automation. It never substitutes for real-hardware validation.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh osc
```

plus `tests/run.sh linux-gpu-next` in the Alpine container. The GUI plays video through the Rust-owned renderer. No mpv video-output machinery remains. Video, subtitle, and OSC behavior is intact.

### S7 Subtitles, scripting, GUI, TUI, clipboard removal

Owns: `rust/player-lua/`, subtitle paths, `rust/player-tui/`, GUI glue, clipboard remnants where found. Prerequisite: S6.

- Subtitles: Rust-managed tracks and timing, narrow libass wrapper, external and embedded codec support, styling, font lookup, positioning. Correct changes after seeks and track switches.
- Lua: keep `player-lua` and the built-in OSC unless a measurement forces replacement. Replace the mpv client API adapter with a direct Rust engine adapter. Preserve supported command, property, event, overlay, and input-binding behavior. Script shutdown never retains a freed engine handle. One Lua interpreter.
- TUI: same Rust engine as the GUI. Keep kitty graphics, truecolor half-block fallback, cover art and metadata, keyboard controls, signal-safe terminal restoration, metadata sanitization. No second scheduler.
- GUI: keep native platform behavior and minimal controls. No redesign.
- Clipboard removal (owner decision 2026-11-03, delete protocol): trace every reference first, starting from `player/clipboard/`, `osdep/mac/clipboard.swift`, `osdep/mac/app_bridge_objc.h`, `player/command.c`, `player/main.c`, `player/playloop.c`, `player/core.h`, the `options/` tables, the default `Ctrl+v` binding, the IPC surface, tests, fixtures, and docs. Remove the implementation, exports, wiring, binding, tests that assert the clipboard contract, and doc mentions including `docs/scope.md`. Add no alias, shim, or fallback. Update remaining callers to the surviving behavior. After the cut, search the live tree for retired names and classify every hit as unrelated or historical.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh lua-host && tests/run.sh lua-api-audit && tests/run.sh osc && tests/run.sh osc-input && tests/run.sh tui
```

GUI and TUI share one playback core. Lua no longer touches the old mpv client API. No C application framework remains for scripting or input. No clipboard path remains anywhere.

### S8 Remove the legacy engine, Cargo takes over

Owns: remaining mpv C playback sources, `meson.build` player rows, build and release scripts, CI workflows, docs. Prerequisite: S7. Full `suite` milestone on both platforms plus release and audit gates.

Delete or retire: playback-loop sources, demux wrappers, filter framework, packet and frame representations, audio and video output abstractions, command and property implementations, client event framework, stream and playlist machinery, mpv-specific allocators and compat utilities, old C build registrations, obsolete headers, dead platform variants, unused build tools and test fixtures, transitional FFI adapters, legacy build switches and feature probes. Keep third-party sources and license notices the release needs.

Cargo becomes authoritative. The actual `player` executable comes from Cargo. Build scripts may still build pinned native dependencies. Meson no longer compiles a player engine. Update the development build, macOS app bundling, Linux musl packaging, GitHub Actions, checksums, dependency audits, symbol audits, source provenance, developer docs, install scripts, and harness executable paths. Keep the public name `player` and existing artifact names.

Negative verification (automated audit, grounded in actual sources and link manifests, never an over-broad symbol ban):

1. No mpv-derived C unit compiles into either release.
2. No embedded or dynamically loaded mpv engine is present.
3. No mpv process launches for playback.
4. No obsolete Rust-to-mpv callback dependency remains.
5. No forbidden network protocol or dependency entered.
6. No unintended runtime library was added.
7. The old `meson.build` player compilation is gone.
8. Release packages hold only intended runtime components.

Acceptance:

```sh
cargo test --manifest-path rust/Cargo.toml && tests/run.sh suite && tests/run.sh build && tests/run.sh rust-link && tests/run.sh release && scripts/audit.sh && scripts/measure.sh final
```

plus in the Alpine container `tests/run.sh suite` and `scripts/audit-linux.sh`. Budgets pass. Both artifacts build from the Rust implementation.

## Standing verification

Run the relevant gates after each slice. Never present a test passing against the legacy binary as evidence for the Rust engine. The harness must operate on the new Rust executable and release artifacts by S8 at the latest.

Add tests for: decoder and demux ownership, zero-copy hardware resource lifetimes, timestamp and clock correctness, command/property/event consistency, shutdown race conditions, IPC and Lua input validation, corrupt metadata and malformed files, repeated open and close cycles, CPU and software-rendering fallback, audio underflow with pause and seek, renderer context loss and recreation, build reproducibility and closure validation.

Differential testing runs while the legacy engine exists. Run both implementations against the same fixtures and compare: tracks and chapters, metadata, command results, event sequences, audio PCM within defined tolerances, decoded video frames or hashes where apt, seek behavior, EOF, exit codes, failures. Timing and float behavior get tolerance bands. Never byte-exact async timing. Keep a regression fixture for each new failure found.

Static analysis on every slice: `cargo fmt` stays clean, Clippy stays clean, dependency audit stays clean, sanitizers run where available. Review each native wrapper for double-free, use-after-free, dangling callbacks, and thread-affinity errors. Avoid large generated bindings for APIs the player barely uses.

Repeat-failure guards (a slice that trips one stops and fixes the cause before retrying):

- Every new command path goes through the Rust dispatcher with a cross-interface equivalence test. The node-command bypass never recurs.
- Every new `unsafe` block states its safety argument and names its audit. No new C glue mirrors an old mpv wrapper.
- No timestamp-exact assertion on asynchronous behavior. New flaky timing test gets rewritten with stated tolerances, not widened sleeps.
- No compatibility path survives a removal slice. Removals carry no alias, shim, or fallback reader.
- The negative audit names actual sources and link manifests. A symbol-name ban that fires on third-party libraries gets rewritten.
- No copy at a Rust/native boundary without a comment-free justification in the commit body: measured, bounded, or removed.

## Performance and footprint

A slower or larger port is not automatically acceptable.

- S0 re-measures the baseline with `scripts/measure.sh final` and records it in `docs/RUST-PORT.md`. The Baseline numbers above are history.
- Compare with `scripts/measure.sh compare` and `scripts/measure.sh cpu`. Target at most 10% regression in latency-sensitive paths after accounting for spread.
- Hard gates (distributable size, executable size, first frame, RSS, seek, zero network schemes, one Lua interpreter) pass at S0 and S8. Between them they are measured and logged per slice.
- Never game a benchmark by disabling decoding, presentation, subtitles, hardware acceleration, or error handling.
- If `perf-gate-ready` reports a busy host, record the run as deferred and rerun under valid conditions. A deferred run never counts as a pass.
- Keep the single-process design. Eliminate avoidable copies at Rust and native boundaries.

## Build and release

macOS keeps: `Player.app`, arm64 only, no Homebrew runtime dependency, bundled native libraries and MoltenVK, correct dylib identities and rpaths, ad-hoc signing with signature verification, native event handling, checksum and packaging workflow.

Linux keeps: `player-x86_64-linux-musl.tar.gz`, Alpine and musl only, Wayland, Vulkan, ALSA, VA-API, minimal shared-library set, no X11, no glibc build, no extra audio server, stripped executable with audited dynamic dependencies.

Prefer source-pinned dependencies. No new system-package resolution and no new vendored tree without clear necessity. Update GitHub Actions so both platforms test the Rust engine and build their artifacts. Never publish a release tag. Never notarize.

## Subagents and commits

Independent research, design probes, and test work may go to subagents: fixture analysis, FFmpeg binding design, audio-output implementation, libplacebo and Vulkan research, Swift bridge, Wayland and VA-API bridge, differential harness, source and dependency audit. Shared core types, event semantics, and resource ownership stay with one integration owner. Parallel writers get disjoint files and parent-owned worktrees. Never integrate speculative parallel code without testing its compatibility.

Commits are Conventional Commits with the workstream as scope, for example `feat(engine): ...`, `feat(demux): ...`, `feat(audio): ...`, `feat(video): ...`, `feat(render): ...`, `refactor(lua): ...`, `refactor(platform): ...`, `refactor(build): ...`, `refactor(port)!: ...`. The subject is imperative, lowercase, at most 72 characters, no trailing period. The body says why. The slice ID goes in the body. A contract change takes `!` or a `BREAKING CHANGE:` footer. Land green slices on the integration branch. No force pushes. No fixing CI in this campaign: reproduce locally and fix locally.

## Definition of done

- [ ] Rust owns the application entry point and playback lifecycle.
- [ ] Rust owns commands, properties, events, playlists, and state.
- [ ] Rust owns demux and decode orchestration with track selection.
- [ ] Rust owns scheduling, seeking, clocks, and buffering.
- [ ] Rust integrates native audio and GPU output.
- [ ] GUI and TUI share one Rust playback engine.
- [ ] Supported Lua behavior works.
- [ ] Supported local playback is preserved, minus clipboard.
- [ ] No mpv-derived C playback source remains in either build.
- [ ] No embedded or external mpv engine remains.
- [ ] FFmpeg stays tightly allowlisted and network-free.
- [ ] macOS release builds and automated tests pass.
- [ ] Linux x86-64 musl release builds and automated tests pass.
- [ ] Hard size, performance, and dependency gates pass.
- [ ] New scheduler, lifecycle, and FFI tests pass.
- [ ] Build and release docs describe the Rust implementation.
- [ ] Retained native dependencies and copied code carry correct license notices.
- [ ] No known reproducible correctness or memory-safety regression remains.
- [ ] All changes are committed. No release is published.

The final report separates automated verification, actual hardware verification, and tests that never ran. Headless CI never stands in for physical GPU, audible output, color accuracy, or user-interaction checks.

## Needs environment

These stay listed for the owner. They never block local completion.

- Real-GPU VA-API decode: `tests/run.sh linux-vaapi` needs a render node. Linux suite covers `vaapi-copy` headless and the default `auto` path with and without a node.
- macOS VideoToolbox hardware paths, audible CoreAudio output, HDR and 10-bit color, subtitle shaping rendering, Retina scaling, real mouse and drag, Finder and Dock behavior: `docs/manual-checklist.md` owner runs.
- Ad-hoc signature verification on a clean machine. Notarization is out of scope.
- Sustained-load audio underrun watch on real ALSA hardware beyond the null-PCM gate.

## Execution boundary and non-goals

Authorized: inspect and modify the repo, introduce and consolidate crates, change the build, remove obsolete mpv files and config, write and run tests/benchmarks/sanitizers/audits, delegate to subagents, commit locally.

Forbidden: pushing, pull requests unless repo permissions require them, force-pushes, rewriting published history, overwriting concurrent work, publishing a release tag, touching external infrastructure, deploying anything, notarizing.

Hard stops (park the item with cause and retry condition): an irreversible or outside-the-repo action, a boundary this doc forbids, a loop that cannot check anything, missing authority for an external effect.

Non-goals: the follow-on list in Decisions, any UI redesign, any new media framework, any encoder or recording path, any network capability, any second Lua interpreter, automatic user config or script loading.

Contract changes for the owner:

1. The `player` executable comes from Cargo. Meson no longer compiles a player engine.
2. The mpv client API adapter is removed. The IPC wire format and supported Lua behavior stay identical.
3. Clipboard support is removed in full (owner decision 2026-11-03).
4. `docs/RUST-PORT.md` is added as the durable resume record.
5. No slice pushes. The campaign closes on accepted local commits.

## Reference

Verified code map at preflight HEAD `d7a5d1b`:

- Startup and control: `player/main.c`, `rust/player-core/src/config.rs`, `command.rs`, `state.rs`, `playlist.rs`, `input.rs`, `local_file.rs`, `rust/player-ffi/src/startup.rs`, `command.rs`, `state.rs`, `playlist.rs`, `local_file.rs`, `mpv.rs`.
- Playback pipeline: `player/`, `demux/demux_lavf.c`, `demux/demux_mkv.c`, `demux/demux_mkv_timeline.c`, `demux/demux_timeline.c`, `demux/demux_playlist.c`, `video/`, `audio/`, `filters/`, `sub/`, `stream/stream_file.c`.
- Audio bridge: `rust/player-audio/src/lib.rs`. Script host: `rust/player-lua/src/host.rs`, `backend.rs`, `queue.rs`, `value.rs`, `utils.rs`. TUI: `rust/player-tui/src/term.rs`, `canvas.rs`, `art.rs`.
- Release: `scripts/build.sh`, `scripts/release.sh`, `scripts/build-deps.sh`, `scripts/bundle.py`, `scripts/release-linux.sh`, `scripts/build-deps-linux.sh`, `scripts/linux-container.sh`, `scripts/audit.sh`, `scripts/audit-linux.sh`, `scripts/measure.sh`, `scripts/ffmpeg-manifest.sh`, `scripts/network-schemes.txt`, `Makefile`.
- Contract docs: `docs/scope.md`, `docs/architecture.md`, `docs/lua-api.md`, `docs/linux.md`, `docs/manual-checklist.md`, `docs/ffmpeg.md`, `docs/metrics.md`, `docs/status.md`, `docs/baseline.md`, `docs/licenses.md`, `TODO.md`.
- Test entry: `tests/run.sh`, `tests/harness.py` (gates include `suite`, `corpus`, `audio`, `play`, `seek`, `cli`, `input`, `playlist`, `state`, `local-only`, `local-paths`, `osc`, `osc-input`, `lua-host`, `lua-api-audit`, `tui`, `hwdec`, `linux-gpu-next`, `linux-vaapi`, `linux-audit`, `build`, `rust-link`, `ffi`, `release`, `failures`, `baseline-docs`, `meta`, `fixtures`), `tests/fixtures/gen.sh`, `tests/local_only.py`, `tests/wayland.py`.
