# User-facing audit

Recorded 2026-11-01 at `7a8958b`. Scope: the shipped `dist/Player.app` and its visible macOS workflows. Automated evidence is headless; items marked **inferred** were traced through code but not exercised on the window server. No fixes were made in this audit. `TODO.md` holds the open work.

## Findings

### 1. The bundle runs as an accessory app (high)

`AppHub.isBundle` is true only when `MPVBUNDLE=true` is in the process environment (`osdep/mac/app_hub.swift:40`). `scripts/bundle.py:142-156` writes an `Info.plist` without the `LSEnvironment` key that the upstream bundle template sets (`TOOLS/osxbundle/mpv.app/Contents/Info.plist:139-145`). The built plist has no `MPVBUNDLE`, so `Application.main` chooses `.accessory` (`osdep/mac/application.swift:103`). An accessory app has no Dock tile and no menu bar, so the menu findings below are unreachable in the shipped build. The app also has no idle default for a Finder launch with no file, and a Finder open event can arrive after the empty-playlist exit.

### 2. `Open URL…` is a dead end (high)

The File menu offers `Open URL…` (`osdep/mac/menu_bar.swift:128`, handler at `:345`), but the player refuses every non-local input. The menu path queues a `loadfile`, which `cmd_loadfile` rejects (`player/command.c:6246-6250`); the error is logged, not shown, and the running file keeps playing or the app later exits as "No files played". The CLI path is different: a URL argument exits 2 with `Refusing non-local input`. Do not re-enable network protocols for this item. Remove the menu item or replace it with a local-only notice.

### 3. Playback failures have no visible feedback (high)

A missing file produces `[file] Cannot open file …` and `Failed to open …` in the log (`player/main.c:617-636`). The only `NSAlert` paths are config and log-file messages (`osdep/mac/dialog.swift:84`, `osdep/mac/menu_bar.swift:324`). A Finder launch has no attached terminal, so a failed open can look like the window closing or doing nothing. Surface a single alert or OSD message for a failed load and keep the log.

### 4. Finder associations are not declared (medium)

The app implements `application(_:open:)` (`osdep/mac/application.swift:48`) and has an `Open File…` menu, but the bundled `Info.plist` has no `CFBundleDocumentTypes`. Finder's Open With and double-click have nothing to match. Declare the supported movie, audio, and playlist UTIs and test both paths. This is inferred from the plist and code.

### 5. Finder and Dock identity is incomplete (low)

`Info.plist` has no `CFBundleIconFile`, although the app sets a Dock image programmatically (`osdep/mac/app_hub.swift:122`, `osdep/mac/menu_bar.swift:303`). Some system surfaces may show a generic icon. Add the icon as a bundle `.icns` and the plist key. Inferred; the visual result is owner-run.

### 6. The menu exposes removed commands (medium)

The Playback menu still offers Shuffle, Toggle Loop File, and Toggle Loop Playlist (`osdep/mac/menu_bar.swift:216-218`). `playlist-shuffle` is not in the command table, and the loop options were removed, so each item is a silent no-op. The Settings and Keyboard Shortcuts items open `~/.config/mpv` and `~/.mpv`, which the shipped player does not load. Remove or replace these items.

### 7. The file pickers advertise unsupported types (medium)

`Dialog.openFiles` allows the image, archive, and playlist extension lists from `options/options.c:979-985`. Picking a `.jpg`, `.zip`, or `.cue` cannot play: the image and archive demuxers are not built, and a `.cue` is parsed as a text playlist. Restrict the picker to the built container, audio, subtitle, and playlist types.

## Limits

Not inspected or run: real mouse and drag on the OSC, fullscreen across displays, HDR and 10-bit output, audio audibility, Retina scaling, double-click and Open With, Gatekeeper quarantine, and other Macs. This audit does not propose changing the local-only contract.
