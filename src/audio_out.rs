use std::cell::Cell;
use std::ffi::c_void;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

use anyhow::{Result, bail};

use crate::decode_audio::AudioBuffer;

// ── CoreAudio FFI ──────────────────────────────────────────────────

type AudioUnit = *mut c_void;
type AudioComponent = *mut c_void;
type OSStatus = i32;
type AudioUnitPropertyID = u32;
type AudioUnitScope = u32;
type AudioUnitElement = u32;
type AudioUnitParameterID = u32;
type AudioUnitParameterValue = f32;

#[repr(C)]
struct AudioComponentDescription {
    component_type: u32,
    component_sub_type: u32,
    component_manufacturer: u32,
    component_flags: u32,
    component_flags_mask: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

type AURenderCallback = unsafe extern "C" fn(
    in_ref_con: *mut c_void,
    io_action_flags: *mut u32,
    in_time_stamp: *const c_void,
    in_bus_number: u32,
    in_number_frames: u32,
    io_data: *mut AudioBufferList,
) -> OSStatus;

#[repr(C)]
struct AURenderCallbackStruct {
    input_proc: AURenderCallback,
    input_proc_ref_con: *mut c_void,
}

#[repr(C)]
struct AudioBufferList {
    number_buffers: u32,
    buffers: [CAudioBuffer; 1], // variable-length tail
}

#[repr(C)]
struct CAudioBuffer {
    number_channels: u32,
    data_byte_size: u32,
    data: *mut c_void,
}

// Constants (FourCC as big-endian u32)
const AUDIO_UNIT_TYPE_OUTPUT: u32 = u32::from_be_bytes(*b"auou");
const AUDIO_UNIT_SUBTYPE_DEFAULT_OUTPUT: u32 = u32::from_be_bytes(*b"def ");
const AUDIO_UNIT_MANUFACTURER_APPLE: u32 = u32::from_be_bytes(*b"appl");
const AUDIO_FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");

const FORMAT_FLAG_IS_FLOAT: u32 = 1;
const FORMAT_FLAG_IS_PACKED: u32 = 8;
const FORMAT_FLAG_IS_NON_INTERLEAVED: u32 = 32;

const PROP_STREAM_FORMAT: AudioUnitPropertyID = 8;
const PROP_SET_RENDER_CALLBACK: AudioUnitPropertyID = 23;
const SCOPE_INPUT: AudioUnitScope = 1;
const SCOPE_GLOBAL: AudioUnitScope = 0;
const HAL_OUTPUT_PARAM_VOLUME: AudioUnitParameterID = 14;

unsafe extern "C" {
    fn AudioComponentFindNext(
        component: AudioComponent,
        desc: *const AudioComponentDescription,
    ) -> AudioComponent;
    fn AudioComponentInstanceNew(component: AudioComponent, out: *mut AudioUnit) -> OSStatus;
    fn AudioComponentInstanceDispose(unit: AudioUnit) -> OSStatus;
    fn AudioUnitSetProperty(
        unit: AudioUnit,
        id: AudioUnitPropertyID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        data: *const c_void,
        size: u32,
    ) -> OSStatus;
    fn AudioUnitInitialize(unit: AudioUnit) -> OSStatus;
    fn AudioUnitUninitialize(unit: AudioUnit) -> OSStatus;
    fn AudioOutputUnitStart(unit: AudioUnit) -> OSStatus;
    fn AudioOutputUnitStop(unit: AudioUnit) -> OSStatus;
    fn AudioUnitSetParameter(
        unit: AudioUnit,
        id: AudioUnitParameterID,
        scope: AudioUnitScope,
        element: AudioUnitElement,
        value: AudioUnitParameterValue,
        buffer_offset: u32,
    ) -> OSStatus;
}

// ── Lock-free SPSC ring buffer ─────────────────────────────────────

/// Ring buffer capacity: 524288 samples (~10.9s at 48kHz). Must be power of 2.
/// Sized to absorb the decode burst after a seek (decoder runs far faster than
/// real-time while the audio callback is just starting up).
const RING_CAPACITY: usize = 524288;

struct SpscRing {
    buf: *mut f32, // owned via Box<[f32]>::into_raw
    mask: usize,
    /// Written by producer, read by consumer.
    head: AtomicUsize,
    /// Written by consumer, read by producer.
    tail: AtomicUsize,
}

impl SpscRing {
    fn new() -> Self {
        let boxed: Box<[f32]> = vec![0.0f32; RING_CAPACITY].into_boxed_slice();
        let buf = Box::into_raw(boxed) as *mut f32;
        Self {
            buf,
            mask: RING_CAPACITY - 1,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    /// Available space for writing.
    fn available_write(&self) -> usize {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        RING_CAPACITY - (head.wrapping_sub(tail))
    }

    /// Available samples for reading.
    fn available_read(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        head.wrapping_sub(tail)
    }

    /// Push samples into the ring. Returns how many were actually written.
    fn push_slice(&self, src: &[f32]) -> usize {
        let avail = self.available_write();
        let n = src.len().min(avail);
        if n == 0 {
            return 0;
        }
        let head = self.head.load(Ordering::Relaxed);
        let start = head & self.mask;
        let first = n.min(RING_CAPACITY - start);
        // SAFETY: `buf` points to a valid allocation of RING_CAPACITY f32s (from
        // Box::into_raw in new()). `start` is masked to [0, RING_CAPACITY), and
        // `first + (n - first)` ≤ available_write() ≤ RING_CAPACITY, so both
        // copy regions are in bounds. Only one producer calls push_slice (player
        // thread), so no concurrent writes.
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), self.buf.add(start), first);
            if first < n {
                std::ptr::copy_nonoverlapping(src.as_ptr().add(first), self.buf, n - first);
            }
        }
        self.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }

