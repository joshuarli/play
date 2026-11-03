#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(target_os = "linux")]
pub mod backend_alsa;
#[cfg(target_os = "macos")]
pub mod backend_coreaudio;
pub mod clock;
pub mod decode;
pub mod engine;
pub mod mix;
pub mod pipeline;
pub mod resample;
pub mod session;
pub mod sink;

pub fn open_default_sink(
    channels: usize,
    rate: u32,
    channel_mask: u64,
) -> Result<Box<dyn sink::AudioSink>, String> {
    #[cfg(target_os = "macos")]
    {
        backend_coreaudio::CoreAudioSink::open(channels, rate, channel_mask)
            .map(|sink| Box::new(sink) as Box<dyn sink::AudioSink>)
    }
    #[cfg(target_os = "linux")]
    {
        let _ = channel_mask;
        backend_alsa::AlsaSink::open("default", channels, rate)
            .map(|sink| Box::new(sink) as Box<dyn sink::AudioSink>)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (channels, rate, channel_mask);
        Err("no native audio backend on this platform".to_string())
    }
}

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;

use symphonia::core::audio::{Channels, Position};
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_ALAC, CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_OPUS,
    CODEC_ID_PCM_F32LE, CODEC_ID_PCM_F64LE, CODEC_ID_PCM_S16LE, CODEC_ID_PCM_S24LE,
    CODEC_ID_PCM_S32LE, CODEC_ID_PCM_S8, CODEC_ID_PCM_U8, CODEC_ID_VORBIS,
};
use symphonia::core::codecs::audio::{
    AudioCodecId, AudioCodecParameters, AudioDecoder, AudioDecoderOptions,
};
use symphonia::core::packet::Packet;
use symphonia::core::units::{Duration, Timestamp};

const ERR_ARG: i32 = 1;
const ERR_UNSUPPORTED: i32 = 2;
const ERR_DECODE: i32 = 3;
const ERR_PANIC: i32 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlayerAudioParams {
    pub codec: u32,
    pub sample_rate: u32,
    pub channels: u32,
    pub channel_mask: u64,
    pub bits_per_sample: u32,
    pub extradata: *const u8,
    pub extradata_size: usize,
}

pub struct PlayerAudioDecoder {
    decoder: Box<dyn AudioDecoder>,
    pcm: Vec<f32>,
}

impl PlayerAudioDecoder {
    fn decode(&mut self, data: &[u8], pts: i64, dur: i64) -> Result<(usize, usize, u32), String> {
        let packet = Packet::new(
            1,
            Timestamp::from(pts),
            Duration::from(dur.max(0) as u64),
            data.to_vec(),
        );
        let buffer = self.decoder.decode(&packet).map_err(|err| err.to_string())?;
        let channels = buffer.spec().channels().count();
        let sample_rate = buffer.spec().rate();
        let frames = buffer.frames();
        buffer.copy_to_vec_interleaved::<f32>(&mut self.pcm);
        Ok((frames, channels, sample_rate))
    }
}

fn codec_id(codec: u32) -> Option<AudioCodecId> {
    match codec {
        0 => Some(CODEC_ID_AAC),
        1 => Some(CODEC_ID_ALAC),
        2 => Some(CODEC_ID_FLAC),
        3 => Some(CODEC_ID_MP3),
        4 => Some(CODEC_ID_OPUS),
        5 => Some(CODEC_ID_VORBIS),
        6 => Some(CODEC_ID_PCM_S8),
        7 => Some(CODEC_ID_PCM_U8),
        8 => Some(CODEC_ID_PCM_S16LE),
        9 => Some(CODEC_ID_PCM_S24LE),
        10 => Some(CODEC_ID_PCM_S32LE),
        11 => Some(CODEC_ID_PCM_F32LE),
        12 => Some(CODEC_ID_PCM_F64LE),
        _ => None,
    }
}

fn channels_from_mask(mask: u64, count: u32) -> Option<Channels> {
    if mask != 0 {
        Position::from_bits(mask).map(Channels::Positioned)
    } else if count > 0 && count <= u16::MAX as u32 {
        Some(Channels::Discrete(count as u16))
    } else {
        None
    }
}

