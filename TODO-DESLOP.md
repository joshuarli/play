Prefix: DSLP
Next: 26

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier D). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-17 (the per-component tickets that replaced DSLP-3 to DSLP-5), work the open P1 and P2 findings, then fill in `## Report` below.

## Report

Not started.

## Tickets

### DSLP-6 · P2 · Types: playback pipeline

- **Area:** deslop · types
- **Where:** crates/player-core, crates/player-media, crates/player-audio, crates/player-engine, crates/player-backend
- **Problem:** Component of 14.2k first-party lines (Rust; vendored Symphonia, ruopus and rusty_aac excluded). Contracts: the FFmpeg FFI boundary in player-media (sys.rs, session.rs, video.rs: unsafe blocks, MAX_* bounds, IO callbacks), local-only input classification (player-core::input), the Symphonia and rusty_aac decode bridge in player-audio, the scheduler, clock and state machine in player-engine, and the Backend trait in player-backend. Consumers: player-app, player-tui, player-gui, and the corpus, audio, seek and failures gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Types step on this component: measure the strict-profile candidates (clippy wildcard_enum_match_arm, cast_possible_truncation, cast_sign_loss, unwrap_used and expect_used outside tests, redundant_clone, needless_pass_by_value; Python ty and Ruff candidates for Python files), and adopt only those that reject a real defect or delete an escape hatch at no more than one false positive per 5k lines. Accept: clean at the adopted profile, escape hatches (rg count from the tools reference) not increased, each fixed contract has a witness. Keep unsafe_code policy as the workspace states it: native-boundary unsafe stays in the crates AGENTS.md lists. Acceptance command: cargo test -p player-core -p player-media -p player-audio -p player-engine -p player-backend, then tests/run.sh corpus, then tests/run.sh audio, then tests/run.sh seek, then tests/run.sh failures.
- **Fix with:** deslop
- **Effort:** M

### DSLP-7 · P2 · Types: presentation

- **Area:** deslop · types
- **Where:** crates/player-output, crates/player-display (including bridge/metal_window.m), crates/player-overlay
- **Problem:** Component of 8.1k first-party lines (Rust and one Objective-C file). Contracts: the libplacebo and Metal/Vulkan FFI in player-output, the window, input ring and Wayland/Cocoa surface in player-display, and the overlay display list, rasterizer and text shaping in player-overlay; each unsafe block states its safety argument. Consumers: player-engine (frame hand-off), player-app, player-gui, and the gpu-api, osc and gui gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Types step on this component: measure the strict-profile candidates (clippy wildcard_enum_match_arm, cast_possible_truncation, cast_sign_loss, unwrap_used and expect_used outside tests, redundant_clone, needless_pass_by_value; Python ty and Ruff candidates for Python files), and adopt only those that reject a real defect or delete an escape hatch at no more than one false positive per 5k lines. Accept: clean at the adopted profile, escape hatches (rg count from the tools reference) not increased, each fixed contract has a witness. Keep unsafe_code policy as the workspace states it: native-boundary unsafe stays in the crates AGENTS.md lists. Acceptance command: cargo test -p player-output -p player-display -p player-overlay, then tests/run.sh gpu-api, then tests/run.sh osc (GPU and window checks need a display: Needs environment where absent).
- **Fix with:** deslop
- **Effort:** M

### DSLP-8 · P2 · Types: application surface

- **Area:** deslop · types
- **Where:** crates/player-app, crates/player-tui, crates/player-gui, crates/player-build
- **Problem:** Component of 9.3k first-party lines. Contracts: the mpv-compatible CLI (options.rs, --help and exit codes), the command and property surface, the JSON IPC socket (ipc.rs), playlist and directory loading (playlist.rs), the OSC (osc.rs), the terminal UI, and the build-script helpers in player-build. Consumers: the mpv-compatible users of build/mpv and Player.app, and the cli, cli-help, input, playlist, state, dispatch, osc-input and tui gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Types step on this component: measure the strict-profile candidates (clippy wildcard_enum_match_arm, cast_possible_truncation, cast_sign_loss, unwrap_used and expect_used outside tests, redundant_clone, needless_pass_by_value; Python ty and Ruff candidates for Python files), and adopt only those that reject a real defect or delete an escape hatch at no more than one false positive per 5k lines. Accept: clean at the adopted profile, escape hatches (rg count from the tools reference) not increased, each fixed contract has a witness. Keep unsafe_code policy as the workspace states it: native-boundary unsafe stays in the crates AGENTS.md lists. Acceptance command: cargo test -p player-app -p player-tui -p player-gui -p player-build, then tests/run.sh cli, then tests/run.sh cli-help, then tests/run.sh playlist, then tests/run.sh state.
- **Fix with:** deslop
- **Effort:** M

