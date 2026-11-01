# Campaign: Minimal Local-Only mpv, Migrating to Rust

## Status

- **Goal:** A compact, dependable, local-only macOS ARM64 media player: mpv's proven playback pipeline, a minimal pinned FFmpeg, no network backend, and a Rust-owned control plane with the OSC running on `mlua`.
- **Last accepted revision:** *(record after the P8.1 commit)*. P0.1–P1.4 and P2.1–P8.1 have run.
- **Accepted:** P0.1–P1.4, P2.1–P8.1. **Red:** none. **Parked:** none.
- **Next slice:** P8.2 — measure and optimize the release.
- **Resume prompt:** `Read PLAN.md and execute P8.2. Continue in item order; stop only at a hard stop or when P8 is complete.`
- **First command:** `scripts/measure.sh release; scripts/measure.sh`.
- **Test mode:** headless (`--vo=null --ao=null`); GUI and renderer checks are owner-run.
- **Scope:** P8.1 built the pinned release closure and app; P8.2 measures and optimizes it.

This file is the campaign log as well as the plan. Update **Status** after every accepted slice. Git history holds the past; do not append dated checkpoints, handoffs, or a second status register.

---

## Charter

1. **Decide and keep going.** Every decision needed to run is below. Make ordinary judgment calls inside the stated bounds and record one only when a later slice depends on it. Do not ask the owner at runtime.
2. **The code is the truth.** If a path, option, or component name here disagrees with the pinned source, the source wins: fix the item, keep the intent, and record the correction in the commit body and in `docs/architecture.md` when the boundary changes.
3. **Parity before optimization.** Working local playback and test coverage come first. Size, CPU, and startup gates are **deferred to P8**; measurements before then are logged in `docs/metrics.md` and never block, revert, or reopen a slice. LTO, stripping, codegen-unit tuning, renderer/UI polish, and package-size work are P8 only.
4. **One writer.** No parallel lanes. Every item is serial and lands on `main`. Scratch goes in `.work/tmp`; worktrees, if ever used, go in `.work/wt/<name>`. `.work/` is excluded through `.git/info/exclude`, never a tracked file. Never use `/tmp` or `$TMPDIR`.
5. **Delete before rewriting.** Remove unsupported features and dependencies first. Every deletion is supported by a caller/dependency search and the corpus gate. No persistent parallel C/Rust implementations.
6. **Fix only what blocks.** In their own commit, fix a crash, a hang over 5 s, a network/secret leak, or a correctness regression that breaks an accepted behavior. Record size, CPU, and startup changes in `docs/metrics.md` and leave them for P8; they never block an earlier slice. Record everything else and leave it.
7. **Red never lands.** An item ends on its acceptance command plus the narrowest affected suite on the integrated tree; every accepted slice leaves a usable player. The full suite runs at P0.4, P4.3, P5.5, P7.5, P8.3, and the close.
8. **Failures get a cause before a retry.** Reproduce, isolate, fix the cause, keep the failing test. Time-box an item at three attempts, then park it on `park/<item>` with the exact cause and a retry condition.
9. **Commits.** Conventional Commit, workstream as scope, item ID in the body. One coherent change per commit; stage only your work. No formatters. No CI changes. **Never push.**
10. **Contract changes are surfaced.** GPL-2.0-or-later distribution, no automatic user config or scripts, the Vulkan/MoltenVK renderer, Lua 5.2, the `.app` release shape, and local-only rejection semantics are contract decisions already recorded below. Any further change to an API, type, schema, permission, invariant, or transaction boundary updates code, tests, and docs together and is called out in the reply.
11. **Docs are deliverables.** `docs/baseline.md`, `docs/ffmpeg.md`, `docs/architecture.md`, `docs/metrics.md`, `docs/scope.md`, `docs/lua-api.md`, and `docs/licenses.md` are maintained with the code, not written at the end.
12. **Steering checkpoint.** After each accepted slice, run the parent steering checkpoint: name what landed, name the single limit on delivery (implementation, a red check, integration, environment, or an owner decision), and record a ruling only when it changes: continue, narrow the slice, serialize, fix one prerequisite, or park. Rewrite **Status** in place; never append a checkpoint section.

### Rust rules

- `unsafe` only in `player-ffi` and `player-audio` (and the minimum platform glue); every block has a `// SAFETY:` comment. No unwinding across FFI; wrap callbacks in `catch_unwind`.
- No `unwrap()` or `expect()` on fallible media input, paths, FFmpeg, or Lua. Typed errors cross the boundary.
- No global mutable state, no duplicate ownership of player state, no leaked FFI pointers, no panic in a callback.
- No per-frame allocation in hot playback paths; no thread per trivial task; no Tokio or async runtime; no redundant buffering or frame copies.
- Standard library first. Add a crate only with a named responsibility, a checked transitive graph, a compatible license, and a recorded footprint. Commit `Cargo.lock`. Enable minimal features. A new dependency's size/startup/CPU impact is measured and recorded; the P8 budgets judge it at the end.
- No speculative generics or trait hierarchies for a second platform that does not exist. Rust line count is not a success metric; a Rust wrapper around an untouched C subsystem is not a port. Delete the replaced C in the same item.

---

## Decisions

### Owner-decided (2026-10-31)

- **D1 — Pins.** Pin mpv **v0.41.0**, commit `41f6a645068483470267271e1d09966ca3b9f413`, and FFmpeg **n9.0.2**, commit `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`. These are the latest tagged releases, not moving branches. Dependencies are pinned by tag and recorded with their resolved commits in `docs/baseline.md`.
- **D2 — Fork and license.** Import mpv into `/Users/josh/d/play` as a **GPL-2.0-or-later** fork, preserving the license files. `plan.md` becomes `PLAN.md` at the root. Do not push. Upstream history is consolidated away under D16.
- **D3 — Campaign depth.** Stop at the **Rust control plane**: P7.1–P7.5, plus the Symphonia audio decoder bridge in P1.2. The A/V scheduler, general demux/decode orchestration, renderer integration (7.8), and Linux are follow-on work, not part of this campaign. The release gate is the completion criteria below plus the measured P8 suites.
- **D4 — OSC ownership.** Keep the **simplified Lua OSC on `mlua`** as the permanent default. Rust owns the player core, not the overlay UI. Revisit only if P8 measurement shows Lua + mlua + OSC exceeds 10% of the distributable footprint or 50 ms of cold startup; then open a follow-on, do not rewrite inside P8.
- **D5 — Verification authority.** Automated gates are IPC/property/log/decode/binding checks. Screenshots and pixel comparisons are **not** acceptance gates. The executor may use a screenshot as a diagnostic, but a slice passes on functional evidence. Visual quality, actual audio audibility, HDR/10-bit color, subtitle shaping, Retina scaling, and motion smoothness are owner-run at milestones through `scripts/manual-checklist.sh`; they do not hold slices open.

### Prescribed

