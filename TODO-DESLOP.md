Prefix: DSLP
Next: 33

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier D). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-17 (the per-component tickets that replaced DSLP-3 to DSLP-5), work the open P1 and P2 findings, then fill in `## Report` below.

## Report

Not started.

## Tickets

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

### DSLP-26 · P3 · player-app uses unsafe zlib FFI but AGENTS.md doesn't list it among the unsafe crates

- **Area:** Security
- **Where:** crates/player-app/src/png.rs:3, AGENTS.md
- **Problem:** png.rs declares unsafe extern "C" compress2 from zlib, but AGENTS.md's list of crates allowed unsafe code omits player-app.
- **Recommendation:** Either list player-app (with the zlib boundary) in AGENTS.md or replace the FFI with a safe encoder.
- **Fix with:** os-boundaries
- **Effort:** S

### DSLP-27 · P3 · The meta gate is red at base and runs in neither suite

- **Area:** Tests
- **Where:** tests/harness.py:861, scripts/fetch-gluon.sh, meta gate
- **Problem:** Its $TMPDIR pattern matches scripts/fetch-gluon.sh and tests/harness.py:861, and its .git/info/exclude check fails in linked worktrees, so it fails at base; it is outside SUITE and LINUX_SUITE so nothing notices.
- **Recommendation:** Fix the two false positives (resolve the common git dir for worktrees) and add meta to a suite.
- **Fix with:** root-cause
- **Effort:** S

### DSLP-28 · P3 · perfstat ProcessCpuTests need 50 ms of CPU inside a 100 ms window

- **Area:** Tests
- **Where:** scripts/test_perfstat.py (ProcessCpuTests)
- **Problem:** The tests assume the process gets 50 ms of CPU within 100 ms of wall time; on a busy host they failed once.
- **Recommendation:** Measure CPU with a work-bounded loop or a generous wall bound, so scheduling latency can't fail them.
- **Fix with:** root-cause
- **Effort:** S

### DSLP-30 · P3 · Linux-only expects block a presentation-wide expect_used deny

- **Area:** Types · presentation
- **Where:** crates/player-overlay/src/text/linux.rs:690, :810, :811, :1052; crates/player-overlay tests/present.rs helpers
- **Problem:** The playback crates now deny unwrap/expect, but the presentation crates can't: Linux-only production code still uses expect, and it can't be compiled on Darwin, so this pass also left two Linux edits (pub(super) in text/linux.rs, AVERROR_EAGAIN in sys.rs) uncompiled.
- **Recommendation:** In the musl container, run make lint to confirm the Linux edits, replace the Linux expects, and extend the unwrap/expect deny to the presentation crates.
- **Fix with:** deslop
- **Effort:** S

### DSLP-31 · P3 · Public playback and presentation items with no callers

- **Area:** Dead code · consumer questions
- **Where:** AudioClock::is_paused, Resampler::buffered_input_frames, PlaybackState::supports_bool and supports_number, VideoFrame::pts_ticks, OverlayImage::alpha_at, Anchor::CENTER; output_latency and render_rgba (test callers only)
- **Problem:** These public items have no workspace callers (two only test callers), so they are either API for external consumers or dead code.
- **Recommendation:** Decide per item: remove, or keep as documented API with a test that exercises the contract.
- **Fix with:** dead-code-archaeology
- **Effort:** S

### DSLP-25 · P4 · ALSA write path may fail playback on EINTR and loop on repeated EPIPE

- **Area:** Needs environment · Linux audio
- **Where:** crates/player-audio/src/backend_alsa.rs:195-225 (AlsaSink::write)
- **Problem:** Source reading only, not reproduced: this host is macOS and the ALSA null PCM needs the Linux container. write treats every negative snd_pcm_writei result other than EPIPE and ESTRPIPE as fatal, so EINTR (-4, for example from a signal delivered to the process during a blocking write) ends the audio path instead of retrying. The EPIPE branch calls snd_pcm_prepare and continues with no bound, so a device that returns EPIPE on every write spins the thread. snd_pcm_pause failing on a device without pause support surfaces as an error with no snd_pcm_drop fallback. None of these has a test: linux-audio runs only the happy path on alsa/null.
- **Recommendation:** In the Linux container with alsa/null, inject the three cases with a fake pcm layer or the null device's error hooks and reproduce before changing code; retry EINTR, bound consecutive EPIPE recoveries, and decide the pause fallback with the owner.
- **Fix with:** lifecycle-concurrency
- **Effort:** M

### DSLP-29 · P4 · Small dead-code and safety leftovers from the application and harness pass

- **Area:** Dead code
- **Where:** crates/player-app (Osc::overlay, Rendered::full), scripts/install-layout.sh:43, scripts/build-deps*.sh, tests/harness.py gates
- **Problem:** Osc::overlay and Rendered::full are test-only; install-layout.sh:43 runs rm -rf "$prefix/lib" with no empty-prefix check; GLSLANG_REV, SPIRV_HEADERS_REV and SPIRV_TOOLS_REV are dead in both build-deps scripts (removing them costs one CI cache rebuild); the play, mpv-diff, meta and build gates have no documented caller; a missing playlist entry still exits 1 at startup where mpv skips it.
- **Recommendation:** Address together in one cleanup change that accepts the cache rebuild; guard the rm with a non-empty prefix check first.
- **Fix with:** dead-code-archaeology
- **Effort:** S

### DSLP-32 · P4 · Playback leftovers: double observe in sched step, unproven decoder finish case, VideoToolbox lease witness

- **Area:** Tests · playback
- **Where:** crates/player-sched (sched.rs step), player-audio decoder finish(), VideoToolbox import ring test
- **Problem:** step calls observe(now) twice (idempotent, harmless); finish() on a decoder with no cursor now discards stray samples instead of panicking but reachability is unshown; an injected early-lease-release fault still passes the VideoToolbox import-ring test, so that path has no witness.
- **Recommendation:** Drop the redundant observe; show or rule out the finish case; add a test that fails when leases are released early.
- **Fix with:** test-audit
- **Effort:** S

