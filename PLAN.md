# Campaign: Linux x86-64 musl static player

## Status

- **Goal:** A fully static `x86_64-unknown-linux-musl` build of Player with the macOS app's local-only scope: mpv's playback pipeline, the pinned minimal FFmpeg, the Rust control plane, one Lua 5.2 interpreter, Wayland software output, and ALSA.
- **Last accepted revision:** `787caec` (macOS release, `make install`, macOS CI). The macOS campaign is complete; `docs/status.md` and `docs/final-report.md` are the record.
- **Accepted:** none. **Red:** none. **Parked:** none.
- **Next slice:** L0.1.
- **Resume prompt:** `Read PLAN.md and continue at Status. Stop at a documented limitation or an owner decision.`
- **First command:** `scripts/linux-check.sh`, created by L0.1.
- **Test mode:** headless (`--vo=null --ao=null`) for logic, a headless Weston compositor for `wlshm`, and owner-run checks for visible and audible output.
- **Scope:** Linux x86-64 only. aarch64, X11, GL, Vulkan, DRM, PulseAudio, PipeWire, systemd integration, and distribution packages are out.

## Charter

1. **Decide and keep going.** Every decision needed to run is below. Make ordinary judgment calls inside the stated bounds and record one only when a later slice depends on it. Do not ask the owner at runtime.
2. **The code is the truth.** If a path, option, or component name here disagrees with the pinned source, the source wins: fix the item, keep the intent, and record the correction in the commit body and `docs/architecture.md` when a boundary changes.
3. **Static first.** A slice that adds a dynamic dependency, a runtime loader, or a second renderer path is red. Build the smallest static closure that plays the corpus; do not carry macOS-only libraries into the Linux build.
4. **One writer.** No parallel lanes. Every item is serial and lands on `main`. Scratch goes in `.work/tmp`; worktrees go in `.work/wt/<name>`. Never use `/tmp` or `$TMPDIR`.
5. **Delete before rewriting.** Restore only the source files the Linux build needs, then delete the macOS-only conditionals that stop compiling. No persistent parallel C or Rust implementations.
6. **Fix only what blocks.** In their own commit, fix a crash, a hang over 5 s, a network or secret leak, or a correctness regression in an accepted behavior. Record everything else and leave it.
7. **Red never lands.** An item ends on its acceptance command plus the narrowest affected suite on the integrated tree; every accepted slice leaves a usable Linux binary. The full suite runs at L4.4 and the close; the macOS suite stays green at every accepted slice.
8. **Failures get a cause before a retry.** Reproduce, isolate, fix the cause, keep the failing test. Time-box an item at three attempts, then park it on `park/<item>` with the exact cause and a retry condition.
9. **Commits.** Conventional Commit, workstream as scope, item ID in the body. One coherent change per commit; stage only your work. No formatters. No CI changes outside `.github/workflows/macos.yml` and `.github/workflows/linux.yml`. Never publish a release.
10. **Contract changes are surfaced.** A new platform, a second VO/AO, a fully static binary, and the loss of GPU rendering on Linux are contract changes. Update code, tests, and docs together and call them out in the reply.
11. **Docs are deliverables.** `docs/linux.md`, `docs/architecture.md`, `docs/ffmpeg.md`, `docs/licenses.md`, `docs/metrics.md`, `docs/status.md`, and `docs/install.md` are maintained with the code.
12. **Steering checkpoint.** After each accepted slice, name what landed, the single limit on delivery, and a ruling only when it changes: continue, narrow the slice, serialize, fix one prerequisite, or park. Rewrite **Status** in place; never append a checkpoint section.

### Porting rules

- **Restore from the pin.** The Linux source set comes from mpv v0.41.0 at `41f6a645068483470267271e1d09966ca3b9f413`. Do not merge upstream history. Copy the smallest file set, keep the license headers, and record each restored file in `docs/architecture.md`.
- **Keep macOS unchanged.** Every platform choice is explicit in Meson and `#if`. No runtime `uname` dispatch for a compile-time decision.
- **No runtime loader.** A fully static musl process cannot `dlopen`. A library that discovers plugins or drivers at runtime does not ship. This is why the Linux build uses `wlshm` and ALSA hardware PCMs.
- **One implementation.** One VO (`wlshm`), one AO (`alsa`), one Lua interpreter, one input classifier. Do not add a second path behind a flag.
- **No speculative abstraction.** Do not build a cross-platform VO/AO interface for a platform that does not exist. Use the existing mpv interfaces and macOS-style `#if` branches.