- **D6 — Renderer.** Use **`vo=gpu-next` + libplacebo + Vulkan via MoltenVK**. There is no macOS OpenGL window context in v0.41.0: `video/out/gpu/context.c:78-97` registers only `ra_ctx_vulkan_mac` under Cocoa+Swift, and `--gpu-context=help` lists only `auto`, `macvk`, and `displayvk`. `--vo=gpu` has no usable context and hangs in the pilot. Disable `gl`, `gl-cocoa`, `videotoolbox-gl`, and `macos-cocoa-cb`; delete the dead GL paths in P2.1/P2.3. Reverse only if a later mpv release reintroduces a Cocoa GL context and a measured win exceeds 20% total footprint without breaking the corpus.
- **D7 — CoreAudio baseline fix.** Upstream v0.41.0 fails CoreAudio init on macOS 27 with error -50 at `audio/out/ao_coreaudio.c:324-329`: it passes an `AudioChannelLayout` to `kAudioOutputUnitProperty_ChannelMap`, which expects a channel-index array. Homebrew mpv 0.41.0 reproduces the failure. **Skip the ChannelMap call entirely**; mono, stereo, and 5.1 all initialize and play through the audio unit's default mapping (5.1 is downmixed to the output device). If a future case needs explicit channel mapping, implement a correctly typed `SInt32` map then; do not add it in this campaign. Reverse when upstream fixes the property; keep the regression test.
- **D8 — Minimal FFmpeg method.** Build **full-minus-network first** (`--disable-network --disable-avdevice --disable-programs --disable-doc`, all components present, `--enable-libdav1d`), prove the corpus, then **subtract** decoders/demuxers/parsers/filters/bsfs while the corpus stays green. Freeze the surviving names into `scripts/ffmpeg-components.txt`; the final configure is `--disable-everything` plus that allowlist and `--enable-libdav1d`. Do not start from a guessed allowlist; do not respond to a failure by enabling everything. `dav1d` is the only permitted external codec library; FFmpeg's native `av1` decoder is hardware-only in n9.0.2 and cannot software-decode.
- **D9 — One Lua.** Use **Lua 5.2.4** for the baseline and the host; no LuaJIT. mpv v0.41.0's Meson dependency table accepts Lua 5.1/5.2 and LuaJIT, and its generic `lua` check caps at `<5.3.0`; master still lists only 5.1/5.2. Lua 5.2.4 is therefore the most modern non-JIT interpreter mpv supports; 5.3/5.4 would be a new port, not a pin. `mlua` uses `lua52` + `vendored` only, no async/serde/Tokio. The transition may briefly link the old host and mlua, but the final executable has exactly one Lua implementation.
- **D10 — No automatic user config or scripts.** The shipped player does not read `mpv.conf`, `input.conf`, or scripts from user directories. Configuration is explicit (`--config=FILE`) and scripts are explicit (`--script=FILE`); external scripts remain disabled by default. The built-in OSC is application code and always loads. This prevents a user config from re-enabling removed facilities.
- **D11 — Release shape.** Produce a minimal `Player.app` bundle, not a bare binary: `Contents/MacOS/player`, bundled `libMoltenVK.dylib` and `Contents/Resources/vulkan/icd.d/MoltenVK_icd.json`, with `VK_ICD_FILENAMES`/`VK_DRIVER_FILES` set to the bundled JSON before Vulkan init. A bare binary cannot reliably locate a bundled ICD. Static-link FFmpeg/libass/libplacebo where it wins; keep the audited `@rpath`/`@loader_path` closure otherwise. No Homebrew path may remain.
- **D12 — Test harness.** Python 3 standard library only, driving mpv's JSON IPC: spawn with `--no-config`, `stdin=DEVNULL`, a per-run Unix socket, bounded shutdown (2 s terminate, 5 s kill). Assert properties, events, log lines, exit codes, `input-bindings`, and protocol/component manifests. No pixel gates. Use `ffmpeg` 9.0.2 only to generate fixtures and to probe them.
- **D13 — Failure protocol.** Three attempts per item, then park. A shared environment blocker gets one owner and one recorded command. Never weaken a test to pass; if a test is wrong, say why in the commit body.
- **D14 — Build dependencies.** Homebrew is allowed for development tools and the P0 reference build only: `ffmpeg 9.0.2_1`, `libplacebo 7.387.0`, `libass 0.17.5_1`, `molten-vk 1.4.2`, `vulkan-loader 1.4.363.0`, `vulkan-headers 1.4.363.0`, `shaderc 2026.4`, `dav1d 1.5.4`, `meson 1.12.1`, `ninja 1.13.2`, `pkgconf 3.0.10`. Lua 5.2.4 is built from source. From P8, `scripts/build-deps.sh` builds the release closure from pinned sources (or checksum-pinned official binaries for MoltenVK) with no Homebrew libraries; the runtime has no package-manager dependency.
- **D15 — Symphonia audio decode (owner, 2026-11-01).** Vendor the sibling Symphonia fork at commit `b6336209bb40d57e4ed71079cf7f7fc6c4626aae` under `vendor/symphonia`, preserving its history and MPL-2.0 license. Use its pure-Rust decoders for AAC, ALAC, FLAC, MP3, Opus, PCM, and Vorbis. FFmpeg remains the demuxer and video decoder; `libswresample` remains for audio conversion and resampling. Remove FFmpeg audio decoders from the final allowlist. This pulls audio decode into scope; the A/V scheduler, general demux orchestration, and renderer integration remain follow-on. MPL-2.0 is compatible with the GPL-2.0-or-later fork subject to its secondary-license terms; preserve Symphonia's LICENSE and notices and audit it at P8.3. This decision keeps one demuxer (FFmpeg) for A/V synchronization; using Symphonia's demuxers in parallel would be a separate follow-on.
- **D16 — Repository history (owner, 2026-11-01).** Consolidate the repository to a single root commit containing the P0.1–P1.4 tree. Do not retain the upstream mpv or Symphonia commit histories; the pins remain in `docs/baseline.md` and `docs/ffmpeg.md`, and Symphonia remains vendored per D15. A tag `campaign-base` marks the consolidated root. Future campaign work appends normal commits.

### Delegated (executor decides inside the bound)

- **DL1 — Exact component allowlist.** Follow D8; any component not exercised by the corpus and not required by mpv's actual call sites is removed. Add a component only after a named fixture fails without it.
- **DL2 — Prune file sets.** P2.2/P2.3 remove the modules named there; the exact translation units are found by caller/dependency search. If a removal breaks playback, restore the smallest piece and record why.
- **DL3 — AV1 decode implementation.** Use **libdav1d 1.5.4** for AV1. FFmpeg n9.0.2's native `av1` decoder is hardware-only (`av1dec.c` returns `ENOSYS` when no hwaccel is initialized) and cannot software-decode; AV1 VideoToolbox is unavailable on this host. Pin dav1d from source for the release; the P1 build may use the pinned Homebrew dev copy. Do not enable `av1` or `av1_videotoolbox`.
- **DL4 — Subtitle font stack.** Build libass with CoreText and without fontconfig if the subtitle corpus (SRT, ASS, WebVTT, embedded) shapes correctly; otherwise pin fontconfig/freetype/harfbuzz/fribidi from source. The corpus and owner checklist decide.
- **DL5 — Optimization knobs.** P8 may tune LTO, codegen units, visibility, strip mode, and static-vs-dynamic per library. Keep a change only if it wins on the measured package size or startup without a regression beyond the P8 budgets.

---

## Baseline

### Host and pins

| Item | Value |
|---|---|
| Host | macOS 27.0.1, Apple Silicon arm64 |
| Toolchain | Xcode 27.1, macOS SDK 27.0, clang, Swift 6.x |
| Build tools | Meson 1.12.1, Ninja 1.13.2, pkgconf 3.0.10, Python 3.13+ |
| mpv | v0.41.0, `41f6a645068483470267271e1d09966ca3b9f413` |
| FFmpeg | n9.0.2, `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3` |
| libplacebo | 7.387.0 (requires Vulkan; `meson.build:29`) |
| libass | 0.17.5 |
| MoltenVK | 1.4.2; Vulkan loader 1.4.363.0 |
| dav1d | 1.5.4 (minimal FFmpeg AV1 software decode) |
| Lua | 5.2.4, source build with `lua52.pc` |
| Rust | current stable rustc/cargo; record exact version in `docs/baseline.md` |

### Known red and constraints (from the pilot build)

- **CoreAudio fails upstream.** `AO: [coreaudio]` aborts with error -50 at the ChannelMap property; both this build and Homebrew mpv reproduce. P0.2 applies D7. Until then, `--ao=null` is the only audio path.
- **No OpenGL renderer.** `vo=gpu` has no macOS context and hangs; only `gpu-next`/Vulkan is usable. D6.
- **Hardware-decoded frames cannot be screenshotted** (`videotoolbox` format is not convertible by libswscale). Diagnostics that need an image must use `--hwdec=no` or `videotoolbox-copy`.
- **`-Dlibmpv=false` still produces `libmpv.2.dylib` in the build directory.** The `mpv` executable does not link it, but the release must exclude it and prove the final dependency closure.
- **No `timeout` binary** in the environment; the harness uses Python socket/read timeouts and process termination.
- **No CI** in this repo. Gates are local commands in `tests/run.sh` and `scripts/audit.sh`; do not add workflows.
- **Network protocols still appear** in the stock build (`--list-protocols` shows 64 entries). P1/P2/P3 must reduce this to the local-only set and gate it.
- **Native FFmpeg AV1 cannot software-decode.** In n9.0.2, `libavcodec/av1dec.c` returns `ENOSYS` without a hardware accelerator; AV1 VideoToolbox is unavailable on this host. P1.3 must use **libdav1d** (1.5.4) for AV1.
- **External `.ass` needs the `ass` demuxer.** With only the `ass` decoder, mpv reports "Can not open external file"; the allowlist includes the `ass` demuxer. `mov_text` in MP4 is the decoder name **`movtext`**, not `mov_text`.

