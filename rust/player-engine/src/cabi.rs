use crate::backend::SchedulerEngine;
use player_audio::sink::SinkKind;
use std::ffi::{c_char, c_int, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

#[repr(C)]
pub struct EngineSnapshotFfi {
    position: f64,
    duration: f64,
    volume: f32,
    paused: c_int,
    muted: c_int,
    ended: c_int,
    has_error: c_int,
    error: [c_char; 256],
}

fn copy_string(out: &mut [c_char], value: &str) {
    let bytes = value.as_bytes();
    let length = bytes.len().min(out.len().saturating_sub(1));
    for (slot, byte) in out[..length].iter_mut().zip(bytes[..length].iter()) {
        *slot = *byte as c_char;
    }
    out[length] = 0;
}

unsafe fn write_ffi_error(err: *mut c_char, cap: usize, message: &str) {
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

fn engine_from_c_path(path: *const c_char) -> Result<String, String> {
    if path.is_null() {
        return Err("null path".to_string());
    }
    // SAFETY: path is NUL-terminated for this call.
    Ok(unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned())
}

/// # Safety
///
/// `path` must be NUL-terminated; `err` must have `err_cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_load(
    path: *const c_char,
    null_sink: c_int,
    err: *mut c_char,
    err_cap: usize,
) -> *mut SchedulerEngine {
    let path = match engine_from_c_path(path) {
        Ok(path) => path,
        Err(message) => {
            unsafe { write_ffi_error(err, err_cap, &message) };
            return ptr::null_mut();
        }
    };
    let kind = if null_sink != 0 { SinkKind::Null } else { SinkKind::Default };
    let result = catch_unwind(AssertUnwindSafe(|| {
        SchedulerEngine::load_audio_only(&path, kind)
    }));
    match result {
        Ok(Ok(engine)) => Box::into_raw(Box::new(engine)),
        Ok(Err(message)) => {
            unsafe { write_ffi_error(err, err_cap, &message) };
            ptr::null_mut()
        }
        Err(_) => {
            unsafe { write_ffi_error(err, err_cap, "panic in player_audio_engine_load") };
            ptr::null_mut()
        }
    }
}

unsafe fn with_engine<R>(
    handle: *mut SchedulerEngine,
    default: R,
    apply: impl FnOnce(&SchedulerEngine) -> R,
) -> R {
    if handle.is_null() {
        return default;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: handle came from player_audio_engine_load and is still live.
        apply(unsafe { &*handle })
    }));
    result.unwrap_or(default)
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_set_paused(handle: *mut SchedulerEngine, paused: c_int) {
    unsafe { with_engine(handle, (), |engine| engine.set_paused(paused != 0)) };
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_seek(handle: *mut SchedulerEngine, seconds: f64) {
    unsafe { with_engine(handle, (), |engine| engine.seek(seconds)) };
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_set_volume(handle: *mut SchedulerEngine, volume: f32) {
    unsafe { with_engine(handle, (), |engine| engine.set_volume(volume)) };
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and still be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_set_muted(handle: *mut SchedulerEngine, muted: c_int) {
    unsafe { with_engine(handle, (), |engine| engine.set_muted(muted != 0)) };
}

/// # Safety
///
/// `handle` and `out` must be valid for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_snapshot(
    handle: *mut SchedulerEngine,
    out: *mut EngineSnapshotFfi,
) -> c_int {
    if handle.is_null() || out.is_null() {
        return -1;
    }
    let result = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: handle and out are valid for this call.
        let engine = unsafe { &*handle };
        let status = engine.snapshot();
        let mut snapshot = EngineSnapshotFfi {
            position: status.position,
            duration: status.duration,
            volume: status.volume,
            paused: status.paused as c_int,
            muted: status.muted as c_int,
            ended: status.ended as c_int,
            has_error: status.error.is_some() as c_int,
            error: [0; 256],
        };
        if let Some(message) = status.error {
            copy_string(&mut snapshot.error, &message);
        }
        // SAFETY: out is writable for one snapshot.
        unsafe { ptr::write(out, snapshot) };
    }));
    if result.is_err() { -1 } else { 0 }
}

/// # Safety
///
/// `handle` must come from `player_audio_engine_load` and not be used again.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_audio_engine_destroy(handle: *mut SchedulerEngine) {
    if handle.is_null() {
        return;
    }
    let _ = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: handle is live and never used after this call.
        drop(unsafe { Box::from_raw(handle) });
    }));
}

