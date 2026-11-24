Prefix: PLAY
Next: 70

# Backlog

Priorities are assigned by impact and likelihood. Source-confirmed paths are distinguished from hardware checks and historical measurements that remain unproven on the current build.

## P2

### PLAY-21 · P2 · Terminal-launched macOS keyboard focus still needs owner verification

- **Area:** Operability
- **Where:** `crates/player-display/bridge/metal_window.m:68`, `crates/player-display/bridge/metal_window.m:380`.
- **Problem:** Unproven on the current build: the previously reported terminal-launch focus failure has not been checked in a real owner session after the AppKit main-loop change. The bridge now dispatches window creation to the main thread, but that source fact does not establish foreground activation. An agent session typing into another app cannot reproduce the intended launch interaction reliably. The owner could not run the interactive check during the 2026-11-12 fix session. No focus-policy change was made without a reproduction.
- **Recommendation:** On an Apple Silicon Mac, launch build/mpv FILE from the terminal and verify immediate keyboard control under both Metal and Vulkan. Record the result; if it fails, trace activation/focus at launch before changing policy. Delete this ticket if the current behavior passes.
- **Fix with:** `os-boundaries`
- **Effort:** S

### PLAY-32 · P2 · Complete the direct VideoToolbox presentation comparison

- **Area:** Performance
- **Where:** `crates/player-output/src/context.rs`, `crates/player-output/src/iosurface.rs`, `crates/player-display/src/mac.rs`, `scripts/patches/libplacebo-metal.patch`.
- **Problem:** Metal imports supported VideoToolbox NV12 and P010 IOSurfaces directly. Retained surfaces and use counts survive GPU execution in a four-entry lease queue. Shutdown drains leases. Vulkan, unsupported imports, and explicit `videotoolbox-copy` use software transfer. Import fallback renders into the acquired drawable before its single submission. Native NV12 and P010 pixel comparisons pass through decoder teardown, repeated buffer reuse, seek, GPU shutdown, explicit copy conversion, and same-target fallback. The full macOS Rust workspace and affected Linux packages pass. The interleaved windowed CPU/RSS comparison was stopped when the owner asked to use the computer. No performance gain is claimed. Keep this ticket open until a qualified comparison is accepted.
- **Recommendation:** Run windowed tests only when the owner permits them. The preserved baseline is `.work/tmp/iosurface-baseline/Player.app/Contents/MacOS/player`; use identical `--hwdec=videotoolbox --gpu-api=metal --ao=null` settings for baseline and candidate. The existing frame harness has prepared `video-1080p` and `video-4k` fixtures. Compare at least five interleaved runs per side, including drops and RSS.
- **Fix with:** `make-it-fast`
- **Effort:** S

## P3

### PLAY-26 · P3 · SubRip formatting is discarded instead of rendered

- **Area:** Types
- **Where:** `crates/player-engine/src/sub.rs:208`, `crates/player-overlay/src/text.rs:35`.
- **Problem:** markup_free strips SubRip italic, bold, underline, strike, and font-color tags. TextBlock carries one style for the whole block, so the native text backends cannot preserve per-span formatting. The current limitation is deliberate in the parser but loses presentation information from supported subtitle files.
- **Recommendation:** Parse the supported SubRip tags into typed text spans and carry attributes through Core Text ranges and HarfBuzz runs, retaining plain-text fallback for malformed markup. Verify mixed styles and color within one line without broadening the SubRip-only format boundary.
- **Fix with:** `typing-audit`
- **Effort:** M

### PLAY-27 · P3 · Start alignment does not follow a paragraph's base direction

- **Area:** Types
- **Where:** `crates/player-overlay/src/display.rs:74`, `crates/player-overlay/src/text/mac.rs:328`, `crates/player-overlay/src/text/linux.rs:1071`.
- **Problem:** Align::Start always resolves to zero offset and both backends position lines with that result, so an RTL paragraph starts at the left edge. Centered subtitles avoid the issue, but start-aligned OSD messages and OSC titles do not.
- **Recommendation:** Resolve logical Start/End against each paragraph's base direction before placing its lines, consistently on both text backends. Verify mixed LTR/RTL paragraphs, wrapped lines, and the unchanged centered-subtitle path.
- **Fix with:** `make-invalid-states-unrepresentable`
- **Effort:** M