### Loop timings (pilot, not the imported tree)

| Step | Time |
|---|---|
| Meson configure | ~5 s |
| Clean build (8 jobs) | 27–34 s |
| Incremental rebuild after one C file | 0.94 s |
| 3 s playback smoke | ~3.4 s |
| IPC load + seek + quit | ~2–3 s |
| `--version` startup | 0.08–0.10 s |
| First frame (`--frames=1 --untimed`) | 0.57 s |

### Pilot measurements (P0.4 re-records these on the imported tree)

| Metric | Pilot value |
|---|---|
| `mpv` executable | 4.3 MB (GL disabled) |
| Unique non-system dependency closure | 60.8 MB in 35 files |
| Direct non-system libs | libass, FFmpeg (6 libs), libplacebo, libvulkan, MoltenVK |
| Startup to first frame | 0.57 s |
| RSS, 1 frame | 250–251 MB |
| RSS, 3 s playback | 261 MB |
| CPU, 3.3 s 720p software decode | 0.49 s user+sys (~15% of one core) |
| VideoToolbox | `Using hardware decoding (videotoolbox)` |
| CoreAudio | red until D7 is applied; then `AO: [coreaudio]` for mono, stereo, and 5.1 |
| OSC | loads under Lua 5.2 with no script errors; `input-bindings` contains `owner=osc` mouse/key bindings |

### Preflight proofs

- mpv v0.41.0 builds on this host and plays video through `gpu-next`/Vulkan/MoltenVK.
- VideoToolbox decodes H.264 and HEVC with `--hwdec=videotoolbox`.
- Lua 5.2.4 loads the upstream OSC without script errors; `input-bindings` includes `owner=osc` mouse/key bindings.
- A preflight build with the corrected P1.3 allowlist (libdav1d, `ass` demuxer, `movtext`, H.264/HEVC VideoToolbox only) played the 11-fixture pilot corpus with `gpu-next`: H.264, HEVC, AV1, VP9, MPEG-4, FLAC, 5.1, MP4 text subs, embedded SRT, WebVTT, and external SRT/ASS.
- The CoreAudio ChannelMap failure reproduces in Homebrew mpv 0.41.0 and is fixed by skipping the property; 5.1 plays through the unit's default mapping.

### Measurement protocol

`scripts/measure.sh` reports, for a named revision: `mpv` size, unique distributable closure size and file count with per-component attribution (FFmpeg, libplacebo, libass, Rust/std, Lua/mlua, MoltenVK/Vulkan, remaining external), direct `otool -L` entries, cold and warm `--version` startup (3 runs, median), first-frame latency (`--frames=1 --untimed`, 3 runs, median), idle/peak/steady-state RSS and CPU for 3 s and 30 s 720p/1080p/4K playback, seek latency, exit latency, audio underruns, frame-drop counts, and decoder-to-renderer copies where observable. Use `/usr/bin/time -l` for RSS and CPU; use the same fixtures and flags every time. Interleave baseline and candidate runs when comparing; report the median and spread. Record the command, revision, and date in `docs/metrics.md`. Do not present archive size as binary size, and state where symbol attribution is unreliable instead of inventing precision.

---

## Sequence

Every item names owned files, prerequisites, and one acceptance command. Acceptance commands are defined in **Acceptance** below. Items are serial; no item starts before its prerequisite is accepted. Sizes are estimates, not stops: **S** ≤ 2 h, **M** ≤ 1 day, **L** ≤ 3 days.

### P0 — Ground

**P0.1 Import and pin.** (S)
- Owns: repo root, `PLAN.md`, `.gitignore`, `.git/info/exclude`, `docs/` skeleton.
- Prereq: none.
- Do: `git init -b main`; commit this plan; add `upstream https://github.com/mpv-player/mpv`; fetch full history; merge tag `v0.41.0` with unrelated histories; rename `plan.md` to `PLAN.md`; create `rust/`, `scripts/`, `tests/`, `docs/`; ignore `.work/`, `build/`, `dist/`, `target/`; write `.git/info/exclude` for `.work/`. Record the imported commit in `docs/baseline.md` header.
- Accept: `git status --porcelain` is empty; `git merge-base --is-ancestor v0.41.0 HEAD`; `git rev-parse v0.41.0^{commit}` is `41f6a645068483470267271e1d09966ca3b9f413`; `test -f PLAN.md`; `test -f video/out/vo_gpu_next.c`; `.git/info/exclude` contains `.work/`.

History note: D16 later consolidated the repository to the single `campaign-base` root. The `v0.41.0` tag is not retained; the pinned commit remains recorded in `docs/baseline.md`.

**P0.2 Dev environment and baseline build.** (M)
- Owns: `scripts/dev-deps.sh`, `scripts/build.sh`, `audio/out/ao_coreaudio.c` (D7 patch), `docs/baseline.md` (build section).
- Prereq: P0.1.
- Do: install/check the pinned Homebrew dev packages; build Lua 5.2.4 into `.work/deps/lua52` with a `lua52.pc`; configure with the exact command below; apply D7; build; play the generated sine. Do not disable `coreaudio`, `cocoa`, `vulkan`, `videotoolbox-pl`, `libass`, `libplacebo`, `lua`, or `swift-build`. Disable every other platform/feature backend explicitly; no `auto` for unsupported features.
- Accept: `scripts/build.sh` exits 0; `build/mpv --version` reports v0.41.0; a generated 1 s stereo sine plays with `--vo=gpu-next --ao=coreaudio` and logs `AO: [coreaudio]`; `--vo=gpu-next --ao=null` plays the same file to EOF.

History note: after D16, `git describe` reports `campaign-base`; `MPV_VERSION` remains 0.41.0 and the source baseline is unchanged.

```sh
meson setup build . --buildtype=release \
  -Dcplayer=true -Dlibmpv=false -Dgpl=true -Dlua=lua52 \
  -Dcocoa=enabled -Dswift-build=enabled -Dcoreaudio=enabled \
  -Dvulkan=enabled -Dvideotoolbox-pl=enabled \
  -Dgl=disabled -Dgl-cocoa=disabled -Dvideotoolbox-gl=disabled -Dmacos-cocoa-cb=disabled \
  -Djavascript=disabled -Dlibarchive=disabled -Duchardet=disabled -Dzimg=disabled \
  -Drubberband=disabled -Dcdda=disabled -Ddvdnav=disabled -Dlibbluray=disabled \
  -Dlibavdevice=disabled -Dcplugins=disabled -Dvapoursynth=disabled \
  -Djpeg=disabled -Dlcms2=disabled -Dplain-gl=disabled \
  -Dx11=disabled -Dwayland=disabled -Ddrm=disabled -Dvaapi=disabled -Dvdpau=disabled \
  -Daudiounit=disabled -Davfoundation=disabled -Dmacos-touchbar=disabled \
  -Dmacos-media-player=disabled -Dtests=false -Dbuild-date=false
```

**P0.3 Fixtures and harness.** (L)
- Owns: `tests/fixtures/gen.sh`, `tests/harness.py`, `tests/run.sh`, `tests/fixtures/manifest.json`.
- Prereq: P0.2.
- Do: generate the corpus into `.work/fixtures`, including a truncated MP4, a corrupted file, and an unsupported-codec file for the failure-handling cases; write a SHA-256 manifest; implement the IPC harness and the gates that are meaningful on the baseline: `fixtures`, `play`, `corpus`, `audio`, `hwdec`, `seek`, `local-paths`, `osc`, `input`, `playlist`, `state`, `build`, `baseline-docs`, `meta`. Later items add their gates. Generate with Homebrew ffmpeg 9.0.2 only. Keep fixtures out of git; the generator is committed.
- Accept: `tests/run.sh fixtures` regenerates and verifies the manifest; `tests/run.sh play h264` passes; `tests/run.sh audio` passes with D7.

