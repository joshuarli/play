# Release licenses (P8.1)

The release app is assembled from the pinned sources in `scripts/build-deps.sh`. Build tools (CMake, Meson, Ninja, Python, glad2, Jinja2, MarkupSafe) are not shipped. P8.4 completes the audit against the final bundle and the Rust crate graph.

| Component | Version | License |
| --- | --- | --- |
| mpv | 0.41.0 (`-Dgpl=true`) | GPL-2.0-or-later |
| FFmpeg (minimal build) | 9.0.2 | LGPL-2.1-or-later |
| libass | 0.17.5 | ISC |
| libplacebo | 7.387.0 | LGPL-2.1-or-later |
| shaderc | 2026.4 | Apache-2.0 |
| glslang | `e1b562a8` | Apache-2.0 |
| SPIRV-Tools | `ef96ed76` | Apache-2.0 |
| SPIRV-Headers | `04fd3caa` | MIT |
| Vulkan-Headers | 1.4.363.0 | Apache-2.0 |
| Vulkan-Loader | 1.4.363.0 | Apache-2.0 |
| dav1d | 1.5.4 | BSD-2-Clause |
| FreeType | 2.15.0 | FreeType License |
| FriBiDi | 1.0.17 | LGPL-2.1-or-later |
| HarfBuzz | 15.0.0 | MIT |
| MoltenVK (binary) | 1.4.2 | Apache-2.0 |
| Lua (vendored by mlua) | 5.2.4 | MIT |
| Rust crates | `Cargo.lock` | per-crate; P8.4 audit |
