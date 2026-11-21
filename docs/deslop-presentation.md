# Deslop: presentation

Component: `player-output`, `player-display` (with `bridge/metal_window.m`), `player-overlay` (8.1k first-party lines). Base `e39e5018`; tickets DSLP-7 (Types), DSLP-11 (Tests), DSLP-15 (Dead code). Host, environment and worktree setup are as in [deslop-playback-pipeline.md](deslop-playback-pipeline.md): macOS 27.2 arm64, `cargo 1.102.0-nightly (c07ac2c4d 2026-11-06)`, busy host, `TMPDIR=$PWD/.work/tmp CARGO_BUILD_JOBS=4`.

Headless only. `tests/run.sh gui` and `gpu-api` open windows and were not run (the owner deferred headful tests); `osc` and `osc-input` use `--vo=null` and were. `cargo test` for these crates is headless: the Metal and Vulkan (MoltenVK) tests render offscreen, no window, no audio. `player-display` has no macOS unit tests; its Wayland tests in `tests/present.rs` are Linux-only and need `--features present-gate`.

Baseline before any change: `cargo test -p player-output -p player-display -p player-overlay` exit 0 (output 28 + `metal_memory` 1, display 0, overlay 21 + `typography` 15); `tests/run.sh osc` 14 passed, `osc-input` 30 passed. `fixtures` is red at base (DSLP-18) and does not touch this component.

## DSLP-7 Types

Measured with `cargo clippy -p player-output -p player-display -p player-overlay --lib --message-format=json -- -W clippy::wildcard_enum_match_arm -W clippy::match_wildcard_for_single_variants -W clippy::cast_possible_truncation -W clippy::cast_sign_loss -W clippy::unwrap_used -W clippy::expect_used -W clippy::redundant_clone -W clippy::needless_pass_by_value -W unreachable_pub -W dead_code`. This sees macOS code only: `player-display/src/display.rs`, `player-overlay/src/text/linux.rs` and `player-display/tests/present.rs` are Linux-only and were read, not linted.

| Candidate | Hits | Disposition |
| --- | ---: | --- |
| `clippy::unwrap_used` | 2 + 4 | 2 in the IOSurface import ring (`output/context.rs`), 4 `CARGO_MANIFEST_DIR` reads in `output` and `display` `build.rs`. All removed. Not adopted as a lint (below) |
| `clippy::expect_used` | 2 + 4 | 2 in `overlay/compose.rs` (`self.stable` bound twice); removed. 4 in `overlay/build.rs` (Linux-only code path, unverifiable here), left |
| `clippy::redundant_clone` | 0 | no signal |
| `clippy::needless_pass_by_value` | 1 | rejected: `release_import(lease: ImportedSurface)` takes the lease so dropping it releases the textures |
| `clippy::wildcard_enum_match_arm` | 3 | rejected: `PixelLayout::storage_depth` default 8 bit, and two `match layout` arms that force RGB layouts to full range; the enum is internal and the default is the intent |
| `clippy::match_wildcard_for_single_variants` | 0 | no signal |
| `clippy::cast_possible_truncation` | 49 | rejected: 32 are float to `u8` or `usize` after `clamp` or `max(0.0)` (rasterizer coverage, compositing, text glyph offsets), the rest are FFI counts (`extensions.len() as u32`, `texs.len() as c_int`) bounded by construction. No defect found reading each site |
| `clippy::cast_sign_loss` | 32 | rejected: all follow `.max(0.0)` or `.max(0)` clamps, or `checked_mul` on values already validated |
| `unreachable_pub` (rustc) | 85 | adopted for `player-output` and `player-overlay`; see DSLP-15 |

`unwrap_used`/`expect_used` are not adopted here, unlike the playback crates. A `deny` would also hit Linux-only production code that cannot be compiled on this host (`player-overlay/src/text/linux.rs:690,810,811,1052` use `expect`) and the whole of `player-display/tests/present.rs` (`expect` in helpers outside `#[test]` functions), and `player-output/tests/metal_memory.rs`. Adopting it blind would risk a red Linux `make lint`. Filed as a finding.

Typing contracts fixed (escape hatches removed), each with its witness:

| Contract | Change | Witness |
| --- | --- | --- |
| IOSurface import ring keeps at most four leases and releases completed ones in order | `pop_front_if(import_complete)` instead of `front()` then `pop_front().unwrap()`; the over-capacity pop is matched (`output/context.rs:168-176`) | `engine/src/video.rs:592` `videotoolbox_nv12_import_matches_transfer_through_seek_and_shutdown` (ignored by default: it needs real VideoToolbox and Metal but opens no window; run with `cargo test -p player-engine -- --ignored videotoolbox_nv12`, passed). Gap: replacing the predicate with `|_| false` still passes, so the early-release path has no witness |
| cached stable overlay layer | `let Some(cached) = self.stable.as_ref() else { return Ok(None) }` bound once and reused (`compose.rs:369`) | `compose.rs:581` `cached_changing_layers_render_identically_to_a_full_render`, `a_stable_hit_does_not_relayout_its_text`, `multi_layer_stable_renders_identically_without_the_cache` |
| build scripts | `env!("CARGO_MANIFEST_DIR")` (as `player-media/build.rs` does) | build of the workspace |

