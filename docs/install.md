# Install and run

## Release app

Build the pinned dependency closure and the app:

```sh
scripts/build-deps.sh
scripts/release.sh
```

The app is `dist/Player.app`. Open it from Finder, or run a file directly:

```sh
dist/Player.app/Contents/MacOS/player FILE
```

The bundle carries its own pinned dylibs, Vulkan loader, MoltenVK, and the Rust cdylib. It does not use Homebrew at runtime. It is ad-hoc signed and not notarized. Do not distribute the build without a signing identity. It declares `MPVBUNDLE`, the supported document types, and the application icon in `Info.plist`.

## Local-only contract

Local paths are supported. Drag-and-drop converts a `file://` URL for an existing path; the CLI, `--playlist`, `loadlist`, IPC `loadfile`, playlists, and external files reject `file://`. Network URLs are rejected before the stream layer with `Refusing non-local input`. The player does not include network protocols, TLS, or a CA bundle.

Clipboard commands are compiled and reachable. Encoding, recording, and the lavfi filter graph are compiled but have no reachable path in the release build.

## Development

```sh
scripts/build.sh
tests/run.sh suite
```

The development build is `build/mpv`; scratch files live under `.work/`. `scripts/release.sh` is the only supported way to produce the app. Do not hand-assemble `Contents/Frameworks`.

## Owner checks

```sh
scripts/manual-checklist.sh --fixture-dir .work/fixtures --release
```

Record results in `docs/manual-checklist.md`. Visual quality, real audio, real mouse and drag, HDR and Retina, and signing or notarization are not automated gates. `TODO.md` lists open user-facing defects.
