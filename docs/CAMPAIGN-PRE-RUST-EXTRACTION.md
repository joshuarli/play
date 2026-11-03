# Campaign: pre-Rust mpv extraction

Goal: shrink the compiled C source set to an explicit two-platform matrix
(macOS arm64 + Linux x86-64 musl) without changing playback capabilities,
then stop. No Rust port in this campaign. Windows removal is done
(`d88dcdd feat!: remove Windows support`); this campaign does not revisit it.

## Baseline (measured)

- C/H: 214 `.c` / 164 `.h`, C-only 121,082 lines, C+H 137,352 lines
  (`find player stream demux filters sub video audio osdep input options
  misc common ta etc | xargs wc -l`, live tree). Recorded
  `docs/repo-audit.md`: 216 files / 137,228 lines; drift is post-audit
  TUI/restore work. Rust: 29 files / 8,830 lines; 75 `#[test]` in 19 files;
  workspace members `player-audio/core/ffi/lua/release/tui`;
  49 `player_ffi_*/player_audio_*` symbols (audit).
- Sizes: `dist/Player.app` 28,437,455 B file bytes (`du -sk` 27,816 KiB),
  exe `dist/Player.app/Contents/MacOS/player` 9,707,328 B.
  `docs/metrics.md` final @`96331a9`: app 27,041,616 B, closure 17,334,096 B
  in 13 files, budget 29,542,136 B (50% of `P0_UNIQUE_CLOSURE` 59,084,272,
  `scripts/measure.sh:79`); exe budget <= 10,000,000 B.
  Linux: `dist/player` 26,760,920 B; tarball
  `dist/player-x86_64-linux-musl.tar.gz` 10,638,162 B.
- Gates: `tests/harness.py:2014` `SUITE` = 20 gates (fixtures, corpus,
  audio, hwdec, seek, local-paths, local-only, osc, osc-input, input,
  playlist, cli, state, failures, rust-link, ffi, lua-host, lua-api-audit,
  tui, release). `:2023` `LINUX_SUITE` = 19 gates (fixtures, linux-corpus,
  linux-audio, linux-gpu-next, linux-local-only, linux-osc,
  linux-mostly-static, linux-hwdec-default, local-paths, seek, input,
  playlist, cli, state, failures, lua-host, lua-api-audit, tui,
  linux-rust-link). Recorded: 315 passed / 0 failed @`96331a9`
  (`docs/status.md`); Linux 307 container pass.
- Perf (recorded `docs/metrics.md` final @`96331a9`, medians of 7, quiet
  host): cold 65.47 ms, warm 63.14 ms, first-frame 1080p 150.12 ms
  (<= 1 s), idle RSS 83.92 MB, 1080p 215.83 MB / 0.18 s CPU (<= 300 MB),
  4K CPU 0.44 s, seek 23.68 ms (<= 250 ms), exit 3.06 ms, 0 underruns,
  1 protocol (`file`), 1 Lua. Headless gaps stay owner-run
  (`docs/manual-checklist.md`, all not run); frame drops are 0 by
  construction under `--vo=null`, not a renderer measurement.
- Build surface: `meson.build` 1485 lines, `meson.options` 101 lines;
  generic sources `meson.build:66-258` (162 files). Matrix is enforced in
  scripts, not meson: `scripts/build.sh:28` enables cocoa coreaudio vulkan
  videotoolbox-pl swift-build iconv zlib; `scripts/build-linux.sh:37`
  enables alsa iconv vaapi vaapi-drm vaapi-wayland vulkan wayland zlib.
  Zero `win32|wasapi|direct3d|d3d11|mingw|uwp` in `meson.build` +
  `meson.options`; no `ao_wasapi`/`ao_dsound`/`vo_direct3d`/`w32` sources;
  `osdep/` holds only darwin/linux/posix/mac/unix/generic variants.
- Registries: 80 top-level C commands (`player/command.c:7128`,
  `run :7433`, `subprocess :7437` retained); Rust validator
  `rust/player-core/src/command.rs:89-150` allows only a narrow subset,
  enforced via `input/cmd.c:455,523`. `af_list` /
  `vf_list` (`filters/user_filters.c:22-52`): scaletempo, scaletempo2,
  format, drop / format, sub, fingerprint, gpu. `af_lavfi`/`vf_lavfi`
  (`filters/f_lavfi.c:1138-1227`) are defined but unlisted and unreachable
  via `--af`/`--vf`.

