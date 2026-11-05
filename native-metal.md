# macOS native Metal backend plan (draft)

## Goal

Add a **native Metal-backed rendering path on macOS** alongside the current Vulkan→MoltenVK path, keep both, and make them comparable at runtime (and in measurements). “Compare the two” means: same input/content, same window/presentation conditions where possible, and an explicit way to select backend + collect timings/behavior.

Default behavior stays unchanged (current MoltenVK/Vulkan path) unless a selector is explicitly set.

## Prior art: `~/d/p` (take with a grain of salt)

`~/d/p` is useful prior art for a *macOS-native presentation* approach, but it is **not** the same architecture as `play`.

- It hands decoded frames to Apple’s display stack: `AVSampleBufferDisplayLayer` driven by a `CMTimebase` (`~/d/p/src/video_out.rs`: `enqueueSampleBuffer`, `controlTimebase`, `CMTimebaseSetTime/Rate`, drift correction, seek flush). Frames are `CVPixelBuffer`s.
- Decode/display path: FFmpeg demux/decode with VideoToolbox hwaccel when available; for VideoToolbox frames it retains `AVFrame.data[3]` as a `CVPixelBufferRef` (`~/d/p/src/decode_video.rs:get_hw_format` prefers `AV_PIX_FMT_VIDEOTOOLBOX`). Software fallback converts to NV12 into an **IOSurface-backed** `CVPixelBuffer` (required by `AVSampleBufferDisplayLayer`) and copies planes.
- Windowing/AppKit integration is straightforward layer hosting (`~/d/p/src/window.rs` adds the display layer as a sublayer, resizes with autoresizing mask).
- OSD/subtitles are handled as separate UI layers/state, not composited by a GPU video renderer.

**Caveat (owner-reported):** seek performance in `~/d/p` really sucks. Treat its seek/sync/enqueue model as reference material, not a target to copy blindly; measure any adaptation in `play`.

**Implication for `play`:** `~/d/p` shows a viable native macOS video presentation/display primitive, but `play` currently renders via libplacebo and composites overlays (subtitle text, `show-text`/OSD message, OSC display list) into the render target (`docs/architecture.md:25,31,39`, `rust/player-output/src/frame.rs` takes an `Overlay`; `render_to_target`/present path composes). An `AVSampleBufferDisplayLayer`-style path would change where video is presented and how overlays are composed/HDR/tone-mapping behave. That makes it a different class of comparison than “same renderer pipeline, different GPU backend”.

## Current coupling (what has to move)

- `rust/player-output` is the rendering boundary, but it is Vulkan-shaped today:
  - `context.rs` owns libplacebo init as `pl_vulkan_create` (`Vulkan`), plus `find_format`/`create_tex`/`upload_strided`/`destroy_tex`/`download` - `rust/player-output/src/context.rs`.
  - `swapchain.rs` wraps `pl_vulkan_create_swapchain` + generic `pl_swapchain_*` - `rust/player-output/src/swapchain.rs`.
  - `render.rs` wraps `pl_renderer` (already mostly backend-agnostic over `pl_gpu`/`PlRenderer`). `frame.rs` does plane upload + `pl_render_image` and takes `&Vulkan` for GPU ops - `rust/player-output/src/{render,frame}.rs`.
  - `instance.rs`/`sys.rs` contain Vulkan instance/surface (`VK_EXT_metal_surface`, `vkCreateMetalSurfaceEXT` on macOS) and transcribed libplacebo/Vulkan layouts with layout tests - `rust/player-output/src/{instance,sys}.rs`.
