use std::cell::Cell;
use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

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

// ── Shared state ───────────────────────────────────────────────────

struct SharedAudio {
    planes: Vec<VecDeque<f32>>,
    /// PTS of the next sample to be consumed (microseconds).
    read_pts_us: i64,
    sample_rate: u32,
}

/// Context leaked into the render callback via `Box::into_raw`.
struct CallbackContext {
    shared: Arc<Mutex<SharedAudio>>,
    clock: Arc<AtomicI64>,
    channels: u32,
}

// ── Render callback ────────────────────────────────────────────────

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

        let Ok(mut audio) = ctx.shared.try_lock() else {
            zero_output(io_data, ch_count, count);
            return 0;
        };

        let available = audio.planes.first().map_or(0, |p| p.len());
        if available == 0 {
            zero_output(io_data, ch_count, count);
            return 0;
        }

        let to_read = count.min(available);
        for ch in 0..ch_count {
            let buf = &(*io_data).buffers.as_ptr().add(ch).read();
            let dest = buf.data as *mut f32;
            let plane = &mut audio.planes[ch];
            let (front, back) = plane.as_slices();
            if to_read <= front.len() {
                std::ptr::copy_nonoverlapping(front.as_ptr(), dest, to_read);
            } else {
                let first = front.len();
                std::ptr::copy_nonoverlapping(front.as_ptr(), dest, first);
                let rest = to_read - first;
                if rest > 0 {
                    std::ptr::copy_nonoverlapping(back.as_ptr(), dest.add(first), rest);
                }
            }
            for i in to_read..count {
                *dest.add(i) = 0.0;
            }
            plane.drain(..to_read);
        }

        // Advance PTS clock
        let us = to_read as i64 * 1_000_000 / audio.sample_rate as i64;
        audio.read_pts_us += us;
        ctx.clock.store(audio.read_pts_us, Ordering::Relaxed);

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

#[allow(dead_code)]
pub struct AudioOutput {
    unit: AudioUnit,
    shared: Arc<Mutex<SharedAudio>>,
    audio_clock: Arc<AtomicI64>,
    /// Leaked pointer recovered in Drop.
    ctx_ptr: *const CallbackContext,
    channels: u32,
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

        let shared = Arc::new(Mutex::new(SharedAudio {
            planes: vec![VecDeque::with_capacity(sample_rate as usize); channels as usize],
            read_pts_us: 0,
            sample_rate,
        }));

        // Leak callback context so the render thread can reference it.
        let ctx = Box::new(CallbackContext {
            shared: shared.clone(),
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

        log::info!("Audio output: {sample_rate}Hz, {channels}ch (CoreAudio AudioUnit)");

        Ok(Self {
            unit,
            shared,
            audio_clock,
            ctx_ptr,
            channels: channels as u32,
            volume: 1.0,
            stopped: Cell::new(true),
            paused: Cell::new(false),
        })
    }

    /// Push decoded audio into the ring buffer. Starts the unit if stopped.
    pub fn schedule_buffer(&self, buf: &AudioBuffer) {
        let n = buf.samples_per_channel;
        if n == 0 {
            return;
        }
        let mut audio = self.shared.lock().unwrap();
        if audio.planes.first().map_or(true, |p| p.is_empty()) {
            audio.read_pts_us = buf.pts_us;
        }
        for ch in 0..buf.channels as usize {
            let src = &buf.samples[ch * n..][..n];
            audio.planes[ch].extend(src);
        }
        drop(audio);

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
            let has_audio = {
                let audio = self.shared.lock().unwrap();
                audio.planes.first().map_or(false, |p| !p.is_empty())
            };
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
        if !self.stopped.get() {
            unsafe { AudioOutputUnitStop(self.unit) };
            self.stopped.set(true);
        }
        if let Ok(mut audio) = self.shared.lock() {
            for plane in &mut audio.planes {
                plane.clear();
            }
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
