# Deslop: application surface

Scope: `crates/player-app`, `crates/player-tui`, `crates/player-gui`, `crates/player-build` (9.3k first-party lines). Base `e39e5018`; worktree branch `deslop/p2`. Commands run with `RUSTC_WRAPPER=` (the shared `kache` wrapper was SIGKILLed twice under host load), `TMPDIR=$PWD/.work/tmp`, `CARGO_BUILD_JOBS=4`. Only headless targets ran: every gate below starts the player with `--vo=null --ao=null` (or `--help`/`--version`).

## DSLP-8 Types

Candidate measurement (production code only, lines before each file's `#[cfg(test)]`):

```
cargo clippy -p player-app -p player-tui -p player-gui -p player-build --message-format json -- \
  -W clippy::wildcard_enum_match_arm -W clippy::match_wildcard_for_single_variants \
  -W clippy::cast_possible_truncation -W clippy::cast_sign_loss -W clippy::unwrap_used \
  -W clippy::expect_used -W clippy::redundant_clone -W clippy::needless_pass_by_value \
  -W clippy::as_conversions -W clippy::indexing_slicing -W unreachable_pub
```

| Candidate | Hits | Disposition |
| --- | ---: | --- |
| `wildcard_enum_match_arm` | 10 | Rejected. All idiomatic: untrusted JSON `NativeValue` kinds (`app.rs:1208,1228,1234`), a foreign `KeyCode` (`term.rs:437`), OSC `Phase` timers (`osc.rs:601`), `StreamKind` filters (`properties.rs:623,628,636`), the build script's own `Json` enum (`ffmpeg_runtime.rs:41,48`). 10 false positives in 9.3k lines exceeds one per 5k. |
| `match_wildcard_for_single_variants`, `redundant_clone` | 0 | Nothing to adopt. |
| `needless_pass_by_value` | 1 | Rejected (`app.rs:1182`, `send_event` takes the value that `defer_event` stores). |
| `cast_possible_truncation`, `cast_sign_loss`, `as_conversions` | 42, 24, 147 | Rejected: 213 hits with no sampled defect (pixel and geometry arithmetic on bounded values) and no witness; far above one false positive per 5k lines. |
| `indexing_slicing` | 26 | Rejected: 26 hits, none sampled as a defect, no witness. |
| `unwrap_used` | 4 | Rejected: two `writeln!` to `String` (`options.rs:490,494`) and two `env::var("CARGO_MANIFEST_DIR")` in build scripts. |
| `expect_used` | 4 | Three were real, adopted by removal below; one is the build script's `TARGET`. |
| `unreachable_pub` | 29 | Not a type defect; visibility narrowing in `player-tui`, see DSLP-16. |

Nothing is adopted as a standing profile. `make lint`'s clippy `-D warnings` stays the gate.

Fixed: three `expect` calls whose invariant a type can carry (commit `1f3eb7a6`). `properties::video_params` re-found the probe with `expect("stream belongs to probe")`; it now binds the probe and its video stream together. `App::write_screenshot_with` rechecked `last_frame` and the renderer slot with `expect`; it now uses `let ... else` and `Option::insert`. Escape-hatch count (`rg -c '\bunsafe\b|#!?\[allow\(|\.unwrap\(\)|\.expect\('`, production lines): 14 to 11. Test-module lines rose 93 to 113 because the new tests unwrap temp-file and socket results.

| Contract | Guarding test | Disposition |
| --- | --- | --- |
| `video-params` reports coded and rotated geometry | `properties.rs:938` `video_params_report_coded_and_rotated_display_geometry` | Witnessed: injecting `pixel_aspect: 1.0` into the metadata branch fails it (`left: Int(64) right: Int(128)`, exit 101). |
| `screenshot-to-file` renderer reuse | `tests/harness.py:436` `screenshot_case` (gui gate) | Compiler-checked only. The gui gate opens a window; Needs environment. |

Unsafe: `crates/player-app/src/png.rs:3,32` declares `unsafe extern "C" { fn compress2 }`, but `AGENTS.md` lists native-boundary unsafe only in `player-media`, `-output`, `-display`, `-overlay`, `-engine` and `-audio`. Not changed: replacing zlib needs a dependency or a bigger PNG (an owner choice). Filed as finding below.

## DSLP-12 Tests

Pyramid. Unit tier: `cargo test -p player-app -p player-tui -p player-gui -p player-build` runs 85 tests in about 33 s warm including compile (app 58, tui 19, build 8). Integration tier: Python gates driving `build/mpv` over IPC, each 1 to 7 s: `cli` 62 checks 7.0 s, `cli-help` 24 checks 1.0 s, `playlist` 15 checks 1.7 s, `state` 13 checks 3.0 s, `osc-input` 30 checks 5.9 s, `osc` 14, `input` 9, `dispatch` 11, `local-paths` 12. The `tui`, `gui`, `gpu-api` and `local-only` gates were not run: `gui` and `gpu-api` open windows, and `tui` includes cases without `--vo=null`.

False passes fixed: none found. The `playlist` gate never read an m3u or pls file (only directories and `loadfile`), which is why DSLP-20 shipped; it now does.

Boundaries that `AGENTS.md` marked `none` and this component owns:

| Boundary | Test | Result |
| --- | --- | --- |
| Playlist with a UTF-8 BOM or non-UTF-8 bytes | `playlist.rs` `read_tests` (4 tests); gate `playlist: utf8-bom m3u entries`, `playlist: latin1 m3u entries` (`tests/harness.py:2059`) | Added with the DSLP-20 fix, commit `c0b0a7c8`. Fails on base: the BOM becomes an entry (`left: [".../\u{feff}#EXTM3U", ".../a.wav"]`), Latin-1 returns `Err`, and the gate exits 1 with `player: failed to open ...﻿#EXTM3U`. |
| Malformed IPC lines | `ipc.rs` tests `malformed_lines_are_marked_or_skipped_and_do_not_hide_valid_ones`, `a_line_split_across_polls_is_delivered_once_complete`, `a_closed_client_is_dropped_and_the_next_one_accepted`, commit `bda9add5` | Added. |
| Oversized IPC line | none | Cannot be tested until the cap in DSLP-21 exists; the test would fail or run the host out of memory. Left open. |
| Oversized playlist file | none | Accepted: a playlist is a local file the user names, read once, and the `none` stays in `AGENTS.md` as a stated limit. |

Injected fault (critical decision: what the IPC reader does with a line it cannot parse). In `ipc.rs` the `Err(_)` arm that pushes the `"invalid"` map was replaced with `Err(_) => {}`:

```
cp crates/player-app/src/ipc.rs .work/tmp/ipc.rs.orig   # then edit
cargo test -p player-app ipc    # exit 101: malformed_lines_are_marked_or_skipped_and_do_not_hide_valid_ones FAILED (5 passed, 1 failed)
cp .work/tmp/ipc.rs.orig crates/player-app/src/ipc.rs
cmp crates/player-app/src/ipc.rs .work/tmp/ipc.rs.orig && echo cmp-ok   # exit 0
```

A second fault, replacing the UTF-8 check with `from_utf8_lossy`, fails the same test.

Gate results after all three code commits (`build/mpv` rebuilt from `deslop/p2`): `cli` 62/62, `cli-help` 24/24, `playlist` 15/15, `state` 13/13, `osc` 14/14, `osc-input` 30/30, `input` 9/9, `dispatch` 11/11, all exit 0.

Needs environment: `cargo-llvm-cov` and `cargo-mutants` are not installed on this host, so there is no line or branch coverage and no mutation run; the "no undispositioned survivor" acceptance is unmet for lack of a tool, and the three hand-injected faults above are the only mutation evidence. Warm `make test` was not run (it builds the release app and runs windowed gates, which the owner deferred).

Still headful-only for this component: `tui`, `gui`, `gpu-api`, screenshot cases, and the macOS window bridge.

## DSLP-16 Dead code

Tools: rustc `dead_code`, `unused_crate_dependencies`, `unreachable_pub`; `cargo clippy ... --all-targets -- -D warnings -W unused_crate_dependencies` is clean at base and after. `player-app` declares `pub mod` for every module, so rustc cannot see unused items. A scratch build with those `pub mod` lines made private (`sed -i '' 's/^pub mod /mod /'`, then restored and `cmp`-checked) reported seven non-test-unused items:

| Item | Workspace-wide search | Disposition |
| --- | --- | --- |
| `App::quit`, `App::should_quit` (`app.rs`) | `rg 'should_quit\|\.quit\(\)' crates tests docs`: definitions only; the `quit` field keeps its nine writers | Removed |
| `App::observe_property` (`app.rs`) | `rg '\.observe_property\('`: none; the `observe_property` IPC command inlines the same insert | Removed |
| `app::entry` | `rg '[^_a-z]entry\('` in `player-app`: only a local test helper in `playlist.rs` | Removed |
| `IpcServer::connected` | no caller before the new tests | Removed; the new tests read the private `client` field |
| `HostWindow::fullscreen` (`window.rs`) | `rg '\.fullscreen\(\)'`: only `player-display` tests on a different type | Removed |
| `properties::to_bool` | `rg to_bool`: definition only | Removed |
| `Osc::overlay`, `Rendered::full` | 16 callers, all in `osc.rs` tests | Kept: a test-only seam that caches a concatenation. Filed as finding. |

All removals are in commit `1e8684d4` (its message says six; it is seven items). Entry points checked: `main.rs`, `lib.rs` re-exports (`run`, `run_program`), `player-gui` (calls `run_program` only), IPC command table, property table, docs. `player_app` is not published; no registry crate depends on it. Consumer tests run: `cargo test -p player-app -p player-tui -p player-gui -p player-build` (85 pass) and the gates listed under DSLP-12. Platforms: macOS aarch64 build only; the Linux-only `cfg` branches were not compiled.

`player-tui` already keeps its modules private, so rustc `dead_code` covers it: clean. `player-build`: all 14 `pub fn` have a caller in a build script. `player-gui`: nothing to remove.

Needs environment: `cargo-machete`, `cargo-udeps` (unused dependencies; `unused_crate_dependencies` is the substitute and is clean), `debloat`, and a Linux build for `cfg(target_os = "linux")` code.

## Findings

- P3 · `png.rs` links zlib through `unsafe extern "C"` in `player-app` while `AGENTS.md` lists the unsafe crates and omits it. Either list `player-app` or replace `compress2` with a safe encoder; owner choice.
- P4 · `Osc::overlay` and `Rendered::full` are test-only; move the concatenation into the tests and drop the cache.
- P3 · IPC reads are unbounded (DSLP-21); the oversized-line boundary test depends on it.
- P4 · A playlist entry that does not exist exits the player with code 1 at startup, where mpv skips to the next entry. DSLP-20 asked for this; it is a separate behaviour change and was not made.