## Decisions

### Owner-decided

- **LD1 — Target.** Linux x86-64, `x86_64-unknown-linux-musl`, fully static. The release artifact is `player-x86_64-linux-musl.tar.gz` containing one stripped `player` binary.
- **LD2 — Renderer.** `vo=wlshm`: Wayland shared-memory software output, restored from mpv v0.41.0. A fully static musl binary cannot load a Vulkan ICD, so Vulkan, OpenGL, EGL, DRM, and libplacebo are not in this build. A GPU build would be a separate mostly-static artifact and is not scheduled.
- **LD3 — Audio.** ALSA only, statically linked. No PulseAudio, PipeWire, JACK, SDL, or OSS. ALSA plugins that need `dlopen` are unavailable; the default device must be a built-in or hardware PCM.
- **LD4 — Output.** `wlshm` uses `libswscale` for scaling and the existing OSD/subtitle path. The binary has no runtime data directory, no service unit, and no desktop-file loader.
- **LD5 — Release.** The macOS release stays `dist/Player.app` with `gpu-next`, CoreAudio, and `make install`. The Linux release is a tarball plus a SHA-256 file. CI builds both; neither publishes or notarizes.
- **LD6 — Build host.** Build natively inside an Alpine edge x86-64 container, not by cross-compiling from macOS. The same container runs locally through Docker and in the Linux CI job.

### Renderer shape

Three whole designs were considered before the sequence was written:

- **A — fully static `wlshm` plus ALSA (chosen).** It meets LD1, uses the software output that already exists in the pin, and needs no runtime loader. It gives up GPU rendering, HDR, and hardware decode on Linux.
- **B — mostly-static `gpu-next` plus Vulkan and ALSA.** It keeps the macOS renderer, but the Vulkan loader must `dlopen` a system ICD, so the binary is not static; the release would depend on the host's graphics stack. It fails LD1 and is the documented follow-on, not this campaign.
- **C — a new fully static software VO.** It duplicates `vo_wlshm` and adds a second rendering path to maintain. The pin already has the software Wayland output, so this design loses on cost.

### Prescribed

- **LD7 — Source set.** Restore `video/out/wayland_common.c`, `video/out/wayland_common.h`, `video/out/vo_wlshm.c`, `audio/out/ao_alsa.c`, and `osdep/als-linux.c` from the pin. Do not restore dmabuf, X11, GL, Vulkan, or PipeWire sources.
- **LD8 — Dependency set.** Build Wayland, wayland-protocols, libxkbcommon, ALSA, fontconfig, expat, FreeType, FriBiDi, HarfBuzz, libass, dav1d, and FFmpeg from pinned sources for musl. Reuse the macOS pins where the component is shared. No libplacebo, shaderc, glslang, SPIRV, Vulkan, or MoltenVK.
- **LD9 — Verification.** `file`, `readelf`, and `ldd` prove the static shape. `tests/run.sh` gates behavior. A headless Weston compositor proves `wlshm`; an ALSA null PCM proves the audio path without a sound card.
- **LD10 — Packaging.** `scripts/release-linux.sh` builds, strips, verifies, and tars the binary. The tarball contains `player` and `README` only. A SHA-256 file sits beside it.

### Delegated

