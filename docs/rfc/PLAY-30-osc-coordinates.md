# PLAY-30: Define OSC coordinates and injected pointer behavior

Status: draft

## Summary

Keep `osc-areas` and IPC pointer commands in the same explicit coordinate space as window hit testing. Recommend making the current 'window pixels' description explicit as output backing pixels, consistent with the native input conversion, and making the tests derive the actual output size. Verify the independently reported reveal/hide failure rather than assuming a unit change will fix it.

## Motivation

The previous Retina run compared a 2560x1440 backing-pixel layout with expectations for a 1280x720 logical window and also reported that injected motion did not reveal the OSC or start its hide timer. `crates/player-display/bridge/metal_window.m:237` converts native motion to backing coordinates; the app routes native and IPC input to its pointer handlers. The current hardware behavior has not been reproduced during this review.

## Design

Document one public unit for `osc-areas` and IPC `mouse` coordinates. Keep the OSC's 720-line virtual canvas internal: transform between that canvas and the actual output dimensions once at the layout/input boundary. Do not derive backing dimensions from the requested window geometry when the platform scale changes them.

Under the recommended policy, native pointer events are converted from platform coordinates to backing pixels by the display boundary, and IPC commands already supply backing pixels. The property reports the same pixels. Resize and scale changes update layout and hit testing from a single output-size snapshot so a click cannot use one size while drawing uses another.

Injected pointer motion should use the same reveal, hover, drag, and hide-deadline transitions as a real pointer. It should not require physically moving the cursor. The headless test still needs an explicit synthetic output size; a windowed test must query the actual size and ensure the pointer lands inside the returned hit area.

Separate two checks: geometry agreement at 1x and Retina 2x, and motion-driven state transitions. Test paused playback, motion into the bar, motion away, the hide deadline, and resize with both macOS renderer backends. Record which checks require a real window; headless success alone does not close the ticket.

## Alternatives considered

- **Backing pixels:** follows the current native input conversion and matches the render target; recommended.
- **Logical points:** convenient for some UI clients, but requires an explicit scale contract and a coordinated API/test change.
- **OSC virtual-canvas coordinates:** stable internally, but would expose implementation layout units to clients and require another public transform.

## Open questions

1. **Public units:** formalize backing pixels or choose logical points? Recommend retaining backing pixels and documenting the queried output-size relationship.
2. **Injection semantics:** should IPC pointer commands drive visibility exactly like real input? Recommend yes, using the same state transitions on both headless and windowed paths.

## Decision

Pending owner decision and hardware reproduction. If the current code already satisfies the adopted contract, correct the gate and docs without inventing a renderer defect. Remove this draft when the contract and windowed verification are in place.