### DSLP-9 · P2 · Types: test harness and scripts

- **Area:** deslop · types
- **Where:** tests/, scripts/, docker/, native/
- **Problem:** Component of 9.6k first-party lines (Python and shell). Contracts: tests/run.sh and tests/harness.py (the gate list, fixture manifest, trace format), the Python unit tests, scripts/build*.sh, release*.sh, audit*.sh, bundle.py, measure.sh, bench.py, perfstat.py, the Dockerfile, and the vendored native inputs. Consumers: make test, make lint, make release and the three GitHub workflows. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Types step on this component: measure the strict-profile candidates (clippy wildcard_enum_match_arm, cast_possible_truncation, cast_sign_loss, unwrap_used and expect_used outside tests, redundant_clone, needless_pass_by_value; Python ty and Ruff candidates for Python files), and adopt only those that reject a real defect or delete an escape hatch at no more than one false positive per 5k lines. Accept: clean at the adopted profile, escape hatches (rg count from the tools reference) not increased, each fixed contract has a witness. Keep unsafe_code policy as the workspace states it: native-boundary unsafe stays in the crates AGENTS.md lists. Acceptance command: python3 -m unittest discover -s scripts -p 'test_*.py', python3 -m unittest discover -s tests -p 'test_*.py', then make test.
- **Fix with:** deslop
- **Effort:** M

### DSLP-10 · P2 · Tests: playback pipeline

- **Area:** deslop · tests
- **Where:** crates/player-core, crates/player-media, crates/player-audio, crates/player-engine, crates/player-backend
- **Problem:** Component of 14.2k first-party lines (Rust; vendored Symphonia, ruopus and rusty_aac excluded). Contracts: the FFmpeg FFI boundary in player-media (sys.rs, session.rs, video.rs: unsafe blocks, MAX_* bounds, IO callbacks), local-only input classification (player-core::input), the Symphonia and rusty_aac decode bridge in player-audio, the scheduler, clock and state machine in player-engine, and the Backend trait in player-backend. Consumers: player-app, player-tui, player-gui, and the corpus, audio, seek and failures gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Tests step on this component: test-pyramid audit, coverage where cargo-llvm-cov exists (Needs environment: it is not installed on this host), a bounded mutation run of the critical contracts where cargo-mutants exists (also not installed), and a boundary test for each `none` in AGENTS.md Verification that this component owns. Accept: no undispositioned survivor in the critical scope, each new test proven by one injected fault, warm make test within 20% or 30s of the Ground baseline. Acceptance command: cargo test -p player-core -p player-media -p player-audio -p player-engine -p player-backend, then tests/run.sh corpus, then tests/run.sh audio, then tests/run.sh seek, then tests/run.sh failures.
- **Fix with:** deslop
- **Effort:** M

### DSLP-11 · P2 · Tests: presentation

- **Area:** deslop · tests
- **Where:** crates/player-output, crates/player-display (including bridge/metal_window.m), crates/player-overlay
- **Problem:** Component of 8.1k first-party lines (Rust and one Objective-C file). Contracts: the libplacebo and Metal/Vulkan FFI in player-output, the window, input ring and Wayland/Cocoa surface in player-display, and the overlay display list, rasterizer and text shaping in player-overlay; each unsafe block states its safety argument. Consumers: player-engine (frame hand-off), player-app, player-gui, and the gpu-api, osc and gui gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Tests step on this component: test-pyramid audit, coverage where cargo-llvm-cov exists (Needs environment: it is not installed on this host), a bounded mutation run of the critical contracts where cargo-mutants exists (also not installed), and a boundary test for each `none` in AGENTS.md Verification that this component owns. Accept: no undispositioned survivor in the critical scope, each new test proven by one injected fault, warm make test within 20% or 30s of the Ground baseline. Acceptance command: cargo test -p player-output -p player-display -p player-overlay, then tests/run.sh gpu-api, then tests/run.sh osc (GPU and window checks need a display: Needs environment where absent).
- **Fix with:** deslop
- **Effort:** M

