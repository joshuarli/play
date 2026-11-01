# Status

## Campaign

The campaign is complete through P8.4. The final report, install/resume material, architecture/provenance, and license audit are in place.

| Slice | State | Evidence |
|---|---|---|
| P0.1–P1.4 | accepted | baseline, minimal FFmpeg, Rust audio bridge |
| P2.1–P2.3 | accepted | scoped build, deleted features, minimal closure |
| P3.1–P3.3 | accepted | Rust playlist, FFI handles, startup config |
| P4.1–P4.3 | accepted | typed CLI, local-only boundary, C/Rust contract |
| P5.1–P5.5 | accepted | mlua host, FFI backend, single interpreter, OSC |
| P6.1–P6.3 | accepted | compact OSC, attribution, release baseline |
| P7.1–P7.5 | accepted | input, playlist, commands, state, local file |
| P8.1 | accepted | pinned closure and `dist/Player.app` |
| P8.2 | accepted | 26.8 MB app / 9.64 MB executable |
| P8.3 | accepted | 292-case suite, audit, final budget gate |
| P8.4 | complete | final report/install/resume |

## Final evidence

- `tests/run.sh suite`: **292 passed, 0 failed** (includes `release`).
- `scripts/audit.sh`: **ok** (FFmpeg manifest, build, local-only, protocol list, linked libs, one interpreter, `cargo audit`).
- `scripts/measure.sh final`: **final: ok** (all P8 budgets).
- `scripts/measure.sh compare`: **compare: ok**.
- Release app: `dist/Player.app`; executable 9,640,432 B; framework closure 16.37 MiB in 13 files; total 26,807,168 B.
- Final metrics: cold 162.11 ms, warm 115.73 ms, first frame 220.07 ms, idle RSS 84.78 MB, 1080p RSS 216.88 MB, 1080p CPU 0.23 s, 4K CPU 0.55 s, seek 3.21 ms, no underruns, no frame drops, one `file` protocol, one Lua interpreter.

## Debt and limits

- Local files only. No network protocol, encode/record/lavfi, or clipboard.
- GUI, HDR/10-bit color, subtitle shaping/positioning, Retina, actual audio, real mouse/drag, and signing/notarization are owner-run (`docs/manual-checklist.md`).
- The jank audit records open user-facing items; none were fixed in P8.3.
- Follow-on work: A/V scheduler, general demux/decode orchestration, renderer integration, Linux.

## Resume

`Read PLAN.md and verify the final state. The campaign is complete; stop at a documented limitation or an owner decision.`

First command: `tests/run.sh suite && scripts/audit.sh`.
