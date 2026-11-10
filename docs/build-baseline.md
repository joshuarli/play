# Build baseline

Reference revision: `1a31a38b04674cc022117138563a6f50eb1d664b`. The isolated checkout is `.work/wt/build-baseline`; it had no `.work/deps` directory when created. No existing prefix was cleared. This is an inventory, not a measured performance comparison.

## Measurement availability

On 2026-11-10 the host was macOS arm64, Xcode 27.0, Apple clang 21.0.0, SDK `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`, Rust nightly 2026-09-15, Cargo 1.100.0-nightly and kache 1.0.0. The host has 10 logical CPUs. Cargo's user configuration sets `rustc-wrapper = "kache"`. The development profile disables Cargo incremental compilation. The dist profile uses fat LTO and one codegen unit.

`perf-gate-ready` exited 75: another process was using roughly eight CPUs and load was above the gate. No time-based build measurements were run. The default native job count would be 10. Docker reports a Linux arm64 engine; x86-64 musl execution and its resource limits have not been qualified. Do not compare an emulated container to a native Linux runner.

| Workload | macOS arm64 | Linux x86-64 musl |
| --- | --- | --- |
| Clean dependency build | Deferred: host busy | Deferred: host busy; target execution unverified |
| Unchanged dependency build | Deferred: host busy | Deferred: host busy; target execution unverified |
| Dependency recipe or patch edit | Deferred: host busy | Deferred: host busy; target execution unverified |
| Rust source edit | Deferred: host busy | Deferred: host busy; target execution unverified |
| Release build | Deferred: host busy | Deferred: host busy; target execution unverified |
| CI cache restore | Logs denied with HTTP 403 | Logs denied with HTTP 403 |

Each deferred measurement needs wall time, child CPU time, exit status, steps executed, output validation, cache state and effective tools. Run `perf-gate-ready` immediately before the command and `perf-gate-ready -1` immediately after it. Discard a measurement if either fails. Preserve the isolated checkout and use the same `JOBS`, frontend, backend, cache state and host limits for both revisions. Use one controlled source edit and restore it after measurement. Capture provisioning separately from the build. Do not claim a speedup from a single cold run.

The reference entrypoints are `scripts/build-deps.sh`, `scripts/build.sh`, and `scripts/release.sh` on macOS; Linux uses `make linux` and `scripts/linux-container.sh` for individual steps. A clean dependency measurement starts with an empty prefix and source cache in the isolated checkout. A warm measurement repeats the identical entrypoint without clearing either. A native edit modifies the libplacebo patch on macOS or the libplacebo recipe on Linux; log every component rebuilt. A Rust edit changes one application source file and uses `scripts/build.sh` or `scripts/linux-container.sh scripts/build-linux.sh`. A full release uses `scripts/release.sh` or `make linux`. Record the initial cache contents, exact edit, compiler-cache statistics and command output alongside each result.

`gh run list --repo joshuarli/play --limit 3` returned a failing macOS Build and a successful Linux run at the reference revision. Their detailed logs returned HTTP 403, so the macOS failure cause and cache restoration cost are unknown. These run outcomes are not local baseline results.

## Native producer and consumer graph

The arrows below describe direct inputs evidenced by the current recipes or local build files. They are not claims about the internal dependency graph of prebuilt artifacts.

| Target and producer | Product and direct consumers | Evidence |
| --- | --- | --- |
| Both: Vulkan-Headers 1.4.363.0 archive | Vulkan and video headers plus registry. Libplacebo receives `vk.xml` via `-Dvulkan-registry` and the prefix supplies headers. | `scripts/build-deps.sh`, `scripts/build-deps-linux.sh` |
| macOS: Vulkan loader bottle | Dylibs and `vulkan.pc`. Player's output build script queries `vulkan`; libplacebo enables Vulkan. The `.pc` file is rewritten to the prefix and its headers. | `scripts/build-deps.sh`, `crates/player-output/build.rs` |
| macOS: shaderc bottle | Shaderc headers, dylibs and `.pc` files consumed by libplacebo. The prebuilt bottle's internal glslang and SPIR-V dependencies are not independently built by Player. | `scripts/build-deps.sh`, `scripts/bundle.py` |
| macOS: SPIRV-Cross archive and `native/spirv-cross` recipes | Static core, GLSL, MSL and C libraries and headers. Patched libplacebo's Metal backend queries the C package and links the other static libraries. | `native/spirv-cross/meson.build`, `scripts/patches/libplacebo-metal.patch` |
| macOS: patched libplacebo and its fast_float, glad, Jinja and MarkupSafe inputs | Shared libplacebo, headers and package metadata. Player links through pkg-config; bundling resolves its dynamic closure. | `scripts/build-deps.sh`, `crates/player-output/build.rs`, `scripts/bundle.py` |
| macOS: MoltenVK archive | Dylib and ICD JSON. The build copies the ICD beside the executable, the Cargo runner names the prefix ICD, and bundling copies and rewrites it. It is not built by Player. | `scripts/build-deps.sh`, `scripts/build.sh`, `.cargo/pinned-moltenvk.sh`, `scripts/bundle.py` |
| Linux: shaderc prebuilt archive | Static combined shaderc archive, headers and generated `.pc` files. Libplacebo consumes shaderc; `linux-link.args` also links the archive into Player. | `scripts/build-deps-linux.sh`, `crates/player-output/linux-link.args` |
| Linux: libplacebo and its fast_float, glad, Jinja and MarkupSafe inputs | Static libplacebo and headers. Player links the static archive. | `scripts/build-deps-linux.sh`, `crates/player-output/linux-link.args` |
| Linux: Alpine Vulkan loader | System shared loader and development files. `linux-link.args` links `/usr/lib/libvulkan.so`; libplacebo enables Vulkan. | `docker/linux/Dockerfile`, `crates/player-output/linux-link.args` |
| Linux: rust-fontconfig | Separate shared library, header and `.pc` prefix. `player-overlay` queries it with HarfBuzz and FreeType; installation layout includes its library. | `scripts/build-rust-fontconfig-linux.sh`, `crates/player-overlay/build.rs`, `scripts/install-layout.sh` |
| Both: FFmpeg runtime | Separately pinned shared runtime, headers, manifest and pkg-config files. Fetch, Cargo validation, bundling and installed layout consume `ffmpeg-runtime.lock.json`. | `scripts/fetch-ffmpeg-runtime.sh`, `crates/player-build/src/ffmpeg_runtime.rs` |

