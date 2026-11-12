# PLAY-45: Apply playback speed to audio and its clock

Status: draft

## Summary

Make the requested playback rate control audible duration and video scheduling together. Add an audio processing stage with a defined mapping between media timestamps and samples consumed by the device. Pitch behavior needs an owner decision; the recommendation is pitch-preserving speed for the mpv-compatible controls.

## Motivation

`crates/player-engine/src/sched.rs:440` changes only the clock. The fixed-ratio resampler in `crates/player-audio/src/decode.rs` and the sample-only sink write in `crates/player-engine/src/backend.rs:326` leave audio at normal duration. A faster clock therefore runs ahead of the sound. Changing the device sample rate alone would also leave queue duration, seek, and clock accounting underspecified.

## Design

Keep decoding, channel mixing, playback-rate processing, and device-rate output as explicit stages. The speed processor owns its buffered samples and delay. It must flush or drain deliberately on seek, track switch, EOF, and shutdown; it must not live as an implicit global setting in a native sink.

Distinguish three quantities: the media timestamp of decoded content, the duration of PCM queued at the device rate, and the device's consumed-sample position. Output queue watermarks remain in device seconds. Video scheduling and reported playback position remain in media seconds. A speed change needs a new mapping between those clocks, anchored at the same audible boundary at which the changed PCM begins. Do not change the clock immediately while old-rate samples remain queued.

Choose an explicit transition policy: retain the old segment's mapping until it drains, or flush and rebuild from a defined media position. Prefer segment-aware transitions that avoid dropping audible content; a simpler bounded flush/re-preroll is acceptable only if its discontinuity is measured and documented. Paused changes should take effect on resume without advancing media position.

Timestamp alignment remains a separate correction (PLAY-46): packet discontinuities must survive decoder coalescing, and delayed starts and gaps must retain their media-time meaning through the speed processor. A processor must not redefine a chunk's media duration as its shortened device duration.

Verify the behavioral boundary with a deterministic sample-counting output: audio-only duration at 0.5x, 1x, and 2x; A/V alignment across speed changes; paused changes; seek/reset; and draining the processor at EOF. Then check audible pitch, artifacts, and synchronization on CoreAudio and ALSA. A clock-only simulation cannot prove these outcomes.

## Alternatives considered

- **Rate-dependent resampling:** smaller implementation and correct duration, but pitch changes with speed. This is a viable policy only if chosen explicitly.
- **Pitch-preserving time stretching:** matches the recommended user experience, with additional processing cost, latency, and implementation or dependency work.
- **Reject non-unit speed temporarily:** an honest interim boundary if rate processing cannot be delivered yet; do not report successful speed changes while audio remains unchanged.
- **Clock-only scaling:** the current behavior; it cannot satisfy the audio contract.

## Open questions

1. **Pitch policy:** preserve pitch, or deliberately use varispeed? Recommend preserving pitch for the existing playback-speed controls. If varispeed is preferred, document it and test its expected frequency shift.
2. **Processor ownership:** implement a bounded processor in `player-audio`, or adopt a small compatible dependency after reviewing its license, latency, and supported targets? Recommend evaluating a focused dependency before committing to a custom time-stretch algorithm; this draft selects no package.
3. **Transition policy:** segment-aware queued playback or flush/re-preroll? Recommend segment-aware transitions unless a smaller implementation demonstrates an acceptable bounded discontinuity.

## Decision

Pending owner decision. This draft does not change playback behavior or authorize a dependency. Complete PLAY-45 only when audible rate and media position agree across the transition cases above; move the adopted contract into `docs/architecture.md` and remove this RFC when implemented.
