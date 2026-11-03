use crate::session::DecoderSession;
use player_media::{MediaSession, StreamKind};
use std::ffi::{c_char, c_int, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

#[repr(C)]
pub struct AudioProbeFfi {
    pub engine_ready: c_int,
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub title: [c_char; 128],
    pub artist: [c_char; 128],
    pub album: [c_char; 128],
    pub date: [c_char; 32],
}

impl AudioProbeFfi {
    fn empty() -> Self {
        AudioProbeFfi {
            engine_ready: 0,
            duration: 0.0,
            sample_rate: 0,
            channels: 0,
            title: [0; 128],
            artist: [0; 128],
            album: [0; 128],
            date: [0; 32],
        }
    }
}

fn copy_string(out: &mut [c_char], value: &str) {
    let bytes = value.as_bytes();
    let length = bytes.len().min(out.len().saturating_sub(1));
    for (slot, byte) in out[..length].iter_mut().zip(bytes[..length].iter()) {
        *slot = *byte as c_char;
    }
    out[length] = 0;
}

fn probe_metadata(session: &MediaSession) -> (String, String, String, String) {
    let entries = session.metadata();
    let find = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| {
                entries
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(key))
                    .map(|(_, value)| value.clone())
            })
            .unwrap_or_default()
    };
    (
        find(&["Title"]),
        find(&["Artist", "album_artist", "AlbumArtist"]),
        find(&["Album"]),
        find(&["Date", "Year"]),
    )
}

pub(crate) unsafe fn write_ffi_error(err: *mut c_char, cap: usize, message: &str) {
    if err.is_null() || cap == 0 {
        return;
    }
    let bytes = message.as_bytes();
    let length = bytes.len().min(cap - 1);
    // SAFETY: err has cap writable bytes.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), err.cast::<u8>(), length);
        *err.add(length) = 0;
    }
}

/// Returns 1 when the file is audio-only and the Rust engine can play it,
/// 0 when the C engine should keep it, and a negative value on probe error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_probe(
    path: *const c_char,
    out: *mut AudioProbeFfi,
    err: *mut c_char,
    err_cap: usize,
) -> c_int {
    if path.is_null() {
        return -1;
    }
    let write_out = !out.is_null();
    // SAFETY: path is a valid NUL-terminated string for this call.
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<AudioProbeFfi, String> {
        let session = MediaSession::open(&path).map_err(|err| err.to_string())?;
        let mut audio = None;
        for stream in session.streams() {
            match stream.kind {
                StreamKind::Audio if audio.is_none() => {
                    audio = Some(stream);
                }
                _ => {}
            }
        }
        let stream = match audio {
            Some(stream) => stream,
            None => return Ok(AudioProbeFfi::empty()),
        };
        let engine_ready = DecoderSession::open(&stream).is_ok();
        let (title, artist, album, date) = probe_metadata(&session);
        let mut probe = AudioProbeFfi::empty();
        if engine_ready {
            probe.engine_ready = 1;
        }
        probe.duration = session.duration_seconds().unwrap_or(0.0).max(0.0);
        probe.sample_rate = stream.sample_rate.max(0) as u32;
        probe.channels = stream.channels.clamp(0, u16::MAX as i32) as u16;
        copy_string(&mut probe.title, &title);
        copy_string(&mut probe.artist, &artist);
        copy_string(&mut probe.album, &album);
        copy_string(&mut probe.date, &date);
        Ok(probe)
    }));
    match result {
        Ok(Ok(probe)) => {
            let ready = probe.engine_ready;
            if write_out {
                // SAFETY: out points to a valid probe struct for this call.
                unsafe { ptr::write(out, probe) };
            }
            if ready == 1 { 1 } else { 0 }
        }
        Ok(Err(message)) => {
            // SAFETY: err has err_cap writable bytes per the function contract.
            unsafe { write_ffi_error(err, err_cap, &message) };
            -2
        }
        Err(_) => {
            // SAFETY: err has err_cap writable bytes per the function contract.
            unsafe { write_ffi_error(err, err_cap, "panic in player_audio_probe") };
            -3
        }
    }
}
