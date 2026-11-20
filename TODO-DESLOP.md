Prefix: DSLP
Next: 20

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier D). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-17 (the per-component tickets that replaced DSLP-3 to DSLP-5), work the open P1 and P2 findings, then fill in `## Report` below.

## Report

Not started.

## Tickets

### DSLP-2 · P2 · Diagnose: ultrareview into this backlog

- **Area:** deslop · diagnose
- **Where:** whole repository
- **Problem:** No review has ranked this repository's risks for the deslop pass.
- **Recommendation:** Run the deslop Diagnose step. Accept: the diff touches only docs, comments, and this file, and every finding names its Fix with.
- **Fix with:** deslop
- **Effort:** M

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

