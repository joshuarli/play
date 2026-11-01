# Linux

Player builds for Linux x86-64 on musl with `vo=gpu-next` over Vulkan and Wayland, ALSA audio, and a mostly static executable. The build host is the Alpine edge image in `docker/linux/Dockerfile`, pinned by digest. `scripts/linux-container.sh COMMAND...` runs a command in that image as the calling user with the repository mounted at `/work`.

## Build

```sh
make linux
```

runs `scripts/build-deps-linux.sh` and `scripts/release-linux.sh` in the container. Individual steps: `scripts/linux-container.sh scripts/build-linux.sh` writes `build-linux/mpv` and a stripped `dist/player` (about 26 MB), and `scripts/linux-container.sh scripts/audit-linux.sh` checks it. `scripts/linux-container.sh` runs with `seccomp=unconfined` so the local-only gate can create a user and network namespace with `unshare -rn`.

## Dependencies

`scripts/build-deps-linux.sh` builds these as static libraries from pinned, checksum-verified sources into `.work/deps/linux`: Vulkan-Headers 1.4.363.0, zlib 1.3.2, expat 2.9.0, dav1d 1.5.4, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, fontconfig 2.18.1, shaderc 2026.4 (with glslang `e1b562a8`, SPIRV-Tools `ef96ed76`, SPIRV-Headers `04fd3caa`), libplacebo 7.387.0, and libass 0.17.5. The prefix holds no shared library.

The executable may link only these shared libraries from the Alpine image:

| Library | Alpine package |
| --- | --- |
| musl libc | `musl` 1.2.6-r6 |
| `libvulkan.so.1` | `vulkan-loader` 1.4.360-r0 |
| `libwayland-client.so.0`, `libwayland-cursor.so.0` | `wayland-libs-client`, `wayland-libs-cursor` 1.26.0-r0 |
| `libxkbcommon.so.0` | `libxkbcommon` 1.13.2-r0 |
| `libasound.so.2` | `alsa-lib` 1.2.16.1-r0 |
| `libva.so.2`, `libva-drm.so.2`, `libva-wayland.so.2` | `libva` 2.24.1-r0 |

Fontconfig reads `/etc/fonts` at run time, so a system needs a fontconfig configuration and fonts for system-font discovery.

## Hardware decode

`--hwdec=vaapi` decodes H.264, HEVC, VP9, and AV1 through VA-API and imports the frame into Vulkan as a dmabuf. `--hwdec=vaapi-copy` decodes on the GPU and copies the frame back, so it also works with `--vo=null`. On Linux the default is `--hwdec=auto`: mpv tries the whitelisted methods in order and takes the first that works, so with a Wayland display and a Vulkan device that imports dmabufs it uses `vaapi`, on `--vo=null` it uses `vaapi-copy` when a render node exists, and otherwise it decodes in software. The default comes from `HAVE_VAAPI` in `video/decode/vd_lavc.c`; macOS keeps `--hwdec=no`. `tests/run.sh linux-hwdec-default` pins this and is part of `suite`; it expects `vaapi-copy` when `/dev/dri/renderD*` exists and software otherwise.

The VA driver comes from the host, the same way the Vulkan ICD does: `mesa-va-gallium` for AMD and Nouveau, `intel-media-driver` for current Intel GPUs, or `libva-intel-driver` for older ones. The three libva libraries are shared because the drivers are dlopened and link libva themselves.

- `vaapi` takes its VA display from the Wayland connection, so it needs a real compositor with `linux-dmabuf` and a Vulkan device that can import dmabufs. It does not work with `--vo=null` or on lavapipe.
- When an explicit `--hwdec=vaapi` cannot load, mpv logs the reason and `Loading failed`, then decodes in software; `hwdec-current` is `no`. `tests/wayland.py` covers this on lavapipe.
- `tests/run.sh linux-vaapi` decodes the h264, hevc, vp9, and av1 fixtures with `vaapi-copy` and asserts `hwdec-current`. It needs a GPU with a render node and fails without one, so it is not part of `suite`. `scripts/linux-container.sh` passes every `/dev/dri/renderD*` into the container. `tests/run.sh linux-vaapi` ran on an AMD Raphael iGPU with `mesa-va-gallium` 26.2.6.
- The zero-copy path, `VK_EXT_image_drm_format_modifier` import, 10-bit output, and a different GPU for decode and display are owner checks.

## Headless GPU gate

`tests/run.sh linux-gpu-next` starts `weston --backend=headless` on a private socket under `.work/tmp`, selects Mesa lavapipe through `VK_DRIVER_FILES`, and plays the 1080p measurement fixture through `--vo=gpu-next --gpu-context=waylandvk`. lavapipe is a software device, so the gate passes `--gpu-sw=yes`; the shipped player has no such default. It also passes `--vulkan-swap-mode=mailbox`: with the default FIFO mode this headless compositor never releases swapchain buffers and `vkAcquireNextImageKHR` blocks after a few frames. FIFO pacing, fractional scale, and `wp_presentation` timing are not covered by the headless gate and stay with the owner checks on a real compositor.

## ALSA

`tests/run.sh linux-audio` plays the mono, stereo, and 5.1 audio fixtures with `--ao=alsa --audio-device=alsa/null --audio-channels=auto`, then checks pause, seek, and device selection on the stereo fixture. ALSA's `null` PCM is built into `libasound`, so the gate needs no sound card and loads no plugin. Without `--audio-channels=auto`, mpv's default `auto-safe` downmixes 5.1 to stereo on this device. The null PCM is not clocked, so a fixture finishes faster than real time; the gate starts paused and keeps the file open so `audio-params` stays readable. mpv logs `audio end or underrun` at end of stream, and the gate ignores that line. Audibility, channel placement, and underruns on real hardware are owner checks, and the release must not rely on the `default` PCM, which may route through a shared plugin.

## Fixtures

`tests/fixtures/gen.sh` generates the 27-file corpus with the image's FFmpeg 9.0.2 (Alpine `ffmpeg`, fixed by the image digest). Stream structure matches `tests/fixtures/manifest.json` exactly, but the encoder builds differ from the macOS Homebrew ones, so the bytes differ for 21 fixtures. `tests/fixtures/hashes-linux.json` records the Linux hashes; the gate compares structure against the tracked manifest and bytes against that file. After an intended encoder change, review the drift and run `tests/fixtures/gen.sh --write-linux-hashes`.
