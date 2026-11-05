# Native Metal backend for macOS

## Status

- **Goal:** a libplacebo Metal backend on macOS beside Vulkan/MoltenVK, chosen at run time with `--gpu-api`, shipped in `Player.app`, and measured against MoltenVK. The default stays Vulkan.
- **Last accepted revision:** `8cdc0c8` (M2).
- **Accepted:** M1 `410e9a3`, M3 `586688e` (Linux suite 385 passed; first run 13 m 06 s wall including the image and Linux dependency build), M2 `8cdc0c8`. **Red:** none. **Parked:** none.
- **Next:** M4 (Metal backend in `player-output`).
- **Resume:** "Execute `native-metal.md` from **Status**." First command: `scripts/build-deps.sh`.
- **Learned:** the MR's own Metal suite passes on the patched v7.387.0 (go). This cargo nightly puts each test binary in its own `build/<pkg>/<hash>/out` directory, so dev ICD discovery uses a macOS cargo runner (`.cargo/pinned-moltenvk.sh` sets `VK_DRIVER_FILES`) for cargo-run processes and an ICD beside `build/mpv` for the dev binary. Cargo finds `.cargo/config.toml` from the working directory, so it lives at the repo root.

## Charter

- Decide and keep going. Record a decision here only when a later slice depends on it, by rewriting the relevant section.
- The owner's grant is everything in **Execution boundary**. A plan does not grant missing permission; park an item that needs more.
- Hard stops park the item: an action outside the repo or irreversible, a boundary this doc forbids, a loop that cannot check anything. Never push.
- A failing item gets the `root-cause` method before any retry: name the cause, then change one thing. After three failed attempts, park it on its branch with the cause and a retry condition in **Status**.
- Fix egregious problems met on the way in their own commit: a crash or hang over 10 s, a gate that ran under a minute now taking over 3× as long, or default-path GUI CPU worse than baseline by more than 10% and more than the spread (the `scripts/measure.sh cpu` rule). Add anything smaller to `TODO.md` and leave it.
- Parity before optimization. The comparison (M8) waits until both backends pass every parity check.
- One writer by default. The only parallel pair is M1 ‖ M3 (disjoint files), in worktrees at `.work/wt/<item>`, following `orchestrate`'s worktree lifecycle. Scratch goes in `.work/tmp`, never `/tmp`.
- After each accepted slice, run `orchestrate`'s parent steering checkpoint (`references/campaigns.md`, "Parent steering checkpoint"), then update **Status**.
- Commits are Conventional Commits scoped by workstream (`deps`, `output`, `app`, `bench`, `docs`); the item ID goes in the body. Code comments never cite this doc. The repo rules in `AGENTS.md` and the owner's global rules apply. Notably: no formatters; `unsafe` only in the crates `AGENTS.md` lists, each block with its safety argument; only `rg`/`fd` for search.

## Decisions

Owner decisions (2026-11-05):

