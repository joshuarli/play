# Install and run

## Release app

Build with:

```
scripts/build-deps.sh
scripts/release.sh
```

The app is `dist/Player.app`. Open it from Finder or run:

```
dist/Player.app/Contents/MacOS/player FILE
```

The bundle carries its own pinned dylibs, Vulkan loader, MoltenVK, and the Rust cdylib. It does not use Homebrew at runtime. It is ad-hoc signed, not notarized; the local build is not quarantined. If you copy it from another machine, use macOS's normal Gatekeeper handling and do not distribution-sign it without the owner's signing identity.

## Local-only contract

`file://` and local paths are supported. Network URLs are rejected before the stream layer, with `Refusing non-local input`. The fork does not include network protocols, TLS, a CA bundle, encode/record/lavfi, or clipboard support.

## Development

```
scripts/build.sh
tests/run.sh suite
```

The development build is `build/mpv`; scratch files live under `.work/`. `scripts/release.sh` is the only supported way to produce the app; do not hand-assemble `Contents/Frameworks`.

## Owner checks

```
scripts/manual-checklist.sh --fixture-dir .work/fixtures --release
```

Record results in `docs/manual-checklist.md`. Visual quality, real audio, real mouse/drag, HDR/Retina, and signing/notarization are not automated gates. See `docs/jank-audit.md` for open user-facing items.