unsafe fn params_from_c(params: &PlayerAudioParams) -> Result<AudioCodecParameters, String> {
    let codec = codec_id(params.codec)
        .ok_or_else(|| format!("unsupported Symphonia codec {}", params.codec))?;
    let mut codec_params = AudioCodecParameters::new();
    codec_params.for_codec(codec);
    if params.sample_rate > 0 {
        codec_params.with_sample_rate(params.sample_rate);
    }
    if params.channels > 0 {
        let channels = channels_from_mask(params.channel_mask, params.channels)
            .ok_or_else(|| "invalid channel layout".to_string())?;
        codec_params.with_channels(channels);
    }
    if params.bits_per_sample > 0 {
        codec_params.with_bits_per_sample(params.bits_per_sample);
    }
    if params.extradata_size > 0 {
        if params.extradata.is_null() {
            return Err("null extradata".to_string());
        }
        // SAFETY: The caller guarantees extradata points to extradata_size readable bytes.
        let extradata = unsafe { slice::from_raw_parts(params.extradata, params.extradata_size) };
        codec_params.with_extra_data(extradata.to_vec().into_boxed_slice());
    }
    Ok(codec_params)
}

unsafe fn write_error(err: *mut std::os::raw::c_char, cap: usize, message: &str) {
    if err.is_null() || cap == 0 {
        return;
    }
    let bytes = message.as_bytes();
    let len = bytes.len().min(cap - 1);
    // SAFETY: The caller provides a writable error buffer of at least cap bytes.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), err.cast::<u8>(), len);
        *err.add(len) = 0;
    }
}