    /// Pop samples directly into a raw pointer (the CoreAudio buffer).
    /// Returns how many were actually read.
    fn pop_to_ptr(&self, dest: *mut f32, count: usize) -> usize {
        let avail = self.available_read();
        let n = count.min(avail);
        if n == 0 {
            return 0;
        }
        let tail = self.tail.load(Ordering::Relaxed);
        let start = tail & self.mask;
        let first = n.min(RING_CAPACITY - start);
        // SAFETY: Same bounds reasoning as push_slice. `dest` is a CoreAudio-
        // provided buffer with at least `count` f32s of space. Only one consumer
        // calls pop_to_ptr (the render callback), so no concurrent reads.
        unsafe {
            std::ptr::copy_nonoverlapping(self.buf.add(start), dest, first);
            if first < n {
                std::ptr::copy_nonoverlapping(self.buf, dest.add(first), n - first);
            }
        }
        self.tail.store(tail.wrapping_add(n), Ordering::Release);
        n
    }

    /// Clear all data. Only safe when no concurrent reader/writer is active.
    fn clear(&self) {
        let head = self.head.load(Ordering::Relaxed);
        self.tail.store(head, Ordering::Relaxed);
    }
}

impl Drop for SpscRing {
    fn drop(&mut self) {
        // SAFETY: `buf` was created via Box<[f32; RING_CAPACITY]>::into_raw in
        // new(). We reconstruct the same Box<[f32]> to free the allocation.
        // &mut self guarantees exclusive access (no concurrent reader/writer).
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.buf,
                RING_CAPACITY,
            )))
        };
    }
}

// ── Callback context ──────────────────────────────────────────────

/// Context leaked into the render callback via `Box::into_raw`.
struct CallbackContext {
    rings: Vec<SpscRing>,
    /// PTS of the next sample to be consumed (microseconds).
    read_pts_us: AtomicI64,
    /// Precomputed `1_000_000.0 / sample_rate` — avoids integer division
    /// (ARM64 `sdiv`, ~10 cycles) in the audio render callback hot path.
    us_per_sample: f64,
    clock: Arc<AtomicI64>,
    channels: u32,
    /// Samples to skip on next callback (set by player, consumed by callback).
    skip_samples: AtomicUsize,
    /// Set by flush_quick(); the callback clears all rings on next invocation.
    /// This avoids the data race of clearing rings from the player thread
    /// while the callback (sole consumer) is reading from them.
    flush_pending: AtomicBool,
}