## Deliverables

- D1 encoder+recorder group delete: `common/encode_lavc.c`,
  `audio/out/ao_lavc.c`, `video/out/vo_lavc.c`, `common/recorder.c` +
  headers, `meson.build:83,92,95,236` rows, `vo.c:52,59` and `ao.c:41,55`
  entries; status readers in `player/playloop.c`,
  `player/loadfile.c`, `player/osd.c`, `player/video.c` left dead-code-free.
  Basis: `encode_config.opts` (`common/encode_lavc.c:76-84`) holds only
  `{0}`, so `encode_opts.file` (`common/encode.h:33`) is never settable and
  `player/main.c:588-589` never fires (`docs/scope.md:47`); release FFmpeg
  has no encoders/muxers (`scripts/ffmpeg-components.txt`,
  `docs/ffmpeg.md:60` gate); `ao.c:159` / `vo.c:249` `encode!=ctx` gates
  never pass; `recorder.h` has no includer but `common/recorder.c:31`.
  Proof: `rg -l encoder_context_alloc|encode_lavc|ao_lavc|vo_lavc|recorder.h`
  zero hits AND `scripts/build.sh && tests/run.sh suite` 0 fail AND
  `scripts/audit.sh` ok AND `scripts/measure.sh final` closure <= 29542136,
  exe <= 10000000.
- D2 `f_lavfi` delete: `filters/f_lavfi.c` + `meson.build:122` only; the
  `libavfilter` dependency STAYS (`common/av_log.c:47-48,202` includes its
  headers for version logging). Basis: `af_lavfi`/`vf_lavfi` unlisted
  (`filters/user_filters.c:22-55`), no `&af_lavfi` users,
  `player/loadfile.c:1364-1371` early-returns (`--lavfi-complex`
  unregistered; zero `lavfi` rows in `options/options.c`),
  `player/command.c:3864` is completion-only.
  Proof: `rg -l f_lavfi|lavfi_complex|UPDATE_LAVFI` shows only the retained
  stubs (`options/options.h:282`, `options/m_option.h:441`) AND
  suite/audit/measure at D1 thresholds.
- D3 render-API removal: `video/out/vo_libmpv.c` (757),
  `video/out/libmpv_sw.c` (208), `video/out/gpu/libmpv_gpu.c` (250)/`.h`,
  `video/out/opengl/libmpv_gl.c`, `include/mpv/render.h` (760) +
  `render_gl.h` (210), `vo.c:50,57` entries + `:152` `opengl-cb` alias,
  `player/client.c:83` field + `:195,198,2156,2163-2164,2180-2183` trio,
  `player/client.h:47-52` decls, `player/scripting.c:357-364` render
  `INIT_SYM`s, `vo_libmpv.c:114-118` `render_backends[]` + `:130` helper,
  `video/out/libmpv.h:25-83` backend types. KEEP `include/mpv/client.h`,
  the client trio, scripting client `INIT_SYM`s, and `stream_cb`.
  Basis: sole-consumer chain verified; Rust is client-API-only
  (`rg mpv_render rust/` zero hits); mac consumer
  `osdep/mac/libmpv_helper.swift:50-161` goes with it.
  Proof: `rg -l mpv_render|vo_libmpv|libmpv_gpu|render_backend` zero hits
  AND `rg mpv_render rust/` zero hits AND suite/audit/measure thresholds.
- D4 `vo_gpu_next` options-header split: move `gl_video_opts` /
  `scaler_config` / `gl_video_conf` out of `video/out/gpu/video.h`
  (`:39-45,:133,:183`) into an options-owned header; `vo_gpu_next.c`
  drops `gpu/video.h:47` + `gpu/video_shaders.h:48` includes and keeps
  `gpu/context.h`, `gpu/hwdec.h`, `gpu_next/context.h`.
  Basis: `vo_gpu_next.c:120` is placebo (`pl_renderer`), zero
  `gl_video_*()` calls (type refs only), user-shader path `:2242-2253`
  is placebo `pl_mpv_user_shader_parse`; `video_shaders.h`
  `sampler_prelude`/`pass_*` unused by it.
  Proof: `rg video_shaders.h video/out/vo_gpu_next.c` zero AND mac
  `scripts/build.sh` + `scripts/linux-container.sh tests/run.sh suite`
  both 0 fail AND measure thresholds.

