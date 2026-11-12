# Upstream pin

This directory contains the published `rusty_aac` 2.0.1 crate from https://static.crates.io/crates/rusty_aac/rusty_aac-2.0.1.crate.

Archive SHA-256: `93388483e43ffd3bb3c347794a849edb204438258d8642b8449992a56433798a`.
Upstream revision: `14e850877147deb698f4c218df3ba815c418dd12`, path `crates/rusty_aac` in https://github.com/Remade-With-Rust/remade_ffmpeg_rs.

The Apache-2.0 license is in `LICENSE`. Player disables default features and uses the safe scalar decoder. There are no runtime dependencies. The crate is vendored because the configured package mirror does not contain this release. Keep upstream source unchanged when refreshing the pin.