- `rust/player-display/src/mac.rs` wires a concrete stack: `Instance` → `create_metal_surface(layer)` → `Vulkan::create_for_surface` → `Renderer` → `Swapchain::open(&Vulkan, &Surface, ...)` → `render_to_target(&self.vulkan, ...)`.
- Build/release: macOS `scripts/build-deps.sh` builds libplacebo with `-Dvulkan=enabled -Dshaderc=enabled -Dopengl=disabled` and bundles Vulkan loader + MoltenVK + shaderc (`scripts/bundle.py`, `tests/harness.py` expectations). Dev deps pin Vulkan/MoltenVK/shaderc (`scripts/dev-deps.sh`).
- Option plumbing: `player-app/src/options.rs` currently *accepts* (consumes) `--gpu-context` and `--vulkan-swap-mode` but does not store them in `Options` (the catch-all arm handles them as known/valued options). Any runtime backend selector must be explicitly parsed/stored and passed through to display/backend creation.

## Viable approaches

### 1) libplacebo `pl_metal` backend (best fit for an apples-to-apples backend comparison)

Keep libplacebo as the renderer (preserves the existing video-correct path: plane mapping, color/transfer, scaling, tone mapping, overlay blending via `frame.rs`/`render.rs`). Add a Metal GPU context (`pl_metal_*` / `CAMetalLayer` params, per pinned libplacebo API) and a Metal-appropriate swapchain/present path, behind the same high-level operations.

- *Pros:* closest to “same rendering pipeline, different GPU backend/present path”; reuses most of `frame.rs`/`render.rs` and overlay composition; fits “alongside MoltenVK”.
- *Cons/unknowns:* depends on pinned **libplacebo v7.387.0** having a usable Metal backend and on meson build flags/symbol availability (can Vulkan and Metal coexist in one build?). Needs new FFI transcriptions in `sys.rs` (`PlMetal`/params/symbols, any Metal swapchain creation) + layout tests pinned to 7.387.0, same discipline as Vulkan today. Maturity/perf/HDR/EDR behavior vs Vulkan+MoltenVK is part of what to measure.
- *Comparison quality:* high, if the libplacebo render path is shared.

### 2) Custom Metal renderer (wgpu/metal-rs/hand-written), no libplacebo on that path

- *Pros:* truly Metal-native/Rust-native on macOS.
- *Cons:* large scope and poor parity by default - must reimplement plane upload/layouts, YUV→RGB/color conversion, primaries/transfer, scaling, tone mapping/HDR, and overlay blending semantics. Behavioral drift makes results noisy; overlay/OSC/subtitle composition must be reproduced. Not a small “add alongside”.
- *Use only if* approach (1) is unavailable and the project explicitly accepts a renderer rewrite/parallel implementation.

### 3) Apple native display path (`AVSampleBufferDisplayLayer` + `CVPixelBuffer`), informed by `~/d/p`

Display video through `AVSampleBufferDisplayLayer`/`CMTimebase` (and `CVPixelBuffer`, potentially VideoToolbox-backed where available), similar in primitives to `~/d/p/src/{video_out,decode_video}.rs`.

- *Pros:* very macOS-native presentation; can reduce copies/GPU shader work for the video image path in some cases; aligns with Apple frameworks (`~/d/p` shows the CoreMedia/objc2 shape).
- *Cons/foundation mismatch:* bypasses libplacebo’s render pipeline. `play`’s overlays (subtitles/OSD/OSC) are currently composed by libplacebo into the presented frame; AVSBDL shows *video*, so overlay composition needs a new strategy (separate layer/compositor, render overlays to buffers, or hybrid). Color/tone-mapping/HDR behavior follows Apple/CoreMedia/VideoToolbox expectations, which may differ from libplacebo - good to compare, but not the same variable. Seek/sync model differs (`~/d/p` caveat). Integrating VideoToolbox `CVPixelBuffer`s also intersects `player-media`/scheduler decode choices (TODO PLAY-20 wires HW decode through engine; scheduler currently requests software decode).
- *Comparison quality:* useful as a “native Apple presentation” experiment, but not an apples-to-apples comparison to the libplacebo+MoltenVK renderer unless overlay/color/seek behavior is carefully normalized and documented.

**Recommended order:** evaluate **(1)** first for the stated “renderer backend alongside MoltenVK” comparison. Treat **(3)** as a follow-on/spike if the goal becomes “best macOS-native path overall”, with explicit design for overlays/composition and seek.