### DSLP-12 · P2 · Tests: application surface

- **Area:** deslop · tests
- **Where:** crates/player-app, crates/player-tui, crates/player-gui, crates/player-build
- **Problem:** Component of 9.3k first-party lines. Contracts: the mpv-compatible CLI (options.rs, --help and exit codes), the command and property surface, the JSON IPC socket (ipc.rs), playlist and directory loading (playlist.rs), the OSC (osc.rs), the terminal UI, and the build-script helpers in player-build. Consumers: the mpv-compatible users of build/mpv and Player.app, and the cli, cli-help, input, playlist, state, dispatch, osc-input and tui gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Tests step on this component: test-pyramid audit, coverage where cargo-llvm-cov exists (Needs environment: it is not installed on this host), a bounded mutation run of the critical contracts where cargo-mutants exists (also not installed), and a boundary test for each `none` in AGENTS.md Verification that this component owns. Accept: no undispositioned survivor in the critical scope, each new test proven by one injected fault, warm make test within 20% or 30s of the Ground baseline. Acceptance command: cargo test -p player-app -p player-tui -p player-gui -p player-build, then tests/run.sh cli, then tests/run.sh cli-help, then tests/run.sh playlist, then tests/run.sh state.
- **Fix with:** deslop
- **Effort:** M

### DSLP-13 · P2 · Tests: test harness and scripts

- **Area:** deslop · tests
- **Where:** tests/, scripts/, docker/, native/
- **Problem:** Component of 9.6k first-party lines (Python and shell). Contracts: tests/run.sh and tests/harness.py (the gate list, fixture manifest, trace format), the Python unit tests, scripts/build*.sh, release*.sh, audit*.sh, bundle.py, measure.sh, bench.py, perfstat.py, the Dockerfile, and the vendored native inputs. Consumers: make test, make lint, make release and the three GitHub workflows. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Tests step on this component: test-pyramid audit, coverage where cargo-llvm-cov exists (Needs environment: it is not installed on this host), a bounded mutation run of the critical contracts where cargo-mutants exists (also not installed), and a boundary test for each `none` in AGENTS.md Verification that this component owns. Accept: no undispositioned survivor in the critical scope, each new test proven by one injected fault, warm make test within 20% or 30s of the Ground baseline. Acceptance command: python3 -m unittest discover -s scripts -p 'test_*.py', python3 -m unittest discover -s tests -p 'test_*.py', then make test.
- **Fix with:** deslop
- **Effort:** M

### DSLP-14 · P2 · Dead code: playback pipeline

- **Area:** deslop · dead code
- **Where:** crates/player-core, crates/player-media, crates/player-audio, crates/player-engine, crates/player-backend
- **Problem:** Component of 14.2k first-party lines (Rust; vendored Symphonia, ruopus and rusty_aac excluded). Contracts: the FFmpeg FFI boundary in player-media (sys.rs, session.rs, video.rs: unsafe blocks, MAX_* bounds, IO callbacks), local-only input classification (player-core::input), the Symphonia and rusty_aac decode bridge in player-audio, the scheduler, clock and state machine in player-engine, and the Backend trait in player-backend. Consumers: player-app, player-tui, player-gui, and the corpus, audio, seek and failures gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Dead code step on this component: rustc dead_code and unreachable_pub, cargo-machete, then debloat; owner decisions pre-confirm cuts and removals of flags or formats become Needs owner tickets. Accept: tools clean or allowlisted (at most 5 entries, each with a reason), and every removal records the entry points, registrations and consumers checked and the consumer tests run. Acceptance command: cargo test -p player-core -p player-media -p player-audio -p player-engine -p player-backend, then tests/run.sh corpus, then tests/run.sh audio, then tests/run.sh seek, then tests/run.sh failures.
- **Fix with:** deslop
- **Effort:** M

### DSLP-15 · P2 · Dead code: presentation