- **DL1 — Pins.** Resolve the exact Wayland, xkbcommon, ALSA, fontconfig, and expat versions at L1.1, record them in `docs/licenses.md` and the build script, and do not use unpinned `apk` packages for shipped libraries.
- **DL2 — Source list.** L0.2 decides the exact file set by compiling the Linux target; restore a file only when the compiler or link step needs it.
- **DL3 — Scaler defaults.** Choose the `wlshm` scaler, color conversion, and scaling filter from the existing `sws_utils` options. Keep the default that plays the corpus without artifacts; record the choice in `docs/architecture.md`.
- **DL4 — Fonts.** Use static fontconfig for system fonts and embedded fonts. If fontconfig cannot be made static cleanly, fall back to a deterministic font provider and record the loss of system-font discovery in `docs/linux.md`.
- **DL5 — Linker flags.** Choose `-static` or `-static-pie`, LTO, and strip flags by measurement. Keep a change only if it wins on size or startup without a regression beyond the release budgets.

## Baseline

### Current state

The macOS release is complete and documented. Linux is not buildable today:

- `meson.build` still contains `wayland` and `alsa` option branches, but the files they name are gone: `video/out/wayland_common.c`, `video/out/vo_wlshm.c`, `audio/out/ao_alsa.c`, and `osdep/als-linux.c`.
- `video/out/vo.c` registers only `gpu_next`, `libmpv`, `null`, and `lavc`. `audio/out/ao.c` registers only CoreAudio, `null`, and `lavc`.
- `meson.build` requires libplacebo and compiles `vo_gpu_next` and the placebo sources unconditionally.
- `player/main.c` contains bundle-specific `MPVBUNDLE` and `VK_DRIVER_FILES` code.
- `scripts/build-deps.sh`, `scripts/release.sh`, `scripts/bundle.py`, `scripts/audit.sh`, and `scripts/measure.sh` use macOS tools (`sysctl`, `shasum`, `otool`, `codesign`, `ditto`, `MACOSX_DEPLOYMENT_TARGET`).
- `tests/harness.py` assumes `dist/Player.app` for the release gate and uses `sandbox-exec` for the network-denied playback check.

The Rust plane is portable. `player-core`, `player-ffi`, `player-lua`, and `player-audio` have no macOS-only source; `rust/player-release` is a macOS cdylib and `build.rs` only adds macOS linker arguments. Linux links the existing staticlibs.

### Host and pins

| Item | Value |
|---|---|
| macOS host | macOS 27.0.1, Apple Silicon arm64, Xcode 27.1 |
| Linux host | Alpine edge x86_64 in Docker or CI |
| Rust | nightly-2026-09-15 from `rust-toolchain.toml` |
| mpv | v0.41.0, `41f6a645068483470267271e1d09966ca3b9f413` |
| FFmpeg | n9.0.2, `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3` |
| Symphonia | `b6336209bb40d57e4ed71079cf7f7fc6c4626aae` |
| Lua | 5.2.4 vendored by `mlua` |
| Shared deps | libass 0.17.5, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, dav1d 1.5.4 |

### Constraints

- A static musl executable has no dynamic linker, so `dlopen` fails. Vulkan ICD discovery, ALSA plugin loading, and any library that loads a `.so` at runtime cannot be used.
- `vo=wlshm` is the only software Wayland output in the pin. It scales with `libswscale`, draws OSD through the software path, and needs `wayland-client`, `wayland-cursor`, `xkbcommon`, and generated protocol code at build time.
- ALSA's `default` PCM may route through a shared plugin. The static build must use a built-in or hardware PCM; the owner check records which device works on the test machine.
- FFmpeg's VideoToolbox decoders and hwaccels are macOS-only. The Linux manifest needs a platform section; the existing allowlist enables `h264_videotoolbox` and `hevc_videotoolbox` unconditionally.
- Static fontconfig reads system font configuration at runtime. Embedded fonts and libass shaping are the paths to verify in L2.3; system-font discovery is the main risk.
- `mlua` with `lua52` and `vendored` builds Lua from source. The Linux build must not add a second interpreter or link a distro Lua.

### Measurement protocol

`scripts/measure-linux.sh` reports, for a named revision: stripped binary size, unique link closure, cold and warm `--version` startup (7 runs, median and spread), first-frame latency (`--frames=1 --untimed`, 7 runs), idle and 1080p RSS, 1080p CPU, seek latency, exit latency, and protocol count. Use `/usr/bin/time -v` for RSS and CPU; use the same fixtures and flags every time. Interleave baseline and candidate runs when comparing. Record the command, revision, and date in `docs/metrics.md`. Do not present the tarball size as the binary size.