Escape hatches (`rg -c '\bunsafe\b|#!?\[allow\(|\.unwrap\(\)|\.expect\('` over the three crates, tests included): 246 at base, 242 now. Eight production hatches were removed (4 `build.rs`, 2 ring, 2 compose); the new tests add 4 `unwrap`/`expect` calls. `unsafe` is unchanged (119 sites in `src`; no `#[allow]` added or removed).

Invariant check (AGENTS.md: each native-boundary `unsafe` block states its safety argument): 123 `unsafe` sites in the three crates; 19 lack a nearby `SAFETY` comment, 15 of them `extern` declarations or function-pointer types, and 4 are `unsafe { std::env::set_var(..) }` blocks in the Linux-only `tests/present.rs` (test code, not changed).

Acceptance: `cargo clippy --workspace --all-targets -- -D warnings` exit 0; `cargo test -p player-output -p player-display -p player-overlay` exit 0 (31 + 1, 0, 24 + 15); `tests/run.sh osc` 14 passed and `osc-input` 30 passed against a rebuilt `build/mpv`, exit 0. `gpu-api` not run (headful).

## DSLP-11 Tests

Pyramid: unit tier 60 `#[test]` functions in the three crates (6 `#[ignore]`d for Linux static-pie or real-device reasons), about 4 s warm of which the Metal and MoltenVK render tests are most (`player-output` 31 tests in 0.9 to 3.7 s depending on load, `metal_memory` 1 to 6 s). Gate tier: `osc` 14 and `osc-input` 30 checks in 1 s and 6 s, both `--vo=null` (no presentation path); the pixels reach a real surface only in `gui` and `gpu-api`, which are headful. So the display-list, rasterizer, compositing and render-geometry logic is guarded only by the unit tier, and that is where the hand mutation run went.

`cargo-llvm-cov` and `cargo-mutants` are not installed (Needs environment). Bounded hand mutation run, one mutant at a time, each run with `cargo test -p <crate>`: `compose.rs` (`blend_onto`, `to_image`, `pixel_box`; 13 mutants) and `frame.rs` (`display_size`, `fit_crop`, `storage_depth`, RGB range override; 9 mutants). 22 mutants: 11 killed by the existing tests, 11 survived.

| Mutant | Before | After (killer) |
| --- | --- | --- |
| `blend_onto` row clip `>=` to `>` | survives | `blending_clips_an_overhanging_overlay_without_wrapping_rows` |
| `blend_onto` column clip `>=` to `>` | survives | same |
| `blend_onto` drop the `+127` rounding term | survives | `blending_rounds_each_channel_and_keeps_the_backdrop_alpha` |
| `blend_onto` skip-transparent check removed | survives | equivalent: `(t * 255 + 127) / 255 == t` for every `t`; no test can see it |
| `pixel_box` empty box accepted | survives | `pixel_boxes_cover_the_rect_clamped_to_the_frame` |
| `pixel_box` right edge not clamped | survives | same |
| `pixel_box` bottom edge not clamped | survives | same |
| `pixel_box` left edge `floor` to `ceil` | survives | same |
| `display_size` 270 degrees not transposed | survives | `display_size_applies_aspect_with_rounding_and_swaps_for_quarter_turns` |
| `display_size` aspect `round` to `floor` | survives | same |
| RGB layouts keep source range and matrix | survives | `rgb_layouts_ignore_the_source_range_and_matrix` (Metal and Vulkan) |
| short-frame check `<` to `<=`, `keep = 256 - alpha`, target alpha ignores the backdrop, unpremultiply skipped, alpha truncated, 90 degrees, aspect ignored, `fit_crop` 270, aspect axis, scale `max`, P010 depth | killed | `blending_onto_a_frame_mixes_by_overlay_alpha`, `images_composite_at_their_pixel_origin_with_opacity`, `anamorphic_rotation_fits_in_display_coordinates`, the Metal and Vulkan rotation, letterbox and P010 tests |

Final: 21 of 22 mutants killed, 1 equivalent. Cost of the three added test functions in `player-overlay` and two in `player-output` (one table test, one GPU test run on Metal and Vulkan): well under 1 s warm. Contract, guard, disposition:

| Contract | Guarding tests | Disposition |
| --- | --- | --- |
| overlay display list to pixels (fill, stroke, clip, coverage, opacity) | `compose.rs:557-700`, `raster.rs` (7) | asserted; the mutated decisions above now killed |
| overlay cache equals uncached render | `compose.rs:581`, `multi_layer_stable_renders_identically_without_the_cache` | asserted |
| text shaping (clusters, bidi, emoji, scripts) | `overlay/tests/typography.rs` (15) on macOS Core Text | asserted on macOS; the Linux HarfBuzz path (`text/linux.rs`) has the same file under `cfg` but was not run (Needs environment) |
| video geometry, fit and rotation | `frame.rs:524` and the new table; Metal and Vulkan rotation, letterbox, P010, NV12 tests | asserted |
| Metal and Vulkan parity | `frame.rs` `metal_matches_vulkan`, `swapchain.rs:115`, `context.rs` init tests, `metal_memory.rs` | asserted (offscreen) |
| IOSurface import (VideoToolbox to Metal) | the ignored `engine/src/video.rs:592` test | passes when run explicitly; early-release path unguarded (above) |
| window, input ring, Wayland and Cocoa surfaces | `player-display/src/display.rs` (6 Linux unit tests), `tests/present.rs` (5, Linux, feature-gated); macOS `mac.rs` has none | not verifiable here: Needs environment (headful for macOS, Linux container for Wayland) |
| `none` boundaries in AGENTS.md Verification | none owned by this component (IPC, playlists, media decode and audio devices belong to the application and playback components) | n/a |

One hand-injected fault in a critical decision: `VideoGeometry::display_size` with 270 degrees no longer transposing. Before the new test it survived all 28 unit tests. In the second mutation run, `cargo test -p player-output` with the fault failed `frame::tests::display_size_applies_aspect_with_rounding_and_swaps_for_quarter_turns` (the runner records the killing test name). The runner rewrites the original file after every mutant, and `git status` showed no diff in `frame.rs` or `compose.rs` beyond the committed tests afterwards.

Cost of the whole step against the budget: no gate was added; `make test` was not run because it includes the headful gates.

## DSLP-15 Dead code

Tools: rustc `dead_code` and `unreachable_pub` over the three crates: `dead_code` 0; `unreachable_pub` 85 (83 in `player-output/src/sys.rs`, 2 in `player-overlay/src/text/mac.rs`). `cargo-machete` and `cargo +nightly udeps` are not installed (Needs environment); a manual search of each `Cargo.toml` dependency against its sources found none unused (`player-display` Wayland crates in `display.rs`, `cc` in its build script, `player-output` in both display backends). `debloat` was not run.

| Finding | Evidence | Disposition |
| --- | --- | --- |
| 83 `pub` declarations in private `mod sys` of `player-output` (plus one `cfg(test)` constant) | `lib.rs` re-exports only five `PL_COLOR_*` constants by name; no other crate names the rest (`rg` of each symbol under `crates/` outside `player-output`) | `pub(crate)`; `dead_code` stays at 0, so no FFI declaration was dead |
| `Backend::new` and `Backend::render` of both overlay text backends | `text.rs` selects the backend by `cfg`; the Linux counterparts at `text/linux.rs:515,1001` had the same visibility | `pub(super)` on both platforms (the Linux edit was not compiled here) |
| `#[allow(dead_code)]` on `Window::surface` and `Window::instance` in `player-display/src/display.rs:340,344` | Linux-only; the fields are held for drop order and never read | left: removing them needs a Linux build |

`unreachable_pub = "deny"` is set in `player-output` and `player-overlay`. Negative control as in the playback record. It is not set in `player-display`: the macOS build reported 0 hits there, but the crate is Linux-heavy and the Linux half could not be compiled.

Consumers and platforms: `cargo clippy --workspace --all-targets -- -D warnings` exit 0 (player-engine, -backend, -app, -tui, -gui build against the narrowed crates); `cargo test` for the three crates and `osc`/`osc-input` pass (above). Platforms: macOS arm64 only.

Public items with no caller anywhere (source, tests, docs, scripts): `OverlayImage::alpha_at` (`player-overlay/src/compose.rs:20`), `Anchor::CENTER` (`player-overlay/src/display.rs:196`); `Renderer::render_rgba` (`player-output/src/render.rs:22`) has only a test caller. Consumer questions, not removed.

## Needs environment

- `cargo-llvm-cov`, `cargo-mutants`, `cargo-machete`, `cargo-udeps`: not installed; no coverage numbers.
- Headful: `tests/run.sh gui`, `gpu-api`; the macOS window path (`player-display/src/mac.rs`, `bridge/metal_window.m`) and the OSC drawn on a real surface.
- Linux container: `player-display/src/display.rs`, `player-overlay/src/text/linux.rs`, `tests/present.rs` (needs weston and `--features present-gate`); the new `pub(super)` edits and `unreachable_pub` denies there were not compiled. `make lint` in the container must confirm them.
- `tests/run.sh fixtures` is red at base (DSLP-18).