### PLAY-28 · P3 · One tall fallback glyph changes spacing throughout a text block

- **Area:** Types
- **Where:** `crates/player-overlay/src/text/mac.rs:235`, `crates/player-overlay/src/text/mac.rs:286`, `crates/player-overlay/src/text/linux.rs:1009`, `crates/player-overlay/src/text/linux.rs:1062`.
- **Problem:** Both text backends compute one block-wide maximum ascent/descent across fallback fonts, then use it as every line's spacing. One tall CJK or Thai fallback glyph therefore spreads otherwise unchanged lines apart; the size depends on the system fallback fonts.
- **Recommendation:** Base line spacing on the primary font's metrics while allocating enough ink bounds/inset for tall fallback glyphs, so layout stays stable without clipping. Verify a multi-line block with a single tall fallback glyph on each backend and preserve wrapping and outline bounds.
- **Fix with:** `typing-audit`
- **Effort:** M

### PLAY-30 · P3 · Windowed OSC input checks lack an agreed coordinate contract

- **Area:** Tests
- **Where:** `crates/player-display/bridge/metal_window.m:237`, `crates/player-app/src/app.rs:811`, `crates/player-app/src/app.rs:1430`, `tests/osc_input.py:61`.
- **Problem:** The earlier Retina run reported osc-areas in 2560x1440 backing pixels where the gate expected a 1280x720 layout, plus IPC pointer motion that did not reveal or hide the OSC. The current bridge converts native motion to backing pixels and real/injected input reach the app handlers; the current windowed failure remains unproven on hardware. Changing API units without a contract could break clients while merely hiding a test mismatch.
- **Recommendation:** Resolve the coordinate and injected-input contract in the linked draft, then reproduce on a real Retina window for both backends. Derive expected hit regions from the actual output size and separately test motion-driven reveal/hide; keep headless coverage and verify 1x as well as 2x scaling.
- **Fix with:** `test-audit`
- **Effort:** M
- **Design:** [Draft RFC](docs/rfc/PLAY-30-osc-coordinates.md)

### PLAY-33 · P3 · Metal swapchain output is limited to 8-bit SDR

- **Area:** Architecture
- **Where:** `scripts/patches/libplacebo-metal.patch:2308`.
- **Problem:** The carried Metal swapchain fixes CAMetalLayer to BGRA8Unorm and does not establish an EDR/HDR output contract. Supporting a 10-bit input alone (PLAY-47) does not provide wide-precision or HDR display output.
- **Recommendation:** Define and implement the layer pixel format, extended-range content setting, EDR color space, and matching libplacebo swapchain color metadata together. Preserve an explicit SDR fallback and verify tone mapping and output range on a suitable display before claiming HDR support.
- **Fix with:** `api-surface-and-module-boundaries`
- **Effort:** L

### PLAY-40 · P3 · The hover benchmark does not establish its intended pointer rate

- **Area:** Performance
- **Where:** `scripts/bench.py:628`, `crates/player-app/src/app.rs:705`, `crates/player-app/src/app.rs:877`.
- **Problem:** The recorded hover run achieved about 6 Hz rather than the intended 60 Hz, with roughly 150 ms per synchronous IPC mouse command. The harness waits for each reply before its 1/60-second sleep; its JSON already reports the achieved pointer_rate_hz. The current bottleneck is unproven: a blocking present and harness waiting are candidates, not established causes. Historical CPU comparisons need this workload qualification.
- **Recommendation:** Measure command round-trip time on a quiet qualified host and use the existing pointer_rate_hz result to establish the workload. Trace the delay through IPC polling and present before changing either the app or harness, and retain the achieved rate in performance comparisons. If app latency is confirmed, fix it separately from benchmark pacing and compare equivalent input workloads.
- **Fix with:** `make-it-fast`
- **Effort:** M

### PLAY-41 · P3 · Fixture generation validates the FFmpeg version but not its encoders