## Proposed design (keep both, selectable) — assumes approach (1)

1. **Abstract the GPU backend in `player-output` without changing render semantics.**
   - Introduce a backend-neutral “GPU context” surface for the operations `frame.rs` actually needs: renderer creation/log/gpu access as needed, `find_format`, `create_tex` (sampleable + render-target paths as used), `upload_strided`, `download` (for tests/headless helpers that use it), `destroy_tex`. `Renderer`/`render_to_target`/`render_frame` should work over that abstraction (trait, generic, or small enum) rather than hard-coding `&Vulkan`.
   - Keep `Vulkan`/MoltenVK implementation intact as one backend. Add a macOS-only Metal backend implemented via libplacebo metal FFI.
   - Abstract swapchain/presentation: `pl_swapchain_*` start/submit/resize/latency are generic; creation is backend-specific today (`pl_vulkan_create_swapchain`). Model swapchain creation per backend (or a factory) so `MacWindow` doesn’t own `Vulkan`/`Instance`/`Surface` concretely.
   - Preserve public behavior used by tests/other callers; if renaming exported types, provide compatible aliases or update all in-tree call sites (`player-display`, benches/tests, consumers).

2. **Resolve the libplacebo Metal API/packaging unknown early (first gate).**
   - Confirm for pinned **libplacebo v7.387.0**: `pl_metal` availability, headers/symbols/param structs, swapchain creation model for `CAMetalLayer`, and meson options to enable Metal (and whether it coexists with Vulkan in the same build/install). Inspect fetched source under `.work/tmp/release/libplacebo-v7.387.0` (or the equivalent build source) - not by guessing.
   - If 7.387 lacks a viable path: decision point is bumping the libplacebo pin (blast radius: `sys.rs` layout tests, `PLACEBO_API_VERSION`/version strings, dylib naming like `libplacebo.387.dylib`, macOS+Linux dep builds, bundle/harness expectations) or stopping/choosing a different approach. Record the decision.

3. **FFI + safety (`sys.rs`).**
   - Add transcribed Metal structs/constants/symbols with `#[cfg(target_os = "macos")]` and **layout/offset tests against pinned headers**, mirroring the existing Vulkan/Wayland/Metal-surface discipline.
   - Document safety invariants on every `unsafe` block (repo rule). Keep backend-specific unsafe isolated.

4. **macOS display selection (`player-display` + `player-app`).**
   - Plumb an explicit backend selector end-to-end: parse/store it in `player-app/src/options.rs` (note: `--gpu-context` is accepted today but not stored) and pass a backend choice into display/window creation. Decide compatibility vs clarity (extend `--gpu-context` semantics, or add a dedicated option) and document it; default remains MoltenVK/Vulkan.
   - In `rust/player-display/src/mac.rs` `MacWindow::open` (and resize/present assumptions), choose backend. For the Metal path, use the existing `CAMetalLayer` from `metal_window.m` directly (no `VkInstance`/`VkSurfaceKHR`/`VK_EXT_metal_surface`). Manage lifetimes/drop order carefully (swapchain/renderer/gpu before window/layer teardown).
   - Make the active backend unambiguous in logs/stats/build stamp (today logs “Vulkan device”; e.g. `backend=vulkan+moltenvk` vs `backend=metal(libplacebo)`).

5. **Build & packaging (both kept).**
   - `scripts/build-deps.sh`: enable/build required libplacebo Metal support *in addition* to Vulkan if supported, keeping Vulkan+MoltenVK+shaderc outputs. Ensure stamp/input-hash behavior invalidates correctly when meson options change.
   - `scripts/bundle.py` and `tests/harness.py`: currently require/bundle MoltenVK + Vulkan loader dylibs and assert specific framework names/ICD. For “compare the two” in one build, prefer bundling both backends’ runtime deps and selecting at runtime; update assertions accordingly. If Metal adds/removes dylibs, update the expected closure.
   - `scripts/dev-deps.sh` / `scripts/build.sh`: keep dev flow working; don’t break Linux.
   - Linux remains Vulkan/Wayland. Gate all Metal FFI/build code with `target_os = "macos"`.