**P0.4 Baseline inventory and metrics.** (M)
- Owns: `scripts/measure.sh`, `scripts/manual-checklist.sh`, `docs/baseline.md`, `docs/metrics.md`, `docs/manual-checklist.md`.
- Prereq: P0.2, P0.3.
- Do: produce `docs/baseline.md` with: upstream commits and exact dependency versions; build commands and options; binary architecture and deployment target; unstripped and stripped executable sizes; `otool -L`; shipped non-system library footprint; direct and transitive dependency count; startup (cold and warm) and time to first frame; idle and active RSS; CPU for 1080p and 4K; the working VideoToolbox path; enabled mpv features; supported FFmpeg protocols; license obligations; known limitations; and the D7 deviation. Create the owner checklist with the corpus paths. Add the first `docs/metrics.md` row.
- Accept: `scripts/measure.sh baseline` writes one row; `tests/run.sh baseline-docs` checks every required heading and value exists.

### P1 — Minimal FFmpeg

**P1.1 Full-minus-network FFmpeg, linked into mpv.** (M)
- Owns: `scripts/build-ffmpeg.sh`, `docs/ffmpeg.md`.
- Prereq: P0.4.
- Do: build FFmpeg n9.0.2 static into `.work/deps/ffmpeg-full` with `--disable-shared --enable-static --disable-programs --disable-doc --disable-network --disable-avdevice --disable-autodetect --enable-libdav1d --enable-videotoolbox --enable-hwaccel=h264_videotoolbox,hevc_videotoolbox`; keep the components for now. Rebuild mpv with `PKG_CONFIG_PATH` pointing at it. Confirm no FFmpeg executable is used at runtime.
- Accept: `tests/run.sh corpus` passes against the new prefix; `build/mpv --list-protocols` contains no `http`, `https`, `ftp`, `rtsp`, or `rtmp`; `otool -L build/mpv` contains no curl/OpenSSL library.

**P1.2 Vendor Symphonia and bridge audio decode.** (L)
- Owns: `vendor/symphonia/`, `rust/Cargo.toml`, `rust/Cargo.lock`, `rust/player-audio/`, `audio/decode/ad_symphonia.c`, `audio/decode/dec_audio.c`, `meson.build`, `scripts/build.sh`, `docs/architecture.md`, `docs/ffmpeg.md`, `docs/licenses.md`, `tests/run.sh`.
- Prereq: P1.1.
- Do: import the pinned Symphonia fork with history and license. Add a Rust staticlib crate (`player-audio`) with a minimal C ABI for decoder create/decode/destroy, typed errors, and no unwinding across FFI. Implement `ad_symphonia` against mpv's audio decoder interface and route AAC, ALAC, FLAC, MP3, Opus, PCM, and Vorbis packets from the FFmpeg demuxer to Symphonia. Keep `ad_lavc` as an explicit fallback until the corpus and audio gates are green. Log `Using Symphonia` when selected. Handle FFmpeg extradata, packet framing, timestamps, sample formats, and channel layouts. Link the staticlib through Meson and `scripts/build.sh`.
- Accept: `cargo test -p player-audio`; `tests/run.sh audio` and `tests/run.sh corpus` pass with Symphonia selected for the listed codecs; `nm build/mpv` contains the player-audio bridge symbols. FFmpeg audio decoders remain present as a fallback in this slice.

**P1.3 Subtract FFmpeg to the video/demux allowlist.** (L)
- Owns: `scripts/ffmpeg-components.txt`, `scripts/build-ffmpeg.sh`, `docs/ffmpeg.md`, `docs/architecture.md`.
- Prereq: P1.2.
- Do: remove FFmpeg audio decoders now that Symphonia is the selected path. Keep the demuxers, audio parsers, bitstream filters, filters, and `libswresample` needed to deliver packets and convert audio; keep libavfilter/libswscale. Remove components in groups (decoders, parsers, bsfs, filters, hwaccels), running the corpus after each group. Start with the expected set below; validate every name against the pinned `configure --list-*`. Freeze the survivors into the allowlist; rebuild from `--disable-everything` plus the allowlist and rerun the corpus. Add no encoders, muxers, devices, external libraries beyond `dav1d`, or protocols. When a fixture fails, the gate prints the missing FFmpeg component or the decoder/parser error; do not respond by enabling all decoders.
- Accept: `tests/run.sh corpus`, `tests/run.sh audio`, `tests/run.sh hwdec`, and `tests/run.sh seek` pass; `build/mpv --list-protocols` has no network scheme; closure and size are recorded in `docs/metrics.md`.

Expected allowlist (validate before freezing):

| Category | Expected names |
|---|---|
| Demuxers | `mov`, `matroska`, `avi`, `mp3`, `flac`, `wav`, `ogg`, `srt`, `webvtt`, `ass` |
| Video decoders | `h264`, `hevc`, `libdav1d`, `vp9`, `mpeg4` |
| Audio decoding | Symphonia (AAC, ALAC, FLAC, MP3, Opus, PCM, Vorbis); no FFmpeg audio decoder |
| Subtitle decoders | `subrip`, `ass`, `webvtt`, `movtext` |
| Parsers | `h264`, `hevc`, `av1`, `vp9`, `mpeg4video`, `aac`, `mpegaudio`, `opus`, `vorbis`, `flac` |
| Bitstream filters | `h264_mp4toannexb`, `hevc_mp4toannexb`, `vp9_superframe`, `aac_adtstoasc`, `extract_extradata`, `null` |
| Filters | `format`, `scale`, `aresample`, `aformat`, `fps`, `setpts`, `crop`, `transpose`, `hflip`, `vflip`, `hwdownload`, `hwupload`, `trim`, `atrim`, `concat`, `volume`, `pan`, `channelsplit`, `null`, `anull` |
| HW accels | `h264_videotoolbox`, `hevc_videotoolbox` |
| Protocols | `file` |
| External libraries | `dav1d` 1.5.4 (AV1 software decode; no other external library) |

Audio parsers and `aac_adtstoasc` remain because the FFmpeg demuxer uses them to produce packets; they do not decode audio. Do not enable native `av1`, `av1_videotoolbox`, or `vp9_videotoolbox`: the native AV1 decoder is hardware-only in n9.0.2, and AV1/VP9 VideoToolbox are not required. `buffer`/`buffersink` are built with libavfilter and are not configurable filters.

**P1.4 FFmpeg manifest gate.** (S)
- Owns: `scripts/ffmpeg-manifest.sh`, `tests/run.sh` (audit subcommand). The generated manifest lives in `.work/deps/ffmpeg-manifest.json`.
- Prereq: P1.3.
- Do: generate the manifest from the actual build tree (`config_components.h`, `config.mak`, and `ffbuild/config.log`, or a small probe linked against the static libs), not from a shipped `ffmpeg` program; compare it with the allowlist. Fail on any network protocol, encoder, muxer, device, audio decoder, or external library outside the allowlist (`dav1d` only). Record the command output in `docs/ffmpeg.md`. The manifest is generated, not committed.
- Accept: `tests/run.sh ffmpeg-audit` passes; a deliberate extra `http` entry in a temporary manifest makes it fail (the gate is tested, then removed).

### P2 — Strip mpv

**P2.1 Lock the Meson feature surface.** (M)
- Owns: `meson.build`, `meson.options`, `scripts/build.sh`, `player/lua/meson.build`, `player/lua.c`, `player/scripting.c`, `tests/harness.py`.
- Prereq: P1.4.
- Do: make every option explicit in `scripts/build.sh`; disable every unsupported audio/video/platform/hwaccel feature named in **Non-goals**; disable external libmpv distribution but keep the internal client API; disable the GL paths per D6; remove unused built-in Lua modules from the install list and the embedded/loading tables (keep `defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`, `select.lua`, `osc.lua`). Do not remove the internal client API. Lock the full feature surface in `tests/run.sh build`.
- Accept: `tests/run.sh build` and `tests/run.sh corpus` pass; `meson configure build` shows every non-macOS feature disabled; `otool -L build/mpv` has no curl/bluray/dvdnav/cdda/pipewire/pulse/jack/avfoundation/X11/Wayland libraries.

