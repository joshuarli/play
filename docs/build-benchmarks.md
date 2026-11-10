# Build experiment results

## Compiler cache

The macOS host uses kache 1.0.0 through Cargo's `rustc-wrapper`. The cross-checkout rpath regression and its fix are recorded in [the baseline](build-baseline.md). The host-wide `kache stats` output combines other projects and is not a Player benchmark.

The [sccache architecture](https://github.com/mozilla/sccache/blob/main/docs/Architecture.md) uses a local server for both its server-side and client-side modes. The campaign forbids a new service, so the sccache pilot was not run or added. There are no sccache hit, miss, overhead, or timing results. This is a constraint-based omission, not evidence that sccache is slower.

## Development shell

The optional flake locks nixpkgs at the revision in `flake.lock`. Muon is 0.7.0, Meson 1.12.1, Samurai 1.3, Ninja 1.13.2, and pkgconf 3.0.10. The pkgconf source has the existing project's version and an ISC license. The Linux x86-64 pkgconf executable was 58,456 bytes and its Nix wrapper was 860 bytes in the test environment. These tools are development inputs and add no runtime dependency to the player.

Nix 2.35.4 evaluated both shell definitions with `nix flake check --no-build --all-systems`. The x86-64 Linux shell ran under emulation in a disposable Linux container. Its `pkg-config` resolved a controlled `.pc` file, and `make -n linux` expanded identically inside and outside the shell. The shell did not expose a C compiler in that container. The Linux player build remains in the existing Alpine container.

The macOS shell was evaluated but not entered on macOS because Nix is not installed there. Its use of the selected Xcode compiler and SDK still needs a native check. Nix's first shell setup fetched its closure and built pkgconf; provisioning time and disk use have not been measured under the performance gate. No Player artifact size, memory, startup, or build-time comparison has been attributed to the shell.

## Before and after

No comparative timings are available. `perf-gate-ready` reported the host busy during baseline work. The clean, warm, Rust edit, native edit, CI restore, and full-release workloads in [the baseline](build-baseline.md) remain to be measured with equivalent hosts, cache states, and limits. The Linux dependency build ran under x86-64 emulation as a correctness check; it is not comparable to native Linux CI timing.
