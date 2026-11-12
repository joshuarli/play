use crate::sink::AudioSink;
use crate::underrun::Underruns;
use std::cell::Cell;
use std::ffi::{c_int, c_uint, c_void, CStr, CString};
use std::ptr;

type SndPcm = c_void;
type SndPcmHwParams = c_void;

const STREAM_PLAYBACK: c_int = 0;
const ACCESS_RW_INTERLEAVED: c_int = 3;
const FORMAT_FLOAT_LE: c_int = 14;
const EPIPE: i64 = -32;
const ESTRPIPE: i64 = -86;

unsafe extern "C" {
    fn snd_pcm_open(pcm: *mut *mut SndPcm, name: *const i8, stream: c_int, mode: c_int) -> c_int;
    fn snd_pcm_close(pcm: *mut SndPcm) -> c_int;
    fn snd_pcm_hw_params_malloc(params: *mut *mut SndPcmHwParams) -> c_int;
    fn snd_pcm_hw_params_free(params: *mut SndPcmHwParams);
    fn snd_pcm_hw_params_any(pcm: *mut SndPcm, params: *mut SndPcmHwParams) -> c_int;
    fn snd_pcm_hw_params_set_access(
        pcm: *mut SndPcm,
        params: *mut SndPcmHwParams,
        access: c_int,
    ) -> c_int;
    fn snd_pcm_hw_params_set_format(
        pcm: *mut SndPcm,
        params: *mut SndPcmHwParams,
        format: c_int,
    ) -> c_int;
    fn snd_pcm_hw_params_set_rate_near(
        pcm: *mut SndPcm,
        params: *mut SndPcmHwParams,
        rate: *mut c_uint,
        dir: *mut c_int,
    ) -> c_int;
    fn snd_pcm_hw_params_set_channels(
        pcm: *mut SndPcm,
        params: *mut SndPcmHwParams,
        channels: c_uint,
    ) -> c_int;
    fn snd_pcm_hw_params_set_buffer_size_near(
        pcm: *mut SndPcm,
        params: *mut SndPcmHwParams,
        size: *mut u64,
    ) -> c_int;
    fn snd_pcm_hw_params_set_period_size_near(
        pcm: *mut SndPcm,
        params: *mut SndPcmHwParams,
        size: *mut u64,
        dir: *mut c_int,
    ) -> c_int;
    fn snd_pcm_hw_params(pcm: *mut SndPcm, params: *mut SndPcmHwParams) -> c_int;
    fn snd_pcm_prepare(pcm: *mut SndPcm) -> c_int;
    fn snd_pcm_writei(pcm: *mut SndPcm, buffer: *const c_void, frames: u64) -> i64;
    fn snd_pcm_delay(pcm: *mut SndPcm, delay: *mut i64) -> c_int;
    fn snd_pcm_drain(pcm: *mut SndPcm) -> c_int;
    fn snd_pcm_drop(pcm: *mut SndPcm) -> c_int;
    fn snd_pcm_pause(pcm: *mut SndPcm, enable: c_int) -> c_int;
    fn snd_strerror(errnum: c_int) -> *const i8;
}

fn alsa_error(status: c_int, what: &str) -> String {
    // SAFETY: snd_strerror returns a static string; null is guarded.
    let pointer = unsafe { snd_strerror(status) };
    let text = if pointer.is_null() {
        std::borrow::Cow::Borrowed("unknown ALSA error")
    } else {
        unsafe { CStr::from_ptr(pointer) }.to_string_lossy()
    };
    format!("{what} failed: {text} ({status})")
}

pub struct AlsaSink {
    pcm: *mut SndPcm,
    channels: usize,
    rate: u32,
    last_delay: Cell<u64>,
    drain_on_finish: bool,
    underruns: Underruns,
}

