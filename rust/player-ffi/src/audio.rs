use std::ffi::{c_char, c_int, CStr, CString};

#[repr(C)]
struct EngineSnapshotFfi {
    position: f64,
    duration: f64,
    volume: f32,
    paused: c_int,
    muted: c_int,
    ended: c_int,
    has_error: c_int,
    error: [c_char; 256],
}

#[repr(C)]
struct AudioProbeFfi {
    engine_ready: c_int,
    duration: f64,
    sample_rate: u32,
    channels: u16,
    title: [c_char; 128],
    artist: [c_char; 128],
    album: [c_char; 128],
    date: [c_char; 32],
}

unsafe extern "C" {
    fn player_audio_probe(
        path: *const c_char,
        out: *mut AudioProbeFfi,
        err: *mut c_char,
        err_cap: usize,
    ) -> c_int;
    fn player_audio_engine_load(
        path: *const c_char,
        null_sink: c_int,
        err: *mut c_char,
        err_cap: usize,
    ) -> *mut EngineHandle;
    fn player_audio_engine_set_paused(handle: *mut EngineHandle, paused: c_int);
    fn player_audio_engine_seek(handle: *mut EngineHandle, seconds: f64);
    fn player_audio_engine_set_volume(handle: *mut EngineHandle, volume: f32);
    fn player_audio_engine_set_muted(handle: *mut EngineHandle, muted: c_int);
    fn player_audio_engine_snapshot(
        handle: *mut EngineHandle,
        out: *mut EngineSnapshotFfi,
    ) -> c_int;
    fn player_audio_engine_destroy(handle: *mut EngineHandle);
}

enum EngineHandle {}

fn c_field(bytes: &[c_char]) -> String {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end].iter().map(|byte| *byte as u8).collect::<Vec<_>>())
        .into_owned()
}

#[derive(Debug, Clone, Default)]
pub struct Probe {
    pub engine_ready: bool,
    pub duration: f64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub date: String,
}

pub fn probe_file(path: &str) -> Result<Probe, String> {
    let path = CString::new(path).map_err(|_| "path contains a NUL".to_string())?;
    let mut out = AudioProbeFfi {
        engine_ready: 0,
        duration: 0.0,
        sample_rate: 0,
        channels: 0,
        title: [0; 128],
        artist: [0; 128],
        album: [0; 128],
        date: [0; 32],
    };
    let mut err = [0 as c_char; 256];
    // SAFETY: the probe reads the path and fills out and err for this call.
    let status = unsafe {
        player_audio_probe(path.as_ptr(), &mut out, err.as_mut_ptr(), err.len())
    };
    if status < 0 {
        return Err(c_field(&err));
    }
    Ok(Probe {
        engine_ready: out.engine_ready == 1,
        duration: out.duration,
        title: c_field(&out.title),
        artist: c_field(&out.artist),
        album: c_field(&out.album),
        date: c_field(&out.date),
    })
}

#[derive(Debug, Clone, Default)]
pub struct EngineStatus {
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub paused: bool,
    pub muted: bool,
    pub ended: bool,
    pub error: Option<String>,
}

pub struct Engine {
    handle: *mut EngineHandle,
}

impl Engine {
    pub fn load(path: &str, null_sink: bool) -> Result<Self, String> {
        let path = CString::new(path).map_err(|_| "path contains a NUL".to_string())?;
        let mut err = [0 as c_char; 256];
        // SAFETY: the engine copies the path and fills err for this call.
        let handle = unsafe {
            player_audio_engine_load(
                path.as_ptr(),
                null_sink as c_int,
                err.as_mut_ptr(),
                err.len(),
            )
        };
        if handle.is_null() {
            Err(c_field(&err))
        } else {
            Ok(Engine { handle })
        }
    }

    pub fn set_paused(&self, paused: bool) {
        // SAFETY: the handle is live; the engine owns its worker.
        unsafe { player_audio_engine_set_paused(self.handle, paused as c_int) };
    }

    pub fn seek(&self, seconds: f64) {
        // SAFETY: the handle is live.
        unsafe { player_audio_engine_seek(self.handle, seconds) };
    }

    pub fn set_volume(&self, volume: f32) {
        // SAFETY: the handle is live.
        unsafe { player_audio_engine_set_volume(self.handle, volume) };
    }

    pub fn set_muted(&self, muted: bool) {
        // SAFETY: the handle is live.
        unsafe { player_audio_engine_set_muted(self.handle, muted as c_int) };
    }

    pub fn snapshot(&self) -> Result<EngineStatus, String> {
        let mut out = EngineSnapshotFfi {
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            paused: 0,
            muted: 0,
            ended: 0,
            has_error: 0,
            error: [0; 256],
        };
        // SAFETY: the handle is live and out is writable for this call.
        let status =
            unsafe { player_audio_engine_snapshot(self.handle, &mut out) };
        if status != 0 {
            return Err("engine snapshot failed".to_string());
        }
        Ok(EngineStatus {
            position: out.position,
            duration: out.duration,
            volume: out.volume,
            paused: out.paused != 0,
            muted: out.muted != 0,
            ended: out.ended != 0,
            error: (out.has_error != 0).then(|| c_field(&out.error)),
        })
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // SAFETY: the handle is live and never used after this call.
        unsafe { player_audio_engine_destroy(self.handle) };
    }
}