The Linux shaderc archive, macOS loader and shaderc bottles, and MoltenVK are independent inputs to these recipes. The `GLSLANG_REV`, `SPIRV_HEADERS_REV` and `SPIRV_TOOLS_REV` values in the scripts do not cause separate source builds. The exact upstream discovery and link edges within libplacebo need confirmation from actual Meson configuration and link output on each target. Keep uncertain edges conservative until then.

## Invalidation and reuse

The macOS script hashes its entire body and the Metal patch into one stamp value shared by every component. A patch or unrelated script edit invalidates all six component stamps. It does not remove old products first. The Linux script hashes its entire body and the shaderc archive, then clears the whole prefix on a mismatch before rebuilding the three stamped components. Both `have` functions compare stamps without checking products. `scripts/build-linux.sh` checks only the presence of the libplacebo stamp before invoking Cargo.

Both scripts use one writable prefix, so a stamp alone cannot establish ownership, completeness or integrity. MacOS `.pc` files and MoltenVK's ICD contain the absolute prefix. `player-build::release_prefix` canonicalizes the macOS prefix; changing it to a symlink can therefore change paths emitted into Cargo link arguments and rpaths. Concurrent consumers and publication need explicit proof.

The macOS CI cache stores `.work/deps/release`, `.work/deps/ffmpeg-sdk`, and `target`; Linux stores `.work/deps/linux`, `.work/deps/ffmpeg-sdk`, the Linux Cargo target, and Cargo home. Both keys reference `ffmpeg-sdk.lock.json` and `scripts/fetch-ffmpeg-sdk.sh`, neither of which is tracked at the reference revision. The current runtime is under `.work/deps/ffmpeg-runtime`. The restore-key prefix permits stale dependency caches, so restore itself does not establish validity.

## Parity and qualification map

| Contract | Existing proof or required comparison |
| --- | --- |
| Entry points, environment overrides, backend selection, effective flags and errors | Run the reference and candidate entrypoints with the same tool selection and inputs. Capture setup and compile commands and compare success and failure cases. |
| Prefix products, links, generated headers and install names | Compare component-owned paths, symlink targets, bytes and link metadata on each native host. Test missing and damaged outputs using the cache fixture. |
| Rust and native linkage | `tests/run.sh rust-link` on macOS and `scripts/linux-container.sh tests/run.sh linux-rust-link` on Linux; inspect emitted link commands and `.pc` output. |
| FFmpeg pin and local-only boundary | `scripts/audit.sh` on macOS and `scripts/linux-container.sh scripts/audit-linux.sh` on Linux, with the existing FFmpeg audit and local-only tests. |
| Release identity, signing, bundle closure and runtime | `tests/run.sh release` on macOS; `make linux` and `scripts/linux-container.sh tests/run.sh suite` on Linux. Preserve artifact checksum verification. |
| Cache corruption, invalidation, interrupted or concurrent publication | New `tests/build-cache.sh`, followed by native producer and consumer checks. Existing stamp checks do not cover these cases. |
| Reproducibility | Build twice independently under the same pinned tools and compare contents, bytes and documented nondeterminism. No result exists yet. |

The default `meson` in this shell is oxidized-meson 1.7.0. A small preflight probe using it generated a Ninja file without an install target. Homebrew Meson 1.12.1 and Homebrew Ninja 1.13.2 passed the same staged-install consumer probe. Actual Player build compatibility remains unverified. Local `scripts/dev-deps.sh` requires pinned Homebrew tools; macOS CI installs tools without checking those pins. The Alpine Dockerfile pins its base image and some packages, but not every package version.