impl AlsaSink {
    pub fn open(device: &str, channels: usize, rate: u32) -> Result<Self, String> {
        if channels == 0 || channels > 32 || rate == 0 {
            return Err("alsa needs 1-32 channels and a positive rate".to_string());
        }
        let name = CString::new(device).map_err(|_| "alsa device name is empty".to_string())?;
        let mut pcm: *mut SndPcm = ptr::null_mut();
        // SAFETY: pcm receives the new handle on success.
        let status = unsafe { snd_pcm_open(&mut pcm, name.as_ptr(), STREAM_PLAYBACK, 0) };
        if status < 0 || pcm.is_null() {
            return Err(alsa_error(status, "snd_pcm_open"));
        }
        let result = Self::configure(pcm, channels, rate);
        match result {
            Ok(actual_rate) => Ok(AlsaSink {
                pcm,
                channels,
                rate: actual_rate,
                last_delay: Cell::new(0),
                drain_on_finish: device != "null",
                underruns: Underruns::new(),
            }),
            Err(message) => {
                // SAFETY: pcm was opened above.
                unsafe { snd_pcm_close(pcm) };
                Err(message)
            }
        }
    }

    fn configure(pcm: *mut SndPcm, channels: usize, rate: u32) -> Result<u32, String> {
        let mut params: *mut SndPcmHwParams = ptr::null_mut();
        // SAFETY: params receives the allocated struct.
        let status = unsafe { snd_pcm_hw_params_malloc(&mut params) };
        if status < 0 || params.is_null() {
            return Err(alsa_error(status, "snd_pcm_hw_params_malloc"));
        }
        // SAFETY: params is live until the free at the end of this function.
        let result = unsafe {
            let step = |status: c_int, what: &str| {
                if status < 0 {
                    Err(alsa_error(status, what))
                } else {
                    Ok(())
                }
            };
            step(snd_pcm_hw_params_any(pcm, params), "snd_pcm_hw_params_any")
                .and_then(|_| {
                    step(
                        snd_pcm_hw_params_set_access(pcm, params, ACCESS_RW_INTERLEAVED),
                        "snd_pcm_hw_params_set_access",
                    )
                })
                .and_then(|_| {
                    step(
                        snd_pcm_hw_params_set_format(pcm, params, FORMAT_FLOAT_LE),
                        "snd_pcm_hw_params_set_format",
                    )
                })
                .and_then(|_| {
                    let mut actual = rate as c_uint;
                    let mut dir = 0 as c_int;
                    step(
                        snd_pcm_hw_params_set_rate_near(pcm, params, &mut actual, &mut dir),
                        "snd_pcm_hw_params_set_rate_near",
                    )
                    .map(|_| actual)
                })
                .and_then(|actual| {
                    step(
                        snd_pcm_hw_params_set_channels(pcm, params, channels as c_uint),
                        "snd_pcm_hw_params_set_channels",
                    )
                    .map(|_| actual)
                })
                .and_then(|actual| {
                    let mut buffer = 8192u64;
                    let mut period = 2048u64;
                    let mut dir = 0 as c_int;
                    step(
                        snd_pcm_hw_params_set_buffer_size_near(pcm, params, &mut buffer),
                        "snd_pcm_hw_params_set_buffer_size_near",
                    )
                    .and_then(|_| {
                        step(
                            snd_pcm_hw_params_set_period_size_near(
                                pcm, params, &mut period, &mut dir,
                            ),
                            "snd_pcm_hw_params_set_period_size_near",
                        )
                    })
                    .map(|_| actual)
                })
                .and_then(|actual| {
                    step(snd_pcm_hw_params(pcm, params), "snd_pcm_hw_params").map(|_| actual)
                })
                .and_then(|actual| {
                    step(snd_pcm_prepare(pcm), "snd_pcm_prepare").map(|_| actual)
                })
        };
        // SAFETY: params was allocated above.
        unsafe { snd_pcm_hw_params_free(params) };
        result
    }
}

impl AudioSink for AlsaSink {
    fn write(&mut self, frames: &[f32]) -> Result<(), String> {
        if !frames.len().is_multiple_of(self.channels) {
            return Err("sink write must hold whole frames".to_string());
        }
        let mut rest = frames;
        while !rest.is_empty() {
            let count = rest.len() / self.channels;
            // SAFETY: pcm is a live playback handle; rest holds count frames.
            let written =
                unsafe { snd_pcm_writei(self.pcm, rest.as_ptr().cast(), count as u64) };
            if written == EPIPE {
                self.underruns.starved();
                // SAFETY: pcm is live.
                let status = unsafe { snd_pcm_prepare(self.pcm) };
                if status < 0 {
                    return Err(alsa_error(status, "snd_pcm_prepare after underrun"));
                }
                continue;
            }
            if written == ESTRPIPE {
                return Err("alsa stream suspended; resume playback to continue".to_string());
            }
            if written < 0 {
                return Err(alsa_error(written as c_int, "snd_pcm_writei"));
            }
            if written > 0 {
                self.underruns.data();
            }
            rest = &rest[written as usize * self.channels..];
        }
        Ok(())
    }

