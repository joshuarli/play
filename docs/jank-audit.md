# Jank audit (P8.3)

Scope: the shipped `dist/Player.app` and its visible macOS workflows. Automated evidence is headless; items marked **inferred** were traced through code but not exercised on the window server. No fixes were made in this audit.

## Findings

### 1. `Open URL…` is a dead end (high)

The File menu still offers `Open URL…` (`osdep/mac/menu_bar.swift:128`, handler at `:345`), but the fork refuses every non-local input. Observed with the release app: `http://127.0.0.1/nope.mp4` prints `Refusing non-local input: …` and exits 2. A GUI launch has no terminal, so the user gets no explanation. Recommendation: remove or disable the menu item, or replace it with a local-only notice. Do not re-enable network protocols for it.

### 2. Playback failures have no visible feedback (high)

A missing file produces `[file] Cannot open file …` and `Failed to open …` and exits 2 (`player/main.c:617-636`); the only `NSAlert` paths are config/log-file messages (`osdep/mac/dialog.swift:84`, `osdep/mac/menu_bar.swift:324`). The release app launched from Finder has no attached terminal, so a failed open can look like the window closing or doing nothing. Recommendation: surface `files_errored`/`files_broken` through an OSD message or a single alert, and keep the terminal log.

### 3. Finder associations are not declared (medium)

The app implements `application(_:open:)` (`osdep/mac/application.swift:48`) and has an `Open File…` menu, but the bundled `Info.plist` has no `CFBundleDocumentTypes`. Finder's Open With/double-click association therefore has nothing to match. Recommendation: declare the supported movie/audio/playlist UTIs and test double-click and Open With. This is inferred from the plist and code; it was not exercised with the window server.

### 4. Finder/Dock identity is incomplete (low)

`Info.plist` has no `CFBundleIconFile`, although the app sets a Dock image programmatically (`osdep/mac/app_hub.swift:122`, `osdep/mac/menu_bar.swift:303`). Finder and some system surfaces may still show a generic app icon. Recommendation: add the existing icon as a bundle `.icns` and the plist key. Inferred; visual result is owner-run.

## Limits

Not inspected or run: real mouse/drag on the OSC, fullscreen across displays, HDR/10-bit output, audio audibility, Retina scaling, double-click/Open With, Gatekeeper/quarantine, and other Macs. The audit does not propose changing the local-only contract. Findings 1–2 are the highest-impact follow-ups; 3–4 are packaging polish.
