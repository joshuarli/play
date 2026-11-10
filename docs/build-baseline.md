# Build baseline

Reference revision: `1a31a38b04674cc022117138563a6f50eb1d664b`. The isolated checkout is `.work/wt/build-baseline`; it had no `.work/deps` directory when created. No existing prefix was cleared. This is an inventory, not a measured performance comparison. An untimed clean macOS reference run failed while copying the Vulkan loader because the prefix had no `lib` directory. The focused fixture `python3 -m unittest discover -s tests -p test_build_deps.py` reproduces the failure; `ead8703` fixes it. The original revision remains the performance reference, with this failure reported rather than omitted.

## Measurement availability

On 2026-11-10 the host was macOS arm64, Xcode 27.0, Apple clang 21.0.0, SDK `/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk`, Rust nightly 2026-09-15, Cargo 1.100.0-nightly and kache 1.0.0. The host has 10 logical CPUs. Cargo's user configuration sets `rustc-wrapper = "kache"`. The development profile disables Cargo incremental compilation. The dist profile uses fat LTO and one codegen unit.

Initially `perf-gate-ready` exited 75: another process was using roughly eight CPUs and load was above the gate. The owner subsequently waived performance gates, and the macOS observations below were collected under contention. The default native job count would be 10; these measurements used a two-job backend. Docker reports a Linux arm64 engine, so x86-64 runs are emulated. Do not compare an emulated container to a native Linux runner.

An untimed x86-64 musl container check on the arm64 host passed all eight `player-build` library tests with kache 1.0.0 and mold 2.39.1. The unmodified container image supplies neither tool: the kache release archive was checked against GitHub's published SHA-256 and mold and its libraries were fetched as Alpine packages into `.work/tmp/linux-host-tools`. The test executable's ELF `.comment` identifies mold. The added linker flag and libraries were confined to this test and are not a production build configuration. Reproduce after provisioning those tools with:

```sh
scripts/linux-container.sh env \
  PATH=/work/.work/tmp/linux-host-tools/bin:/opt/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin \
  LD_LIBRARY_PATH=/work/.work/tmp/linux-host-tools/root/usr/lib \
  RUSTC_WRAPPER=/work/.work/tmp/linux-host-tools/kache \
  KACHE_CACHE_DIR=/work/.work/tmp/linux-host-tools/cache \
  KACHE_RUNTIME_DIR=/work/.work/tmp/linux-host-tools/runtime \
  KACHE_LOCAL_ONLY=1 KACHE_SCHEDULER=0 KACHE_AUTO_GC=0 \
  KACHE_DEFERRED_DURABILITY=0 KACHE_DAEMON_PUBLISH=0 \
  CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/work/.work/tmp/linux-host-tools/target \
  TMPDIR=/work/.work/tmp RUSTFLAGS='-C link-arg=-B/work/.work/tmp/linux-host-tools/bin' \
  cargo test --locked -p player-build --lib --target x86_64-unknown-linux-musl
```

A subsequent `kache stats` command unexpectedly started a daemon inside its disposable container; the container exited and the daemon stopped. Do not use that command for this campaign. This unit test does not validate the Linux player, its native dependency closure, or release acceptance.

| Workload | macOS arm64 | Linux x86-64 musl |
| --- | --- | --- |
| Clean dependency build | 50.49 s, one run with warm archives and the clean-prefix fix | 49.11 s inside the emulated container, one run |
| Unchanged dependency build | 0.19 s median, five runs with the clean-prefix fix | 0.14 s median inside the emulated container, five runs |
| Dependency recipe or patch edit | Reference failed on read-only bottle files; 52.99 s after the reinstall fix | 48.58 s inside the emulated container, one run |
| Rust source edit | 5.58 s on accepted code, one run | Unmeasured |
| Release build | 97.66 s on accepted code, one run with warm archives | Emulated local release passed; timing unmeasured |
| CI cache restore | Logs denied with HTTP 403 | Logs denied with HTTP 403; local restore simulation recorded |

Each measurement needs wall time, child CPU time, exit status, steps executed, output validation, cache state, effective tools and host load. Preserve the isolated checkout and use the same `JOBS`, frontend, backend, cache state and host limits for both revisions. Use one controlled source edit and restore it after measurement. Capture provisioning separately from the build. Do not claim a speedup from a single cold run. The original revision's clean macOS dependency build failed; the runnable reference includes the clean-prefix repair, and the patch-edit retry also includes the bottle-reinstall repair. The accepted-code Rust and release observations are not original-revision baselines. Details and limitations are in [the benchmark report](build-benchmarks.md).

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