## Sequence

Every item names owned files, prerequisites, and one acceptance command. Items are serial. Sizes are estimates: **S** ≤ 2 h, **M** ≤ 1 day, **L** ≤ 3 days.

### L0 — Ground

**L0.1 Linux build probe.** (S)
- Owns: `scripts/linux-check.sh`, `docs/status.md`.
- Prereq: none.
- Do: run `alpine:edge` x86_64, install `build-base clang cmake meson ninja pkgconf python3 wayland-dev wayland-protocols libxkbcommon-dev alsa-lib-dev`, and configure `build-linux` with `-Dcplayer=true -Dlibmpv=false -Dgpl=true -Dtests=false -Dfuzzers=false -Dbuild-date=false -Dwayland=enabled -Dalsa=enabled -Dvulkan=disabled -Dgl=disabled`. Record the first failure.
- Accept: `scripts/linux-check.sh` exits nonzero and names the missing files; `docs/status.md` records the stop.

**L0.2 Restore the Linux source set.** (M)
- Owns: `video/out/wayland_common.c`, `video/out/wayland_common.h`, `video/out/vo_wlshm.c`, `audio/out/ao_alsa.c`, `osdep/als-linux.c`, `docs/licenses.md`.
- Prereq: L0.1.
- Do: copy each file from mpv v0.41.0 `41f6a645068483470267271e1d09966ca3b9f413`, keep the headers, and make no edits. Diff each against the pin.
- Accept: `git diff --no-index` is empty for every restored file; `meson setup` gets past source resolution.

**L0.3 Platform gates and the first Linux binary.** (M)
- Owns: `meson.build`, `meson.options`, `video/out/vo.c`, `audio/out/ao.c`, `player/main.c`, `scripts/build-linux.sh`.
- Prereq: L0.2.
- Do: make libplacebo optional and the placebo/gpu-next sources conditional; add `wlshm` and `alsa` to the driver tables under `HAVE_WAYLAND` and `HAVE_ALSA`; guard the bundle code in `player/main.c`; build `build-linux/player` with `wlshm` and ALSA enabled and `--vo=null --ao=null`.
- Accept: `build-linux/player --version` runs; `--vo=help` lists `wlshm`; `--ao=help` lists `alsa`; the binary links no macOS framework.

**L0.4 Linux baseline.** (M)
- Owns: `scripts/measure-linux.sh`, `docs/metrics.md`, `docs/architecture.md`.
- Prereq: L0.3.
- Do: record the binary size, link closure, cold and warm `--version`, idle RSS, and protocol count; write the platform split into `docs/architecture.md`.
- Accept: `scripts/measure-linux.sh baseline` writes one row; the doc names the remaining Linux gaps.

### L1 — Static closure

**L1.1 musl dependency prefix.** (L)
- Owns: `scripts/build-deps-linux.sh`, `docs/licenses.md`, `docs/ffmpeg.md`.
- Prereq: L0.4.
- Do: pin and build Wayland, wayland-protocols, libxkbcommon, ALSA, fontconfig, expat, FreeType, FriBiDi, HarfBuzz, libass, dav1d, and FFmpeg into `.work/deps/linux`. Build static libraries only. Do not build libplacebo, shaderc, glslang, SPIRV, Vulkan, or MoltenVK.
- Accept: `pkg-config --static` resolves every dependency; the prefix contains no `.so`; every shipped `.a` has a recorded version.

**L1.2 Linux FFmpeg allowlist.** (M)
- Owns: `scripts/ffmpeg-components.txt`, `scripts/build-ffmpeg.sh`, `scripts/ffmpeg-manifest.sh`.
- Prereq: L1.1.
- Do: split the allowlist into common and platform sections. Linux drops `h264_videotoolbox`, `hevc_videotoolbox`, and `--enable-videotoolbox`; it keeps the demuxers, decoders, parsers, bitstream filters, filters, and the `file` protocol. Validate every name against the pinned `configure --list-*`.
- Accept: `tests/run.sh ffmpeg-audit` passes on Linux; `--list-protocols` reports `file` only; the manifest has no encoder, muxer, device, or network protocol.

