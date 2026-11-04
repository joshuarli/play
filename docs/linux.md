# Linux

Player builds for Linux x86-64 on musl. The playback engine and application layer are the same Rust code as macOS; the platform split is Wayland for the window and input, Vulkan for rendering, and ALSA for audio. The executable is mostly static. It needs a small shared-library set because a fully static binary cannot load a Vulkan ICD, and a `wlshm` fallback was rejected because it has no GPU, HDR, or hardware decode. The build host is the Alpine edge image in `docker/linux/Dockerfile`, pinned by digest. `scripts/linux-container.sh COMMAND...` runs a command in that image as the calling user with the repository mounted at `/work`.

## Build

```sh
make linux
```

runs `scripts/build-deps-linux.sh` and `scripts/release-linux.sh` in the container. Individual steps: `scripts/linux-container.sh scripts/build-linux.sh` builds the Cargo `dist` profile for `x86_64-unknown-linux-musl`, writes the unstripped binary to `build-linux/mpv`, and writes a stripped `dist/player` (about 24 MB); `scripts/linux-container.sh scripts/audit-linux.sh` checks it. `scripts/linux-container.sh` runs with `seccomp=unconfined` and `apparmor=unconfined` so the local-only gate can create a user and network namespace with `unshare -rn`.

The build sets `-C target-feature=-crt-static`. A static-pie executable with Wayland in `NEEDED` dies in the musl loader before `main`; the production binary therefore uses the dynamic musl loader and a narrow shared-library set.

## Dependencies

`scripts/build-deps-linux.sh` builds these as static libraries from pinned, checksum-verified sources into `.work/deps/linux`: Vulkan-Headers 1.4.363.0, shaderc 2026.4 (with glslang `e1b562a8`, SPIRV-Tools `ef96ed76`, SPIRV-Headers `04fd3caa`), and libplacebo 7.387.0. The static FFmpeg SDK (FFmpeg 9.0.2 with dav1d and zlib) comes from `scripts/fetch-ffmpeg-sdk.sh x86_64-unknown-linux-musl` into `.work/deps/ffmpeg-sdk/x86_64-unknown-linux-musl`; see `docs/ffmpeg.md`. Neither prefix holds a shared library; text shaping uses the system Fontconfig, HarfBuzz, and FreeType, which `rust/player-overlay/build.rs` links through pkg-config.

The executable links only these shared libraries from the Alpine image:

| Library | Alpine package |
| --- | --- |
| musl libc | `musl` |
| `libvulkan.so.1` | `vulkan-loader` |
| `libwayland-client.so.0` | `wayland-libs-client` |
| `libasound.so.2` | `alsa-lib` |
| `libva.so.2`, `libva-drm.so.2` | `libva` |
| `libfontconfig.so.1` | `fontconfig` |
| `libharfbuzz.so.0` | `harfbuzz` |
| `libfreetype.so.6` | `freetype` |

`scripts/audit-linux.sh` rejects any other `NEEDED` entry and any GNU runtime import (`@GCC_*` versions, `_Unwind_*`, `__popcountdi2`, frame registration). Rust std asks for `libgcc_s` on a dynamic musl target; `docker/linux/musl-cc-wrap.sh` links GCC's static `libgcc_eh.a` in its place, beside the static `libgcc.a` and `libstdc++.a`. The audit also rejects any libass, FriBiDi, Pango, Cairo, or Pixman in `NEEDED`, in the binary's symbols, or in the dependency prefix. Fontconfig reads `/etc/fonts` at run time, so a system needs a fontconfig configuration and fonts for system-font discovery.

## Hardware decode

`player-media` carries VideoToolbox and VA-API decode paths and a software fallback. The scheduler currently opens video decoders in software, so the automated corpus and GUI gates do not depend on a GPU decoder. `--hwdec` values are accepted for compatibility, and `tests/run.sh linux-hwdec-default` checks the default property and the headless software path in the container. A real-GPU VA-API run needs a render node, a VA driver, and a compositor; it is an owner check, not part of `suite`.

The VA driver comes from the host, the same way the Vulkan ICD does: `mesa-va-gallium` for AMD and Nouveau, `intel-media-driver` for current Intel GPUs, or `libva-intel-driver` for older ones. The two libva libraries are shared because the drivers are dlopened and link libva themselves.

- `tests/run.sh linux-vaapi` decodes the h264, hevc, vp9, and av1 fixtures with `vaapi-copy` and asserts `hwdec-current`. It needs a GPU with a render node and fails without one, so it is not part of `suite`. `scripts/linux-container.sh` passes every `/dev/dri/renderD*` into the container.
- The zero-copy path, `VK_EXT_image_drm_format_modifier` import, 10-bit output, and a different GPU for decode and display are owner checks.

## Headless GPU gate

`tests/run.sh linux-gpu-next` starts `weston --backend=headless` on a private socket under `.work/tmp`, selects Mesa lavapipe through `VK_DRIVER_FILES`, and plays the 1080p measurement fixture through `--vo=gpu-next --gpu-context=waylandvk`. lavapipe is a software device, so the gate passes `--gpu-sw=yes`; the shipped player has no such default. It also passes `--vulkan-swap-mode=mailbox`: with the default FIFO mode this headless compositor never releases swapchain buffers and `vkAcquireNextImageKHR` blocks after a few frames. FIFO pacing, fractional scale, and `wp_presentation` timing are not covered by the headless gate and stay with the owner checks on a real compositor.

## ALSA

`tests/run.sh linux-audio` plays the mono, stereo, and 5.1 audio fixtures with `--ao=alsa --audio-device=alsa/null --audio-channels=auto`, then checks pause, seek, and device selection on the stereo fixture. ALSA's `null` PCM is built into `libasound`, so the gate needs no sound card and loads no plugin. The engine mixes to the requested layout and opens the named ALSA device. The gate starts paused and keeps the file open so `audio-params` stays readable. Audibility, channel placement, and underruns on real hardware are owner checks, and the release must not rely on the `default` PCM, which may route through a shared plugin.

## Fixtures

`tests/fixtures/gen.sh` generates the corpus with the image's FFmpeg 9.0.2; `tests/fixtures/manifest.json` lists the files. Stream structure matches the manifest exactly. The external encoders choose threads and SIMD paths from the host at run time, so their bytes differ from the macOS Homebrew ones and between Linux hosts; the gate compares structure only on Linux. Byte hashes are checked on macOS against the tracked manifest. After an intended structural change, review the drift and run `tests/fixtures/gen.sh --write-manifest`.
