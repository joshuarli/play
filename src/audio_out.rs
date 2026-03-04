use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};

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
    buf: *mut f32,
    mask: usize,
    /// Written by producer, read by consumer.
    head: AtomicUsize,
    /// Written by consumer, read by producer.
    tail: AtomicUsize,
}

impl SpscRing {
    fn new() -> Self {
        let layout = std::alloc::Layout::from_size_align(
            RING_CAPACITY * std::mem::size_of::<f32>(),
            std::mem::align_of::<f32>(),
        )
        .unwrap();
        let buf = unsafe { std::alloc::alloc_zeroed(layout) as *mut f32 };
        assert!(!buf.is_null(), "ring buffer allocation failed");
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
        let layout = std::alloc::Layout::from_size_align(
            RING_CAPACITY * std::mem::size_of::<f32>(),
            std::mem::align_of::<f32>(),
        )
        .unwrap();
        unsafe { std::alloc::dealloc(self.buf as *mut u8, layout) };
    }
}

// ── Callback context ──────────────────────────────────────────────

/// Context leaked into the render callback via `Box::into_raw`.
struct CallbackContext {
    rings: Vec<SpscRing>,
    /// PTS of the next sample to be consumed (microseconds).
    read_pts_us: AtomicI64,
    sample_rate: u32,
    clock: Arc<AtomicI64>,
    channels: u32,
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
    unsafe {
        let ctx = &*(in_ref_con as *const CallbackContext);
        let count = in_number_frames as usize;
        let ch_count = ctx.channels as usize;

        // Read from first ring to determine available samples
        let available = ctx.rings.first().map_or(0, |r| r.available_read());
        if available == 0 {
            zero_output(io_data, ch_count, count);
            return 0;
        }

        let to_read = count.min(available);
        for ch in 0..ch_count {
            let ab = &(*io_data).buffers.as_ptr().add(ch).read();
            let dest = ab.data as *mut f32;
            let read = ctx.rings[ch].pop_to_ptr(dest, to_read);
            // Zero any remaining frames
            for i in read..count {
                *dest.add(i) = 0.0;
            }
        }

        // Advance PTS clock
        let us = to_read as i64 * 1_000_000 / ctx.sample_rate as i64;
        let prev = ctx.read_pts_us.fetch_add(us, Ordering::Relaxed);
        ctx.clock.store(prev + us, Ordering::Relaxed);

        0
    }
}

fn zero_output(abl: *mut AudioBufferList, channels: usize, count: usize) {
    unsafe {
        for ch in 0..channels {
            let buf = &(*abl).buffers.as_ptr().add(ch).read();
            std::ptr::write_bytes(buf.data as *mut f32, 0, count);
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

        let component = unsafe { AudioComponentFindNext(std::ptr::null_mut(), &desc) };
        if component.is_null() {
            bail!("No default audio output component found");
        }

        let mut unit: AudioUnit = std::ptr::null_mut();
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
            unsafe { AudioComponentInstanceDispose(unit) };
            bail!("Failed to set stream format: {status}");
        }

        let rings: Vec<SpscRing> = (0..channels).map(|_| SpscRing::new()).collect();

        // Leak callback context so the render thread can reference it.
        let ctx = Box::new(CallbackContext {
            rings,
            read_pts_us: AtomicI64::new(0),
            sample_rate,
            clock: audio_clock.clone(),
            channels: channels as u32,
        });
        let ctx_ptr = Box::into_raw(ctx);

        let cb = AURenderCallbackStruct {
            input_proc: render_callback,
            input_proc_ref_con: ctx_ptr as *mut c_void,
        };

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
            unsafe {
                drop(Box::from_raw(ctx_ptr));
                AudioComponentInstanceDispose(unit);
            }
            bail!("Failed to set render callback: {status}");
        }

        status = unsafe { AudioUnitInitialize(unit) };
        if status != 0 {
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
    pub fn schedule_buffer(&self, buf: &AudioBuffer) {
        let n = buf.samples_per_channel;
        if n == 0 {
            return;
        }
        let ctx = unsafe { &*self.ctx_ptr };

        // If rings are empty, set the read PTS to this buffer's PTS
        if ctx.rings.first().map_or(true, |r| r.available_read() == 0) {
            ctx.read_pts_us.store(buf.pts_us, Ordering::Relaxed);
        }

        for ch in 0..buf.channels as usize {
            ctx.rings[ch].push_slice(&buf.planes[ch]);
        }

        if self.stopped.get() && !self.paused.get() {
            unsafe { AudioOutputUnitStart(self.unit) };
            self.stopped.set(false);
        }
    }

    pub fn pause(&self) {
        if !self.stopped.get() {
            unsafe { AudioOutputUnitStop(self.unit) };
        }
        self.stopped.set(true);
        self.paused.set(true);
    }

    pub fn play(&self) {
        self.paused.set(false);
        if self.stopped.get() {
            let ctx = unsafe { &*self.ctx_ptr };
            let has_audio = ctx.rings.first().map_or(false, |r| r.available_read() > 0);
            if has_audio {
                unsafe { AudioOutputUnitStart(self.unit) };
                self.stopped.set(false);
            }
        }
    }

    pub fn stop(&self) {
        if !self.stopped.get() {
            unsafe { AudioOutputUnitStop(self.unit) };
            self.stopped.set(true);
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
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
            unsafe { AudioOutputUnitStop(self.unit) };
            self.stopped.set(true);
        }
        let ctx = unsafe { &*self.ctx_ptr };
        for ring in &ctx.rings {
            ring.clear();
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        unsafe {
            AudioOutputUnitStop(self.unit);
            AudioUnitUninitialize(self.unit);
            AudioComponentInstanceDispose(self.unit);
            drop(Box::from_raw(self.ctx_ptr as *mut CallbackContext));
        }
    }
}