**P2.2 Remove unsupported stream/network/disc/capture modules.** (L)
- Owns: `stream/`, `demux/` (disc and remote-playlist modules), `player/lua/ytdl_hook.lua`, `meson.build`, `docs/scope.md`.
- Prereq: P2.1.
- Do: remove network stream modules, optical/disc demuxers, capture/avdevice paths, yt-dlp hook, and their registration tables. Reduce the remaining protocol list to `file://` and, only if internally required, `mpv://`; remove `memory://`, `edl://`, `fd://`, `slice://`, `appending://`, `hex://`, `null://`, `avdevice://`, `mf://`, `lavf://`, and `ffmpeg://` wrappers. Keep `demux_playlist.c` for local playlist files (M3U/M3U8/PLS) so URL entries can be rejected at the boundary; remove remote-only playlist formats and resolvers. Keep local file path handling and external subtitle paths. Use caller search; do not delete by filename alone.
- Accept: `tests/run.sh corpus`, `tests/run.sh local-paths`; `build/mpv --list-protocols` contains only `file://` (plus `mpv://` if still needed internally; record and justify any extra).

**P2.3 Remove encoding, recording, JS, plugins, and unused UI/platform integrations.** (L)
- Owns: `common/`, `video/out/`, `audio/out/`, `player/javascript.c`, `filters/`, `meson.build`.
- Prereq: P2.2.
- Do: remove encode/record/transcode paths (`vo_lavc`, `ao_lavc`, `common/encode_lavc.c`, recorder), JavaScript/MuJS, C plugins, VapourSynth, Touch Bar, Media Player, clipboard if unused, and platform code for Windows/Linux/Android. Keep screenshot-to-file (tests use it only as a diagnostic), PNG writing, CoreAudio, Cocoa/Swift, VideoToolbox, libass, libplacebo, Lua. Record any integration that cannot be removed as remaining technical debt in `docs/scope.md` or `docs/architecture.md`.
- Accept: `tests/run.sh corpus`, `tests/run.sh hwdec`, `tests/run.sh osc`; `otool -L` unchanged except intended removals; `tests/run.sh build` clean.

**P2.4 Reduce the CLI surface.** (M)
- Owns: `options/options.c`, `options/parse_commandline.c`, `docs/scope.md`.
- Prereq: P2.3.
- Do: keep `player FILE...`, `--fullscreen`, `--start=TIME`, `--audio-file=FILE`, `--sub-file=FILE`, `--version`, `--help`, and the playback controls needed by the OSC. Remove option groups for networking, optical devices, capture, encoding, remote services, and unsupported renderers. Do not attempt a wholesale option-table rewrite; P7.1 handles typed startup config.
- Accept: `tests/run.sh cli` asserts the supported forms work and a removed option (e.g. `--http-header-fields`) fails cleanly.

**P2.5 Simplify the macOS source list.** (M)
- Owns: `meson.build`, `docs/architecture.md`.
- Prereq: P2.4.
- Do: replace inherited platform conditionals with an explicit macOS source list; keep reusable Rust-ready boundaries in separate files; do not delete abstractions the control-plane port needs. Run the full suite because this touches every target.
- Accept: `tests/run.sh suite` (P0 subset) and `tests/run.sh build` pass; `meson introspect --targets` shows only the `mpv` executable and the intended Rust static library; the stray `libmpv.2.dylib` is neither installed nor bundled.

### P3 — Local-only boundary

**P3.1 Enforce the input boundary.** (L)
- Owns: `player/loadfile.c`, `demux/demux_playlist.c`, `stream/stream.c`, `stream/stream_lavf.c`, `player/configfiles.c`, `tests/local_only.py`, `tests/run.sh`, `docs/scope.md`.
- Prereq: P2.5.
- Do: define one path-shaped input boundary. Reject `scheme://` URLs, protocol wrappers (`lavfi:`, `edl://`, `memory://`, `fd://`, `data:`), remote playlist entries, and remote subtitle references. Accept existing local paths containing `:`, spaces, Unicode, relative paths, symlinks, and mounted volumes; a mounted network volume is still an ordinary filesystem path and the player itself makes no network connection. Accept a `file://` URI only from the drag-and-drop path when it resolves to an existing local path, and convert it to a filesystem path; reject every other scheme. Apply the boundary to CLI, IPC loadfile, playlist entries, external subtitles, and drag-and-drop. Disable automatic user config/scripts per D10. Remove subprocess URL resolution and download paths. Keep errors helpful and non-hanging.
- Accept: `tests/run.sh local-only` passes: URL matrix rejected with exit≠0 and a clear message; local path matrix plays; a playlist with one URL entry rejects that entry and continues; `sandbox-exec -p '(version 1)(deny network*)'` playback succeeds; `build/mpv --list-protocols` has no network scheme.

**P3.2 Lock the boundary with tests.** (M)
- Owns: `tests/local_only.py`, `tests/run.sh`, `scripts/audit.sh`, `docs/scope.md`.
- Prereq: P3.1.
- Do: add the URL matrix (http, https, ftp, sftp, ssh, rtsp, rtmp, hls, dash, smb), malformed input, embedded playlist URLs, hostile temp `HOME` config/script, and local path edge cases. Add the linked-library and protocol-manifest assertions to `scripts/audit.sh`.
- Accept: `tests/run.sh local-only` and `scripts/audit.sh` pass; the hostile config test proves no script was loaded.

### P4 — Rust boundary

**P4.1 Rust workspace and Meson/Cargo link.** (M)
- Owns: `rust/Cargo.toml`, `rust/Cargo.lock`, `rust/player-core/`, `rust/player-ffi/`, `meson.build`, `scripts/build.sh`, `tests/run.sh`, `tests/harness.py`.
- Prereq: P3.2.
- Do: add `player-core` (pure Rust) and `player-ffi` (C ABI) to the existing workspace that already links `player-audio`; `player-lua` comes in P5. Build the staticlib with `cargo build --release`, link it into `mpv` through Meson, propagate target/release configuration, and commit `Cargo.lock`. No duplicate runtime libraries. Start with one exported function and one C caller; prove the link before expanding.
- Accept: `tests/run.sh rust-link` passes (symbol present, `mpv` starts and plays one fixture); `cargo test` passes.

**P4.2 First subsystem: local input validation.** (M)
- Owns: `rust/player-core/src/input.rs`, `rust/player-ffi/src/lib.rs`, `rust/player-ffi/include/player_ffi.h`, `player/loadfile.c`.
- Prereq: P4.1.
- Do: move the P3.1 predicate to `player-core::input` (`classify_input -> LocalPath | Rejected`), expose it as an opaque, typed-error C function, call it at the single load choke point, and delete the duplicated C checks. Keep C path normalization only where it is platform glue. Unit-test URL/path cases; integration-test through the player.
- Accept: `cargo test -p player-core input` and `tests/run.sh local-only` pass; `rg -n 'https?://' player/loadfile.c` is empty (no second URL policy).

**P4.3 FFI invariants and bridge test.** (S)
- Owns: `docs/architecture.md`, `tests/ffi_test.c`, `tests/run.sh`.
- Prereq: P4.2.
- Do: document opaque handles, ownership, lifetime/destruction, threading restrictions, callback registration/cleanup, typed errors, and no-unwind rules. Add a C test that calls the bridge with valid and invalid input, asserts typed errors, and proves a Rust panic is caught at the boundary. No shared mutable globals.
- Accept: `tests/run.sh ffi` and the full suite subset pass; `docs/architecture.md` contains the boundary section.

### P5 — mlua host

**P5.1 Add `player-lua` with mlua.** (M)
- Owns: `rust/player-lua/`, `rust/Cargo.toml`, `rust/Cargo.lock`.
- Prereq: P4.3.
- Do: pin a current stable `mlua`, features exactly `lua52` + `vendored`; no async, serde, or Tokio. One interpreter implementation/version for the final executable. Keep the old C host compiling until P5.5; do not add a second vendored Lua version.
- Accept: `cargo test -p player-lua` loads and runs a Lua 5.2 script; `cargo tree -p player-lua` shows only the intended features.