**L1.3 Static link.** (L)
- Owns: `meson.build`, `scripts/build-linux.sh`, `rust/player-release/Cargo.toml`, `rust/Cargo.toml`.
- Prereq: L1.2.
- Do: build `player-ffi` and `player-audio` for `x86_64-unknown-linux-musl`; link them statically into the C player; do not build the `player-release` cdylib on Linux; add `-static` or `-static-pie`; strip.
- Accept: `file dist/player` reports `statically linked` or `static-pie linked`; `readelf -lW` has no `INTERP`; `readelf -dW` has no `NEEDED`; `ldd` reports not dynamic; `dist/player --version` runs.

**L1.4 Static audit.** (S)
- Owns: `scripts/audit-linux.sh`, `tests/run.sh`.
- Prereq: L1.3.
- Do: check the static shape, absence of network symbols and protocol strings, one `luaL_newstate` in the pre-strip binary, one `file` protocol, and license files. Add a negative case that a temporary `NEEDED` entry fails.
- Accept: `scripts/audit-linux.sh` passes; the negative case fails as expected and is removed.

### L2 — Display and audio

**L2.1 `wlshm` output.** (L)
- Owns: `video/out/vo.c`, `video/out/wayland_common.c`, `video/out/vo_wlshm.c`, `video/out/meson.build`, `docs/architecture.md`.
- Prereq: L1.4.
- Do: register `wlshm`, disable `dmabuf-wayland`, and verify xdg-shell, fractional scale, resize, fullscreen, OSD, and clean shutdown against a headless Weston compositor.
- Accept: a 1080p fixture plays through Weston; logs show `VO: [wlshm]`; no hang or crash.

**L2.2 Wayland input.** (M)
- Owns: `video/out/wayland_common.c`, `input/input.c`, `player/lua/osc.lua`.
- Prereq: L2.1.
- Do: wire keyboard and pointer input through `wayland_common.c`, bind the shipped keys, and verify OSC areas, visibility, fullscreen, title, and cursor handling.
- Accept: `input-bindings` contains the OSC mouse and key bindings; `tests/run.sh osc-input` passes; real clicks and drags are owner-run.

**L2.3 Subtitles and fonts.** (M)
- Owns: `sub/`, `scripts/build-deps-linux.sh`, `tests/harness.py`.
- Prereq: L2.1.
- Do: link static libass, FreeType, FriBiDi, HarfBuzz, and fontconfig; render embedded and external SRT, ASS, and WebVTT; verify non-ASCII paths and font fallback.
- Accept: the subtitle corpus cases pass under Weston; the owner checklist records shaping and positioning.

**L2.4 ALSA output.** (M)
- Owns: `audio/out/ao.c`, `audio/out/ao_alsa.c`, `scripts/build-deps-linux.sh`, `docs/linux.md`.
- Prereq: L1.4.
- Do: link static ALSA, register `alsa`, and test mono, stereo, 5.1, pause, seek, and device selection. Use a built-in or hardware PCM; do not depend on a shared ALSA plugin.
- Accept: `--ao=help` lists `alsa`; playback through an ALSA null PCM exits cleanly; audible output is owner-run.

### L3 — Control plane and boundary

**L3.1 Rust staticlibs on musl.** (M)
- Owns: `rust/player-release/Cargo.toml`, `rust/Cargo.toml`, `scripts/build-linux.sh`.
- Prereq: L1.3.
- Do: build the staticlibs for `x86_64-unknown-linux-musl`, link them through Meson, and remove any macOS-only link argument or cdylib assumption from the Linux path.
- Accept: `cargo test --target x86_64-unknown-linux-musl` passes; `nm` shows the bridge symbols; no duplicate Rust runtime.

