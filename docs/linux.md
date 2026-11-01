# Linux

Player builds for Linux x86-64 on musl with `vo=gpu-next` over Vulkan and Wayland, ALSA audio, and a mostly static executable. The build host is the Alpine edge image in `docker/linux/Dockerfile`, pinned by digest. `scripts/linux-container.sh COMMAND...` runs a command in that image as the calling user with the repository mounted at `/work`.

## Dependencies

`scripts/build-deps-linux.sh` builds these as static libraries from pinned, checksum-verified sources into `.work/deps/linux`: Vulkan-Headers 1.4.363.0, zlib 1.3.2, expat 2.9.0, dav1d 1.5.4, FreeType 2.15.0, FriBiDi 1.0.17, HarfBuzz 15.0.0, fontconfig 2.16.0, shaderc 2026.4 (with glslang `e1b562a8`, SPIRV-Tools `ef96ed76`, SPIRV-Headers `04fd3caa`), libplacebo 7.387.0, and libass 0.17.5. The prefix holds no shared library.

The executable may link only these shared libraries from the Alpine image:

| Library | Alpine package |
| --- | --- |
| musl libc | `musl` 1.2.6-r6 |
| `libvulkan.so.1` | `vulkan-loader` 1.4.360-r0 |
| `libwayland-client.so.0`, `libwayland-cursor.so.0` | `wayland-libs-client`, `wayland-libs-cursor` 1.26.0-r0 |
| `libxkbcommon.so.0` | `libxkbcommon` 1.13.2-r0 |
| `libasound.so.2` | `alsa-lib` 1.2.16.1-r0 |

Fontconfig reads `/etc/fonts` at run time, so a system needs a fontconfig configuration and fonts for system-font discovery.