The Linux shaderc archive, macOS loader and shaderc bottles, and MoltenVK are independent inputs to these recipes. The `GLSLANG_REV`, `SPIRV_HEADERS_REV` and `SPIRV_TOOLS_REV` values in the scripts do not cause separate source builds. An untimed macOS build at `ead8703` completed with Homebrew Meson and Ninja limited to two jobs. Its Meson log confirms libplacebo discovers shaderc, Vulkan and SPIRV-Cross through pkg-config. `otool -L` confirms dynamic shaderc and Vulkan references. Its libplacebo pkg-config file also embeds absolute SPIRV-Cross archive paths. An untimed x86-64 musl container build also completed under emulation. Its Meson log confirms the combined shaderc archive and the system Vulkan loader, version 1.4.360, as libplacebo inputs. Native Linux hardware and the remaining link closure still need qualification. Keep uncertain edges conservative until then.

## Invalidation and reuse

The macOS script hashes its entire body and the Metal patch into one stamp value shared by every component. A patch or unrelated script edit invalidates all six component stamps. It does not remove old products first. The Linux script hashes its entire body and the shaderc archive, then clears the whole prefix on a mismatch before rebuilding the three stamped components. Both `have` functions compare stamps without checking products. `scripts/build-linux.sh` checks only the presence of the libplacebo stamp before invoking Cargo.

Both scripts use one writable prefix, so a stamp alone cannot establish ownership, completeness or integrity. An untimed `scripts/build.sh` on the fixed macOS checkout built the player. `tests/run.sh rust-link` failed because the media fixture was absent and a cached backend test executable contained rpaths from `.work/wt/kr`, another checkout. A fresh unsalted `CARGO_TARGET_DIR` reproduced the foreign rpaths. A different fresh target with `KACHE_KEY_SALT=player-build-proof-ead8703` produced the current checkout rpaths; five backend tests then failed because the media fixture was absent. After creating the focused media fixtures, the salted backend unit tests and `tests/run.sh rust-link` passed. The same gate later passed without the salt after the rpath input fix. This comparison establishes a cache isolation risk, not a speed result or a general proof about every kache key. MacOS `.pc` files and MoltenVK's ICD contain the absolute prefix. `player-build::release_prefix` canonicalizes the macOS prefix; changing it to a symlink can therefore change paths emitted into Cargo link arguments and rpaths. Concurrent consumers and publication need explicit proof.

The generation-switch rpath test also has a timestamp limit. In a controlled variant of `test_cargo_relinks_after_native_prefix_changes`, both generations' stamp mtimes were set to 2000 before the first build. After an atomic switch to the second generation, Cargo reported the build fresh and the executable still named the first generation's library path. The existing test creates the second stamp after the first build and passes. A restored generation can have older timestamps, so the stamp watches in `emit_macos_prefix_rpath` do not prove safe generation switching; C1 must address this case before using that layout. Watching the shared `.work/deps` directory made the same switch relink, but adding an unrelated directory there also caused a relink. That workaround does not provide narrow invalidation or per-invocation integrity validation.

The FFmpeg runtime fetcher has a separate reuse gap: it accepts an installed prefix when its manifest matches the pin, without checking the other installed files. Appending bytes to a header in a disposable copy of the pinned macOS runtime and rerunning `scripts/fetch-ffmpeg-runtime.sh aarch64-apple-darwin` reported an existing runtime and left the damaged header in place. The current CI cache does not restore this runtime prefix, but local reuse can still accept corruption. The original archive is checksum-verified when fetched; that check does not verify an already-installed prefix.

A separate Cargo fixture tested the pinned nightly's `-Z checksum-freshness` with `CARGO_BUILD_FINGERPRINT=content`, kache, an `include_bytes!` input through a symlink, and a build script watching the containing directory. Cargo detected a same-length change to the original target with its mtime restored. It then reported the crate fresh after the symlink was redirected and the new target's contents were changed, with the affected mtimes restored. In this fixture, Cargo's recorded dependency resolved to the original target. Content freshness therefore cannot by itself validate the native prefix's symlink identities or replace the required reader check.

A later fixture explored Cargo's compiler probes as a possible reader check. With `build.build-dir` configured and its `.rustc_info.json` path occupied by a directory, a fresh `cargo build` invoked `rustc-workspace-wrapper` three times for compiler information without compiling the crate. Setting `CARGO_BUILD_BUILD_DIR` to a different warmed directory made the next fresh build invoke the wrapper zero times. A target-directory override also bypassed the probe when the build directory was not separately configured. These experiments depend on Cargo's cache internals and do not provide a reliable validation hook for direct Cargo invocations.

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