Out of scope for this campaign (owner decision, conservative):
ad_spdif removal with `f_decoder_wrapper.c` + `player/audio.c` follow-ups;
`vf_gpu` + `video/out/gpu/video.c` family removal; build-surface deletion
beyond flipping dead `option()` defaults to disabled.

## Slices

- Slice 1 (no decisions): D1+D2. Acceptance: `scripts/build.sh &&
  tests/run.sh suite` 0 fail, `scripts/audit.sh` ok,
  `scripts/measure.sh final` closure <= 29542136 and exe <= 10000000,
  `cargo test --manifest-path rust/Cargo.toml` pass. `ad_lavc` is kept:
  it is live in `filters/f_decoder_wrapper.c:392-397,418,464`, contributes
  zero decoders under the allowlist, and audio is served by `ad_symphonia`
  first.
- Slice 2 (one pre-check): confirm `video/out/opengl/` presence in the
  release tree (absent in this checkout vs `meson.build:972`), then D3+D4.
  Acceptance: Slice 1 gates on mac plus
  `scripts/linux-container.sh tests/run.sh suite` (19-gate LINUX_SUITE),
  `rg mpv_render rust/` zero, `rg video_shaders.h vo_gpu_next.c` zero.
- Ordering: unconditional dead code (D1/D2) before API removal (D3) before
  header split (D4). Each slice is independently shippable and gated.

## Execution authority

- Agent-owned (this checkout, macOS): slice edits, `rg`/`wc`/`fd`/`sed`
  reads, `scripts/build.sh`, `tests/run.sh suite|<gate>`,
  `scripts/audit.sh`, `scripts/measure.sh final`,
  `cargo test --manifest-path rust/Cargo.toml`.
- Needs environment: Linux gates via the pinned Alpine image (`make linux`,
  `scripts/linux-container.sh tests/run.sh suite|audit-linux.sh`); perf
  medians need a quiet host (`perf-gate-ready` protocol, deferred when the
  host is busy); `scripts/release.sh` / `make install` only when asked (no
  notarize; `v*` tag flow is CI).
- Owner-only: `docs/manual-checklist.md` visual/audio/HDR/Retina/mouse/
  signing checks; any publish/push.

## Decisions

- Owner made (this round): conservative scope (D1-D4 only); minimal
  build-surface hygiene (flip dead `option()` defaults to disabled, keep
  the mpv v0.41.0 `meson.build` pin per `docs/architecture.md:93`, keep
  the `av_log.c` libavfilter include, keep `osdep/terminal.h:67-73` +
  `terminal-dummy.c:31,41` stubs, keep `lavfi_complex` /
  `UPDATE_LAVFI_COMPLEX` stubs).
- Prescribed (overridable): keep `ad_lavc`; keep `libavfilter` dep; keep
  the client API / `stream_cb`; keep `vf_gpu` + `video.c` family until a
  later campaign scopes `--vf=gpu` removal.

## Not verified

- Recorded numbers (315/303/169 pass counts, app/closure/exe budgets, perf
  medians, 1 protocol / 1 Lua, Linux 307/295, P0 row, manual-checklist
  items) are single-source from docs and unremeasured this session; live
  tree is dirty (`rust/player-tui` + `tests/harness.py` vs `96331a9`).
- `video/out/opengl/` body (absent in checkout, referenced at
  `meson.build:972`); `tests/` render-API absence audit is partial;
  `options.c:803` post-split reader set assumed `vo_gpu_next` + `vf_gpu`;
  `Cargo.lock` `windows-sys`/`windows-link` release-closure absence
  unconfirmed by `nm` (never compiled on unix is standard).
- `campaign-preflight` is disabled in this environment, so no verdict step
  ran; treat this doc as a draft awaiting that gate rather than a
  preflighted plan.

## First slice prompt

Execute Slice 1 of `docs/CAMPAIGN-PRE-RUST-EXTRACTION.md` (D1+D2 only):
delete the encoder/recorder group and `f_lavfi` with the listed
`meson.build`/registry follow-ups, then prove with the D1 negative `rg`,
`scripts/build.sh && tests/run.sh suite`, `scripts/audit.sh`,
`scripts/measure.sh final`, and `cargo test --manifest-path rust/Cargo.toml`.
