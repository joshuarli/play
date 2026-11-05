# Scope

Player opens local filesystem paths and local playlist files. It does not implement network protocols, remote playlists, capture, optical discs, or remote metadata.

## Input

- Ordinary paths, including relative and absolute paths, spaces, `:`, Unicode, symlinks, and mounted volumes.
- Local playlists in M3U, M3U8, and PLS form. A network URL in a playlist is rejected while the local entries continue.
- External audio files and SubRip (`.srt`) subtitle files beside a local file. Subtitles are SubRip only, external or embedded.
- `file://` from drag-and-drop when it resolves to an existing local path. The CLI, `--playlist`, `loadlist`, IPC `loadfile`, playlist entries, and external files reject `file://`.

`player-core::input::classify_input` rejects every other `scheme://` URL and the protocol wrappers `data:`, `lavfi:`, `edl:`, `memory:`, `fd:`, `fdclose:`, `av:`, `avdevice:`, `mf:`, `null:`, `slice:`, `appending:`, `mpv:`, `ffmpeg:`, and `lavf:`. A mounted network volume is an ordinary filesystem path; the player makes no network connection of its own. The network scheme set is `scripts/network-schemes.txt`; the manifest, measure, audit, and protocol-list gates read it.

One boundary limit remains. Startup parsing and the classifier take UTF-8, so a non-UTF-8 argument is rejected with `command line argument is not UTF-8`; the path API is narrower than upstream mpv's byte-transparent one.

The local-only gate creates a hostile temporary `HOME` with `mpv.conf`, `input.conf`, and a nonexistent script. Playback succeeds without loading them, and a positive control asserts that the config's `volume=42` did not take effect.

## Retained modules

- `player-media`: FFmpeg demuxing for the allowlisted local containers, video decode through FFmpeg, and the embedded album-art decode path.
- `player-audio`: Symphonia audio decode, channel mixing, resampling, the audio clock, and the CoreAudio and ALSA sinks.
- `player-engine`: the session scheduler, packet and frame queues, seek and pause state machine, frame pacing, subtitle tracks, and the engine worker.
- `player-overlay`: the overlay display list, rasterizer, and native text shaping (Core Text; Fontconfig, HarfBuzz, and FreeType).
- `player-backend`: the engine-facing backend and the subtitle, OSD, and OSC overlay layers.
- `player-output`: libplacebo renderer over Vulkan or Metal (macOS), swapchain, overlays, and frame download.
- `player-display`: Cocoa/Metal window bridge on macOS and Wayland window/input on Linux.
- `player-tui`: the terminal renderer and input surface.
- `player-app`: CLI, IPC, playlist, chapters, track switching, properties and events, command dispatch, the native OSC, GUI and TUI modes, Finder open, drag-and-drop, and screenshots.
- `player-gui`: a compatibility launcher over `player-app`.

There is no C playback loop or Meson player build.

## Removed

- The mpv C playback engine and libmpv.
- glibc Linux. The Linux build targets x86-64 musl only, and glibc is explicitly out of scope.
- Network and wrapper streams, including HTTP, HTTPS, FTP, SFTP, SSH, RTSP, RTMP, HLS, DASH, SMB, libavformat, AVDevice, Blu-ray, DVD, CDDA, DVB, EDL, memory, `fd`, `slice`, `appending`, `mpv`, `ffmpeg`, `lavf`, `hex`, `null`, and the image-file reader.
- Disc, external CUE, EDL, libarchive, image-file, `mpv`, null, and raw audio or video demuxers.
- The yt-dlp hook and the option tables for stream-lavf, raw demuxers, discs, and capture.
- Windows and Android audio outputs, video outputs, and window systems; GL and OpenGL; and all Linux audio outputs other than ALSA and window systems other than Wayland.
- Video outputs other than libplacebo, including image, X11, DRM, SDL, VA-API output, VDPAU, Direct3D, and MediaCodec. VA-API and VideoToolbox decoder support is retained in `player-media`; the scheduler currently opens decoders in software. The `--tui` frontend is not a video output: it renders audio metadata and cover art to the terminal and keeps `vo=null`. Unlike mpv, a file with audio whose only video streams are cover art (attached pictures, or PNG, BMP, TIFF, or WebP streams) plays as audio only and opens no window unless `--gui` is given or the app is bundled. From an interactive terminal, such a file as the first playlist entry starts the `--tui` frontend.
- Scripting: the Lua 5.2 interpreter, the `mp` script API, the bundled `defaults`, `assdraw`, `options`, `input`, and `osc` Lua modules, script threads and event queues, and the commands and properties that only scripts used (`script-message`, `script-message-to`, `script-binding`, `define-section`, `enable-section`, `disable-section`, `osd-overlay`, `expand-text`, `run`, `subprocess`, `screenshot-raw`, and `user-data/*`). A Rust OSC replaces the Lua one.
- JavaScript, VapourSynth, C plugins, Touch Bar, and Media Player integration.
- Encoding, stream recording, the lavfi filter graph, and the encoding option group. The release FFmpeg has no encoders or muxers.
- FFmpeg audio fallback and SPDIF passthrough. Symphonia handles supported audio decoding.
- Playlist shuffle and loop commands and options. A-B loop remains.
- Clipboard support entirely: no clipboard property, command, backend, bridge, or binding. `${clipboard/text}` expansions no longer resolve.

## CLI

Supported: `player FILE...`, `--tui`, `--gui`, `--gate` (GUI gate), `--fullscreen` and `--fs` (a window opens fullscreen unless `--fs=no` or `--geometry` is given), `--start`, `--playlist`, `--audio-file` and `--audio-files`, `--sub-file` and `--sub-files`, `--osc` and `--no-osc`, `--input-ipc-server`, `--audio-device` (Linux ALSA), `--geometry`, `--gpu-api=auto|vulkan|metal` (renderer backend; `auto` means `vulkan`, `metal` is macOS-only, any other value fails startup; readable as `options/gpu-api`), `--frames`, `--term-playing-msg`, `--list-protocols`, `--version`, `--help`, and the playback, track, and OSD controls used by the OSC and local playback. `--tui` is audio-first and keeps `vo=null`; it is not a terminal video output.

Removed: `--ytdl*`, `--hls-bitrate`, `--lavfi-complex`, `--stream-dump`, `--stretch-dvd-subs`, the encoding group (`--o`, `--of`, `--ovc`, `--oac`, their option tables, `--orawts`, and metadata flags), the Windows window and capture group, the scripting options (`--script`, `--scripts`, `--script-opts`, and `--load-scripts=yes` fail with an unsupported-option error), `--audio-spdif`, and `--ad-lavc-*`. `--http-header-fields` and the other network groups were removed with the network modules.

## Configuration

The player has no scripting and no key-binding files. It does not read `mpv.conf`, `input.conf`, or scripts from user directories. Defaults and bindings are compiled in. `PLAYER_OPTIONS` holds options, whitespace-separated, that apply before the command line, so command-line options win; anything that is not an `--option` is an error. `--input-conf` fails as removed.

## Remaining debt

- The UTF-8-only path API.
- The playback scheduler does not yet request VideoToolbox or VA-API decode (`PLAY-20`).
