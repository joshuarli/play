# Scope

Player opens local filesystem paths and local playlist files. It does not implement network protocols, remote playlists, capture, optical discs, or remote metadata.

## Input

- Ordinary paths, including relative and absolute paths, spaces, `:`, Unicode, symlinks, and mounted volumes.
- Local playlists in M3U, M3U8, and PLS form. A network URL in a playlist is rejected while the local entries continue.
- External audio and subtitle files beside a local file.
- `file://` from drag-and-drop when it resolves to an existing local path. The CLI, IPC `loadfile`, playlist entries, and external files reject `file://`.

`player-core::input::classify_input` rejects every other `scheme://` URL and the protocol wrappers `data:`, `lavfi:`, `edl:`, `memory:`, `fd:`, `fdclose:`, `av:`, `avdevice:`, `mf:`, `null:`, `slice:`, `appending:`, `mpv:`, `ffmpeg:`, and `lavf:`. A mounted network volume is an ordinary filesystem path; the player makes no network connection of its own.

Two boundary limits remain. `--playlist=FILE` and `loadlist FILE` hand the playlist path to the stream layer without the classifier, so `file://` is accepted there and only a network scheme fails closed. Startup parsing and the classifier take UTF-8, so a non-UTF-8 argument is rejected with `command line argument is not UTF-8`; the path API is narrower than upstream mpv's byte-transparent one.

## Retained modules

- `stream_file`: `file`, including stdin and stdout, and the write path used by screenshots.
- `stream_cb`: the internal client API callback. No external caller registers a protocol.
- `stream_memory`: internal character-set conversion for subtitles. It is not registered as a protocol.
- `demux_lavf`: FFmpeg demuxing for the allowlisted local containers.
- `demux_mkv` and `demux_mkv_timeline`: local Matroska and ordered chapters.
- `demux_playlist`: local M3U, M3U8, and PLS files.
- `demux_timeline`: the generic timeline demuxer used by ordered chapters.
- `demux/cue.c`: embedded CUE metadata in local containers.
- `demux_lavf.c`: the local file and subtitle character-set conversion path.

## Removed

- Network and wrapper streams, including HTTP, HTTPS, FTP, SFTP, SSH, RTSP, RTMP, HLS, DASH, SMB, libavformat, AVDevice, Blu-ray, DVD, CDDA, DVB, EDL, memory, `fd`, `slice`, `appending`, `mpv`, `ffmpeg`, `lavf`, `hex`, `null`, and the image-file reader.
- Disc, external CUE, EDL, libarchive, image-file, `mpv`, null, and raw audio or video demuxers.
- The yt-dlp hook and the option tables for stream-lavf, raw demuxers, discs, and capture.
- Windows, Linux, and Android audio outputs, video outputs, window systems, GL and OpenGL, Vulkan display backends, and their supporting sources.
- Video outputs that are not `gpu-next`, including image, terminal, X11, Wayland, DRM, SDL, VA-API, VDPAU, Direct3D, and MediaCodec.
- JavaScript, VapourSynth, C plugins, Touch Bar, and Media Player integration.
- Built-in Lua modules that the player no longer loads: `auto_profiles`, `commands`, `console`, `context_menu`, `fzy`, `positioning`, and `stats`.
- Filter entry points for lavfi, VapourSynth, VA-API, VDPAU, D3D11, fingerprint, and AC-3 encoding.
- Playlist shuffle and loop commands and options. A-B loop remains.

## CLI

Supported: `player FILE...`, `--fullscreen` and `--fs`, `--start`, `--audio-file` and `--audio-files`, `--sub-file` and `--sub-files`, `--version`, `--help`, and the playback, track, and OSD controls used by the OSC and local playback.

Removed: `--ytdl*`, `--hls-bitrate`, `--lavfi-complex`, `--stream-dump`, `--stretch-dvd-subs`, the encoding group (`--o`, `--of`, `--ovc`, `--oac`, their option tables, `--orawts`, and metadata flags), the Windows window and capture group, and options for deleted Lua overlay modules. `--http-header-fields` and the other network groups were removed with the network modules.

## Compiled but not reachable

- Encoding, stream recording, and the `f_lavfi` filter graph remain compiled. The release FFmpeg has no encoders or muxers, and the CLI has no entry point for them.
- The C `subprocess` command and the `.run` script backend remain compiled and reachable from an explicit script or an IPC client. They are not reachable from the built-in OSC. `select.lua`, which loads by default, uses both.
- The clipboard property, the clipboard command, and the macOS clipboard bridge remain compiled and reachable. The default `Ctrl+v` binding pastes clipboard text as a local file operand.
- The cplugin loader is compiled out; `HAVE_CPLUGINS` is not defined in the release configuration.

## Remaining debt

- The UTF-8-only path API and the classifier bypass in `--playlist` and `loadlist`.
- The command checker rejects some valid mpv forms (`aid auto`, `seek 1:30`) that the C parser still accepts.
- The Rust fullscreen mirror is not updated when the window changes fullscreen.
- The Lua event queue is unbounded and script completion has no timeout.
- Clipboard support is live although the release report previously described the player as having none. Decide whether to keep it or remove it.
