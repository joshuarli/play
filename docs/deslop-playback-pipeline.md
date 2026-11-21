# Deslop: playback pipeline

Component: `player-core`, `player-media`, `player-audio`, `player-engine`, `player-backend` (14.2k first-party lines). Base `e39e5018`; tickets DSLP-6 (Types), DSLP-10 (Tests), DSLP-14 (Dead code). Host: macOS 27.2 arm64 (Apple M5 Max), `cargo 1.102.0-nightly (c07ac2c4d 2026-11-06)`, busy host (load average about 50). All commands run in the worktree with `TMPDIR=$PWD/.work/tmp CARGO_BUILD_JOBS=4`; gates also need `PATH=/opt/homebrew/Cellar/ffmpeg/9.0.2_1/bin:$PATH`, as `make test` arranges, because the first `ffmpeg` on `PATH` is the pinned runtime without the fixture encoders. The worktree has a clone of the canonical `.work/deps` and `.work/tools` with the `.pc` prefixes rewritten to the worktree.

Headless only. The owner deferred headful tests, so no `gui`, `gpu-api` or `audio` gate ran (see Needs environment). `tests/run.sh audio` opens CoreAudio at `--volume=0`; it counts as audio playback.

Baseline, before any change: `cargo test -p player-core -p player-media -p player-audio -p player-engine -p player-backend` passed (36, 24, 55 + 2 ignored, 93 + 2 ignored, 9 tests); `tests/run.sh` `corpus` 109 passed, `cover-art` 10, `seek` 38, `failures` 8, `osc` 14, `osc-input` 30, all exit 0. `fixtures` exits 1 with 4 passed, 2 failed (`generated manifest matches the tracked manifest`, `30 file hashes`, for corrupt, hevc and mpeg4): DSLP-18, recorded, not fixed. DSLP-19 (Python unit tests outside `make test`) does not touch this component. One `scripts/build.sh` died with SIGKILL from resource pressure on the first try and passed on retry.

## DSLP-6 Types

Profile measured with `cargo clippy -p player-core -p player-media -p player-audio -p player-engine -p player-backend --lib --message-format=json -- -W clippy::wildcard_enum_match_arm -W clippy::match_wildcard_for_single_variants -W clippy::cast_possible_truncation -W clippy::cast_sign_loss -W clippy::unwrap_used -W clippy::expect_used -W clippy::redundant_clone -W clippy::needless_pass_by_value -W unreachable_pub -W dead_code`, then counted by code. The sample is macOS code only; Linux-only sources (`backend_alsa.rs`, two `cfg(linux)` blocks in `player-media`) were read, not linted.

| Candidate | Hits | Disposition |
| --- | ---: | --- |
| `clippy::unwrap_used` | 3 | all `build.rs` `CARGO_MANIFEST_DIR` reads; replaced by `env!` as `player-media/build.rs` already does. Adopted (`deny`) |
| `clippy::expect_used` | 18 | 17 in library code, each a check followed by an unwrap of the same value; all removed (below). 1 in a build script. Adopted (`deny`, `allow-expect-in-tests`/`allow-unwrap-in-tests` in root `clippy.toml`) |
| `clippy::redundant_clone` | 1 | real: `Attachment.codec_name` cloned from a value used nowhere else (`session.rs`). Fixed; not adopted, the lint is nursery and its single hit is gone |
| `clippy::needless_pass_by_value` | 3 + 1 | rejected: `present_frame`/`drop_late`/`feed_sub` take ownership so the frame or packet is released at the call; `fail(message: String)` in a build script |
| `clippy::wildcard_enum_match_arm` | 8 | rejected: all are `_ =>` on state or codec enums where the default arm is the intent (`State::Playing | Draining`, `StreamKind`, symphonia `Position`); 8 hits on 14.2k lines exceeds one per 5k lines, zero defects |
| `clippy::match_wildcard_for_single_variants` | 0 | no signal |
| `clippy::cast_possible_truncation` | 43 | rejected: every site read; FFI sizes are checked against `MAX_PACKET_BYTES`, `MAX_STREAMS` or `> 0` first, float seconds go through saturating `as i64`, `as u32` samples rates are guarded by `<= 0` checks. No defect |
| `clippy::cast_sign_loss` | 38 | rejected for the same reason (`packet.size`, `stream_index`, `linesize` all guarded by `< 0` checks before the cast) |
| `unreachable_pub` (rustc) | 93 | adopted as `deny`; see DSLP-14 |

