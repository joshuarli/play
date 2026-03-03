use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};
use objc2::msg_send;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject, Bool};

use crate::decode_audio::AudioBuffer;

/// Audio output via AVAudioEngine.
#[allow(dead_code)]
pub struct AudioOutput {
    engine: Retained<AnyObject>,
    player_node: Retained<AnyObject>,
    audio_clock: Arc<AtomicI64>,
    sample_rate: f64,
    channels: u32,
    volume: f32,
}

unsafe impl Send for AudioOutput {}

impl AudioOutput {
    pub fn new(sample_rate: u32, channels: u16, audio_clock: Arc<AtomicI64>) -> Result<Self> {
        let engine_cls =
            AnyClass::get(c"AVAudioEngine").expect("AVAudioEngine not found");
        let player_cls =
            AnyClass::get(c"AVAudioPlayerNode").expect("AVAudioPlayerNode not found");

        log::debug!("AudioOutput: creating AVAudioEngine...");
        let engine: Retained<AnyObject> = unsafe { msg_send![engine_cls, new] };
        log::debug!("AudioOutput: creating AVAudioPlayerNode...");
        let player_node: Retained<AnyObject> = unsafe { msg_send![player_cls, new] };
        log::debug!("AudioOutput: AVAudioPlayerNode created");

        // Attach player node to engine
        log::debug!("AudioOutput: attaching node to engine...");
        let _: () = unsafe { msg_send![&*engine, attachNode: &*player_node] };
        log::debug!("AudioOutput: node attached");

        // Create audio format (non-interleaved float32)
        let format = create_standard_format(sample_rate as f64, channels as u32)?;

        // Connect player node to main mixer
        let mixer: Retained<AnyObject> =
            unsafe { msg_send![&*engine, mainMixerNode] };
        let _: () = unsafe {
            msg_send![&*engine, connect: &*player_node, to: &*mixer, format: &*format]
        };

        // Start engine
        log::debug!("AudioOutput: starting engine...");
        let mut error: *mut AnyObject = std::ptr::null_mut();
        let ok: Bool = unsafe { msg_send![&*engine, startAndReturnError: &mut error] };
        if !ok.as_bool() {
            bail!("Failed to start AVAudioEngine");
        }
        log::debug!("AudioOutput: engine started");

        // Start player node
        let _: () = unsafe { msg_send![&*player_node, play] };

        log::info!("Audio output: {sample_rate}Hz, {channels}ch");

        Ok(Self {
            engine,
            player_node,
            audio_clock,
            sample_rate: sample_rate as f64,
            channels: channels as u32,
            volume: 1.0,
        })
    }

    /// Schedule an audio buffer for playback.
    pub fn schedule_buffer(&self, buf: &AudioBuffer) {
        let frame_count = buf.samples.len() as u32 / buf.channels as u32;
        if frame_count == 0 {
            return;
        }

        let format = match create_standard_format(buf.sample_rate as f64, buf.channels as u32) {
            Ok(f) => f,
            Err(e) => {
                log::error!("Failed to create audio format: {e}");
                return;
            }
        };

        // Create AVAudioPCMBuffer
        let pcm_buf_cls =
            AnyClass::get(c"AVAudioPCMBuffer").expect("AVAudioPCMBuffer not found");
        let pcm_alloc: Allocated<AnyObject> = unsafe { msg_send![pcm_buf_cls, alloc] };
        let pcm_buf: Retained<AnyObject> = unsafe {
            msg_send![pcm_alloc, initWithPCMFormat: &*format, frameCapacity: frame_count]
        };

        // Set frame length
        let _: () = unsafe { msg_send![&*pcm_buf, setFrameLength: frame_count] };

        // Copy interleaved samples to non-interleaved buffer
        let float_data: *mut *mut f32 = unsafe { msg_send![&*pcm_buf, floatChannelData] };
        if !float_data.is_null() {
            let ch_count = buf.channels as usize;
            for ch in 0..ch_count {
                let dest = unsafe { *float_data.add(ch) };
                if dest.is_null() {
                    continue;
                }
                for i in 0..frame_count as usize {
                    unsafe {
                        *dest.add(i) = buf.samples[i * ch_count + ch];
                    }
                }
            }
        }

        // Schedule with completion handler that updates the clock
        let end_pts = buf.pts_us
            + (frame_count as i64 * 1_000_000 / buf.sample_rate as i64);
        let clock = self.audio_clock.clone();
        let completion = block2::RcBlock::new(move || {
            clock.store(end_pts, Ordering::Relaxed);
        });

        let _: () = unsafe {
            msg_send![
                &*self.player_node,
                scheduleBuffer: &*pcm_buf,
                completionHandler: &*completion
            ]
        };

        // Update clock to start of this buffer
        self.audio_clock.store(buf.pts_us, Ordering::Relaxed);
    }

    pub fn pause(&self) {
        let _: () = unsafe { msg_send![&*self.player_node, pause] };
    }

    pub fn play(&self) {
        let _: () = unsafe { msg_send![&*self.player_node, play] };
    }

    pub fn stop(&self) {
        let _: () = unsafe { msg_send![&*self.player_node, stop] };
        let _: () = unsafe { msg_send![&*self.engine, stop] };
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        let _: () = unsafe { msg_send![&*self.player_node, setVolume: self.volume] };
    }

    #[allow(dead_code)]
    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn flush(&self) {
        let _: () = unsafe { msg_send![&*self.player_node, stop] };
        let _: () = unsafe { msg_send![&*self.player_node, play] };
    }
}

impl Drop for AudioOutput {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Create AVAudioFormat using the standard format (non-interleaved float32).
fn create_standard_format(sample_rate: f64, channels: u32) -> Result<Retained<AnyObject>> {
    let cls = AnyClass::get(c"AVAudioFormat").expect("AVAudioFormat not found");

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
