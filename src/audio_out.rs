use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Result};
use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};

use crate::decode_audio::AudioBuffer;

/// Shared state between the player thread (producer) and the audio
/// render thread (consumer). Protected by a mutex; the render thread
/// uses try_lock to avoid blocking — outputs silence on contention.
struct SharedAudio {
    planes: Vec<VecDeque<f32>>,
    /// PTS of the next sample to be consumed (microseconds).
    read_pts_us: i64,
    sample_rate: u32,
    paused: bool,
}

// CoreAudio types for the render callback
#[repr(C)]
struct ABL {
    number_buffers: u32,
    buffers: [AB; 1], // variable length
}

#[repr(C)]
struct AB {
    _number_channels: u32,
    data_byte_size: u32,
    data: *mut f32,
}

/// Audio output via AVAudioEngine + AVAudioSourceNode (pull-based).
#[allow(dead_code)]
pub struct AudioOutput {
    engine: Retained<AnyObject>,
    source_node: Retained<AnyObject>,
    shared: Arc<Mutex<SharedAudio>>,
    audio_clock: Arc<AtomicI64>,
    channels: u32,
    volume: f32,
}

unsafe impl Send for AudioOutput {}

impl AudioOutput {
    pub fn new(sample_rate: u32, channels: u16, audio_clock: Arc<AtomicI64>) -> Result<Self> {
        let engine_cls = AnyClass::get(c"AVAudioEngine").expect("AVAudioEngine");
        let engine: Retained<AnyObject> = unsafe { msg_send![engine_cls, new] };

        let format = create_standard_format(sample_rate as f64, channels as u32)?;

        let shared = Arc::new(Mutex::new(SharedAudio {
            planes: vec![VecDeque::with_capacity(sample_rate as usize); channels as usize],
            read_pts_us: 0,
            sample_rate,
            paused: false,
        }));

        // Build render block for AVAudioSourceNode
        let shared_r = shared.clone();
        let clock_r = audio_clock.clone();
        let ch_count = channels as usize;

        let render_block = block2::RcBlock::new(
            move |is_silence: *mut Bool,
                  _timestamp: *const c_void,
                  frame_count: u32,
                  output_data: *mut c_void|
                  -> i32 {
                let count = frame_count as usize;
                let abl = output_data as *mut ABL;

                let Ok(mut audio) = shared_r.try_lock() else {
                    // Lock contended — output silence
                    unsafe { *is_silence = Bool::YES };
                    zero_output(abl, ch_count, count);
                    return 0;
                };

                if audio.paused {
                    unsafe { *is_silence = Bool::YES };
                    zero_output(abl, ch_count, count);
                    return 0;
                }

                let available = audio.planes.first().map_or(0, |p| p.len());
                if available == 0 {
                    unsafe { *is_silence = Bool::YES };
                    zero_output(abl, ch_count, count);
                    return 0;
                }

                let to_read = count.min(available);
                unsafe {
                    for ch in 0..ch_count {
                        let ab = &(*abl).buffers.as_ptr().add(ch).read();
                        let dest = ab.data;
                        let plane = &mut audio.planes[ch];
                        let (front, _) = plane.as_slices();
                        if to_read <= front.len() {
                            std::ptr::copy_nonoverlapping(front.as_ptr(), dest, to_read);
                        } else {
                            let (a, b) = plane.as_slices();
                            let first = to_read.min(a.len());
                            std::ptr::copy_nonoverlapping(a.as_ptr(), dest, first);
                            let rest = to_read - first;
                            if rest > 0 {
                                std::ptr::copy_nonoverlapping(
                                    b.as_ptr(),
                                    dest.add(first),
                                    rest,
                                );
                            }
                        }
                        // Zero remaining
                        for i in to_read..count {
                            *dest.add(i) = 0.0;
                        }
                        // Drain consumed samples
                        plane.drain(..to_read);
                    }
                }

                // Advance PTS clock
                let us = to_read as i64 * 1_000_000 / audio.sample_rate as i64;
                audio.read_pts_us += us;
                clock_r.store(audio.read_pts_us, Ordering::Relaxed);

                0
            },
        );

        // Create AVAudioSourceNode with format + render block
        let src_cls = AnyClass::get(c"AVAudioSourceNode").expect("AVAudioSourceNode");
        let src_alloc: Allocated<AnyObject> = unsafe { msg_send![src_cls, alloc] };
        let source_node: Retained<AnyObject> = unsafe {
            msg_send![src_alloc, initWithFormat: &*format, renderBlock: &*render_block]
        };

        // Attach source node and connect to main mixer
        let _: () = unsafe { msg_send![&*engine, attachNode: &*source_node] };
        let mixer: Retained<AnyObject> = unsafe { msg_send![&*engine, mainMixerNode] };
        let _: () = unsafe {
            msg_send![&*engine, connect: &*source_node, to: &*mixer, format: &*format]
        };

        // Start engine
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: Bool = unsafe { msg_send![&*engine, startAndReturnError: &mut error] };
        if !ok.as_bool() {
            bail!("Failed to start AVAudioEngine");
        }

        log::info!("Audio output: {sample_rate}Hz, {channels}ch (source node)");

        Ok(Self {
            engine,
            source_node,
            shared,
            audio_clock,
            channels: channels as u32,
            volume: 1.0,
        })
    }

    /// Push decoded audio into the ring buffer for the render thread.
    pub fn schedule_buffer(&self, buf: &AudioBuffer) {
        let n = buf.samples_per_channel;
        if n == 0 {
            return;
        }
        let mut audio = self.shared.lock().unwrap();
        // If the queue was empty, set the read PTS from this buffer
        if audio.planes.first().map_or(true, |p| p.is_empty()) {
            audio.read_pts_us = buf.pts_us;
        }
        for ch in 0..buf.channels as usize {
            let src = &buf.samples[ch * n..][..n];
            audio.planes[ch].extend(src);
        }
    }

    pub fn pause(&self) {
        if let Ok(mut audio) = self.shared.lock() {
            audio.paused = true;
        }
    }

    pub fn play(&self) {
        if let Ok(mut audio) = self.shared.lock() {
            audio.paused = false;
        }
    }

    pub fn stop(&self) {
        let _: () = unsafe { msg_send![&*self.engine, stop] };
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        let mixer: Retained<AnyObject> =
            unsafe { msg_send![&*self.engine, mainMixerNode] };
        let _: () = unsafe { msg_send![&*mixer, setOutputVolume: self.volume] };
    }

    pub fn flush(&self) {
        if let Ok(mut audio) = self.shared.lock() {
            for plane in &mut audio.planes {
                plane.clear();
            }
        }
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.stop();
    }
}

fn zero_output(abl: *mut ABL, channels: usize, count: usize) {
    unsafe {
        for ch in 0..channels {
            let ab = &(*abl).buffers.as_ptr().add(ch).read();
            std::ptr::write_bytes(ab.data, 0, count);
        }
    }
}

/// Create AVAudioFormat (non-interleaved float32).
fn create_standard_format(sample_rate: f64, channels: u32) -> Result<Retained<AnyObject>> {
    let cls = AnyClass::get(c"AVAudioFormat").expect("AVAudioFormat");
    let alloc: Allocated<AnyObject> = unsafe { msg_send![cls, alloc] };
    let format: Retained<AnyObject> = unsafe {
        msg_send![
            alloc,
            initStandardFormatWithSampleRate: sample_rate,
            channels: channels
        ]
    };
    Ok(format)
}
