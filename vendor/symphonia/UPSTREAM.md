# Upstream pin

This directory is the owner's Symphonia fork, https://github.com/joshuarli/symphonia, at commit `b6336209bb40d57e4ed71079cf7f7fc6c4626aae` (decision D15, vendored in `e07d8f55`, 2026-10-31; the fork's history was not kept). The workspace version is 0.6.1 and the fork derives from https://github.com/pdeljanov/Symphonia (crates.io `symphonia` 0.6.1 was published from `ee35874b571a35a9a6e15d3bc9a3aaf8f11fbeee`). MPL-2.0; the license files are kept in each crate.

The revision is confirmed against the GitHub API (`repos/joshuarli/symphonia/commits/b6336209...`). The fork's differences from pdeljanov/Symphonia were not diffed here.

Modified in this repository after vendoring, so not pristine:

- `symphonia-codec-aac`: multichannel AAC-LC in speaker order (`db7d1688`), configuration 12 and program channel layouts (`2f559d21`), a test (`5cefe2df`).
- `symphonia-codec-opus`, `symphonia-common/src/xiph/audio/opus`: surround Opus through ruopus's multistream decoder (`05192ccb`).
- `symphonia-common/src/mpeg/audio`, `symphonia-bundle-mp3`: MPEG layout handling and a clippy fix (`2f559d21`, `0a7cf7a6`).
- `Cargo.toml`: smallvec 1.16.5 (`6552f015`).

`git log -- vendor/symphonia` lists every local change. Keep local changes in those commits so a refresh can reapply them.