1. **Carry upstream MR !858 as a pinned patch.** Upstream libplacebo has no Metal backend: none in v7.387.0 (the newest tag), none on master. The only implementation is the closed, unmerged [libplacebo MR !858](https://code.videolan.org/videolan/libplacebo/-/merge_requests/858) by `simonch`, head `ea7599ef9e71f66930b567f25e2c2de73913b38c`. It adds a `pl_gpu` over Metal and compiles shaders GLSL → SPIR-V (shaderc) → MSL (SPIRV-Cross). The draft's alternatives, an `AVSampleBufferDisplayLayer` path modeled on `~/d/p` and a hand-written Metal renderer, are out of scope; neither is a fallback. If M1's go/no-go fails, park the campaign and report.
2. **Switch both GPU devices; keep the readback.** Each frame uses two libplacebo devices. The engine renders YUV→RGBA on a headless device and reads it back to the CPU (`rust/player-engine/src/video.rs:185`, `render_frame`). The window then uploads that RGBA and composites overlays (`rust/player-display/src/mac.rs:157`, `render_to_target`). `--gpu-api` selects the backend for both. Collapsing them into one device is a separate campaign: M9 files a `TODO.md` ticket for it.
3. **Ship both backends in `Player.app`.** The bundle carries both; Vulkan stays the default.
4. **The prescribed decisions below are approved.** That covers `--gpu-api`, no fallback, the `Gpu` seam, and dev builds linking the release prefix. Do not reopen them without new evidence.

Prescribed (reason; what would reverse it):

- **Selector `--gpu-api=auto|vulkan|metal`**, mpv's option name. `auto` means `vulkan`. `metal` exists on macOS only; on Linux, and for any other value, startup fails with `player: --gpu-api: unsupported value <v>` and a nonzero exit. The value is stored as a closed `GpuApi` enum in `Options` and readable as the `options/gpu-api` property. Today `rust/player-app/src/options.rs` silently drops unknown options (`_ => {}` at `:253`), so `--gpu-api` must get its own match arm. *Reverse:* the owner wants a non-mpv name.
- **No fallback.** If Metal creation fails, the error surfaces exactly as a Vulkan failure does today. There is no silent retry on Vulkan.
- **Backend-neutral context in `player-output`.** Rename the public `Vulkan` (`rust/player-output/src/context.rs:6`) to `Gpu`, holding `log`, `gpu`, and a private backend enum: Vulkan `pl_vulkan`, or Metal `pl_metal` under `cfg(target_os = "macos")`. Every texture, format, upload, and download method already runs over `pl_gpu` and stays as it is. Only creation and swapchain creation differ: `Gpu::vulkan()`, `Gpu::vulkan_for_surface(..)`, `Gpu::metal()`, `Swapchain::vulkan(&Gpu, &Surface, ..)`, `Swapchain::metal(&Gpu, layer, ..)`. On Metal, `MacWindow` holds no `Instance` or `Surface` and passes the existing `CAMetalLayer` from `player_mac_window_layer` straight to `pl_metal_create_swapchain`. Callers to update: `player-display/src/{mac,display}.rs`, `player-engine/src/video.rs`, and the tests and examples that name `Vulkan`. No aliases. *Reverse:* none expected.
- **Log identity.** Creation logs `libplacebo: GPU 0: Metal device (libplacebo <version>)` beside the existing `... Vulkan device ...` (`context.rs:68`). A Metal run logs it twice (engine and window) and contains no `mvk-info` or `Vulkan device` lines.
- **The patch.** It is committed as `scripts/patches/libplacebo-metal.patch` and applied by `scripts/build-deps.sh` after extraction. It carries only `meson_options.txt` and `src/**` from the MR diff; `.gitlab-ci.yml` and `demos/**` are dropped. One hunk is rejected against v7.387.0: the `tdep_static` include list in `src/meson.build` (around line 301) needs `inc_dirs` appended by hand. Everything else applies cleanly (checked at preflight). Get the MR diff from `https://code.videolan.org/videolan/libplacebo/-/merge_requests/858.diff` and confirm it is still the head named above. The build script may carry one comment linking the MR, because the patch's origin is a constraint the code cannot express.
- **SPIRV-Cross.** Pin the KhronosGroup tag `vulkan-sdk-$VULKAN_VERSION` (`vulkan-sdk-1.4.363.0`; the archive exists) and checksum it like the other fetches. Build only static `core`, `glsl`, `msl`, and the C API, with the CLI and tests off. Link it into `libplacebo.387.dylib` so the bundle gains no file. Configure libplacebo with `-Dmetal=enabled` beside the existing `-Dvulkan=enabled -Dshaderc=enabled`. Linux (`scripts/build-deps-linux.sh`) is unchanged.
- **Dev builds link the release prefix on macOS.** Homebrew's libplacebo will never have Metal, so `scripts/build.sh`, `cargo test --manifest-path rust/Cargo.toml -p <crate>`, and the examples must link `.work/deps/release`. Preflight showed what that takes: the prefix's dylibs use `@rpath` install names, so dev and test binaries need an rpath to `.work/deps/release/lib`. The prefix's `MoltenVK_icd.json` points at `./libMoltenVK.dylib` next to itself, which does not exist; `scripts/bundle.py` rewrites it only inside the bundle. The prefix's loader also does not search the prefix for ICDs. With an rpath and `VK_DRIVER_FILES` set to a JSON with the absolute library path, all 10 `player-output` tests pass. Drop the `libplacebo` pin from `scripts/dev-deps.sh`, and any other pin that falls out of use. `scripts/build.sh` on macOS requires `scripts/build-deps.sh` to have run. *Reverse:* a Homebrew libplacebo with Metal.
- **Comparison method.** It reuses `scripts/bench.py` (`frames`, `gui-cpu`, `seek`) and `scripts/measure.sh` rules: `perf-gate-ready` before and `-1` after, interleaved runs, medians with spread, exit 75 when the host is busy. Both backends run in the same binary, Vulkan as baseline. Every row names its backend.

Delegated (bound; tie-break):

- Rust-side names and internal shape inside the seam above. Bound: no new public type beyond `Gpu`, `GpuApi`, and the swapchain constructors. Tie-break: the smaller diff.
- How dev builds get the rpath and find the ICD (M2). Bound: no absolute `.work` or `/opt/homebrew` path in the release binary or bundled dylibs (`tests/run.sh release` checks this). Nothing the user must export in their shell. No fallback to Homebrew. Tie-break: build-script output over wrapper scripts.
- The `bench.py` flag that passes `--gpu-api` to the player. Tie-break: a repeatable `--player-arg`.
- Which `frame.rs`/`render.rs`/`context.rs` tests run per backend. Bound: every GPU test runs on both backends on macOS, with the same tolerances as today (`frame.rs:501`, `:587`, `:686`).

## Baseline

Preflight measured these on `1c4de78`, macOS arm64 (Apple M5 Max), warm caches.

| Command | Result | Wall |
| --- | --- | --- |
| `cargo test --manifest-path rust/Cargo.toml -p player-output` | 10 passed | 2 s |
| `scripts/build.sh` | ok | 4 s |
| `tests/run.sh` gates `cover-art gui play playlist cli state tui osc osc-input input failures` | all pass | 32 s total |
| `tests/run.sh mpv-diff` | 134 passed | 3 m 11 s |
| `scripts/build-deps.sh` after a script edit (the input hash rebuilds every dependency) | ok | 68 s |
| `scripts/release.sh` with dependencies ready | ok | 11 s |
| `tests/run.sh release` | 14 passed | under 1 min |
| `tests/run.sh` gates `fixtures corpus audio hwdec seek local-paths local-only rust-link traces dispatch` | all pass | 96 s total (corpus 62 s) |
| Full macOS suite (the two gate rows above plus `release`) | all pass | about 2.5 min |

Known red: none. Not run at preflight: the Linux suite. `cover-art` is new in `LINUX_SUITE` and has not run on Linux. Distributable size: about 19.7 MB against the 29,542,136 B budget.

## Sequence

The loop: an item ends when its acceptance command passes on the integrated tree. Red work never lands on `main`. The full macOS suite (`tests/run.sh suite`) runs after M5, M6, and at close. The Linux check (`scripts/linux-container.sh tests/run.sh suite`) runs after M3 and at close; report its wall time the first time.

**Phase 1: foundation and parity**

- **M1 · deps · patched libplacebo with Metal.** Prereqs: none. Owns `scripts/build-deps.sh`, `scripts/patches/libplacebo-metal.patch`.
  Go/no-go first. Build the patched tree separately with `-Dtests=true -Dmetal=enabled` in `.work/tmp/metal-tests` and run its Metal GPU tests (`meson test -C .work/tmp/metal-tests/build`, Metal suite). They must pass. If they still fail after three attempts, park the campaign.
  Acceptance: `scripts/build-deps.sh && nm -gU .work/deps/release/lib/libplacebo.387.dylib | rg -q '_pl_metal_create$' && ! otool -L .work/deps/release/lib/libplacebo.387.dylib | rg -q spirv-cross && strings .work/deps/release/lib/libplacebo.387.dylib | rg -qx 'v7\.387\.0'`, plus `scripts/release.sh && tests/run.sh release` still green.
- **M2 · deps · dev and tests link the release prefix.** Prereqs: M1. Owns `rust/player-output/build.rs`, `scripts/build.sh`, `scripts/dev-deps.sh`, the ICD written by `scripts/build-deps.sh`, and any build-script rpath in `rust/player-app`. Acceptance, in a fresh shell with no `PKG_CONFIG_*`, `VK_*`, or `DYLD_*` set: `cargo test --manifest-path rust/Cargo.toml -p player-output && scripts/build.sh && otool -L build/mpv | rg -q '@rpath/libplacebo' && tests/run.sh gui && scripts/release.sh && tests/run.sh release`.
- **M3 · output · backend-neutral seam, no behavior change.** Prereqs: none (may run beside M1). Owns `rust/player-output/src/{context,swapchain,lib,frame,render}.rs`, `rust/player-display/src/{mac,display}.rs`, `rust/player-engine/src/video.rs`, and the tests and examples naming `Vulkan`. Acceptance: `cargo test --manifest-path rust/Cargo.toml -p player-output -p player-engine -p player-display && scripts/build.sh && tests/run.sh gui`, then the Linux check.
- **M4 · output · Metal backend.** Prereqs: M2, M3. Owns `rust/player-output/src/{sys,context,swapchain}.rs` and the `player-output` tests. Add the Metal FFI from the patched `metal.h`: `pl_metal_t`, `pl_metal_params`, `pl_metal_swapchain_params`, `pl_metal_create`/`destroy`/`create_swapchain`, all `cfg(target_os = "macos")`. Extend `transcribed_layouts_match_pinned_headers` (`sys.rs:418`) with their sizes and offsets. Run every GPU test on both backends. Add `metal_matches_vulkan`: the same YUV420 and NV12 frames with an overlay, rendered on each backend, must differ by at most 2 in any channel. Acceptance: `cargo test --manifest-path rust/Cargo.toml -p player-output` shows each GPU test passing per backend, plus the new test.
- **M5 · app · `--gpu-api` end to end.** Prereqs: M4. Owns `rust/player-app/src/{options,app,window,properties}.rs`, `rust/player-engine/src/{backend,video}.rs` (threading `GpuApi` into `EngineVideoOutput::open`), `rust/player-display/src/mac.rs` (`MacWindow::open(.., GpuApi)`), `rust/player-display/examples/mac_window_check.rs` (backend from argv), `tests/gui.py`, `tests/harness.py`. The `gui` gate runs once per backend on macOS. A new `gpu-api` gate in `SUITE` checks four things. Under `--gpu-api=metal`, the GUI gate passes and the log has exactly two `Metal device` lines and no `mvk-info`. `options/gpu-api` reads back the value. `--gpu-api=bogus` exits nonzero with the message above. With no flag, the run logs `Vulkan device`. Acceptance: `scripts/build.sh && tests/run.sh gpu-api && tests/run.sh gui && cargo run --manifest-path rust/Cargo.toml -p player-display --example mac_window_check -- metal`, then the full macOS suite.
- **M6 · deps · bundle both.** Prereqs: M5. Owns `scripts/bundle.py`, the `gate_release` section of `tests/harness.py`. The release gate also requires `_pl_metal_create` in the bundled libplacebo and no `spirv-cross` dylib. Run `release: renders through the bundled libplacebo` once per backend. The expected closure set is unchanged. Acceptance: `scripts/release.sh && tests/run.sh release && scripts/measure.sh final`. Size budgets must pass; timing budgets may print `perf gate deferred: host busy`, which does not count as a pass. Then the full macOS suite.

**Phase 2: hardening**

- **M7 · output/docs · edge cases and documentation.** Prereqs: M6. Cases: resize and fullscreen round trips on Metal (`mac_window_check -- metal`); teardown order (swapchain → renderer → `pl_metal` → window); file-to-file reload and seek on Metal (`tests/run.sh seek` and `osc-input` under Metal; `Mpv` in `tests/harness.py` strips `PLAYER_OPTIONS`, so pass the backend as a gate argument); the error when Metal creation fails. Docs: `docs/architecture.md` (renderer backends, carried patch), `docs/scope.md` (CLI `--gpu-api`), `docs/install.md` (dev build now needs `scripts/build-deps.sh`; `dev-deps.sh` changes), `docs/manual-checklist.md` (rows in **Needs environment**), `AGENTS.md` commands if any changed. Acceptance: `tests/run.sh suite` green with `--gpu-api=metal` applied to the GPU gates, and `mdtool check --untracked` (or a manual link check) clean on the touched docs.

**Phase 3: comparison and optimization**

- **M8 · bench · Vulkan vs Metal.** Prereqs: M7. Owns `scripts/bench.py`, `docs/metrics.md`. Run `bench.py frames`, `gui-cpu` (candidate `--gpu-api=metal`, baseline `--gpu-api=vulkan`, same binary), and `seek` on both fixtures, interleaved. Write a "Metal backend" section in `docs/metrics.md`: medians and spread per scenario and backend, host-busy deferrals stated, and a note that both backends pay the engine readback. Optimize only a measured gap above the spread with a named cause; for each, the before/after numbers go in the commit body. Acceptance: `scripts/bench.py frames --player-arg=--gpu-api=metal` (and the other modes) exit 0, or exit 75 with the deferral recorded in **Status**.

**Close**

- **M9 · close.** Prereqs: M8. Delete code the seam left dead: unused Vulkan-only helpers, and `sys.rs` items the `dead_code` warnings name that M3 or M4 orphaned. Prove liveness with `rg` over `rust/` and a clean build. Run the `review` skill over `1c4de78..HEAD` and fix every P1 and P2 finding. File `TODO.md` tickets for: (a) one GPU device with no CPU readback; (b) VideoToolbox zero-copy through `pl_metal_wrap_iosurface` (ties to PLAY-20); (c) EDR/10-bit output on Metal, since the patch's swapchain fixes `BGRA8Unorm`; (d) the OSD error `OSC was laid out for 2560x1440 but the overlay is 3024x1898`, seen at preflight in a default-fullscreen window, if it still reproduces. Acceptance: `tests/run.sh suite`, the Linux check, `cargo test --manifest-path rust/Cargo.toml`, and `scripts/measure.sh final` green; **Status** rewritten as final.

## Acceptance

| Deliverable | Proof | Fails when |
| --- | --- | --- |
| Patched libplacebo exports Metal, still reports the pinned version | M1 command | symbol missing, a `spirv-cross` dylib appears, or the version string is not exactly `v7.387.0` |
| Default path unchanged | `tests/run.sh suite` with no flag; `gui` logs `Vulkan device` | any gate red, or a `Metal device` line without the flag |
| Metal renders correctly | M4 tests: every GPU test per backend, `metal_matches_vulkan` ≤ 2 | any channel delta over the existing tolerance |
| Selection works end to end, fails loudly | `tests/run.sh gpu-api` | wrong device count, Vulkan lines under Metal, bad value accepted |
| Release ships both | `tests/run.sh release` (both backends render) + `scripts/measure.sh final` size budgets | either backend fails to render; distributable over 29,542,136 B; executable over 10 MB; extra bundled dylib |
| Linux untouched | Linux check after M3 and at close | any Linux gate red |
| Comparison recorded | `docs/metrics.md` "Metal backend" section from M8 | numbers without spread, without backend attribution, or from a busy host presented as a result |

## Needs environment

The owner runs these; they do not hold the campaign open. M7 adds them to `docs/manual-checklist.md`.

- Visual parity on a real display: `build/mpv --gpu-api=metal FILE` beside `--gpu-api=vulkan` for color, scaling, subtitle and OSC placement, resize, fullscreen.
- HDR/EDR content and an external or ProMotion display: frame pacing and tone mapping (`bench.py frames` on that display).
- Any non-Apple-Silicon Mac: the patch was validated only on Apple GPUs.
- Keyboard focus and window behavior under `--gpu-api=metal` (as PLAY-21, agents cannot test focus).

## Execution boundary and non-goals

Authorized: edit anything under the repo; fetch the MR diff and the pinned SPIRV-Cross archive; build under `.work/`; run Docker through `scripts/linux-container.sh`; commit to `main` as slices are accepted.

Forbidden: push or tag; `make install` or other writes outside the repo; notarizing; changing the pinned Rust nightly; changing the libplacebo version pin; adding a glibc build; any fallback from Metal to Vulkan; an `AVSampleBufferDisplayLayer` or hand-written Metal renderer.

Non-goals: removing MoltenVK; making Metal the default; collapsing the engine readback (ticket at M9); hardware decode wiring (PLAY-20); Linux Metal (does not exist).

Contract changes for the owner:

- New CLI option `--gpu-api=auto|vulkan|metal` and property `options/gpu-api`; unsupported values are a startup error.
- `player-output` public API: `Vulkan` becomes `Gpu`; swapchain construction becomes per-backend constructors. In-tree callers only.
- Build contract: macOS dev builds and Rust tests link `.work/deps/release`, so `scripts/build-deps.sh` must run before `scripts/build.sh`; `scripts/dev-deps.sh` no longer pins Homebrew libplacebo.
- Dependencies: a carried LGPL-2.1 patch on libplacebo (unreviewed upstream, owned by this repo from now on) and SPIRV-Cross (Apache-2.0), statically linked into `libplacebo.387.dylib`. Gates they must stay within: the distributable and executable size budgets and no new bundled dylib (always), and `scripts/build-deps.sh` wall time (reported in the M1 commit body, not gated).

## Reference

- Rendering boundary: `rust/player-output/src/context.rs` (`Vulkan`, creation at `:24`), `swapchain.rs` (`pl_vulkan_create_swapchain` at `:26`; mailbox, depth 3), `frame.rs` (`render_frame` `:53`, `render_to_target` `:112`, tests from `:415`), `sys.rs` (layout test `:418`), `instance.rs` (`create_metal_surface` `:80`).
- Callers: `rust/player-display/src/mac.rs` (`MacWindow::open` `:148`), `rust/player-display/src/display.rs` (Linux), `rust/player-engine/src/video.rs` (`EngineVideoOutput::open` `:184`), `rust/player-engine/src/backend.rs:643`, `rust/player-app/src/window.rs` (`HostWindow::open` `:27`).
- Patched API (`src/include/libplacebo/metal.h` in the MR): `pl_metal_create(log, {device, queue})` (NULL means the system default device), `pl_metal_create_swapchain(metal, {layer})` (sets the layer's device, `BGRA8Unorm`, `framebufferOnly = NO`), `pl_metal_wrap_tex`, `pl_metal_wrap_iosurface`. Shaders still go through shaderc.
- Build and bundle: `scripts/build-deps.sh` (libplacebo meson at `:184`), `scripts/bundle.py` (MoltenVK and ICD at `:84-116`), `tests/harness.py` `gate_release` (closure set, ICD, bundled-render check).