- **Area:** deslop · dead code
- **Where:** crates/player-output, crates/player-display (including bridge/metal_window.m), crates/player-overlay
- **Problem:** Component of 8.1k first-party lines (Rust and one Objective-C file). Contracts: the libplacebo and Metal/Vulkan FFI in player-output, the window, input ring and Wayland/Cocoa surface in player-display, and the overlay display list, rasterizer and text shaping in player-overlay; each unsafe block states its safety argument. Consumers: player-engine (frame hand-off), player-app, player-gui, and the gpu-api, osc and gui gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Dead code step on this component: rustc dead_code and unreachable_pub, cargo-machete, then debloat; owner decisions pre-confirm cuts and removals of flags or formats become Needs owner tickets. Accept: tools clean or allowlisted (at most 5 entries, each with a reason), and every removal records the entry points, registrations and consumers checked and the consumer tests run. Acceptance command: cargo test -p player-output -p player-display -p player-overlay, then tests/run.sh gpu-api, then tests/run.sh osc (GPU and window checks need a display: Needs environment where absent).
- **Fix with:** deslop
- **Effort:** M

### DSLP-16 · P2 · Dead code: application surface

- **Area:** deslop · dead code
- **Where:** crates/player-app, crates/player-tui, crates/player-gui, crates/player-build
- **Problem:** Component of 9.3k first-party lines. Contracts: the mpv-compatible CLI (options.rs, --help and exit codes), the command and property surface, the JSON IPC socket (ipc.rs), playlist and directory loading (playlist.rs), the OSC (osc.rs), the terminal UI, and the build-script helpers in player-build. Consumers: the mpv-compatible users of build/mpv and Player.app, and the cli, cli-help, input, playlist, state, dispatch, osc-input and tui gates. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Dead code step on this component: rustc dead_code and unreachable_pub, cargo-machete, then debloat; owner decisions pre-confirm cuts and removals of flags or formats become Needs owner tickets. Accept: tools clean or allowlisted (at most 5 entries, each with a reason), and every removal records the entry points, registrations and consumers checked and the consumer tests run. Acceptance command: cargo test -p player-app -p player-tui -p player-gui -p player-build, then tests/run.sh cli, then tests/run.sh cli-help, then tests/run.sh playlist, then tests/run.sh state.
- **Fix with:** deslop
- **Effort:** M

### DSLP-17 · P2 · Dead code: test harness and scripts

- **Area:** deslop · dead code
- **Where:** tests/, scripts/, docker/, native/
- **Problem:** Component of 9.6k first-party lines (Python and shell). Contracts: tests/run.sh and tests/harness.py (the gate list, fixture manifest, trace format), the Python unit tests, scripts/build*.sh, release*.sh, audit*.sh, bundle.py, measure.sh, bench.py, perfstat.py, the Dockerfile, and the vendored native inputs. Consumers: make test, make lint, make release and the three GitHub workflows. The whole repository is over 50k lines including vendored codecs, so each step runs per component.
- **Recommendation:** Run the deslop Dead code step on this component: rustc dead_code and unreachable_pub, cargo-machete, then debloat; owner decisions pre-confirm cuts and removals of flags or formats become Needs owner tickets. Accept: tools clean or allowlisted (at most 5 entries, each with a reason), and every removal records the entry points, registrations and consumers checked and the consumer tests run. Acceptance command: python3 -m unittest discover -s scripts -p 'test_*.py', python3 -m unittest discover -s tests -p 'test_*.py', then make test.
- **Fix with:** deslop
- **Effort:** M

### DSLP-20 · P2 · m3u playlists with a UTF-8 BOM or non-UTF-8 bytes fail the whole startup

- **Area:** Playlist
- **Where:** crates/player-app/src/playlist.rs:170-216 (read_playlist, parse_m3u)
- **Problem:** read_playlist uses std::fs::read_to_string and never strips a BOM. Reproduced on build/mpv at 3db6e14a with a.wav beside the playlists: printf '\xef\xbb\xbf#EXTM3U\n#EXTINF:3,Tone\na.wav\n' > bom.m3u, then build/mpv --no-config --vo=null --ao=null --playlist=bom.m3u prints 'player: failed to open \ufeff#EXTM3U: open: ... No such file or directory' and exits 1: the BOM glues to the header line, so it is no longer a comment and becomes the first entry (the same file without the BOM plays). printf '#EXTM3U\ncaf\xe9.wav\na.wav\n' > latin1.m3u (classic .m3u is Latin-1 or the system code page) prints 'cannot read playlist latin1.m3u: stream did not contain valid UTF-8' and exits 1, losing the valid a.wav entry too. Both are common outputs of Windows and older tools. parse_srt already strips a BOM (sub.rs:173), so the omission is local to playlists.
- **Recommendation:** Strip a leading U+FEFF, read bytes and decode lossily or per extension (m3u8 is UTF-8 by definition), and skip an entry that does not resolve instead of failing the load. Test both files through the playlist gate, and add a unit test beside insertion_tests; prove each with one injected fault.
- **Fix with:** error-model
- **Effort:** S