- **Area:** Tests
- **Where:** `tests/fixtures/gen.sh:12`, `tests/fixtures/gen.sh:21`, `tests/fixtures/gen.sh:38`.
- **Problem:** Any binary reporting ffmpeg version 9.0.2 passes the preflight, including builds without the corpus's encoders. The generator then removes the previous fixture directory and fails later on options such as -preset, leaving an incomplete corpus. The earlier claim that every later gate necessarily times out was not established and is removed.
- **Recommendation:** Check the exact encoders and required generator capabilities before deleting or replacing the fixture directory, with an actionable message naming what is missing. Generate into a temporary directory and publish only a complete validated corpus. Exercise a same-version encoder-deficient stub and assert that existing fixtures survive.
- **Fix with:** `error-model`
- **Effort:** S

### PLAY-51 · P3 · CoreAudio buffer-allocation failure disposes the queue twice

- **Area:** Concurrency
- **Where:** `crates/player-audio/src/backend_coreaudio.rs:198`, `crates/player-audio/src/backend_coreaudio.rs:213`, `crates/player-audio/src/backend_coreaudio.rs:425`.
- **Problem:** Once CoreAudioSink owns the queue, an AudioQueueAllocateBuffer failure manually calls AudioQueueDispose and returns. Dropping the already-constructed sink calls stop_streaming and AudioQueueDispose on the same handle again. The duplicate cleanup is source-confirmed, but the allocation-failure path and its native outcome were not exercised on macOS.
- **Recommendation:** Transfer cleanup responsibility exactly once when constructing the sink and let its owner handle subsequent failures. Add an injected allocation failure after ownership transfer and assert one disposal, covering failure after an earlier buffer allocation as well.
- **Fix with:** `os-boundaries`
- **Effort:** S

### PLAY-52 · P3 · Seek preroll reaches EOF without finishing the video decoder

- **Area:** Concurrency
- **Where:** `crates/player-engine/src/sched.rs:559`, `crates/player-engine/src/sched.rs:810`, `crates/player-engine/src/sched.rs:945`, `crates/player-engine/src/video.rs:183`.
- **Problem:** Normal demux EOF calls video.finish, which signals end-of-input to the decoder. EOF inside command_seek only sets eof_seen, and the subsequent draining path calls video.drain, which only receives available frames. A decoder holding delayed frames can therefore miss its tail or leave a paused seek on an old frame. Unproven with a real delayed-frame file in this environment; the inconsistent EOF control flow is confirmed.
- **Recommendation:** Share an explicit, once-only decoder finish transition between normal playback EOF and seek preroll EOF, then drain its frames before declaring the seek complete. First reproduce with the existing delayed-decoder simulation and a short B-frame fixture near EOF, both paused and playing; avoid broad scheduler changes if the real trigger is refuted.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** S

### PLAY-53 · P3 · Large Finder open batches silently lose files

- **Area:** Concurrency
- **Where:** `crates/player-display/bridge/metal_window.m:44`, `crates/player-display/bridge/metal_window.m:57`, `crates/player-display/bridge/metal_window.m:108`, `crates/player-display/bridge/metal_window.m:373`.
- **Problem:** The 64-slot input ring holds only 63 events and silently drops the next event when full. Early Finder opens accumulate in an array, then window creation enqueues the entire batch while the playback caller is blocked in dispatch_sync and cannot drain it. A batch of more than 63 valid local files therefore loses entries, while the Finder delegate reports success. This is deterministic from the queue/control flow; Finder was not exercised here.
- **Recommendation:** Deliver file-open batches through a representation that preserves the complete batch, or surface an explicit failure before claiming success. Keep any coalescing limited to input for which loss is acceptable. Verify a startup batch larger than the ring and repeated batches while ordinary input is pending.
- **Fix with:** `lifecycle-concurrency`
- **Effort:** M

### PLAY-54 · P3 · GUI letter bindings use US physical key positions

- **Area:** Operability
- **Where:** `crates/player-display/src/display.rs:203`, `crates/player-app/src/window.rs:103`, `crates/player-app/src/window.rs:157`, `crates/player-app/src/window.rs:177`.
- **Problem:** Wayland discards the keymap and forwards keycodes; macOS drops the key event's character/modifier information. Both gui_key_name tables map fixed physical positions to US letters. A non-US layout therefore does not follow the displayed letter bindings; on AZERTY the US-Q position is labeled A but still invokes quit. TUI text input follows a different path.
- **Recommendation:** Translate printable keys through the active platform layout while retaining explicit handling for navigation and other non-text keys. On Wayland retain and interpret the owned keymap; on macOS use the event's translated character information. Verify quit, mute, and OSC controls with US and a non-US layout, including modifier behavior.
- **Fix with:** `os-boundaries`
- **Effort:** M