Adopted profile (`[lints.clippy] unwrap_used, expect_used = "deny"` and `[lints.rust] unreachable_pub = "deny"` in the five crates' `Cargo.toml`, plus root `clippy.toml`): `cargo clippy --workspace --all-targets -- -D warnings` exits 0. Negative controls: `Ok(String::from_utf8(out).unwrap())` in `player-core/src/input.rs` fails `cargo clippy -p player-core --all-targets -- -D warnings` with `used unwrap() on a Result value` (exit 101); `pub fn injected() {}` appended to `player-media/src/error.rs` fails with `unreachable pub item`. Both restored and checked with `cmp`.

Escape hatches (`rg -c '\bunsafe\b|#!?\[allow\(|\.unwrap\(\)|\.expect\('` over the five crates, tests included): 538 at base, 525 now. Production library code: 17 `expect` and 0 `unwrap` at base, 0 now. The test count includes 8 new `expect`/`unwrap` in the added tests. `unsafe` count (native-boundary, stays): 180 in `player-media`, `player-audio`, `player-engine`; no `#[allow]` added.

Typing contracts fixed, each with its witness:

| Contract | Change | Witness |
| --- | --- | --- |
| scheduler coalesces a queued seek | `if let Some(last @ Command::Seek(..)) = commands.last_mut()` replaces `matches!` then `expect` (`sched.rs:233`) | `sim.rs:856` `seek_between_keyframes_resumes_at_the_target`, `sim.rs:1153` |
| paused seek presents the chosen frame | `pop_front()` matched instead of `expect("chosen frame")` (`sched.rs:636`) | `sim.rs:1026` `seek_while_paused_stays_paused` |
| due frames are judged in order | `while let Some(frame) = pop_front()`; a `Wait` verdict pushes the frame back (`sched.rs:987`) | `sim.rs:725` `normal_av_plays_to_ended_with_no_drops`, `clock.rs:229` |
| video queue eviction and `take` | `let Some(..) = pop_front() else` and `remove(..).map` (`video.rs:186,205`) | `sim.rs` accounting (`assert_accounting`) |
| SubRip markup stripping and cue bodies | `next_if` and one `markup_end` option (`sub.rs:190,215`) | `sub.rs:260` `keeps_international_text_and_strips_formatting` (both `<>` and `{}` forms, `<3`, `{not a tag}`) |
| fps estimator origin | `let Some((origin_position, origin_presented)) = self.origin else { return }` (`backend.rs:965`) | `backend.rs:1681` `a_null_output_is_named_and_the_frame_rate_is_measured` |
| codec option names | `option_string` returns `Err` on a NUL byte instead of panicking (`player-media/video.rs:107`) | `video.rs` decode tests (happy path); no NUL witness, the names are literals |
| owned-session checks | one `ok_or_else` binding, no `expect("checked")` (`video.rs:478,726`) | `video.rs:1004` decode tests |
| HE-AAC reset cannot fail | the parsed `StreamConfig` is kept (boxed, `large_enum_variant`) and the decoder rebuilt with `with_stream_config`; `with_config_bytes` is `parse` then `with_stream_config`, so the decoder is identical (`session.rs:43`) | `session.rs:449` HE-AAC test decodes twice with `reset()` between, implicit and explicit configuration |
| audio chunk timestamp | `take_chunk(pts)` takes the cursor instead of unwrapping it; `finish()` with no cursor now discards stray samples instead of panicking (`decode.rs:178`). Reachability of that case was not shown | `pipeline.rs` drain and seek tests, `mutated_audio_files_end_cleanly` |
| overlay renderer creation | `match &mut self.overlay { Some(r) => r, empty => empty.insert(OverlayRenderer::new()?) }` (`player-backend/engine.rs:167`) | `engine.rs:300` `osc_overlay_composites_into_a_frame` |

Acceptance (`e39e5018` to `c3040eef`): `cargo test -p player-core -p player-media -p player-audio -p player-engine -p player-backend` exit 0 (42, 25, 56, 93, 9 passed; 2 + 2 ignored), then `corpus` 109/0, `seek` 38/0, `failures` 8/0, each exit 0. `audio` not run (headful, below).

## DSLP-10 Tests

Pyramid. Unit tier: 231 `#[test]` functions in the five crates (about 4 s warm; a cold `cargo test` of these crates took 3m40s on the busy host, almost all compilation). `player-engine` `sim.rs` drives the scheduler with fake demux, decoder, output and a simulated clock (41 tests); that is the wide tier for scheduling. Gate tier (`tests/run.sh`, external `build/mpv`): `corpus` 109 checks in 66 s, `cover-art` 10 in 7 s, `seek` 38 in 2 s, `failures` 8 in 1 s, `osc` 14 and `osc-input` 30 in 1 s and 6 s, all `--vo=null --ao=null`. Not headless: `audio` (CoreAudio), `gui`, `gpu-api`. No test without an assertion was found (script over all `#[test]` bodies); the three without an assert macro (`handles_malformed_input_without_panicking`, two ignored VideoToolbox tests) assert through `expect` or `panic!`. `rg` over the five crates finds 6 `#[ignore]` tests, all with a stated environment reason.

False pass found in the harness: none. A false-pass risk in the new work: a bounded-loop assert is the only guard against a decode that never ends; the existing `drain_all` tests would hang instead.

Coverage and mutation tools: `cargo-llvm-cov` and `cargo-mutants` are not installed (Needs environment). A bounded hand mutation run replaced them for the one critical contract this component owns that had `none`-class weakness, local-only input classification (`player-core/src/input.rs`, scope `classify_input`, `valid_scheme`, `percent_decode`, `hex`, `WRAPPERS`): 16 mutants, applied one at a time with `sed`-style replacement, each run with the four original tests and with all ten.

| Mutant | Original four tests | New killer |
| --- | --- | --- |
| empty-input check removed | survives | `empty_input_is_refused_with_or_without_file_uris` |
| `file` scheme compared case-sensitively | survives | `schemes_are_matched_in_any_case_and_reported_lowercase` |
| file URI accepted without opt-in | caught | `file_uri_requires_opt_in` |
| scheme not lowercased in the error | survives | `schemes_are_matched_in_any_case_and_reported_lowercase` |
| wrapper match case-sensitive | survives | `every_wrapper_prefix_is_refused_in_any_case` |
| wrapper `fd:`, `slice:`, `appending:` each dropped | survive | `every_wrapper_prefix_is_refused_in_any_case` (first draft iterated the constant itself and left all three alive; it now carries its own literal list and asserts it equals `WRAPPERS`) |
| scheme may start with a digit | survives | `a_separator_inside_a_path_is_not_a_scheme` |
| scheme may contain `/` | survives | `a_separator_inside_a_path_is_not_a_scheme` (`dir/x://a.mp4`) |
| truncated escape bound `>=` to `>` | survives (the mutant indexes past the end) | `malformed_file_uri_escapes_are_refused` |
| escape consumes two bytes | caught | `file_uri_escapes_decode_in_either_hex_case`, `file_uri_requires_opt_in` |
| lowercase hex offset 10 to 9 | survives | `file_uri_escapes_decode_in_either_hex_case` |
| uppercase hex offset 10 to 9 | survives | same |
| invalid UTF-8 decoded lossily | survives | `malformed_file_uri_escapes_are_refused` |
| empty `file://` path accepted | caught | `file_uri_requires_opt_in` |

Result: 13 of 16 survived the original tests (the three drops counted once each); none survives the new set (`input.rs:166-224`). No equivalent mutants. The timestamp and `finish` paths of the audio decoder and the scheduler boundary `MAX_LATE_SECONDS` were not mutation-run beyond the fault below.

Contract, guard, disposition:

| Contract | Guarding tests | Disposition |
| --- | --- | --- |
| input classification (local only) | `input.rs:122-224`; `session.rs:848` `rejects_nonlocal_input`; gates `local-only`, `local-paths` (not run, outside this scope) | strengthened, 10 tests |
| container probe, packets, seek, chapters, cover art | `session.rs:754-846`, gates `corpus`, `seek`, `cover-art` | asserted |
| truncated and corrupt files | `session.rs:867`, gate `failures` | asserted |
| mutated or fuzzed media (`none` in AGENTS.md Verification) | new `video.rs:1049` `mutated_video_files_end_cleanly`, `pipeline.rs:241` `mutated_audio_files_end_cleanly` | added, below |
| scheduler, clock, state machine | `sim.rs` (41), `clock.rs:229`, `state.rs` (13) | asserted |
| frame lateness boundary `MAX_LATE_SECONDS` | `clock.rs:229` at 0.5 s and 0.95 s lateness only; exactly 0.1 s is untested | accepted: float equality at the boundary is not a stable contract |
| audio decode, mix, resample, stretch | `mix.rs` (11), `resample.rs` (7), `session.rs` (8), gate `audio` (headful, not run) | asserted headless; device path in Needs environment |
| Backend trait | `player-backend/engine.rs` 5, `overlay.rs` 4 | asserted |

New boundary tests, each with one injected fault:

- `mutated_video_files_end_cleanly`: five video fixtures, eight fixed seeds, 16 xorshift byte flips each; every run opens or fails, then pumps at most 2000 times and must reach `Ended` or an error. Fault: `pump` returns `PumpResult::NeedMore` instead of `Ended` at EOF. Result: test fails, `h264.mp4 seed 0: decode did not end` (cargo test exit 101); `drain_all` tests would loop forever under the same fault. File restored, `cmp` clean.
- `mutated_audio_files_end_cleanly`: six audio-bearing fixtures, eight seeds, 5000-chunk bound. First injected fault (`Ok(true)` to `Ok(false)` only on the first EOF return) survived because the next call returns the finished branch; that was an equivalent fault. Second fault (every `Ok(true)` in `next_chunk` to `Ok(false)`): fails, `audio-flac.flac seed 2: decode did not end`. Restored, `cmp` clean.
- Critical-decision fault in the scheduler: `judge_frame` threshold `MAX_LATE_SECONDS` to `MAX_LATE_SECONDS * 10.0` fails `clock::tests::frame_verdicts_never_present_stale_frames` (`clock.rs:233`); restored, `cmp` clean.

Boundary `none` entries owned elsewhere (IPC line limits, playlist files) belong to the application component. Mutated media found no panic and no hang in this sample (opened at least 20 video and 30 audio runs per the test's own floor); the mutated samples are 16 flips of small files and are a bound, not a fuzz campaign.

Cost: new tests add under 1 s warm (`mutated_*` 0.3 s each, input tests under 10 ms); the warm `make test` budget is unaffected within measurement noise. The full `make test` was not run (it includes headful gates).

## DSLP-14 Dead code

Tools: rustc `dead_code` and `unreachable_pub` (`cargo clippy -p player-core -p player-media -p player-audio -p player-engine -p player-backend --lib -- -W unreachable_pub -W dead_code`): `dead_code` 0, `unreachable_pub` 93 (92 in `player-media/src/sys.rs`, 1 in `player-engine/src/clock.rs`). `cargo-machete` and `cargo +nightly udeps` are not installed (Needs environment); replaced by a manual crate-name search (`tomllib` over each `Cargo.toml` against every `.rs` in the crate). `debloat` was not run: the component has no bloat tool installed here.

Removals and restrictions:

| Finding | Evidence | Disposition |
| --- | --- | --- |
| 92 `pub` declarations in private `mod sys` of `player-media`, 4 more under `cfg(test)` and `cfg(not(macos))` | `lib.rs` has `mod sys;`, no `pub use` of them; no other crate can name them; workspace-wide `rg` of each name outside the crate finds none | `pub(crate)`; afterwards `dead_code` still reports 0, so no FFI declaration was dead |
| `MAX_LATE_SECONDS` | used only inside `player-engine` | `pub(crate)` |
| `player-backend` dependency on `player-audio` | `player_audio` appears only at `engine.rs:186` inside `#[cfg(test)]`; `player-engine` already depends on it | moved to `[dev-dependencies]`; `Cargo.lock` unchanged |
| unused dependencies | manual search, all other dependencies referenced in source or `build.rs` | none |

Consumers and platforms checked: `cargo clippy --workspace --all-targets -- -D warnings` exit 0 (player-app, -tui, -gui included); unit tests, `corpus`, `seek`, `failures`, `cover-art`, `osc`, `osc-input` after the change all exit 0 (counts above). Platforms: macOS arm64 only; the Linux `AVERROR_EAGAIN` constant and the `cfg(test)` externs were edited by hand and not compiled on Linux (Needs environment: Linux container).

Public items with no caller anywhere in the workspace (source, tests, docs, scripts): `AudioClock::is_paused` (`player-audio/src/clock.rs:52`), `Resampler::buffered_input_frames` (`resample.rs:42`), `PlaybackState::supports_bool` and `supports_number` (`player-core/src/state.rs:160,164`), `VideoFrame::pts_ticks` (`player-media/src/video.rs:235`). `output_latency` (`player-engine/src/clock.rs:100`) has only a test caller. They are public methods of workspace-internal crates; per policy they are reported as consumer questions, not removed.

## Needs environment

- `cargo-llvm-cov`, `cargo-mutants`, `cargo-machete`, `cargo-udeps`: not installed; no coverage numbers and no tool mutation run. The input classifier was hand-mutated instead (above).
- `tests/run.sh audio` (CoreAudio) and the audio device tests `repeated_flushes_keep_every_buffer` and the ALSA ones (`#[ignore]`): headful or Linux-only; deferred by the owner.
- Linux container: the playback crates' Linux-only code (`backend_alsa.rs`, `video.rs` `render_node_present`, `sys.rs` `AVERROR_EAGAIN`) and the new lints were not compiled for `x86_64-unknown-linux-musl`; `make lint` there must confirm the `deny` profile.
- `tests/run.sh fixtures`: red at base and after (DSLP-18).