**L3.2 Local file and input.** (M)
- Owns: `rust/player-core/src/local_file.rs`, `rust/player-core/src/input.rs`, `stream/stream_file.c`, `stream/stream.c`.
- Prereq: L3.1.
- Do: verify UTF-8 paths, Unicode, symlinks, relative paths, permission errors, short reads, and seek beyond EOF on Linux; keep the classifier's rejection semantics.
- Accept: `cargo test -p player-core` and `tests/run.sh local-paths` pass on Alpine.

**L3.3 Local-only gate.** (M)
- Owns: `tests/harness.py`, `tests/local_only.py`, `scripts/audit-linux.sh`.
- Prereq: L3.2.
- Do: replace the `sandbox-exec` check with `unshare -n` on Linux, keep the URL and malformed-input matrices, and keep the hostile `HOME` test. Assert one `file` protocol and no network symbols.
- Accept: `tests/run.sh local-only` passes with `unshare -n`; the hostile config loads no script.

**L3.4 Lua host.** (M)
- Owns: `rust/player-lua`, `player/lua/`, `meson.build`.
- Prereq: L3.1.
- Do: build the vendored Lua 5.2 through `mlua` for musl, load the built-in OSC, and prove one interpreter in the static link.
- Accept: `tests/run.sh lua-host` and `tests/run.sh osc` pass; the unstripped binary has one `luaL_newstate`.

### L4 — Tests and release

**L4.1 Harness platform split.** (M)
- Owns: `tests/harness.py`, `tests/run.sh`, `tests/local_only.py`.
- Prereq: L3.3.
- Do: select the player path, IPC socket directory, `XDG_RUNTIME_DIR`, and network-denial command by platform; keep `--no-config`, `--vo=null`, and `--ao=null` for logic gates.
- Accept: `linux-corpus`, `linux-audio`, `linux-local-only`, and `linux-osc` pass on Alpine x86_64; the macOS suite still passes.

**L4.2 Headless Wayland gate.** (M)
- Owns: `tests/wayland.py`, `tests/run.sh`, `scripts/linux-check.sh`.
- Prereq: L2.2, L4.1.
- Do: start `weston --backend=headless-backend.so` with a private socket, play a fixture through `wlshm`, and assert the log, exit status, and OSD path. Keep screenshots diagnostic only.
- Accept: `tests/run.sh linux-wlshm` passes in CI and locally.

**L4.3 Fixtures.** (M)
- Owns: `tests/fixtures/gen.sh`, `scripts/build-deps-linux.sh`.
- Prereq: L4.1.
- Do: provide an FFmpeg 9.0.2 development binary for fixture generation, either built from the pin or checksum-pinned; keep the 27 fixture hashes unchanged.
- Accept: `tests/run.sh fixtures` regenerates and verifies the corpus on Linux.

**L4.4 Release packaging.** (M)
- Owns: `scripts/release-linux.sh`, `Makefile`, `docs/install.md`, `docs/linux.md`.
- Prereq: L4.2, L4.3.
- Do: build, strip, verify, and tar `dist/player`; write `player-x86_64-linux-musl.tar.gz` and a SHA-256 file. Add `make linux`, make `make install` select the host (app on macOS, `~/.local/bin/player` on Linux), and extend the checklist with a `--release-linux` mode.
- Accept: the tarball extracts to `player`; `make install` installs and runs it on Linux; the checksum matches.

### L5 — Close

**L5.1 Linux docs.** (M)
- Owns: `README.md`, `docs/install.md`, `docs/linux.md`, `docs/architecture.md`, `docs/scope.md`, `docs/ffmpeg.md`, `docs/licenses.md`, `docs/metrics.md`, `docs/status.md`, `AGENTS.md`.
- Prereq: L4.4.
- Do: document the Alpine build, the static limits (`wlshm`, ALSA PCMs, no GPU), runtime requirements, licenses, measurements, and the macOS/Linux split. Keep one topic per file and link from `AGENTS.md`.
- Accept: every changed path and link resolves; the docs match the built binary.

**L5.2 Linux CI.** (M)
- Owns: `.github/workflows/linux.yml`.
- Prereq: L4.4.
- Do: add an `alpine:edge` x86_64 job that installs the build tools, runs `scripts/build-deps-linux.sh`, `scripts/release-linux.sh`, `scripts/audit-linux.sh`, and `tests/run.sh suite`, and uploads the tarball.
- Accept: the job is green on a push; the artifact contains a static binary.