    fn channels(&self) -> usize {
        self.channels
    }

    fn rate(&self) -> u32 {
        self.rate
    }

    fn queued_frames(&self) -> u64 {
        let mut delay = 0i64;
        // SAFETY: pcm is live and delay is writable.
        let status = unsafe { snd_pcm_delay(self.pcm, &mut delay) };
        if status < 0 {
            return self.last_delay.get();
        }
        self.last_delay.set(delay.max(0) as u64);
        self.last_delay.get()
    }

    fn pause(&mut self) -> Result<(), String> {
        // SAFETY: pcm is live.
        let status = unsafe { snd_pcm_pause(self.pcm, 1) };
        if status < 0 {
            Err(alsa_error(status, "snd_pcm_pause"))
        } else {
            self.underruns.pause();
            Ok(())
        }
    }

    fn underruns(&self) -> u64 {
        self.underruns.count()
    }

    fn backend_name(&self) -> &'static str {
        "alsa"
    }

    fn resume(&mut self) -> Result<(), String> {
        // SAFETY: pcm is live.
        let status = unsafe { snd_pcm_pause(self.pcm, 0) };
        if status < 0 {
            Err(alsa_error(status, "snd_pcm_resume"))
        } else {
            Ok(())
        }
    }

    fn flush(&mut self) -> Result<(), String> {
        // SAFETY: pcm is live.
        let status = unsafe { snd_pcm_drop(self.pcm) };
        if status < 0 {
            return Err(alsa_error(status, "snd_pcm_drop"));
        }
        // SAFETY: pcm is live.
        let status = unsafe { snd_pcm_prepare(self.pcm) };
        if status < 0 {
            Err(alsa_error(status, "snd_pcm_prepare"))
        } else {
            self.underruns.flush();
            Ok(())
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        self.underruns.finish();
        if !self.drain_on_finish {
            return Ok(());
        }
        // SAFETY: pcm is live.
        let status = unsafe { snd_pcm_drain(self.pcm) };
        if status < 0 {
            Err(alsa_error(status, "snd_pcm_drain"))
        } else {
            Ok(())
        }
    }
}

impl Drop for AlsaSink {
    fn drop(&mut self) {
        // SAFETY: pcm is live until close returns.
        unsafe {
            snd_pcm_drop(self.pcm);
            snd_pcm_close(self.pcm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "static-pie test binaries cannot call into the container's shared libasound; run with RUSTFLAGS=\"-C target-feature=-crt-static\""]
    fn null_device_plays_and_reports() {
        let mut sink = AlsaSink::open("null", 2, 48000).expect("null device opens");
        assert_eq!(sink.channels(), 2);
        assert!(sink.rate() >= 8000);
        let tone: Vec<f32> = (0..4800)
            .flat_map(|frame| {
                let sample =
                    (2.0 * std::f32::consts::PI * 440.0 * frame as f32 / sink.rate() as f32).sin()
                        * 0.1;
                [sample, sample]
            })
            .collect();
        sink.write(&tone).expect("write works");
        let _ = sink.queued_frames();
        let pause = sink.pause();
        let _ = sink.resume();
        sink.flush().expect("flush works");
        sink.finish().expect("finish works");
        let _ = pause;
    }

    #[test]
    #[ignore = "static-pie test binaries cannot call into the container's shared libasound; run with RUSTFLAGS=\"-C target-feature=-crt-static\""]
    fn missing_device_fails_loudly() {
        let err = match AlsaSink::open("no-such-device-xyz", 2, 48000) {
            Ok(_) => panic!("missing device must fail"),
            Err(err) => err,
        };
        assert!(err.contains("snd_pcm_open"), "unexpected error: {err}");
    }
}