### DSLP-18 · P3 · Fixture byte hashes drift with Homebrew encoder upgrades, so macOS make test is red

- **Area:** Tests
- **Where:** tests/fixtures/manifest.json, tests/fixtures/gen.sh, tests/harness.py gate_fixtures
- **Problem:** On macOS, make test fails two checks, fixtures: generated manifest matches the tracked manifest and fixtures: 30 file hashes, for corrupt, hevc and mpeg4. Cause (reproduced): gen.sh encodes with the host's Homebrew libx265 and libmp3lame; this host has x265 4.3 and lame 4.0, whose bytes differ from the Nov 5 manifest. Regeneration is deterministic here (three runs gave identical hashes), so it is version drift, not flakiness. Proven at base a4d901f in a scratch worktree: tests/run.sh fixtures gives 4 passed, 2 failed, the same two checks. Linux compares structure only, so Linux CI does not see it.
- **Recommendation:** Decide with the owner whether byte hashes belong in the gate. Cheapest fix that keeps the intent: compare stream structure plus decoded-output hashes on macOS too, as Linux does, or pin the Homebrew formula versions in scripts/dev-deps.sh and say so in docs/linux.md. Do not regenerate the manifest alone: it would break again on the next brew upgrade.
- **Fix with:** test-audit
- **Effort:** S

### DSLP-19 · P3 · Python unit tests are outside make test and CI, and one file costs 235 s

- **Area:** Tests
- **Where:** tests/test_*.py, scripts/test_perfstat.py, Makefile, .github/workflows/
- **Problem:** make test runs tests/run.sh suite and cargo test, but not tests/test_build_deps.py, test_build_flags.py, test_bundle.py, test_linux_audit.py, test_linux_container.py, test_native_rpaths.py or scripts/test_perfstat.py. CI runs only tests/test_release_archives.py. Measured here: python3 -m unittest discover -s tests -p 'test_*.py' took 235 s (16 tests, 5 skipped), nearly all in test_build_deps.py running cargo build --offline; scripts/ took 0.6 s (20 tests). Both pass.
- **Recommendation:** In the Tests step for the harness component, add the fast files to make test and either shrink test_build_deps.py or keep it behind a separate target; record the cost against the test budget.
- **Fix with:** test-audit
- **Effort:** S

### DSLP-21 · P3 · The IPC socket reads unbounded input and rescans it on every poll