**P5.2 Audit the Lua API closure.** (M)
- Owns: `docs/lua-api.md`, `tests/lua-api-audit.sh`.
- Prereq: P5.1.
- Do: enumerate every `mp.*` call in `osc.lua` and its imported helpers (`defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`, `select.lua`); table each function, call sites, semantics, and whether it is in scope. The audit is authoritative; do not implement from a guessed list. The pilot found the closure includes mouse/OSD/key-binding and property helpers beyond the obvious command/property calls.
- Accept: `tests/run.sh lua-api-audit` regenerates the table and fails if any call is unclassified.

**P5.3 Rust host core.** (L)
- Owns: `rust/player-lua/src/`.
- Prereq: P5.2.
- Do: implement interpreter lifecycle, `mp` table registration, event delivery in order, timers without busy-waiting, property observation/unobservation, command dispatch, OSD overlay updates, key binding/message registration, and bounded shutdown (2 s). Use one script thread and standard synchronization; no generic executor. Catch Rust panics at callbacks; never unwind across FFI. Invalid script behavior produces a controlled error, not a hang. The built-in OSC is trusted application code; explicitly loaded external scripts are trusted code with no new network or subprocess capability, and the docs must say so rather than claim a sandbox.
- Accept: `tests/run.sh lua-host` runs a fixture script exercising command/get/set/observe/timer/event/OSD/binding and asserts the recorded results; a deliberate error script exits cleanly.

**P5.4 Load the upstream OSC through mlua.** (L)
- Owns: `rust/player-lua/src/`, `player/lua/osc.lua`, `tests/lua/`.
- Prereq: P5.3.
- Do: load the built-in modules in mpv's order, register the OSC's messages and bindings, and verify mouse-area/key-binding registration. Keep the upstream OSC unmodified in this item.
- Accept: `tests/run.sh osc` passes: no script errors, `input-bindings` contains the OSC mouse/key bindings, the OSC registers its messages, and a fixture event/timer updates an OSD overlay through the host.

**P5.5 Cut over and delete the C host.** (M)
- Owns: `player/lua.c`, `player/scripting.c`, `meson.build`, `rust/player-lua/`.
- Prereq: P5.4.
- Do: make the Rust host the only host; remove `player/lua.c` and the Lua-specific Meson dependency; keep `scripting.c` only for non-Lua responsibilities (audit first); delete the old bindings. Prove one Lua implementation in the final link.
- Accept: `tests/run.sh osc`, `tests/run.sh corpus`, and `cargo test` pass; `rg -n 'lua_State|luaL_newstate'` outside `rust/player-lua` is empty; `otool -L`/`nm` show no duplicate interpreter; `docs/metrics.md` row.

### P6 — Minimal OSC

**P6.1 Simplify `osc.lua`.** (M)
- Owns: `player/lua/osc.lua`, `tests/osc_input.py`, `tests/run.sh`.
- Prereq: P5.5.
- Do: keep play/pause, seek bar, elapsed/duration, volume/mute, fullscreen, and an optional audio/subtitle selector. Remove unused layouts, options, context menus, extra elements, and compatibility branches. Keep auto-hide and pointer-move reveal; preserve Retina scaling. Measure script size, redraw cost, and startup. Do not introduce egui, Slint, GTK, or web rendering; reuse the existing OSD/renderer interface.
- Accept: `tests/run.sh osc` and `tests/run.sh osc-input` (binding-level) pass; `wc -l` and timings recorded in `docs/metrics.md`.

**P6.2 Record the OSC decision and measurements.** (S)
- Owns: `docs/architecture.md`, `docs/metrics.md`.
- Prereq: P6.1.
- Do: record D4, the size/startup attribution of Lua + mlua + OSC, and the threshold that would justify a follow-on Rust OSC. Do not start a second UI implementation.
- Accept: `scripts/measure.sh size` attributes the script host; `tests/run.sh osc` passes; `docs/architecture.md` states the decision.

### P7 — Rust control plane

Apply the migration rule to every P7 item: map the existing implementation and callers; identify the narrowest ownership boundary; characterize behavior with tests; implement in Rust; integrate into the production path; verify correctness; record performance/size; delete the replaced C; remove unused headers/flags/deps/build rules; record complexity/size; commit. Do not rewrite FFmpeg, libass, or libplacebo. Do not gate on size, CPU, or startup before P8.

**P7.1 Typed startup configuration.** (M)
- Owns: `rust/player-core/src/config.rs`, `options/parse_commandline.c`, `player/main.c`.
- Prereq: P6.2.
- Do: parse the supported CLI into a typed Rust `StartupConfig`; expose it to C through the FFI; stop parsing those options in C. Keep mpv's runtime property system for OSC/Lua commands; do not build a generic dynamic-option framework. Remove inherited parsers for removed features.
- Accept: `cargo test -p player-core config`, `tests/run.sh cli`, `tests/run.sh corpus` pass.

**P7.2 Local playlist.** (M)
- Owns: `rust/player-core/src/playlist.rs`, `common/playlist.c`, `player/loadfile.c`.
- Prereq: P7.1.
- Do: Rust owns playlist entries, current index, insertion/removal, ordering, and next/prev. Local paths only; URL entries are rejected at the boundary. Delete the general-purpose playlist ownership in C. Remove shuffle/repeat; they are not in the required scope.
- Accept: `cargo test -p player-core playlist`, `tests/run.sh playlist` (multi-file, next/prev, removal, Unicode, rejected URL) pass.

**P7.3 Typed command and input dispatch.** (L)
- Owns: `rust/player-core/src/command.rs`, `input/cmd.c`, `input/input.c`, `player/command.c`.
- Prereq: P7.2.
- Do: define a compact typed command set for the supported features and OSC needs (`play/pause`, `seek`, `frame-step`, `volume`, `mute`, `speed`, `fullscreen`, `stop`, `quit`, `loadfile`, track selection, subtitle add, playlist next/prev, screenshot only as a diagnostic). Route keyboard/mouse bindings through it; keep the stringly Lua API boundary at the edge. Remove command paths for deleted features.
- Accept: `cargo test -p player-core command`, `tests/run.sh input` (keypress matrix changes the expected properties), `tests/run.sh osc-input` pass.

**P7.4 Player state and event dispatch.** (L)
- Owns: `rust/player-core/src/state.rs`, `player/client.c`, `player/command.c`, `player/osd.c`.
- Prereq: P7.3.
- Do: Rust owns playback state visible to the control plane, property change notification, and event routing; C's playback pipeline reports facts through the FFI. Remove duplicated state and fragile lifecycle assumptions. Do not move the audio/video pipeline or A/V scheduler.
- Accept: `tests/run.sh state` asserts pause/time-pos/duration/fullscreen/volume/speed/track lists across load, seek, pause, track switch, EOF, and a 100-event burst with no loss; `tests/run.sh osc` passes.

**P7.5 Local file/stream abstraction.** (L)
- Owns: `rust/player-core/src/local_file.rs`, `stream/stream_file.c`, `stream/stream.c`, `stream/stream_lavf.c`.
- Prereq: P7.4.
- Do: replace generic stream/protocol plumbing with a narrow filesystem abstraction: open, read, seek, size, EOF, and typed I/O errors; preserve FFmpeg-compatible seeking and error propagation. Keep the minimal FFmpeg demuxer path; do not build a general filesystem or async I/O library. Remove unused protocol dispatch.
- Accept: `cargo test -p player-core local_file` (short reads, seek beyond EOF, permission error, Unicode path), `tests/run.sh corpus`, `tests/run.sh seek` pass.

### P8 — Release

**P8.1 Reproducible dependency build and app bundle.** (L)
- Owns: `scripts/build-deps.sh`, `scripts/release.sh`, `packaging/`, `tests/release.py`, `tests/run.sh`, `docs/licenses.md`.
- Prereq: P7.5.
- Do: build FFmpeg/libass/libplacebo/shaderc (or glslang)/dav1d/Vulkan headers+loader/Lua from pinned sources (or checksum-pinned official MoltenVK binaries) into `.work/deps/release`; no Homebrew libraries. Link the smallest viable closure; bundle `libMoltenVK.dylib` and the ICD JSON in `Player.app`; set `VK_ICD_FILENAMES`/`VK_DRIVER_FILES` before Vulkan init; audit `@rpath`/`@loader_path`. Ad-hoc sign for local testing only.
- Accept: `scripts/release.sh` produces `dist/Player.app`; `tests/run.sh release` launches it, plays the corpus, and `otool -L` contains no `/opt/homebrew` path and no unexpected dylib.

