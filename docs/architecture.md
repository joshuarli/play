# Architecture

Player is one Rust process with pinned native multimedia libraries, one Lua 5.2 interpreter, and a small native window bridge. It opens local files only. The mpv-derived C playback engine is gone; `player` is built by Cargo.

## Player shape

- **Rust playback engine:** `player-engine` owns the scheduler, clock, playback state, frame pacing, and session worker. `player-media` owns FFmpeg demuxing and video decode. `player-audio` owns Symphonia audio decode, the channel mixer, resampling, the clock, and the CoreAudio and ALSA sinks. `player-ass` owns libass subtitle tracks and rendering. `player-output` owns the libplacebo/Vulkan renderer. `player-display` owns the native window, input, and presentation surface: a Cocoa/Metal bridge on macOS and Wayland on Linux.
- **Rust application:** `player-app` owns argument parsing, IPC, playlists, chapters, track switching, the property/event surface, command dispatch, OSC lifecycle, scripts, GUI and TUI modes, Finder open, drag-and-drop, and screenshots. `player-gui` is a thin compatibility launcher.
- **Lua:** the built-in on-screen controls and any script the user loads explicitly. The interpreter is vendored Lua 5.2.4 through `mlua`. The Lua sources live in `rust/player-lua/lua/`.
- **Native code:** the Objective-C window bridge under `rust/player-display/bridge/`, the vendored Lua C sources, the pinned FFmpeg/libass/libplacebo/Vulkan/MoltenVK/shaderc/dav1d/zlib closure, and the platform frameworks. There is no mpv playback loop, libmpv, or C player process.

The release is one Rust executable. On macOS it links the pinned multimedia dylibs, which are copied into `Player.app/Contents/Frameworks`. On Linux it is an x86-64 musl executable that links the pinned static prefix and a small set of system libraries.

## Input boundary

`player-core::input::classify_input` is the single policy for input strings. It accepts ordinary local paths, including paths with spaces, Unicode, `:`, relative components, symlinks, and mounted volumes. It rejects every `scheme://` URL, the known protocol wrappers (`data:`, `lavfi:`, `edl:`, `memory:`, `fd:`, `av:`, `mf:`, and the rest), and network prefixes. `file://` is converted only when the caller passes `allow_file_uri`; drag-and-drop does this for an existing local path before queueing `loadfile`.

The classifier is called at startup, IPC and script `loadfile`, `--playlist` and `loadlist` paths, playlist entries, and external audio or subtitle files. One limit is open:

- Startup parsing rejects a non-UTF-8 argument before the classifier runs. The classifier and the path API take UTF-8, so they are narrower than upstream mpv's byte-transparent paths. macOS paths are Unicode in normal use.

FFmpeg is configured with `--disable-network` and a `file`-only protocol allowlist. Network protocols, TLS, and a CA bundle are not built.

## Playback pipeline

FFmpeg demuxes every container and decodes video. `player-media` can open VideoToolbox and VA-API decoders with a software fallback; the scheduler currently requests software decode for playback. Symphonia decodes AAC, ALAC, FLAC, MP3, Opus, PCM, and Vorbis from packets supplied by the FFmpeg demuxer. A Rust mixer and resampler feed the audio sink; a decode error logs and skips the packet, while argument, unsupported, and panic statuses fail the audio path. The renderer is libplacebo over Vulkan, with MoltenVK on macOS and the Wayland surface on Linux.

The scheduler uses the audio device position as the master clock when audio is present, falls back to system time when drained or paused, and drops late video frames. The null audio sink paces in real time so headless playback keeps normal timing. Null video still decodes and counts presented frames without creating a GPU surface.

Subtitles are demuxed through FFmpeg and rendered by libass. The OSD composes libass bitmaps with the Lua overlay into an RGBA surface. Null video keeps OSD composition off so libass does not emit error lines in headless gates.

## Application surface

