# Terminal UI design

Chosen direction: **Sleeve** (A). It gives the cover the room, keeps one metadata column, and scales down to a 67-column pane.

## Layout

At 136 × 39 the cover is 52 × 26 cells, centered. The block is vertically centered, with no header; the hint line sits on the last terminal row.

```text
                           ┌──────────────────────┐
                           │                      │
                           │      album art       │
                           │                      │
                           └──────────────────────┘

                                Paper Satellites
                          Mira Vale · Low Light · 2026

                           ━━━━━━━━━━━━━●────────────
                           1:42                  3:58



        space pause   ←/→ seek   +/− volume   m mute   n/p track   q quit
```

The cover width is `min(52, cols - 20, (rows - 13) * 2)`, with a 16-cell floor and a 52-cell cap. The height is half the width because a terminal cell is about twice as tall as it is wide. Below 30 × 16 the UI prints `TERMINAL TOO SMALL` instead of distorting the frame.

The accent is sampled from the cover and used only for the progress fill, the knob, and the transient HUD line. Metadata uses three levels: title (bold), artist (dim), album and year (faint). Long titles truncate with an ellipsis using display width, not byte or character count.

## Rejected

**Deck** (B) wrapped the same content in a 96 × 36 card with a header, transport rail, volume bar, and track number. It read as a dashboard and cost roughly 30% of the cover area (44 × 22 instead of 52 × 26). Persistent format and track chrome did not earn that space. Volume, seek, and mute feedback appears as a transient HUD line in the Sleeve layout instead.

## Art

- Kitty graphics protocol when Ghostty, kitty, or WezTerm is identified and tmux passthrough is on.
- Truecolor half-block cells everywhere else.
- Inside tmux with `allow-passthrough off`, the half-block fallback is deliberate. The half-block cover is about 52 × 52 pixels, so text baked into the artwork is not legible; that is the cost of the fallback, not a bug.
- Sixel is not implemented. Linux terminals without kitty graphics get the fallback.
- No embedded picture: a dim bordered `NO ARTWORK` plate.

## Scope

Audio-first. Video files play audio with the placeholder; the TUI never opens a window and keeps `vo=null`. Embedded pictures only; external `cover.jpg` / `folder.jpg` lookup is not implemented.

When a track ends in the TUI and there is no next playlist entry, playback restarts from zero instead of unloading the file, so the transport stays live and seeking keeps working. A following playlist entry still takes over, `--keep-open` still pauses at the end, and a failed or empty file is not replayed.