**P8.2 Optimization.** (M)
- Owns: `scripts/build.sh`, `scripts/release.sh`, `meson.build`, `rust/Cargo.toml` (profile only).
- Prereq: P8.1.
- Do: measure and keep wins from LTO, cross-language LTO (only after the plain build is stable), dead stripping, symbol visibility, codegen units, duplicate-library elimination, and static-vs-dynamic choices. Do not remove error handling, validation, or diagnostics to save space. Measure the final package, not archives.
- Accept: `scripts/measure.sh compare` passes the P8 budgets below; `tests/run.sh suite` passes; a rejected optimization is recorded with its measurement.

**P8.3 Correctness, security, and license audit.** (L)
- Owns: `tests/`, `scripts/audit.sh`, `scripts/manual-checklist.sh`, `docs/scope.md`, `docs/licenses.md`.
- Prereq: P8.2.
- Do: implement the full correctness list in **Acceptance**; audit network absence, protocols, subprocess execution, runtime package-manager dependence, linked dylibs, duplicate interpreters, unexpected runtime resource loading, dependency advisories, and licenses. Preserve mpv/FFmpeg/libass/libplacebo notices and source obligations. Update the owner checklist with the release app path.
- Accept: `tests/run.sh suite`, `scripts/audit.sh`, and `scripts/measure.sh final` pass; owner checklist is scheduled.

**P8.4 Final docs and report.** (M)
- Owns: `PLAN.md`, `docs/*.md`.
- Prereq: P8.3.
- Do: update Status, ownership boundaries, renderer/audio integration, script host, FFI, migration sequence, remaining C code, dependency inventory, upstream divergence and provenance, and complexity accounting (production C lines remaining, Rust lines added, net source lines removed, compiled translation units, internal modules removed, external dependencies removed, supported commands/options, active FFI functions, unsafe Rust locations, total distributable size). Record final measurements, known limitations, and the follow-on boundary (7.6–7.8, Linux). Write the final report. Verify every path and link; run `mdtool check --untracked PLAN.md` if available, otherwise `rg -n '\[[^]]+\]\('` and inspect targets.
- Accept: all required docs exist and agree with the code; `git status` clean; `tests/run.sh suite` and `scripts/audit.sh` pass; final report committed.

---

## Acceptance

### Harness contract

`tests/run.sh <gate> [args]` prints one `PASS`/`FAIL` line per case and a total; non-zero on any failure. `tests/harness.py` is the only IPC implementation. It spawns `mpv` with `--no-config`, `--idle=yes`, `--input-ipc-server=<socket>`, `--terminal=yes`, `--msg-level=all=info`, `stdin=DEVNULL`, and a per-run socket in `.work/tmp`; it never uses screenshots as a gate. A timeout is a failure; the process is terminated at 2 s and killed at 5 s.

### Gates

| Gate | Checks |
|---|---|
| `fixtures` | Generate/verify the corpus and SHA-256 manifest; fail if the generator ffmpeg is not 9.0.2. |
| `build` | Clean configure+build; version string; expected feature summary; no `/opt/homebrew` in the release closure (P8). |
| `play FILE` | Load, reach first frame or audio clock, log `VO: [gpu-next]` for video, toggle fullscreen through the property and assert the change, no error/end-file failure. |
| `corpus` | All fixtures play: H.264 MP4, HEVC MKV, AV1 WebM, VP9 WebM, MPEG-4 AVI, FLAC/MP3/WAV/Ogg/ALAC audio, multi-audio MKV, embedded/external SRT/ASS, WebVTT, 5.1, VFR, non-ASCII path. Assert expected codecs/tracks and clean exit. |
| `audio` | Mono, stereo, and 5.1 initialize CoreAudio (`AO: [coreaudio]`), play 3 s, no underrun/error lines. |
| `hwdec` | `--hwdec=videotoolbox` logs `Using hardware decoding (videotoolbox)` for H.264/HEVC; `hwdec --software-fallback` decodes an unsupported profile without crashing. |
| `seek` | Seek to start/mid/end and arbitrary timestamps; `time-pos` within 0.25 s; 10 repeated seeks; no hang. |
| `local-only` | URL matrix rejected; local path matrix plays; playlist URL entry rejected while later entries play; `sandbox-exec` deny-network playback succeeds; `--list-protocols` has no network scheme; hostile temp `HOME` loads no config/script. |
| `local-paths` | Relative/absolute, spaces, `:`, Unicode, symlink, mounted path. |
| `osc` | OSC loads through the host with no script error; `input-bindings` has `owner=osc` mouse and key bindings; messages/bindings registered; timer/property/OSD path exercised. No pixel gate. |
| `osc-input` | Binding-level: a mouse-area/button binding and key binding dispatch to the expected command; `pause`, `time-pos`, and `fullscreen` respond. Visual layout is owner-run. |
| `lua-api-audit` | Regenerate `docs/lua-api.md`; fail on an unclassified `mp.*` call. |
| `lua-host` | Fixture script exercises command/get/set/observe/unobserve/timer/event/OSD/binding; deliberate error exits cleanly. |
| `ffi` | C test links the staticlib; valid/invalid calls return typed errors; a Rust panic is caught; no crash. |
| `rust-link` | Rust symbol present; player starts and plays; no duplicate runtime libs. |
| `cli` | Supported forms work; removed options fail cleanly; `--help` is the supported surface. |
| `playlist` | Multi-file load, next/prev, removal, Unicode, rejected URL; expected current entry. |
| `input` | Keyboard key matrix changes pause, time-pos, fullscreen, volume, mute, speed, frame-step, stop; bindings registered. |
| `state` | Property/event matrix across load/seek/pause/track switch/EOF; 100-event burst has no loss. |
| `ffmpeg-audit` | Build manifest matches the allowlist; only `file` protocol; no encoders/muxers/devices/external libs. |
| `release` | `dist/Player.app` launches, plays corpus, bundles MoltenVK ICD, no Homebrew path, no unexpected dylib. |
| `baseline-docs` | Every required baseline heading/value exists. |
| `meta` | Repo layout, ignore rules, plan/docs presence, no `/tmp` paths in scripts/tests. |

`tests/run.sh suite` is the union of `corpus`, `audio`, `hwdec`, `seek`, `local-only`, `local-paths`, `osc`, `osc-input`, `input`, `playlist`, and `state`, plus the source's correctness cases that can be automated: rapid pause/resume, rapid fullscreen, repeated open/close, window resize (property/layout checks), truncated/malformed/corrupted files, unsupported codec, clean shutdown. `release` is run separately at P8 and at the close. Visual/audio judgment is owner-run and is never a gate.

### Definition of done

The campaign is complete when all of these hold on the release revision:

- The corpus plays reliably on macOS ARM64; seeking, A/V synchronization, and VideoToolbox decoding remain dependable.
- FFmpeg is minimal, reproducibly built, and carries only the `file` protocol; no network backend, TLS stack, or CA bundle is linked, and no runtime package manager is required.
- Unsupported mpv features and obsolete code are removed; the remaining C is the playback pipeline, platform glue, and multimedia integration deliberately retained at the control-plane boundary (D3).
- Lua is owned by Rust through `mlua`; the OSC is the simplified Lua OSC and is responsive.
- The control plane (config, playlist, input/commands, state/events, local file) is Rust-owned.
- The distributable is materially smaller and documented; the dependency/complexity metrics are recorded with revisions.
- A fresh checkout builds and passes the suite with the documented commands.
- The Linux path is documented but not implemented.

### P8 budgets

Measure against the P0 baseline on the same fixtures and host. **These gates apply only at P8.** No earlier slice may fail, revert, or reopen for a size, CPU, or startup regression; record it in `docs/metrics.md` and continue.

| Metric | Target |
|---|---|
| Distributable size | ≤ 50% of the P0 unique closure (pilot 60.8 MB → ≤ 30 MB) |
| `mpv`/app executable | ≤ 10 MB stripped (pilot minimal-FFmpeg static build: 12 MB unstripped) |
| Cold startup to first frame | ≤ 1.0 s (pilot 0.57 s) |
| Active playback CPU, 1080p software decode | ≤ baseline + 10% |
| RSS, 1080p playback | ≤ 300 MB (pilot 261 MB) |
| Seek to first new frame | ≤ 250 ms |
| Frame drops | 0 in 30 s hardware-decoded 1080p; < 1% software 1080p |
| Network schemes | 0 |
| Duplicate Lua interpreters | 0 |