- **Startup:** `player-app::options` parses the supported command line into typed options. Unknown options are ignored for compatibility except the explicitly removed set, which fails. `--version`, `--help`, and `--list-protocols` are handled before an engine opens.
- **Playlist:** `player-app::playlist` owns order, the current entry, insertion, removal, and next and previous. Startup files, `--playlist`, `loadlist`, and `loadfile` expand local M3U/M3U8/PLS files. Remote playlist entries are logged and skipped.
- **Commands and properties:** `player-app` validates and dispatches commands, routes IPC JSON, delivers property observers, and maps the emulated mpv property names used by the Lua modules and the tests. The playback engine supplies scalar state, tracks, chapters, metadata, and frame statistics.
- **OSC and scripts:** `player-app` embeds `osc.lua` plus its `mp.assdraw`, `mp.options`, and `mp.input` modules. Explicit `--script` files are trusted code with the full command table. Each script runs on its own thread with a bounded event queue. On quit a script has two seconds to return; a spinning script is reported and abandoned without killing the thread.
- **Presentation:** GUI mode opens a native `player-display` window, presents engine frames and a separate OSD overlay at the window pixel size, and feeds key and pointer input through the same dispatcher. TUI mode drives `player-tui` on the Rust engine, decodes embedded cover art to a 64x64 thumbnail, and does not create a window. `screenshot` and `screenshot-to-file` encode the last presented RGBA frame as PNG; `screenshot-raw` returns the pixels as a native map over IPC.

## Release and dependencies

`scripts/build-deps.sh` builds Vulkan-Headers and Loader, dav1d, FreeType, FriBiDi, HarfBuzz, shaderc, libplacebo, libass, and FFmpeg from pinned sources, and fetches the checksum-pinned MoltenVK binary. `scripts/release.sh` builds `player-app` with the `dist` Cargo profile and calls `scripts/bundle.py` to copy the non-system closure into `Player.app/Contents/Frameworks`, rewrite install names to `@rpath`, add the executable rpath, bundle MoltenVK and its ICD manifest, write `Info.plist`, write a build stamp with the revision and release binary hash, and ad-hoc sign the app.

The bundler sets `MPVBUNDLE=true` in `Info.plist`; the app uses that value to stay idle with a window when launched without a file, so a Finder open event can play. It also declares the movie, audio, and playlist types and bundles the icon.

## Linux

Linux x86-64 musl uses the same engine, application, and Lua host as macOS. The platform split is:

| Area | macOS | Linux |
|---|---|---|
| Video | libplacebo, Vulkan over MoltenVK, Cocoa | libplacebo, Vulkan, Wayland |
| Audio | CoreAudio | ALSA |
| Window | `player-display/bridge/metal_window.m` | Wayland client |
| Build | `scripts/build.sh`, `scripts/release.sh` | `scripts/build-linux.sh` |

`scripts/build-linux.sh` builds `x86_64-unknown-linux-musl` with `-C target-feature=-crt-static`, because a static-pie with Wayland in NEEDED dies in the loader before `main`. It links the static prefix from `scripts/build-deps-linux.sh`. The executable needs only musl, libgcc, the Vulkan loader, Wayland client, ALSA, and libva, and `scripts/audit-linux.sh` enforces the actual NEEDED set. `player-media` carries VA-API and VideoToolbox decode paths, while the scheduler currently requests software decode; `docs/linux.md` covers the decoder status and drivers. There is no glibc build.

## Provenance

- mpv v0.41.0 at `41f6a645068483470267271e1d09966ca3b9f413` remains the behavioral reference for the ported application surface, and the bundled Lua client scripts come from it.
- FFmpeg n9.0.2 at `ce8f11b9fac4cd7ccfb41ca7c01bc17f5cda8cc3`.
- Symphonia at `b6336209bb40d57e4ed71079cf7f7fc6c4626aae`, MPL-2.0, vendored under `vendor/symphonia`.
- The repository history was consolidated to the `campaign-base` root commit. Upstream mpv and Symphonia histories are not retained; the pins above are the provenance.

## Follow-on

Follow-on work is Linux aarch64, HDR output, and a fully static musl `wlshm` fallback. A network capability, a second Lua interpreter, or automatic user config and scripts would need an explicit decision.