**L5.3 Final report.** (S)
- Owns: `docs/final-report.md`, `docs/status.md`, `docs/repo-audit.md`.
- Prereq: L5.1, L5.2.
- Do: record the outcome, budgets, known limits (software rendering, no Pulse/PipeWire, no GPU), provenance, and follow-on boundary.
- Accept: `docs/status.md` lists the evidence; the report and audit agree with the built artifacts.

## Acceptance

### Gates

| Gate | Checks |
|---|---|
| `linux-check` | The Alpine container has the build tools; configure fails only at the recorded gaps. |
| `linux-build` | `scripts/release-linux.sh` produces `dist/player` and the tarball. |
| `linux-static` | `file`, `readelf -l`, `readelf -d`, and `ldd` prove a static x86-64 executable. |
| `linux-audit` | No network symbols or protocols, one `file` protocol, one Lua state, and the license set. |
| `linux-corpus` | Every fixture plays with `--vo=null --ao=null` and exits cleanly. |
| `linux-wlshm` | A fixture plays through `wlshm` under headless Weston with no crash or hang. |
| `linux-audio` | ALSA opens a null or hardware PCM for mono, stereo, and 5.1. |
| `linux-local-only` | The URL matrix is rejected, local paths play, and playback succeeds under `unshare -n`. |
| `linux-osc` | The OSC loads, registers its bindings and messages, and updates the OSD. |
| `linux-release` | The tarball extracts, runs, and matches the recorded SHA-256. |

`tests/run.sh suite` is the union of `linux-corpus`, `linux-audio`, `linux-wlshm`, `linux-local-only`, `linux-osc`, `linux-static`, and `linux-audit`, plus rapid pause/resume, repeated open/close, truncated and malformed input, unsupported codecs, and clean shutdown. Visual and audible judgment is owner-run and is never a gate.

### Definition of done

- A fresh Alpine x86_64 checkout builds with `scripts/build-deps-linux.sh` then `scripts/release-linux.sh`.
- `dist/player` is a stripped static x86-64 executable with no interpreter and no `NEEDED` entries.
- The corpus plays under a Wayland compositor through `wlshm`, with working OSD, subtitles, keyboard, and pointer input.
- The player exposes one `file` protocol and makes no network connection.
- ALSA plays on real hardware; the owner checklist records audibility and channel placement.
- The macOS app, suite, and `make install` are unchanged.
- `docs/linux.md`, the platform docs, the CI job, and the final report agree with the code.

### Budgets

Measured against the L0.4 baseline on the same host and fixtures.

| Metric | Target |
|---|---:|
| Stripped binary | ≤ 40,000,000 B |
| Cold `--version` | ≤ 300 ms |
| Warm `--version` | ≤ 150 ms |
| First frame, 1080p | ≤ 1 s |
| 1080p RSS | ≤ 300 MB |
| Seek to first new frame | ≤ 250 ms |
| Network schemes | 0 |
| Dynamic dependencies | 0 |
| Duplicate Lua interpreters | 0 |

Report a failure as a failure. Do not relax a budget to hide a regression; investigate it.

## Needs environment

These need the owner or hardware the executor does not have. They do not hold slices open; record the result in `docs/manual-checklist.md` at each milestone and before release.

- A real Wayland compositor (sway, GNOME, KDE) for window, fullscreen, fractional-scale, and input behavior.
- A real ALSA device for audibility, channel placement, and underrun checks. CI has no sound card.
- x86-64 Linux hosts other than Alpine and the CI runner, if the tarball is distributed broadly.
- Visual quality: subtitle shaping and positioning, motion smoothness, letterboxing, and scaling artifacts from the software output.
- Signing or distribution. Linux binaries are unsigned; no package repository or notarization is in scope.

Command: `scripts/manual-checklist.sh --fixture-dir .work/fixtures --release-linux` opens the Linux binary with each fixture and prints the checklist; the owner records pass/fail in `docs/manual-checklist.md`.