### PLAY-56 · P3 · Python unit tests are outside make test and CI, and one file costs 235 s

- **Area:** Tests
- **Where:** tests/test_*.py, scripts/test_perfstat.py, Makefile, .github/workflows/
- **Problem:** make test runs tests/run.sh suite and cargo test, but not tests/test_build_deps.py, test_build_flags.py, test_bundle.py, test_linux_audit.py, test_linux_container.py, test_native_rpaths.py or scripts/test_perfstat.py. CI runs only tests/test_release_archives.py. Measured here: python3 -m unittest discover -s tests -p 'test_*.py' took 235 s (16 tests, 5 skipped), nearly all in test_build_deps.py running cargo build --offline; scripts/ took 0.6 s (20 tests). Both pass.
- **Recommendation:** In the Tests step for the harness component, add the fast files to make test and either shrink test_build_deps.py or keep it behind a separate target; record the cost against the test budget.
- **Fix with:** test-audit
- **Effort:** S

### PLAY-57 · P3 · The IPC socket reads unbounded input and rescans it on every poll

- **Area:** Concurrency
- **Where:** crates/player-app/src/ipc.rs:29-72 (IpcServer::poll)
- **Problem:** poll appends everything the client sends to client.buffer with no cap and searches the whole buffer for a newline on every call, so a client that never sends a newline grows memory and makes each poll O(buffer). Reproduced on build/mpv with --input-ipc-server and a local client writing 'A' bytes in 1 MiB writes (script in the finding's scratch dir, .work/tmp/r/ipc_nl.py): sending 4 MiB took 1.2 s, 16 MiB 7.3 s, 32 MiB 43.2 s (RSS 12 MB to 56 MB, superlinear), and the next valid request got no reply within 8 s because it was glued to the junk line. A valid get_property answered in under 0.1 s before the flood. The socket is created with the user's umask, so any local process that can open it can do this. A 1 MiB flood of bare newlines took 6.6 s and then recovered; a quadratic drain was not demonstrated.
- **Recommendation:** Cap one line (for example 1 MiB): past the cap discard to the next newline and reply with an error, or close the client. Scan only the new bytes for the newline. Add an ipc.rs test that feeds an oversized line and asserts the bound and that a later valid line is answered; prove it with the cap removed.
- **Fix with:** security
- **Effort:** S

### PLAY-58 · P3 · File names with control characters reach the terminal unsanitized

- **Area:** Security
- **Where:** crates/player-app/src/app.rs:445 (Playing: line), app.rs:330 and :382 (failed to open), expand_message for ${filename}
- **Problem:** println!("Playing: {}", entry.path) and the open-failure message print the path raw. Reproduced: cp t.flac $'x\e[31mevil\e]0;pw\a.flac' then build/mpv --no-config --vo=null --ao=null that file | cat -v shows 'Playing: x^[[31mevil^[]0;pw^G.flac', and a missing $'nofile\e[2Jx.mp3' prints '^[[2J' in 'failed to open'. A directory argument or an m3u entry can name such a file, so a downloaded folder or playlist can inject cursor, clear and title sequences. The TUI is safe (Canvas::put maps controls to spaces; tested) and container title, artist and stream-title tags carrying ESC were not printed by the plain player in my probes (chapter titles were not probed).
- **Recommendation:** Escape or replace C0, C1 and DEL characters in every path or tag string the plain player writes (one helper next to expand_message), keeping the file path itself untouched for opening. Test the Playing line, the failed-to-open line and ${filename}.
- **Fix with:** security
- **Effort:** S

### PLAY-59 · P3 · parse_srt timestamp arithmetic is unchecked: dev builds kill the scheduler, release builds wrap

- **Area:** Resource bounds
- **Where:** crates/player-engine/src/sub.rs:144-161 (parse_timestamp), :122 (feed: start_ms + duration_ms)
- **Problem:** parse_timestamp parses unbounded digit strings into i64 and computes ((hours * 60 + minutes) * 60 + seconds) * 1000 without checks. Reproduced: printf '1\n99999999999999:00:00,000 --> 99999999999999:00:01,000\nboom\n' > big.srt, then build/mpv --no-config --vo=null --ao=null --sub-file=big.srt a.wav prints 'player: scheduler worker died' (dev profile: overflow checks on, the worker thread panics). dist/Player.app with the same arguments exits normally because release wraps, so the shipped build shows a garbage cue time rather than rejecting the file. EngineSub::feed has the same sum for an embedded packet with a huge duration (not reproduced).
- **Recommendation:** Use checked arithmetic and treat an out-of-range timestamp as a malformed cue (skipped, as parse_srt already skips bad cues); cap hours at a sane value. Add a parse_srt test for 99999999999999:00:00,000 that fails before the change.
- **Fix with:** error-model
- **Effort:** S

### PLAY-60 · P3 · Playlist and subtitle readers load whole files with no size bound

- **Area:** Resource bounds
- **Where:** crates/player-app/src/playlist.rs:175 (read_to_string), crates/player-engine/src/sub.rs:90 (fs::read)
- **Problem:** Reproduced for playlists: build/mpv --no-config --vo=null --ao=null --playlist=/dev/zero reached 6.5 GB RSS after 6 s and 7.0 GB after 12 s with no error. A regular multi-gigabyte file or a FIFO behaves the same; the player never checks the size or file type first. An .srt passed to sub-file or found next to a video is read the same way (not measured separately). Other bounds checked and fine: MAX_STREAMS, MAX_CHAPTERS, MAX_PACKET_BYTES and metadata caps in player-media, 32k-entry playlists (ready in 50 ms, 27 MB), speed clamped to 0.01..100.
- **Recommendation:** Open the file, require a regular file, and read at most a fixed cap (for example 16 MiB for playlists and 4 MiB for subtitles) through Read::take, failing with a clear error past it. Test with a file one byte over the cap.
- **Fix with:** os-boundaries
- **Effort:** S

### PLAY-61 · P3 · player-app uses unsafe zlib FFI but AGENTS.md doesn't list it among the unsafe crates

- **Area:** Security
- **Where:** crates/player-app/src/png.rs:3, AGENTS.md
- **Problem:** png.rs declares unsafe extern "C" compress2 from zlib, but AGENTS.md's list of crates allowed unsafe code omits player-app.
- **Recommendation:** Either list player-app (with the zlib boundary) in AGENTS.md or replace the FFI with a safe encoder.
- **Fix with:** os-boundaries
- **Effort:** S

### PLAY-62 · P3 · The meta gate is red at base and runs in neither suite

- **Area:** Tests
- **Where:** tests/harness.py:861, scripts/fetch-gluon.sh, meta gate
- **Problem:** Its $TMPDIR pattern matches scripts/fetch-gluon.sh and tests/harness.py:861, and its .git/info/exclude check fails in linked worktrees, so it fails at base; it is outside SUITE and LINUX_SUITE so nothing notices.
- **Recommendation:** Fix the two false positives (resolve the common git dir for worktrees) and add meta to a suite.
- **Fix with:** root-cause
- **Effort:** S

### PLAY-63 · P3 · perfstat ProcessCpuTests need 50 ms of CPU inside a 100 ms window

- **Area:** Tests
- **Where:** scripts/test_perfstat.py (ProcessCpuTests)
- **Problem:** The tests assume the process gets 50 ms of CPU within 100 ms of wall time; on a busy host they failed once.
- **Recommendation:** Measure CPU with a work-bounded loop or a generous wall bound, so scheduling latency can't fail them.
- **Fix with:** root-cause
- **Effort:** S

### PLAY-64 · P3 · Linux-only expects block a presentation-wide expect_used deny

- **Area:** Types · presentation
- **Where:** crates/player-overlay/src/text/linux.rs:690, :810, :811, :1052; crates/player-overlay tests/present.rs helpers
- **Problem:** The playback crates deny unwrap/expect, but Linux-only presentation production code still uses expect. The Linux edits to text/linux.rs and AVERROR_EAGAIN in player-media/src/sys.rs and the playback lint profile were not compiled in the recorded macOS verification.
- **Recommendation:** In the musl container, run make lint to verify the playback and presentation Linux code and lint profiles, replace the Linux expects, and extend the unwrap/expect deny to the presentation crates. Run the Linux typography and Wayland presentation tests where their required environment is available.
- **Fix with:** typing-audit
- **Effort:** S

### PLAY-65 · P3 · Public playback and presentation items with no callers

- **Area:** Dead code · consumer questions
- **Where:** AudioClock::is_paused, Resampler::buffered_input_frames, PlaybackState::supports_bool and supports_number, VideoFrame::pts_ticks, OverlayImage::alpha_at, Anchor::CENTER; output_latency and render_rgba (test callers only)
- **Problem:** These public items have no workspace callers (two only test callers), so they are either API for external consumers or dead code.
- **Recommendation:** Decide per item: remove, or keep as documented API with a test that exercises the contract.
- **Fix with:** dead-code-archaeology
- **Effort:** S

### PLAY-69 · P3 · Verify windowed and audio paths after renderer changes

- **Area:** Tests
- **Where:** crates/player-app/src/app.rs; tests/harness.py:436; docs/manual-checklist.md
- **Problem:** The screenshot renderer reuse change was compiler-checked but its windowed gate was deferred. The existing screenshot case takes one screenshot, so it does not establish reuse. The GUI, GPU API, TUI, and CoreAudio gates were also deferred during the recorded verification.
- **Recommendation:** When window and audio testing is permitted, run the GUI, GPU API, TUI and audio gates on macOS. Extend the screenshot case to take repeated screenshots and verify correct output through renderer reuse; investigate any failures.
- **Fix with:** test-audit
- **Effort:** S

## P4

### PLAY-66 · P4 · ALSA write path may fail playback on EINTR and loop on repeated EPIPE

- **Area:** Needs environment · Linux audio
- **Where:** crates/player-audio/src/backend_alsa.rs:195-225 (AlsaSink::write)
- **Problem:** Source reading only, not reproduced: this host is macOS and the ALSA null PCM needs the Linux container. write treats every negative snd_pcm_writei result other than EPIPE and ESTRPIPE as fatal, so EINTR (-4, for example from a signal delivered to the process during a blocking write) ends the audio path instead of retrying. The EPIPE branch calls snd_pcm_prepare and continues with no bound, so a device that returns EPIPE on every write spins the thread. snd_pcm_pause failing on a device without pause support surfaces as an error with no snd_pcm_drop fallback. None of these has a test: linux-audio runs only the happy path on alsa/null.
- **Recommendation:** In the Linux container with alsa/null, inject the three cases with a fake pcm layer or the null device's error hooks and reproduce before changing code; retry EINTR, bound consecutive EPIPE recoveries, and decide the pause fallback with the owner.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

### PLAY-67 · P4 · Small dead-code and safety leftovers from the application and harness pass

- **Area:** Dead code
- **Where:** crates/player-app (Osc::overlay, Rendered::full), scripts/install-layout.sh:43, scripts/build-deps*.sh, tests/harness.py gates
- **Problem:** Osc::overlay and Rendered::full are test-only; install-layout.sh:43 runs rm -rf "$prefix/lib" with no empty-prefix check; GLSLANG_REV, SPIRV_HEADERS_REV and SPIRV_TOOLS_REV are dead in both build-deps scripts (removing them costs one CI cache rebuild); the play, mpv-diff, meta and build gates have no documented caller; a missing playlist entry still exits 1 at startup where mpv skips it.
- **Recommendation:** Address together in one cleanup change that accepts the cache rebuild; guard the rm with a non-empty prefix check first.
- **Fix with:** dead-code-archaeology
- **Effort:** S

### PLAY-68 · P4 · Playback leftovers: double observe in sched step, unproven decoder finish case, VideoToolbox lease witness

- **Area:** Tests · playback
- **Where:** crates/player-sched (sched.rs step), player-audio decoder finish(), VideoToolbox import ring test
- **Problem:** step calls observe(now) twice (idempotent, harmless); finish() on a decoder with no cursor now discards stray samples instead of panicking but reachability is unshown; an injected early-lease-release fault still passes the VideoToolbox import-ring test, so that path has no witness.
- **Recommendation:** Drop the redundant observe; show or rule out the finish case; add a test that fails when leases are released early.
- **Fix with:** test-audit
- **Effort:** S
