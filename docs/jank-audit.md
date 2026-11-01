# User-facing audit

Recorded 2026-11-01 at `7a8958b`. Scope: the shipped `dist/Player.app` and its visible macOS workflows. Automated evidence is headless; items marked **inferred** were traced through code but not exercised on the window server. `TODO.md` holds the open work.

## Findings

### 1. Playback failures have no visible feedback (high)

A missing file produces `[file] Cannot open file …` and `Failed to open …` in the log (`player/main.c:617-636`). The only `NSAlert` paths are config and log-file messages (`osdep/mac/dialog.swift:84`, `osdep/mac/menu_bar.swift:324`). A Finder launch has no attached terminal, so a failed open can look like the window closing or doing nothing. Surface a single alert or OSD message for a failed load and keep the log.

## Limits

Not inspected or run: real mouse and drag on the OSC, fullscreen across displays, HDR and 10-bit output, audio audibility, Retina scaling, double-click and Open With, Gatekeeper quarantine, and other Macs. This audit does not propose changing the local-only contract.