## Execution boundary and non-goals

### Authorized

- Restore the named files from the mpv v0.41.0 pin and rewrite the Linux build surface.
- Build the dependency closure from pinned sources in `.work/deps/linux`.
- Add Linux tests, scripts, docs, and the Linux CI job.
- Build and run the player, drive IPC, run a headless Wayland compositor, and inspect the static binary.
- Commit local changes. Do not publish a release, push tags, or notarize.

### Hard stops

Park the item on `park/<item>` with the exact cause and a retry condition when: the next action falls outside the authorized list; a contract, license, or distribution decision changes and is not already recorded; the item is destructive outside `.work/` and `dist/`; or no local check can prove the acceptance.

### Forbidden

- Network access by the built player; network protocols, TLS, CA bundles, remote resource loading, or subprocess URL resolution.
- X11, OpenGL, Vulkan, EGL, DRM, libplacebo, shaderc, MoltenVK, or a GPU driver in the static build.
- PulseAudio, PipeWire, JACK, SDL, or a shared ALSA plugin in the release path.
- aarch64 Linux, universal binaries, distribution packages, systemd units, or desktop-file integration.
- Changing macOS behavior, the macOS release shape, or the macOS dependency pins.
- Automatic user config or scripts in the shipped player.
- Feature parity with mpv. This is the defined local-playback scope only.

### Contract changes carried by this campaign

- A second platform with a static binary and a tarball release.
- A second VO (`wlshm`) and AO (`alsa`) on Linux.
- No GPU rendering, HDR, or hardware video decode on Linux.
- No PulseAudio or PipeWire routing; ALSA devices only.
- The local-only rejection boundary is unchanged.

### Non-goals

aarch64 Linux, X11, Wayland dmabuf, Vulkan/OpenGL rendering, VA-API, DRM/KMS, PulseAudio, PipeWire, JACK, SDL, Flatpak, Snap, AppImage, distro packages, systemd integration, desktop portals, HDR, hardware decode, a Rust OSC, universal binaries, notarization, and upstream feature parity.

### Follow-on (not scheduled)

- A mostly-static GPU build with `gpu-next`, Vulkan, and VA-API, packaged separately from the fully static build.
- aarch64 Linux and additional musl architectures.
- Packaging for one distribution family after the static tarball is stable.

## Reference

Verified against mpv v0.41.0 at `41f6a645068483470267271e1d09966ca3b9f413` and FFmpeg n9.0.2 at `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`.

### Files to restore

| Path | Why it matters |
|---|---|
| `video/out/wayland_common.c`, `.h` | Wayland registry, surface, seat, input, and xdg-shell state. |
| `video/out/vo_wlshm.c` | Software Wayland output over `wl_shm`; the only static VO in the pin. |
| `audio/out/ao_alsa.c` | ALSA output; the only static audio path. |
| `osdep/als-linux.c` | Ambient light sensor reader for Linux; `meson.build` names it but the file is gone. |
| `video/out/vo.c` | VO registry; add `wlshm` under a feature guard. |
| `audio/out/ao.c` | AO registry; add `alsa` under a feature guard. |
| `player/main.c` | Guard bundle-only `MPVBUNDLE` and `VK_DRIVER_FILES` code for macOS. |

### Upstream and prior art

- mpv: <https://github.com/mpv-player/mpv> — authoritative playback, Wayland, and ALSA behavior.
- FFmpeg: <https://github.com/FFmpeg/FFmpeg> — component selection and static build.
- libass: <https://github.com/libass/libass> — subtitle shaping and rendering.
- Wayland: <https://gitlab.freedesktop.org/wayland/wayland> — client library and protocol scanner.
- ALSA: <https://github.com/alsa-project/alsa-lib> — audio output.

### Campaign documents

`docs/status.md` (current state), `docs/architecture.md` (platform split), `docs/linux.md` (build and runtime), `docs/ffmpeg.md` (platform allowlist), `docs/licenses.md` (new components), `docs/metrics.md` (Linux rows), and `docs/manual-checklist.md` (owner checks).