- **Area:** Concurrency
- **Where:** crates/player-app/src/ipc.rs:29-72 (IpcServer::poll)
- **Problem:** poll appends everything the client sends to client.buffer with no cap and searches the whole buffer for a newline on every call, so a client that never sends a newline grows memory and makes each poll O(buffer). Reproduced on build/mpv with --input-ipc-server and a local client writing 'A' bytes in 1 MiB writes (script in the finding's scratch dir, .work/tmp/r/ipc_nl.py): sending 4 MiB took 1.2 s, 16 MiB 7.3 s, 32 MiB 43.2 s (RSS 12 MB to 56 MB, superlinear), and the next valid request got no reply within 8 s because it was glued to the junk line. A valid get_property answered in under 0.1 s before the flood. The socket is created with the user's umask, so any local process that can open it can do this. A 1 MiB flood of bare newlines took 6.6 s and then recovered; a quadratic drain was not demonstrated.
- **Recommendation:** Cap one line (for example 1 MiB): past the cap discard to the next newline and reply with an error, or close the client. Scan only the new bytes for the newline. Add an ipc.rs test that feeds an oversized line and asserts the bound and that a later valid line is answered; prove it with the cap removed.
- **Fix with:** security
- **Effort:** S

### DSLP-22 · P3 · File names with control characters reach the terminal unsanitized

- **Area:** Security
- **Where:** crates/player-app/src/app.rs:445 (Playing: line), app.rs:330 and :382 (failed to open), expand_message for ${filename}
- **Problem:** println!("Playing: {}", entry.path) and the open-failure message print the path raw. Reproduced: cp t.flac $'x\e[31mevil\e]0;pw\a.flac' then build/mpv --no-config --vo=null --ao=null that file | cat -v shows 'Playing: x^[[31mevil^[]0;pw^G.flac', and a missing $'nofile\e[2Jx.mp3' prints '^[[2J' in 'failed to open'. A directory argument or an m3u entry can name such a file, so a downloaded folder or playlist can inject cursor, clear and title sequences. The TUI is safe (Canvas::put maps controls to spaces; tested) and container title, artist and stream-title tags carrying ESC were not printed by the plain player in my probes (chapter titles were not probed).
- **Recommendation:** Escape or replace C0, C1 and DEL characters in every path or tag string the plain player writes (one helper next to expand_message), keeping the file path itself untouched for opening. Test the Playing line, the failed-to-open line and ${filename}.
- **Fix with:** security
- **Effort:** S

### DSLP-23 · P3 · parse_srt timestamp arithmetic is unchecked: dev builds kill the scheduler, release builds wrap

- **Area:** Resource bounds
- **Where:** crates/player-engine/src/sub.rs:144-161 (parse_timestamp), :122 (feed: start_ms + duration_ms)
- **Problem:** parse_timestamp parses unbounded digit strings into i64 and computes ((hours * 60 + minutes) * 60 + seconds) * 1000 without checks. Reproduced: printf '1\n99999999999999:00:00,000 --> 99999999999999:00:01,000\nboom\n' > big.srt, then build/mpv --no-config --vo=null --ao=null --sub-file=big.srt a.wav prints 'player: scheduler worker died' (dev profile: overflow checks on, the worker thread panics). dist/Player.app with the same arguments exits normally because release wraps, so the shipped build shows a garbage cue time rather than rejecting the file. EngineSub::feed has the same sum for an embedded packet with a huge duration (not reproduced).
- **Recommendation:** Use checked arithmetic and treat an out-of-range timestamp as a malformed cue (skipped, as parse_srt already skips bad cues); cap hours at a sane value. Add a parse_srt test for 99999999999999:00:00,000 that fails before the change.
- **Fix with:** error-model
- **Effort:** S

### DSLP-24 · P3 · Playlist and subtitle readers load whole files with no size bound

- **Area:** Resource bounds
- **Where:** crates/player-app/src/playlist.rs:175 (read_to_string), crates/player-engine/src/sub.rs:90 (fs::read)
- **Problem:** Reproduced for playlists: build/mpv --no-config --vo=null --ao=null --playlist=/dev/zero reached 6.5 GB RSS after 6 s and 7.0 GB after 12 s with no error. A regular multi-gigabyte file or a FIFO behaves the same; the player never checks the size or file type first. An .srt passed to sub-file or found next to a video is read the same way (not measured separately). Other bounds checked and fine: MAX_STREAMS, MAX_CHAPTERS, MAX_PACKET_BYTES and metadata caps in player-media, 32k-entry playlists (ready in 50 ms, 27 MB), speed clamped to 0.01..100.
- **Recommendation:** Open the file, require a regular file, and read at most a fixed cap (for example 16 MiB for playlists and 4 MiB for subtitles) through Read::take, failing with a clear error past it. Test with a file one byte over the cap.
- **Fix with:** os-boundaries
- **Effort:** S

### DSLP-25 · P4 · ALSA write path may fail playback on EINTR and loop on repeated EPIPE

- **Area:** Needs environment · Linux audio
- **Where:** crates/player-audio/src/backend_alsa.rs:195-225 (AlsaSink::write)
- **Problem:** Source reading only, not reproduced: this host is macOS and the ALSA null PCM needs the Linux container. write treats every negative snd_pcm_writei result other than EPIPE and ESTRPIPE as fatal, so EINTR (-4, for example from a signal delivered to the process during a blocking write) ends the audio path instead of retrying. The EPIPE branch calls snd_pcm_prepare and continues with no bound, so a device that returns EPIPE on every write spins the thread. snd_pcm_pause failing on a device without pause support surfaces as an error with no snd_pcm_drop fallback. None of these has a test: linux-audio runs only the happy path on alsa/null.
- **Recommendation:** In the Linux container with alsa/null, inject the three cases with a fake pcm layer or the null device's error hooks and reproduce before changing code; retry EINTR, bound consecutive EPIPE recoveries, and decide the pause fallback with the owner.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

