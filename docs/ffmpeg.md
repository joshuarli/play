# FFmpeg

Player links the shared FFmpeg runtime from `laputa-systems/ffmpeg-build` (profile `runtime`, see that repository's `docs/runtime.md`). It does not build FFmpeg and carries no FFmpeg code in its executable or bundle. `ffmpeg`, `ffprobe`, and Player load the same libav* shared libraries. The producer owns the sources, configure flags, components, and the full audit; Player pins one runtime release and checks it against its own playback contract. The runtime is licensed GPL version 2 or later; its notices are in the runtime's `share/licenses/`.

## Pin

`ffmpeg-runtime.lock.json` names the one runtime release Player requires:

```json
{
  "contract": 2,
  "repository": "laputa-systems/ffmpeg-build",
  "profile": "runtime",
  "ffmpeg": "9.0.2",
  "release": "<producer tag>",
  "libraries": ["libavformat", "libavcodec", "libswscale", "libavutil"],
  "sha256": {
    "aarch64-apple-darwin": "<sha256 of the archive>",
    "x86_64-unknown-linux-musl": "<sha256 of the archive>"
  }
}
```

The runtime release is `<ffmpeg>+<release>`, such as `9.0.2+20261107`. The archive for a target is `ffmpeg-<ffmpeg>+<release>-<target>-runtime.tar.gz`, derived from those two fields; the pin stores only its digest. `libraries` is the set Player links; each must appear in the runtime manifest's `libraries` with a SONAME (Linux) or install name (macOS). An installer keeps exactly this release installed for Player, beside any newer ones, and reads it from the pin at the Player release tag, from `FFmpeg runtime:` in the Linux tarball's README, or from `PlayerFFmpegRuntime` in `Player.app/Contents/Info.plist`.

## Installed layout

Player finds the runtime through a search path relative to its executable. The build input `PLAYER_FFMPEG_RPATH` sets it when `player-app` links:

| Platform | Default | Meaning |
| --- | --- | --- |
| Linux | `$ORIGIN/../lib` | `DT_RUNPATH` of `player`. `none` builds without a RUNPATH. Any other value must be one path under `$ORIGIN`. |
| macOS | `@executable_path/../../../lib` | `LC_RPATH` of `Player.app/Contents/MacOS/player`. Any other value must be one path under `@executable_path` or `@loader_path`. |

An absolute path is rejected, so no build-tree path reaches the executable. The layouts are:

- **Linux, user-local (`usr`).** `player` is in a `bin/` directory whose sibling `lib/` holds the pinned runtime's libraries: either inside the versioned runtime tree as `<runtime>/bin/player`, or in a Player prefix `<player>/bin/player` whose `<player>/lib` is a symlink to `<runtime>/lib`. A symlink to `player` on `PATH` works, because the loader resolves `$ORIGIN` from the real executable.
- **Laputa.** The runtime's libraries are in `/usr/lib`, so build with `PLAYER_FFMPEG_RPATH=none` and the normal loader search finds them; `player` can live in `/usr/bin`.
- **macOS.** `Player.app` sits in a directory whose `lib/` holds the pinned runtime's libraries: `<prefix>/Player.app` and `<prefix>/lib`. The executable links each library as `@rpath/<library>.<major>.dylib`, the producer's install name. An installer that cannot use that layout can add its own `LC_RPATH` with `install_name_tool -add_rpath` and must then re-sign the app ad hoc (`codesign --force --deep --sign - Player.app`).

The runtime itself needs only musl and the system `libva` and `libva-drm` on Linux; dav1d and zlib are inside libavcodec and libavformat.

## Fetch

```sh
scripts/fetch-ffmpeg-runtime.sh aarch64-apple-darwin
scripts/linux-container.sh scripts/fetch-ffmpeg-runtime.sh x86_64-unknown-linux-musl
```

The script downloads the pinned archive from the producer's GitHub release into `.work/tmp/ffmpeg-runtime`, or takes a local archive given as the second argument, and verifies its SHA-256 against the pin either way. The archive must contain exactly one directory named after it, and its `share/ffmpeg-build/manifest.json` must agree with the pin on profile, target, FFmpeg version, release tag, archive name, and libraries. Only then does the script move it to `.work/deps/ffmpeg-runtime/<target>`. A prefix whose manifest already matches the pin is reused unless an archive is given. `scripts/build.sh`, `scripts/release.sh`, and `scripts/build-linux.sh` call the script, so a normal build fetches on first use.

To build against an unpublished runtime, package it in the producer with the tag the pin names (`python3 build.py package --target <target> --profile runtime --tag local`), set that target's `sha256`, and pass the archive once:

```sh
scripts/fetch-ffmpeg-runtime.sh x86_64-unknown-linux-musl ../ffmpeg-build/dist/ffmpeg-9.0.2+local-x86_64-unknown-linux-musl-runtime.tar.gz
```

## Link

`crates/player-media/build.rs` uses `crates/player-build/src/ffmpeg_runtime.rs`:

- The prefix is `FFMPEG_PREFIX` if set, otherwise `.work/deps/ffmpeg-runtime/$TARGET`. There is no fallback to a system, Homebrew, or older prefix.
- The prefix must pass the manifest check above, hold the FFmpeg headers, the `.pc` file and each library file and link of the pinned libraries, and hold no static archive.
- Link flags come from `pkg-config --libs` for the pinned libraries with `PKG_CONFIG_LIBDIR` set to the runtime only. The only accepted tokens are `-L` for the runtime's `lib/` and `-l` for a pinned library; each links as a shared library, so the executable's `NEEDED` (or load commands) name the runtime's SONAMEs (or install names).
- The search path is a symlink to the runtime's `lib/` inside `OUT_DIR`, because Cargo puts build-script search paths on the library path of `cargo test` and `cargo run` only inside the target directory. On macOS each crate that links `player-media` also adds an rpath to the runtime's `lib/` for development and test binaries; `scripts/bundle.py` deletes every absolute rpath from the app.
- The build reruns when the pin, `FFMPEG_PREFIX`, `PLAYER_FFMPEG_RPATH`, or the runtime's manifest, `include/`, or `lib/` changes.

Development and release trees use the installed layout. On Linux `scripts/install-layout.sh` writes `build-linux/bin/mpv` and `dist/bin/player`, a `build-linux/mpv` or `dist/player` link to them, and a `lib/` directory of links to the runtime's libraries and `librust_fontconfig`. On macOS `scripts/bundle.py` writes `dist/lib` beside `dist/Player.app`.

## Gates

`tests/run.sh ffmpeg-audit` (`scripts/ffmpeg-manifest.sh --check [manifest.json]`) reads the runtime manifest, by default from `.work/deps/ffmpeg-runtime/<target>`. It checks Player's contract, not the producer's component allowlist:

- the profile is `runtime`, the target is the Rust target, FFmpeg is the pinned 9.0.2, and the release tag matches the pin;
- no output devices and no input device but `lavfi` (in `libavdevice`, which Player does not link); every protocol is local (`file`, `pipe`, or `fd`), none is a network scheme from `scripts/network-schemes.txt`, and `--disable-network` is configured;
- each pinned library carries its SONAME or install name;
- the required playback demuxers (`mov`, `matroska`, `avi`, `mp3`, `flac`, `wav`, `ogg`) and decoders (`h264`, `hevc`, `libdav1d`, `vp9`, `mpeg4`, `png`, `mjpeg`) are present. macOS also needs the VideoToolbox hwaccels. Linux needs the native `av1` decoder and the VA-API hwaccels;
- the license is GPL version 2 or later;
- `dav1d` is the only external library, and `dav1d` and `zlib` are the only bundled dependencies.

The runtime also carries encoders, muxers, FFmpeg audio decoders, and filters for `ffmpeg` and `ffprobe`. Player does not use them, and its own code enforces that rather than the component list:

- `player-media` sets `format_whitelist` to the playback demuxers above, so a container the runtime can read but Player does not support fails to open. The session reads through a custom IO context over the local file, so no FFmpeg protocol opens the input.
- `VideoSession` opens only video streams, and only the video codec IDs in `codec_id_for_name`; Symphonia decodes audio from FFmpeg's demuxed packets. `player-media`'s tests cover both.

`scripts/audit-linux.sh` checks that the executable's FFmpeg `NEEDED` entries are exactly the pinned SONAMEs from the manifest, that its RUNPATH is the `PLAYER_FFMPEG_RPATH` value and there is no RPATH, that `ldd` resolves through that layout, that `avformat_open_input` is an import, that the executable exports no FFmpeg symbol, and that the unstripped executable defines no FFmpeg or dav1d symbol. The macOS `release` gate checks that `Player.app` bundles no FFmpeg dylib, links exactly the pinned install names, carries the runtime rpath, and records the runtime release in `Info.plist`.

## Update

Move to a new runtime release only after it is published with its `SHA256SUMS`:

1. Set `release` and each target's `sha256` in `ffmpeg-runtime.lock.json` from the release's `SHA256SUMS`.
2. Run `scripts/fetch-ffmpeg-runtime.sh aarch64-apple-darwin`, `tests/run.sh ffmpeg-audit`, `scripts/audit.sh`, and `tests/run.sh suite`, and run the Linux half in the container.
3. Publish a Player patch release; its installers then install the new runtime release beside the old one.

## Fixtures

`tests/fixtures/gen.sh` needs FFmpeg 9.0.2 with the corpus's encoders: Homebrew `ffmpeg` on macOS (`scripts/dev-deps.sh` pins it), or the checksum-pinned ffmpeg-build fixtures archive installed at `/opt/ffmpeg-fixtures` by `docker/linux/Dockerfile`. The container puts that archive's `bin/` first on `PATH`. This is a test-only dependency; the app never runs the fixture generator's FFmpeg. The generator currently checks the version before generation but does not preflight all required encoders (PLAY-41).
