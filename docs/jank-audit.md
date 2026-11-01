# User-facing audit

Recorded 2026-11-01 at `7a8958b`. Scope: the shipped `dist/Player.app` and its visible macOS workflows. Automated evidence is headless; items marked **inferred** were traced through code but not exercised on the window server. `TODO.md` holds the open work.

## Findings

### 1. `Open URL…` is a dead end (high)

The File menu offers `Open URL…` (`osdep/mac/menu_bar.swift:128`, handler at `:345`), but the player refuses every non-local input. The menu path queues a `loadfile`, which `cmd_loadfile` rejects (`player/command.c:6246-6250`); the error is logged, not shown, and the running file keeps playing or the app later exits as "No files played". The CLI path is different: a URL argument exits 2 with `Refusing non-local input`. Do not re-enable network protocols for this item. Remove the menu item or replace it with a local-only notice.

### 2. Playback failures have no visible feedback (high)

A missing file produces `[file] Cannot open file …` and `Failed to open …` in the log (`player/main.c:617-636`). The only `NSAlert` paths are config and log-file messages (`osdep/mac/dialog.swift:84`, `osdep/mac/menu_bar.swift:324`). A Finder launch has no attached terminal, so a failed open can look like the window closing or doing nothing. Surface a single alert or OSD message for a failed load and keep the log.

### 3. The menu exposes removed commands (medium)

The Playback menu still offers Shuffle, Toggle Loop File, and Toggle Loop Playlist (`osdep/mac/menu_bar.swift:216-218`). `playlist-shuffle` is not in the command table, and the loop options were removed, so each item is a silent no-op. The Settings and Keyboard Shortcuts items open `~/.config/mpv` and `~/.mpv`, which the shipped player does not load. Remove or replace these items.

### 4. The file pickers advertise unsupported types (medium)

`Dialog.openFiles` allows the image, archive, and playlist extension lists from `options/options.c:979-985`. Picking a `.jpg`, `.zip`, or `.cue` cannot play: the image and archive demuxers are not built, and a `.cue` is parsed as a text playlist. Restrict the picker to the built container, audio, subtitle, and playlist types.

## Limits

Not inspected or run: real mouse and drag on the OSC, fullscreen across displays, HDR and 10-bit output, audio audibility, Retina scaling, double-click and Open With, Gatekeeper quarantine, and other Macs. This audit does not propose changing the local-only contract.