// ── Render callback (lock-free) ───────────────────────────────────

unsafe extern "C" fn render_callback(
    in_ref_con: *mut c_void,
    _io_action_flags: *mut u32,
    _in_time_stamp: *const c_void,
    _in_bus_number: u32,
    in_number_frames: u32,
    io_data: *mut AudioBufferList,
) -> OSStatus {
    // SAFETY: `in_ref_con` is a `*const CallbackContext` leaked via
    // Box::into_raw in AudioOutput::new(). It remains valid for the lifetime
    // of the AudioUnit (recovered and freed in AudioOutput::drop). The render
    // callback is the sole consumer of the SPSC rings, and CoreAudio
    // guarantees it is not called concurrently with itself.
    unsafe {
        let ctx = &*(in_ref_con as *const CallbackContext);
        let count = in_number_frames as usize;
        let ch_count = ctx.channels as usize;

        // Handle pending flush (from flush_quick). Must run in the callback
        // because only the consumer may modify tail — avoids a data race.
        if ctx.flush_pending.load(Ordering::Acquire) {
            for ring in &ctx.rings {
                let head = ring.head.load(Ordering::Acquire);
                ring.tail.store(head, Ordering::Release);
            }
            ctx.skip_samples.store(0, Ordering::Relaxed);
            ctx.flush_pending.store(false, Ordering::Release);
        }

        // Handle pending skip (requested by player thread for instant seeking).
        // The callback is the sole ring consumer, so advancing tail is safe.
        // Use fetch_sub (not swap) to preserve any excess that couldn't be
        // skipped this callback — it carries over to the next invocation.
        let skip = ctx.skip_samples.load(Ordering::Relaxed);
        if skip > 0 {
            let avail = ctx.rings.first().map_or(0, |r| r.available_read());
            let to_skip = skip.min(avail);
            if to_skip > 0 {
                for ring in &ctx.rings {
                    let tail = ring.tail.load(Ordering::Relaxed);
                    ring.tail
                        .store(tail.wrapping_add(to_skip), Ordering::Release);
                }
                let us = (to_skip as f64 * ctx.us_per_sample) as i64;
                ctx.read_pts_us.fetch_add(us, Ordering::Relaxed);
            }
            ctx.skip_samples.fetch_sub(to_skip, Ordering::Relaxed);
        }

        // Read from first ring to determine available samples
        let available = ctx.rings.first().map_or(0, |r| r.available_read());
        if available == 0 {
            zero_output(io_data, ch_count, count);
            return 0;
        }

        let to_read = count.min(available);
        // SAFETY: CoreAudio provides `io_data` with `ch_count` non-interleaved
        // buffers, each with space for `in_number_frames` f32 samples. We read
        // the buffer pointer from each AudioBuffer entry and write up to
        // `to_read` samples via pop_to_ptr, zeroing any remainder.
        for ch in 0..ch_count {
            let ab = &(*io_data).buffers.as_ptr().add(ch).read();
            let dest = ab.data as *mut f32;
            let read = ctx.rings[ch].pop_to_ptr(dest, to_read);
            // Zero any remaining frames (memset, not scalar loop)
            if read < count {
                ptr::write_bytes(dest.add(read), 0, count - read);
            }
        }

        // Advance PTS clock
        let us = (to_read as f64 * ctx.us_per_sample) as i64;
        let prev = ctx.read_pts_us.fetch_add(us, Ordering::Relaxed);
        ctx.clock.store(prev + us, Ordering::Relaxed);

        0
    }
}

fn zero_output(abl: *mut AudioBufferList, channels: usize, count: usize) {
    // SAFETY: `abl` is the CoreAudio-provided AudioBufferList with `channels`
    // non-interleaved buffers, each with space for at least `count` f32s.
    unsafe {
        for ch in 0..channels {
            let buf = &(*abl).buffers.as_ptr().add(ch).read();
            ptr::write_bytes(buf.data as *mut f32, 0, count);
        }
    }
}

