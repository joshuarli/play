# Upstream pin

This directory is the published `ruopus` 0.1.2 crate (MIT) from https://static.crates.io/crates/ruopus/ruopus-0.1.2.crate.

Archive SHA-256: `fcfcd963c83fc1ada2180ccdac7f1568d87fb0f2ad7236b266fdc9fb9122fbfd`.
Upstream revision: `b7aab670bce04eaa10731b152e9834b89a21cde3` (recorded in the archive's `.cargo_vcs_info.json`), https://github.com/jmg049/ruopus.

Source: the revision comes from the archive's own metadata; the vendoring commit is `9526b047` (2026-11-03) and does not name a revision.

Modified, so not pristine:

- `Cargo.toml`: the `python` feature, its `pyo3` and `numpy` dependencies, the `cdylib` crate type, the `opus` dev-dependency and `[profile.release]` are removed; `src/lib.rs` drops `mod python`. The Python bindings, docs, tests and `Cargo.lock` are not vendored.
- `src/celt/mdct.rs`: a mixed-radix FFT replaces the O(n^2) direct DFT used when the `spectrograms` feature is off (`f2a64e73`).

The workspace `[patch.crates-io]` entry in the root `Cargo.toml` routes Symphonia's `ruopus` dependency here. Refresh by re-vendoring the new release, then reapplying the two changes above.