Report a failure as a failure. Do not relax a budget to hide a regression; investigate it.

---

## Needs environment

These need the owner or hardware the executor does not have. They do not hold slices open; record the result in `docs/manual-checklist.md` at each milestone and before release.

- Visual quality: HDR/10-bit color, subtitle shaping/positioning, motion smoothness, letterboxing/aspect, Retina 2× scaling, fullscreen on an external display, window resize.
- Audio: actual audibility and channel placement on the built-in speakers/headphones; the executor can only prove CoreAudio initialization and error-free playback.
- Interaction: real mouse clicks/drags on the OSC, drag-and-drop, menu/fullscreen behavior with the window server; automated gates cover bindings and state, not visual hit targets.
- Other Apple Silicon Macs and older macOS versions, if distribution targets them.
- Signing/notarization and distribution, if the owner distributes the app.
- A physical network monitor (Little Snitch or equivalent), if the owner wants an independent no-egress check beyond `sandbox-exec`, linked-library, and protocol-manifest gates.

Command: `scripts/manual-checklist.sh [--fixture-dir .work/fixtures]` opens the release app with each fixture and prints the checklist; the owner records pass/fail in `docs/manual-checklist.md`.

---

## Execution boundary and non-goals

### Authorized

- Initialize and commit in `/Users/josh/d/play`; import the pinned upstream trees; rewrite/delete repo source.
- Install the pinned Homebrew **development** packages in D14; build Lua/FFmpeg/deps from pinned sources; download pinned tarballs.
- Build and run the player, open windows, drive IPC, inspect processes/logs/dylibs, run `sandbox-exec` with network denied.
- Add/remove Rust crates and C dependencies within the campaign scope; add tests; produce `dist/Player.app`; ad-hoc sign locally.
- Commit local changes. **No push, publish, or distribution.**

### Hard stops

Park the item on `park/<item>` with the exact cause and retry condition when: the next action falls outside the authorized list; a contract, license, or distribution decision changes and is not already recorded; the item is destructive outside `.work/`; or no local check can prove the acceptance. Do not wait on the owner; continue with the next independent item if one exists, otherwise stop and report.

### Forbidden

- Push, publish, notarize, or distribute; change CI workflows; run formatters.
- Network access by the built player; network protocols, TLS, CA bundles, remote resource loading, yt-dlp, or subprocess URL resolution.
- Writing outside the repo except the pinned Homebrew installs, `.work/`, and `dist/`; using `/tmp`/`$TMPDIR` for work.
- Changing system audio/window/security settings; deleting user media or files outside the repo.
- Automatic loading of user config or scripts in the shipped player (D10).
- Feature parity with mpv or rtv. This is the defined local-playback scope only.

### Contract changes carried by this campaign

- The fork is GPL-2.0-or-later; distribution carries source/notice obligations.
- The default renderer is gpu-next/libplacebo/Vulkan/MoltenVK; no OpenGL path.
- CoreAudio uses the D7 workaround until upstream fixes the channel map.
- The shipped app does not auto-load user config/scripts.
- Lua 5.2 is the only interpreter; the OSC stays Lua on mlua.
- Local-only rejection is a hard boundary, not a convention.
- Release output is a `.app` bundle containing MoltenVK.

### Non-goals

HTTP/HTTPS/FTP/SSH/SFTP/RTSP/RTMP/HLS/DASH, yt-dlp, remote playlists/subtitles/metadata, optical discs/Blu-ray/DVD/CDDA, capture cards/webcams/screen capture, transcoding/recording/encoding, Chromecast/UPnP/web servers, browser embedding, Qt/GTK/Electron/SDL/egui/Slint/web UI, JavaScript, VapourSynth/arbitrary filter pipelines, general C plugins, network metadata/artwork, Linux implementation, general demux/decode orchestration beyond the Symphonia audio decoder bridge (7.6), audio/A-V sync replacement (7.7), renderer integration (7.8), Rust OSC, universal binaries, App Store/notarization, upstream feature parity, CI changes.

### Follow-on (not scheduled)

- **7.6–7.8.** General demux/decode orchestration (excluding the P1.2 Symphonia audio decoder bridge), audio/A-V synchronization, and renderer integration. Entry: the release passes the P8 suites, the control plane is stable, and characterization tests cover the pipeline. Do not start them opportunistically inside this campaign.
- **Linux.** x86-64 and aarch64, musl, Wayland only, ALSA only, no X11, PulseAudio, PipeWire, systemd, D-Bus, GTK, or Qt; a small, preferably statically linked executable. Keep window creation, audio output, hardware decoding, presentation, filesystem paths, and platform events behind clean interfaces now. Do not add trait hierarchies or a second implementation before Linux work begins. When it begins, restore the same local-playback scope first.

---

## Reference

Verified against mpv v0.41.0 and FFmpeg n9.0.2 at the pinned commits.

### mpv code map

| Path | Why it matters |
|---|---|
| `meson.build:29` | libplacebo ≥ 6.338.2 is a hard dependency. |
| `meson.build:1300` | gpu-next requires libplacebo with Vulkan and Vulkan ≥ 1.3.238. |
| `video/out/gpu/context.c:78-97` | GPU context table; on macOS only `ra_ctx_vulkan_mac` under Cocoa+Swift. No Cocoa OpenGL context. |
| `video/out/vulkan/context_mac.m` | macOS Vulkan/MoltenVK context and swapchain. |
| `audio/out/ao_coreaudio.c:324-329` | The ChannelMap call that fails with -50 on macOS 27 (D7). |
| `player/loadfile.c:2080` | `mp_play_files`, the load/playlist choke point. |
| `player/client.c:1149` | `mpv_command_node`, the internal client command entry. |
| `player/lua.c:105` | C Lua host `lua_State`. |
| `player/scripting.c:276` | `mp_load_scripts`, built-in and user script loading. |
| `player/lua/osc.lua` | Upstream OSC (2977 lines) and its API closure. |
| `player/lua/defaults.lua`, `assdraw.lua`, `options.lua`, `input.lua`, `select.lua` | OSC helpers and the Lua API surface. |
| `player/lua/meson.build` | Built-in Lua module install list. |
| `options/parse_commandline.c`, `options/options.c` | CLI/option parsing to reduce and then type. |
| `stream/stream.c`, `stream/stream_lavf.c`, `stream/stream_file.c` | Stream/protocol dispatch and the local file path. |
| `demux/demux_lavf.c`, `demux/demux_playlist.c` | FFmpeg demux bridge and local playlist parsing; keep. |
| `demux/demux_disc.c` | Disc path; remove in P2.2. |
| `video/out/vo_gpu.c`, `video/out/opengl/` | Dead GL path on macOS; remove in P2.1/P2.3. |
| `common/playlist.c` | Playlist ownership to move to Rust. |
| `input/cmd.c`, `input/input.c`, `player/command.c` | Command table and dispatch to type and move. |

### Upstream and prior art

- mpv: <https://github.com/mpv-player/mpv> — authoritative playback and scripting behavior.
- FFmpeg: <https://github.com/FFmpeg/FFmpeg> — component selection, demux/decode, VideoToolbox.
- mlua: <https://github.com/mlua-rs/mlua> — Rust-owned Lua host.
- libplacebo: <https://github.com/haasn/libplacebo> — rendering and color management.
- libass: <https://github.com/libass/libass> — subtitle shaping/rendering.
- rtv: <https://github.com/Laotian-telephonebell2559/rtv> — architectural inspiration only; do not adopt as the implementation, pursue parity, or copy without license/attribution review.

### Campaign documents

`docs/baseline.md` (P0.4), `docs/ffmpeg.md` (P1), `docs/architecture.md` (P1.2+), `docs/metrics.md` (P0.4+), `docs/scope.md` (P2–P3), `docs/lua-api.md` (P5.2), `docs/licenses.md` (P1.2+), `docs/manual-checklist.md` (owner runs).