unsafe fn write_code(err_code: *mut i32, code: i32) {
    if !err_code.is_null() {
        // SAFETY: The caller provides a writable int.
        unsafe { *err_code = code };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_create(
    params: *const PlayerAudioParams,
    err: *mut std::os::raw::c_char,
    err_cap: usize,
    err_code: *mut i32,
) -> *mut PlayerAudioDecoder {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<*mut PlayerAudioDecoder, (i32, String)> {
        if params.is_null() {
            return Err((ERR_ARG, "null params".to_string()));
        }
        // SAFETY: params is non-null and points to a valid PlayerAudioParams for this call.
        let params = unsafe { &*params };
        // SAFETY: Caller guarantees the extradata pointer and size are valid.
        let codec_params = unsafe { params_from_c(params) }
            .map_err(|message| (ERR_UNSUPPORTED, message))?;
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
            .map_err(|message| (ERR_UNSUPPORTED, message.to_string()))?;
        Ok(Box::into_raw(Box::new(PlayerAudioDecoder {
            decoder,
            pcm: Vec::new(),
        })))
    }));
    match result {
        Ok(Ok(decoder)) => {
            // SAFETY: err_code is either null or points to a writable int for this call.
            unsafe { write_code(err_code, 0) };
            decoder
        }
        Ok(Err((code, message))) => {
            // SAFETY: err, err_cap, and err_code came from the caller.
            unsafe {
                write_error(err, err_cap, &message);
                write_code(err_code, code);
            }
            ptr::null_mut()
        }
        Err(_) => {
            // SAFETY: err, err_cap, and err_code came from the caller.
            unsafe {
                write_error(err, err_cap, "panic in player_audio_create");
                write_code(err_code, ERR_PANIC);
            }
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_decode(
    decoder: *mut PlayerAudioDecoder,
    data: *const u8,
    len: usize,
    pts: i64,
    dur: i64,
    samples: *mut *const f32,
    frames: *mut u32,
    channels: *mut u32,
    sample_rate: *mut u32,
    err: *mut std::os::raw::c_char,
    err_cap: usize,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), (i32, String)> {
        if decoder.is_null() || samples.is_null() || frames.is_null()
            || channels.is_null() || sample_rate.is_null()
        {
            return Err((ERR_ARG, "null output or decoder".to_string()));
        }
        let decoder = unsafe { &mut *decoder };
        let input = if len == 0 {
            &[][..]
        } else {
            if data.is_null() {
                return Err((ERR_ARG, "null packet data".to_string()));
            }
            // SAFETY: The caller guarantees data points to len readable bytes.
            unsafe { slice::from_raw_parts(data, len) }
        };
        let (frame_count, channel_count, rate) = match decoder.decode(input, pts, dur) {
            Ok(values) => values,
            Err(message) => return Err((ERR_DECODE, message)),
        };
        // SAFETY: The output pointers were checked non-null above; the sample pointer remains
        // valid until the next decode or destroy call on this decoder.
        unsafe {
            *samples = if decoder.pcm.is_empty() { ptr::null() } else { decoder.pcm.as_ptr() };
            *frames = frame_count as u32;
            *channels = channel_count as u32;
            *sample_rate = rate;
        }
        Ok(())
    }));
    match result {
        Ok(Ok(())) => 0,
        Ok(Err((code, message))) => {
            // SAFETY: err and err_cap came from the caller.
            unsafe { write_error(err, err_cap, &message) };
            code
        }
        Err(_) => {
            // SAFETY: err and err_cap came from the caller.
            unsafe { write_error(err, err_cap, "panic in player_audio_decode") };
            ERR_PANIC
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_reset(decoder: *mut PlayerAudioDecoder) {
    if decoder.is_null() {
        return;
    }
    // SAFETY: decoder is non-null and was created by player_audio_create.
    let decoder = unsafe { &mut *decoder };
    let _ = catch_unwind(AssertUnwindSafe(|| {
        decoder.decoder.reset();
        decoder.pcm.clear();
    }));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_destroy(decoder: *mut PlayerAudioDecoder) {
    if decoder.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: decoder was created by player_audio_create and is not used afterwards.
        drop(unsafe { Box::from_raw(decoder) });
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm_s16le_params() -> PlayerAudioParams {
        PlayerAudioParams {
            codec: 8,
            sample_rate: 48000,
            channels: 2,
            channel_mask: (1 << 0) | (1 << 1),
            bits_per_sample: 16,
            extradata: ptr::null(),
            extradata_size: 0,
        }
    }

    #[test]
    fn pcm_s16le_decodes_to_f32() {
        let samples: [i16; 8] = [0, 32767, -32768, 16384, 0, 0, 0, 0];
        let mut bytes = Vec::new();
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let mut err = [0i8; 128];
        let mut code = -1;
        // SAFETY: The params, error buffer, and status are valid for the call.
        let decoder = unsafe {
            player_audio_create(&pcm_s16le_params(), err.as_mut_ptr(), err.len(), &mut code)
        };
        assert!(!decoder.is_null());
        assert_eq!(code, 0);
        let mut output: *const f32 = ptr::null();
        let mut frames = 0u32;
        let mut channels = 0u32;
        let mut rate = 0u32;
        // SAFETY: All pointers are valid for the call and output remains valid until destroy.
        let status = unsafe {
            player_audio_decode(
                decoder,
                bytes.as_ptr(),
                bytes.len(),
                0,
                0,
                &mut output,
                &mut frames,
                &mut channels,
                &mut rate,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        assert_eq!(status, 0);
        assert_eq!(frames, 4);
        assert_eq!(channels, 2);
        assert_eq!(rate, 48000);
        // SAFETY: output points to frames * channels floats in the decoder-owned buffer.
        let decoded = unsafe { slice::from_raw_parts(output, (frames * channels) as usize) };
        assert!((decoded[0] - 0.0).abs() < 1e-6);
        assert!((decoded[1] - 1.0).abs() < 1e-4);
        assert!((decoded[2] + 1.0).abs() < 1e-4);
        // SAFETY: decoder was created above and is not used afterwards.
        unsafe { player_audio_destroy(decoder) };
    }

    #[test]
    fn unknown_codec_is_rejected() {
        let mut params = pcm_s16le_params();
        params.codec = 99;
        let mut err = [0i8; 128];
        let mut code = 0;
        // SAFETY: The params, error buffer, and status are valid for the call.
        let decoder = unsafe {
            player_audio_create(&params, err.as_mut_ptr(), err.len(), &mut code)
        };
        assert!(decoder.is_null());
        assert_eq!(code, ERR_UNSUPPORTED);
    }
}
