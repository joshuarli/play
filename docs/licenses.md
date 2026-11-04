# Release licenses

The macOS release app is assembled from the pinned sources in `scripts/build-deps.sh`. Build tools (CMake, Meson, Ninja, Python, glad2, Jinja2, MarkupSafe) are not shipped.

| Component | Version | License |
| --- | --- | --- |
| Player Rust code (this repository) | this revision | MPL-2.0 |
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
| mpv Lua client scripts (`defaults`, `assdraw`, `options`, `input`, `osc`) | 0.41.0 | GPL-2.0-or-later |
| Symphonia (vendored) | `b6336209` | MPL-2.0 |
| Rust crates | `Cargo.lock` | MIT/Apache-2.0/MPL-2.0/Zlib/Unicode-3.0 |

The Linux build adds these components, linked statically by `scripts/build-deps-linux.sh`; Vulkan-Headers, shaderc, glslang, SPIRV-Tools, SPIRV-Headers, libplacebo, libass, dav1d, FreeType, FriBiDi, and HarfBuzz keep the versions above.

| Component | Version | License |
| --- | --- | --- |
| zlib | 1.3.2 | Zlib |
| expat | 2.9.0 | MIT |
| fontconfig | 2.18.1 | HPND-sell-variant |

The musl runtime libraries come from the Alpine image and are not shipped: musl (MIT), libgcc (GPL-3.0-or-later WITH GCC-exception-3.1), vulkan-loader (Apache-2.0), wayland-libs-client (MIT), alsa-lib (LGPL-2.1-or-later), and libva (MIT).

`cargo audit` reports no RustSec vulnerabilities for the locked crates. The release audit checked `cargo metadata` license fields. The set is MIT OR Apache-2.0, MPL-2.0, MIT, Apache-2.0 OR MIT, Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT, Apache-2.0/MIT, Zlib OR Apache-2.0 OR MIT, Unlicense OR MIT, and (MIT OR Apache-2.0) AND Unicode-3.0. Symphonia's MPL-2.0 notice is preserved under `vendor/symphonia`; FFmpeg, libass, and libplacebo notices and source obligations remain with the repository and pinned sources. The bundled Lua scripts retain their mpv provenance and GPL-2.0-or-later headers.