6. **Validation & comparison plan (define before/with implementation).**
   - *Correctness parity:* parameterize backend-neutral `player-output` tests where feasible (macOS/headless constraints apply; some existing Vulkan tests are ignored on Linux static-pie contexts - keep that discipline). Add Metal-specific smoke/parity tests: RGBA passthrough, YUV420/NV12 mapping, overlay alpha/blend, crop/fit/flip. Compare within tolerance.
   - *Behavior checks:* run relevant macOS suite/GUI gates on both backends; confirm **no regressions on the default (MoltenVK)** path.
   - *Performance comparison:* repeatable matrix (codecs, pixel formats/layouts, resolutions, HDR if applicable, windowed/fullscreen, with/without overlays/OSC/subtitles). Measure frame pacing/present latency, dropped frames (`frame-drop-count` where meaningful), CPU/GPU impact, and power/thermals qualitatively if relevant. Use existing measurement practice (`scripts/measure.sh` patterns; automated headless `--vo=null` gates don’t cover GUI renderer behavior - owner-run/real-window measurements needed) and the benchmark-checklist mindset: like-for-like, warmup, interleaving, report variance; don’t claim wins from noisy runs. Attribute every result to `vulkan+moltenvk` vs `metal`.
   - *Visual parity:* color/tone mapping, subtitle/OSC overlay positioning, scaling/crop, resize/fullscreen, HDR behavior. Expect close results under approach (1), but verify.
   - *Seek (informed by `~/d/p`):* include interactive/step seek cases in the comparison; `~/d/p` had poor seek performance with an AVSBDL/CMTimebase model - if approach (3) is ever prototyped, treat seek as a primary risk.

## Risks / watch-outs

- **libplacebo v7.387.0 Metal availability/maturity** is the main unknown for approach (1); resolve before deep refactors.
- **Present/timing differences** (swapchain present modes, vsync/mailbox semantics, EDR/HDR colorspace, `flipped`/coordinate conventions) can confound comparisons - log/normalize and document.
- **Overlay composition** is central in `play` (libplacebo composes overlays). Any path that bypasses libplacebo (approach 3) must solve overlays explicitly.
- **libplacebo pin bump** has wide blast radius: FFI layouts, API version, dylib names, bundle/harness, macOS+Linux deps.
- **Scope creep** into a pure-Rust renderer rewrite. Keep libplacebo as the rendering engine for the backend comparison unless deliberately decided.

## Suggested execution order

1. **Gate:** inspect pinned libplacebo 7.387.0 for `pl_metal` + build flags/coexistence; record findings and go/no-go (and whether pin bump is needed).
2. **Seam, no behavior change:** refactor `player-output` internals to a backend-neutral GPU context/swapchain creation seam; Vulkan path + tests green, bundle unchanged.
3. **Metal backend:** add Metal FFI + implementation + layout tests (macOS), behind the seam.
4. **Selection/plumbing:** store/plumb backend selector in `player-app` + `player-display` (default MoltenVK); unambiguous logs.
5. **Build/package:** update macOS deps/build/bundle/harness to carry both; verify default path unchanged.
6. **Parity + compare:** add parity tests, run comparison matrix (including seek), write up results.
7. **(Optional spike):** evaluate approach (3) / AVSBDL path for macOS-native presentation only if approach (1) is insufficient or the goal shifts - design overlay composition and seek behavior first.

## Concrete next planning step (still no implementation)

Map exact functions/types to touch for step (2)+(4): `player-output`’s `Vulkan`/`Instance`/`Surface`/`Swapchain` API surface and all `frame.rs` call sites; `player-display/src/mac.rs` construction; `player-app` option parsing (`--gpu-context` currently consumed-but-not-stored) and where display config flows into `player-display`. Also enumerate `scripts/bundle.py`/`tests/harness.py` assertions that encode the current Vulkan/MoltenVK closure.
