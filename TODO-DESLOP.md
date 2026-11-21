Prefix: DSLP
Next: 33

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier D). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-17 (the per-component tickets that replaced DSLP-3 to DSLP-5), work the open P1 and P2 findings, then fill in `## Report` below.

## Report

**Scorecard:** Base `a4d901f` to `a26c56ce7`. First-party Rust is 21,825 source lines to 21,813 and 9,322 test lines to 9,675, excluding `vendor/`; the shell harness is unchanged at 2,803 lines and the Python harness went 6,761 to 6,878. Escape hatches (`rg -c '\bunsafe\b|#!?\[allow\(|\.unwrap\(\)|\.expect\('` over the first-party crates) went from 370 production and 543 test to 341 and 572: 29 production hatches removed and 29 added in the new tests, with the raw total flat at 913. Line and branch coverage and mutants caught/missed/timeout/unviable are unmeasured: `cargo-llvm-cov` and `cargo-mutants` are absent (Needs environment). Two bounded hand-mutation runs stand in: the local-input classifier went from 13 of 16 surviving to 0, and the presentation geometry and blending scope from 11 of 22 surviving to 21 killed with one equivalent. Warm `make test` is unmeasured because it builds the windowed release app and runs the headful gates the owner deferred; measured instead at this check-up, `cargo clippy --workspace --all-targets -- -D warnings` 43.5 s warm and exit 0, and the 17 other headless suite gates passed in 296 s total; `fixtures` ran separately and stays red (4 passed, 2 failed) as DSLP-18 records. Dead code removed: seven `player-app` items (34 net lines), 93 unreachable `pub` items in the playback crates and 85 in the presentation crates made `pub(crate)`/`pub(super)`, and one dependency (`player-backend`'s `player-audio`) made dev-only. Findings filed and fixed: P2 15 filed and all 15 closed (DSLP-1 Ground, DSLP-2 Diagnose, DSLP-3 to DSLP-5 rewritten as the twelve component tickets DSLP-6 to DSLP-17, and DSLP-20 fixed in #16), P3 11 filed and 0 fixed, P4 3 filed and 0 fixed. Perf verdict: no production hot path changed shape and no workload gate was needed; the perf gate is deferred because the host was busy (load 31 on 18 cores). Escaped regressions: none observed; one pre-existing flake was found and filed (DSLP-28).

**Fixes shipped:** #15 (Ground, Diagnose) made CI's Linux step run `make test`, made the macOS `make test` build `dist/Player.app` first, made the `rust-link` gate run the whole workspace, fixed a clippy `-D warnings` failure in a `player-app` test, added `UPSTREAM.md` for `vendor/ruopus` and `vendor/symphonia`, added AGENTS.md `## Verification` and `## Components`, and split Types/Tests/Dead code into twelve per-component tickets. #16 closed the application surface and test harness: DSLP-20 fixed m3u BOM and Latin-1 startup failure with unit tests and two gate cases that fail without the fix; three `expect`s removed; three new IPC tests for malformed, split and dropped client input; seven dead `player-app` functions removed; a new `unit` gate runs `test_harness.py` in both suites in about 1.5 s. #17 is open and parked while the headful gates stay deferred: it closed the playback pipeline and presentation passes, removed the `expect` calls in `scheduler`, `media`, `audio` and `backend`, adopted `unwrap_used`/`expect_used`/`unreachable_pub` denies in those crates, narrowed 178 `pub` items, added safety comments to two `unsafe` blocks, added input-classifier tests and mutated-media decode tests, and made `player-backend`'s `player-audio` dependency dev-only.

**Per step:** Ground (DSLP-1) measured cold `make test` 481 s and warm 202 s with the release gate and `make lint` 41 s warm, and shipped in #15. Diagnose (DSLP-2) reviewed untrusted media, playlists, the IPC socket, terminal output and audio, reproducing six findings on `build/mpv`, and shipped in #15. The twelve component tickets (DSLP-6 to DSLP-17, docs/deslop-*.md) each measured nine clippy candidates, mapped each contract to a guarding test, dispositioned each `none` boundary, removed the witnessed escape hatches, and injected one hand fault; they shipped in #16 and #17. Wall time per step was not recorded beyond the gate totals above. CI durations on the merged PRs: #16 Linux 7m9s and 9m37s, lint 2m46s, macOS 1m25s and 1m45s; #17 Linux 9m35s and 8m4s, lint 2m40s, macOS 1m5s and 1m56s; #15 Linux 7m41s and 9m33s, lint 3m11s, macOS 2m8s and 2m2s.

**Types, per component (adopted of candidates):** playback `unwrap_used`, `expect_used` and `unreachable_pub` adopted (`deny`); `redundant_clone` fixed and not adopted; `needless_pass_by_value`, `wildcard_enum_match_arm`, `cast_possible_truncation`, `cast_sign_loss` rejected with counts. presentation `unreachable_pub` adopted for `player-output` and `player-overlay`; `unwrap_used`/`expect_used` rejected there because the Linux-only `text/linux.rs` `expect`s and the Linux test helpers cannot be compiled on this host (DSLP-30). application and harness adopted no standing profile: every candidate was a false positive at the sampled rates, and the harness's Python and shell rules found nothing adoptable, so `make lint`'s clippy `-D warnings` stays the gate. Escape hatches removed per component: playback 18, presentation 8, application 3.

**Tests, per component:** playback and presentation carried the hand-mutation runs above and added `mutated_video_files_end_cleanly`, `mutated_audio_files_end_cleanly`, the input-classifier set, and the geometry and blending boundaries; application added the IPC and playlist boundary tests; harness added `tests/test_harness.py` (gate registry, exit codes, fixture names) and the `unit` gate. Each new boundary test was proven by one injected fault, restored and `cmp`-verified. The check-up re-injected the recorded `MAX_LATE_SECONDS` clock fault (`MAX_LATE_SECONDS * 10.0` in `judge_frame`, `crates/player-engine/src/clock.rs:122`): `cargo test -p player-engine clock` failed `clock::tests::frame_verdicts_never_present_stale_frames` (`left: Present right: DropLate`), and after `git checkout --` the same test passed.

**Dead-code consumer questions (retained, public):** `AudioClock::is_paused`, `Resampler::buffered_input_frames`, `PlaybackState::supports_bool` and `supports_number`, `VideoFrame::pts_ticks`, `OverlayImage::alpha_at`, `Anchor::CENTER` have no workspace caller, and `output_latency` and `render_rgba` have only test callers. These are workspace-internal crates that are not published, so local absence is strong evidence but external use is not ruled out; DSLP-31 keeps the decision open.

**Remaining tickets:** 14 open, all follow-up: DSLP-18 (P3) fixture byte hashes drift with Homebrew encoders; DSLP-19 (P3) two Python unit files take about four minutes and sit outside `make test`; DSLP-21 (P3) unbounded IPC line buffer; DSLP-22 (P3) control characters in file names reach the terminal; DSLP-23 (P3) unchecked SRT arithmetic; DSLP-24 (P3) unbounded playlist and subtitle reads; DSLP-26 (P3) `player-app` zlib FFI is not listed in AGENTS.md; DSLP-27 (P3) `meta` gate red at base and outside both suites; DSLP-28 (P3) perfstat CPU-window flake; DSLP-30 (P3) Linux-only `expect`s block a presentation-wide deny; DSLP-31 (P3) public items with no callers; DSLP-25 (P4, Needs environment) ALSA `EINTR`/`EPIPE` handling; DSLP-29 (P4) small dead-code and `rm -rf` guard leftovers; DSLP-32 (P4) playback test leftovers. No P1 or P2 remains open.

**Needs environment:** `cargo-llvm-cov`, `cargo-mutants`, `cargo-machete`, `cargo-udeps`, `debloat`, `vulture`, `deptry`, `mutmut` and `coverage` are absent, so coverage, tool mutation, dependency liveness and size attribution are unmeasured for every component. The owner deferred the headful gates (`gui`, `gpu-api`, `audio`, `tui`, `hwdec`, `linux-*`); they did not run here and the Linux container measurement of the new `pub(super)` edits, the Linux `expect` removals and the `unreachable_pub` denies is unverified locally. The perf gate is deferred on a busy host.

**Method feedback:** The `expect_used` deny showed its limit at component boundaries: the presentation crates could not adopt it because their Linux half does not compile on a Darwin host, so a fleet rule should require the Linux container lint before a `deny` is called complete rather than after (DSLP-30). Strict-lint candidate counts found nothing to adopt in the application and harness components, which suggests the Types step should target escape hatches the code can shed rather than a fixed nine-lint sweep. Diagnose, not the component passes, found every reproduced production defect, and the component passes found only one (DSLP-20 was found in Diagnose but fixed in the application pass). Two `make test` checks are red from encoder drift that no local run can distinguish from a real regression; a structure-plus-decoded-hash comparison would have kept the intent without the flake (DSLP-18).

**Shared-file gaps:** The deslop report format has no field for tools absent on the host, so Tests and Dead code land in Needs environment instead of numbers; stuff or usr should provide `cargo-llvm-cov`, `cargo-mutants`, `cargo-machete`, `cargo-udeps` and `debloat` on the Darwin lane host. `TODO.md` carries ticket fields (`Implemented`, `Verified`, `Remaining`, `Resume`, `Status`) that `todo` rejects, so `todo --file TODO.md check` exits 1 before any Phase 4 ticket is added; the tool or the backlog should agree on the allowed fields. The campaign plan link in this file points at the stuff repository; this repository records its outcome only here.

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