// ── AudioOutput ────────────────────────────────────────────────────

pub struct AudioOutput {
    unit: AudioUnit,
    /// Leaked pointer recovered in Drop.
    ctx_ptr: *const CallbackContext,
    volume: f32,
    stopped: Cell<bool>,
    paused: Cell<bool>,
}

// SAFETY: AudioOutput is only accessed from the player thread. The AudioUnit
// and CallbackContext are thread-safe by construction (atomics + SPSC contract).
unsafe impl Send for AudioOutput {}

impl AudioOutput {
    pub fn new(sample_rate: u32, channels: u16, audio_clock: Arc<AtomicI64>) -> Result<Self> {
        let desc = AudioComponentDescription {
            component_type: AUDIO_UNIT_TYPE_OUTPUT,
            component_sub_type: AUDIO_UNIT_SUBTYPE_DEFAULT_OUTPUT,
            component_manufacturer: AUDIO_UNIT_MANUFACTURER_APPLE,
            component_flags: 0,
            component_flags_mask: 0,
        };

        // SAFETY: AudioComponentFindNext is a CoreAudio API that searches for
        // a matching audio component. NULL first arg means search from start.
        let component = unsafe { AudioComponentFindNext(std::ptr::null_mut(), &desc) };
        if component.is_null() {
            bail!("No default audio output component found");
        }

        let mut unit: AudioUnit = std::ptr::null_mut();
        // SAFETY: AudioComponentInstanceNew creates a new AudioUnit instance
        // from a valid component handle. Writes to `unit` on success.
        let mut status = unsafe { AudioComponentInstanceNew(component, &mut unit) };
        if status != 0 {
            bail!("AudioComponentInstanceNew failed: {status}");
        }

        // Non-interleaved float32 stream format
        let asbd = AudioStreamBasicDescription {
            sample_rate: sample_rate as f64,
            format_id: AUDIO_FORMAT_LINEAR_PCM,
            format_flags: FORMAT_FLAG_IS_FLOAT
                | FORMAT_FLAG_IS_PACKED
                | FORMAT_FLAG_IS_NON_INTERLEAVED,
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            channels_per_frame: channels as u32,
            bits_per_channel: 32,
            reserved: 0,
        };

        // SAFETY: `unit` is a valid AudioUnit from AudioComponentInstanceNew.
        // We pass a properly initialized ASBD struct with correct size.
        status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROP_STREAM_FORMAT,
                SCOPE_INPUT,
                0,
                &asbd as *const _ as *const c_void,
                std::mem::size_of::<AudioStreamBasicDescription>() as u32,
            )
        };
        if status != 0 {
            // SAFETY: Disposing a valid AudioUnit that failed property setup.
            unsafe { AudioComponentInstanceDispose(unit) };
            bail!("Failed to set stream format: {status}");
        }

        let rings: Vec<SpscRing> = (0..channels).map(|_| SpscRing::new()).collect();

        // Leak callback context so the render thread can reference it.
        let ctx = Box::new(CallbackContext {
            rings,
            read_pts_us: AtomicI64::new(0),
            us_per_sample: 1_000_000.0 / sample_rate as f64,
            clock: audio_clock.clone(),
            channels: channels as u32,
            skip_samples: AtomicUsize::new(0),
            flush_pending: AtomicBool::new(false),
        });
        let ctx_ptr = Box::into_raw(ctx);

        let cb = AURenderCallbackStruct {
            input_proc: render_callback,
            input_proc_ref_con: ctx_ptr as *mut c_void,
        };

        // SAFETY: `unit` is valid; `cb` contains our render function pointer
        // and the leaked context pointer. CoreAudio will call render_callback
        // with ctx_ptr as in_ref_con.
        status = unsafe {
            AudioUnitSetProperty(
                unit,
                PROP_SET_RENDER_CALLBACK,
                SCOPE_INPUT,
                0,
                &cb as *const _ as *const c_void,
                std::mem::size_of::<AURenderCallbackStruct>() as u32,
            )
        };
        if status != 0 {
            // SAFETY: Recover leaked context and dispose the unit on failure.
            unsafe {
                drop(Box::from_raw(ctx_ptr));
                AudioComponentInstanceDispose(unit);
            }
            bail!("Failed to set render callback: {status}");
        }

        // SAFETY: `unit` is fully configured; AudioUnitInitialize prepares
        // the audio processing graph for rendering.
        status = unsafe { AudioUnitInitialize(unit) };
        if status != 0 {
            // SAFETY: Recover leaked context and dispose on init failure.
            unsafe {
                drop(Box::from_raw(ctx_ptr));
                AudioComponentInstanceDispose(unit);
            }
            bail!("AudioUnitInitialize failed: {status}");
        }

        log::info!("Audio output: {sample_rate}Hz, {channels}ch (CoreAudio AudioUnit, lock-free)");

        Ok(Self {
            unit,
            ctx_ptr,
            volume: 1.0,
            stopped: Cell::new(true),
            paused: Cell::new(false),
        })
    }

    /// Push decoded audio into the ring buffers. Starts the unit if stopped.
    /// Blocks if the ring is full, waiting for the CoreAudio callback to
    /// drain enough space. This naturally throttles decode to real-time.
    pub fn schedule_buffer(&self, buf: &AudioBuffer) {
        let n = buf.samples_per_channel;
        if n == 0 {
            return;
        }
        // SAFETY: ctx_ptr is valid for the lifetime of AudioOutput (leaked in
        // new(), recovered in drop()).
        let ctx = unsafe { &*self.ctx_ptr };

        // If rings are empty, set the read PTS to this buffer's PTS
        if ctx.rings.first().is_none_or(|r| r.available_read() == 0) {
            ctx.read_pts_us.store(buf.pts_us, Ordering::Relaxed);
        }

        // Push all samples, spinning briefly if the ring is full.
        // The CoreAudio callback drains the ring in real-time, so waits
        // are bounded and short (a few ms at most).
        for ch in 0..buf.channels as usize {
            let mut offset = 0;
            let samples = &buf.planes[ch];
            while offset < samples.len() {
                let written = ctx.rings[ch].push_slice(&samples[offset..]);
                offset += written;
                if offset < samples.len() {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }

        if self.stopped.get() && !self.paused.get() {
            // SAFETY: unit is a valid, initialized AudioUnit. Starting it
            // begins invoking the render callback on the audio thread.
            unsafe { AudioOutputUnitStart(self.unit) };
            self.stopped.set(false);
        }
    }

    /// Non-blocking push: push the entire buffer if the ring has space,
    /// otherwise do nothing. Returns true if all samples were pushed.
    pub fn try_schedule_buffer(&self, buf: &AudioBuffer) -> bool {
        let n = buf.samples_per_channel;
        if n == 0 {
            return true;
        }
        // SAFETY: ctx_ptr valid for AudioOutput lifetime (see schedule_buffer).
        let ctx = unsafe { &*self.ctx_ptr };
        let avail = ctx.rings.first().map_or(0, |r| r.available_write());
        if avail < n {
            return false;
        }

        if ctx.rings.first().is_none_or(|r| r.available_read() == 0) {
            ctx.read_pts_us.store(buf.pts_us, Ordering::Relaxed);
        }

        for ch in 0..buf.channels as usize {
            ctx.rings[ch].push_slice(&buf.planes[ch]);
        }

        if self.stopped.get() && !self.paused.get() {
            // SAFETY: unit is valid and initialized (see schedule_buffer).
            unsafe { AudioOutputUnitStart(self.unit) };
            self.stopped.set(false);
        }
        true
    }

    pub fn pause(&self) {
        if !self.stopped.get() {
            // SAFETY: unit is a valid, running AudioUnit. Stop guarantees the
            // render callback is not executing when it returns.
            unsafe { AudioOutputUnitStop(self.unit) };
        }
        self.stopped.set(true);
        self.paused.set(true);
    }

    pub fn play(&self) {
        self.paused.set(false);
        if self.stopped.get() {
            // SAFETY: ctx_ptr valid for AudioOutput lifetime.
            let ctx = unsafe { &*self.ctx_ptr };
            let has_audio = ctx.rings.first().is_some_and(|r| r.available_read() > 0);
            if has_audio {
                // SAFETY: unit is valid and initialized; starting resumes the
                // render callback.
                unsafe { AudioOutputUnitStart(self.unit) };
                self.stopped.set(false);
            }
        }
    }

    pub fn stop(&self) {
        if !self.stopped.get() {
            // SAFETY: unit is a valid, running AudioUnit.
            unsafe { AudioOutputUnitStop(self.unit) };
            self.stopped.set(true);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        // SAFETY: unit is valid and initialized. HAL_OUTPUT_PARAM_VOLUME
        // is a valid parameter for the default output AudioUnit.
        unsafe {
            AudioUnitSetParameter(
                self.unit,
                HAL_OUTPUT_PARAM_VOLUME,
                SCOPE_GLOBAL,
                0,
                self.volume,
                0,
            );
        }
    }

    /// Stop the unit and clear all buffered audio.
    pub fn flush(&self) {
        // AudioOutputUnitStop guarantees the callback is not running after it returns.
        if !self.stopped.get() {
            // SAFETY: unit is valid and running.
            unsafe { AudioOutputUnitStop(self.unit) };
            self.stopped.set(true);
        }
        // SAFETY: ctx_ptr valid for AudioOutput lifetime. Callback is stopped,
        // so ring.clear() has no concurrent consumer.
        let ctx = unsafe { &*self.ctx_ptr };
        for ring in &ctx.rings {
            ring.clear();
        }
    }

    /// Request the callback to skip `samples` per channel on its next
    /// invocation. Returns immediately — the skip happens in the callback
    /// thread within ~2-5ms, respecting the SPSC ring contract.
    pub fn request_skip(&self, samples: usize) {
        // SAFETY: ctx_ptr valid for AudioOutput lifetime.
        let ctx = unsafe { &*self.ctx_ptr };
        ctx.skip_samples.fetch_add(samples, Ordering::Relaxed);
    }

    /// How many decoded samples are buffered ahead of the playback position,
    /// minus any pending skip that hasn't been consumed by the callback yet.
    pub fn buffered_samples(&self) -> usize {
        // SAFETY: ctx_ptr valid for AudioOutput lifetime.
        let ctx = unsafe { &*self.ctx_ptr };
        let pending = ctx.skip_samples.load(Ordering::Relaxed);
        ctx.rings
            .iter()
            .map(|r| r.available_read())
            .min()
            .unwrap_or(0)
            .saturating_sub(pending)
    }

    /// Update the callback's PTS tracking so the clock advances from the
    /// new position immediately, even while old audio drains from the ring.
    pub fn set_clock_position(&self, pts_us: i64) {
        // SAFETY: ctx_ptr valid for AudioOutput lifetime.
        let ctx = unsafe { &*self.ctx_ptr };
        ctx.read_pts_us.store(pts_us, Ordering::Relaxed);
        ctx.clock.store(pts_us, Ordering::Relaxed);
    }

    /// Clear buffered audio without stopping the unit. The actual clear
    /// happens in the next callback invocation (the callback is the sole
    /// ring consumer, so only it may safely modify tail pointers).
    pub fn flush_quick(&self) {
        // SAFETY: ctx_ptr valid for AudioOutput lifetime.
        let ctx = unsafe { &*self.ctx_ptr };
        ctx.flush_pending.store(true, Ordering::Release);
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        // SAFETY: unit is a valid AudioUnit; we stop, uninitialize, and
        // dispose it in the correct order. ctx_ptr was leaked via
        // Box::into_raw in new() and is recovered here. &mut self guarantees
        // no other references exist (the render callback is stopped).
        unsafe {
            AudioOutputUnitStop(self.unit);
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
            drop(Box::from_raw(self.ctx_ptr as *mut CallbackContext));
        }
    }
}
