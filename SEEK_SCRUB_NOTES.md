# Seek Scrubbing Research Notes

## Problem
Seeking is fast (packet cache works), but holding arrow keys doesn't show
intermediate keyframes. The display pipeline is asynchronous and frames get
flushed before the GPU composites them.

## What was tried (on master, commits 814e23e + staged changes)
1. **Signal-based frame delivery** — `signal_frame_ready()` via `dispatch_async`
   to eliminate 0–8ms timer poll delay. Works, reduces latency.
2. **Tighter timer during seeking** — 8ms → 2ms interval. Minor improvement.
3. **Direct IOSurface display** — Bypass AVSampleBufferDisplayLayer via
   `CALayer.setContents:` with IOSurface. Failed after 6+ iterations:
   view-managed layers ignore setContents, encoding mismatches, pixel buffer
   lifetime issues. Removed entirely.
4. **FrameDisplayed gating** — mpv-style: enqueue frame → wait for display →
   dispatch next seek. The wait never worked because AVSampleBufferDisplayLayer
   is asynchronous — no way to know when a frame is actually composited.
5. **kCMSampleAttachmentKey_DisplayImmediately** — Tells layer to present ASAP.
   Didn't help because the flush from the next seek still arrives before VSync.
6. **dispatch_after 16ms delay** — Delay FrameDisplayed by one VSync. The
   `SeekRelative` handler wasn't checking the gating flag, so new seeks bypassed
   it. Fixed that, but still didn't work.

## How mpv does it (studied from ~/dev/mpv source)

### Architecture
mpv uses **synchronous rendering** via CAMetalLayer/CAOpenGLLayer. The VO (video
output) thread draws frames directly to a Metal/OpenGL surface. `flip_page()`
presents the frame and optionally waits for VSync. This is fundamentally different
from AVSampleBufferDisplayLayer which is fully asynchronous.

### Seek gating: `execute_queued_seek()` in `player/playloop.c`
```c
void execute_queued_seek(struct MPContext *mpctx) {
    if (mpctx->seek.type) {
        if ((mpctx->seek.flags & MPSEEK_FLAG_DELAY) &&
            mp_time_sec() - mpctx->start_timestamp < 0.3)
        {
            if (mpctx->video_status < STATUS_PLAYING)
                return;  // WAIT until frame is displayed
        }
        mp_seek(mpctx, mpctx->seek);
    }
}
```

### Key mechanisms
- **MPSEEK_FLAG_DELAY**: Arrow key seeks are tagged with this flag
- **video_status**: Progresses SYNCING → READY → PLAYING. Only at PLAYING has
  a frame been fully rendered to screen.
- **vo_is_ready_for_frame()**: VO-level gate — checks `!frame_queued &&
  !rendering && timing_ok`. Prevents queueing until previous frame is done.
- **vo_wait_frame()**: Blocks until `!frame_queued && !rendering`. Used after
  first post-seek frame to ensure it's visible.
- **300ms window**: `mp_time_sec() - start_timestamp < 0.3` — coalescing window
  for rapid seeks.
- **Dual-level gating**: VO-level (frame queue empty) + playloop-level (status).

### Playloop ordering
```
write_video()           → queue frame to VO (if VO ready)
...
execute_queued_seek()   → execute next seek (only if status >= PLAYING)
```

### Key insight
The real difference is that mpv's rendering is **synchronous**: when `flip_page()`
returns, the frame IS on screen (or at least submitted to the display controller).
AVSampleBufferDisplayLayer has no equivalent — `enqueueSampleBuffer:` just queues
internally and composites at VSync via the render server.

## Likely actual fix
The problem may be simpler than the display pipeline: **too many seek events from
keyboard repeat**. macOS key repeat at ~30Hz generates seeks faster than we can
decode+display keyframes. The packet cache makes demux instant, so seeks pile up.
mpv's 300ms coalescing window + MPSEEK_FLAG_DELAY handles this at the input level.

Options:
1. **Coalesce seeks more aggressively** — batch all pending SeekRelative commands
   before dispatching (already partially implemented in the staged changes).
2. **Rate-limit seeks** — Don't dispatch a new seek until at least ~30ms after
   the last one, giving the display pipeline time to present.
3. **Drop to CAMetalLayer** — Synchronous rendering like mpv. Major rewrite.
