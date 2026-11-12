# PLAY-47: Preserve decoded pixel depth through presentation

Status: draft

## Summary

Extend the decoded-frame presentation boundary to describe component depth and storage layout. Prefer retaining 10-bit precision into libplacebo so ordinary 10-bit video can render on existing SDR outputs. Metal HDR/EDR output remains the independent work in PLAY-33.

## Motivation

`crates/player-engine/src/video.rs:287` recognizes only 8-bit layouts, and normal presentation returns an error for formats such as `yuv420p10le`. The two upload paths in `crates/player-output/src/frame.rs:89` and `crates/player-output/src/frame.rs:150` also select 8-bit textures. The null output returns before this boundary, so decoding a 10-bit fixture to completion does not prove display support.

## Design

Describe each supported pixel layout with its plane count, subsampling, components per plane, storage width, meaningful component depth, and bit alignment. Use that description for plane dimensions, row-byte bounds, stride validation, texture selection, and libplacebo's sample normalization. A 10-bit component stored in a 16-bit word must not be interpreted as either an 8-bit byte or an arbitrary full-range 16-bit sample.

Begin with the software decoder formats required by the existing High 10/Main 10 corpus, while retaining the current 8-bit layouts. Preserve FFmpeg frame ownership through upload, including negative-stride handling and checked byte lengths. Hardware surface import remains separate and must reuse a compatible format description when PLAY-20/PLAY-32 reach that path.

Use the same depth-aware upload description for the window and screenshot renderer. Keep the current single-device normal frame path: this change must not reintroduce a headless render, CPU readback, and second-device upload for every frame. Unsupported layouts should fail with an actionable format name or take an explicitly chosen conversion path.

Verify known decoded formats with the pinned runtime before asserting coverage. Render a gradient and neutral/chromatic samples through the actual upload path, checking range and bit alignment against a reference. Test row padding and both existing renderer backends where available, and exercise screenshot creation separately. Do not use `--vo=null` as the display acceptance check.

This work does not establish HDR output, correct display rotation, or sample aspect ratio. Those have independent contracts in PLAY-33 and PLAY-48. A 10-bit SDR input is sufficient to demonstrate the pixel-depth defect without requiring HDR hardware.

## Alternatives considered

- **Preserve depth in libplacebo uploads:** retains input precision and uses the existing renderer; recommended.
- **Convert unsupported frames to an existing 8-bit layout:** a smaller compatibility fallback, but loses precision and adds a per-frame CPU conversion. It must be documented and measured as such.
- **Reject 10-bit video permanently:** simple but excludes common profiles of codecs already presented as supported; not recommended.

## Open questions

1. **Initial delivery:** preserve 10-bit precision, or ship an explicit 8-bit conversion fallback first? Recommend native depth-aware uploads; accept conversion only as a deliberate interim tradeoff.
2. **Fallback scope:** convert every FFmpeg output format or maintain a bounded supported set? Recommend the bounded set required by the player's allowlisted codecs and verified fixtures, with clear errors for the remainder.

## Decision

Pending owner decision. No renderer code changes in this review. After the chosen representation and rendered checks are implemented, document the supported layouts in `docs/architecture.md` and remove this draft with PLAY-47.
