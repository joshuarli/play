use std::ffi::{c_char, c_int, CStr};
use std::ptr;

use player_core::state::PlaybackState;

#[repr(C)]
pub struct PlayerState {
    _private: [u8; 0],
}

struct StateHandle {
    state: PlaybackState,
}

unsafe fn as_state<'a>(handle: *mut PlayerState) -> Option<&'a mut StateHandle> {
    if handle.is_null() {
        None
    } else {
        // SAFETY: handle was created by player_ffi_state_create and is live for the call.
        Some(unsafe { &mut *(handle as *mut StateHandle) })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_state_create() -> *mut PlayerState {
    Box::into_raw(Box::new(StateHandle {
        state: PlaybackState::new(),
    })) as *mut PlayerState
}

/// # Safety
///
/// `handle` must be a live state handle and must not be used again after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_destroy(handle: *mut PlayerState) {
    if handle.is_null() {
        return;
    }
    // SAFETY: handle was created by player_ffi_state_create and is owned here.
    drop(unsafe { Box::from_raw(handle as *mut StateHandle) });
}

/// # Safety
///
/// `handle` must be live and `name` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_set_bool(
    handle: *mut PlayerState,
    name: *const c_char,
    value: c_int,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    if name.is_null() {
        return -1;
    }
    // SAFETY: name is a NUL-terminated string.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return -1;
    };
    match handle.state.set_bool(name, value != 0) {
        Some(_) => 1,
        None if handle.state.get_bool(name).is_some() => 0,
        None => -1,
    }
}

/// # Safety
///
/// `handle` must be live and `name` must be a valid NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_set_number(
    handle: *mut PlayerState,
    name: *const c_char,
    value: f64,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    if name.is_null() {
        return -1;
    }
    // SAFETY: name is a NUL-terminated string.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return -1;
    };
    let existed = handle.state.get_number(name).is_some();
    match handle.state.set_number(name, value) {
        Some(_) => 1,
        None if existed || handle.state.get_number(name).is_some() => 0,
        None => -1,
    }
}

/// # Safety
///
/// `handle` must be live, `name` must be a valid NUL-terminated string, and `out`
/// must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_get_bool(
    handle: *mut PlayerState,
    name: *const c_char,
    out: *mut c_int,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    if name.is_null() || out.is_null() {
        return -1;
    }
    // SAFETY: name is a NUL-terminated string.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return -1;
    };
    let Some(value) = handle.state.get_bool(name) else {
        return -1;
    };
    // SAFETY: out is writable.
    unsafe { *out = value as c_int };
    0
}

/// # Safety
///
/// `handle` must be live, `name` must be a valid NUL-terminated string, and `out`
/// must be writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_get_number(
    handle: *mut PlayerState,
    name: *const c_char,
    out: *mut f64,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    if name.is_null() || out.is_null() {
        return -1;
    }
    // SAFETY: name is a NUL-terminated string.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return -1;
    };
    let Some(value) = handle.state.get_number(name) else {
        return -1;
    };
    // SAFETY: out is writable.
    unsafe { *out = value };
    0
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_set_time_pos(
    handle: *mut PlayerState,
    value: f64,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    handle.state.set_time_pos(value) as c_int
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_set_duration(
    handle: *mut PlayerState,
    value: f64,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    handle.state.set_duration(value) as c_int
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_set_eof_reached(
    handle: *mut PlayerState,
    value: c_int,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    handle.state.set_eof_reached(value != 0) as c_int
}

/// # Safety
///
/// `handle` must be live, and `out` must have `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_describe(
    handle: *mut PlayerState,
    out: *mut c_char,
    cap: usize,
) -> c_int {
    let Some(handle) = (unsafe { as_state(handle) }) else {
        return -1;
    };
    if out.is_null() || cap == 0 {
        return -1;
    }
    let state = &handle.state;
    let duration = state.get_number("duration").unwrap_or(-1.0);
    let text = format!(
        "pause={} fullscreen={} mute={} volume={:.3} speed={:.3} time-pos={:.3} duration={:.3} eof={} seq={}",
        state.get_bool("pause").unwrap_or(false) as c_int,
        state.get_bool("fullscreen").unwrap_or(false) as c_int,
        state.get_bool("mute").unwrap_or(false) as c_int,
        state.get_number("volume").unwrap_or(0.0),
        state.get_number("speed").unwrap_or(0.0),
        state.get_number("time-pos").unwrap_or(0.0),
        duration,
        state.get_bool("eof-reached").unwrap_or(false) as c_int,
        state.sequence()
    );
    let bytes = text.as_bytes();
    let length = bytes.len().min(cap - 1);
    // SAFETY: out has cap writable bytes.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), length);
        *out.add(length) = 0;
    }
    0
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_state_sequence(handle: *mut PlayerState) -> u64 {
    unsafe { as_state(handle) }
        .map(|handle| handle.state.sequence())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn state_ffi_sets_and_reads_values() {
        let handle = player_ffi_state_create();
        let pause = CString::new("pause").unwrap();
        let volume = CString::new("volume").unwrap();
        let duration = CString::new("duration").unwrap();
        // SAFETY: the handle and names are valid for the calls.
        unsafe {
            assert_eq!(player_ffi_state_set_bool(handle, pause.as_ptr(), 1), 1);
            assert_eq!(player_ffi_state_set_bool(handle, pause.as_ptr(), 1), 0);
            let mut value = 0;
            assert_eq!(player_ffi_state_get_bool(handle, pause.as_ptr(), &mut value), 0);
            assert_eq!(value, 1);
            assert_eq!(player_ffi_state_set_number(handle, volume.as_ptr(), 55.0), 1);
            assert_eq!(player_ffi_state_set_number(handle, volume.as_ptr(), 55.0), 0);
            let mut number = 0.0;
            assert_eq!(
                player_ffi_state_get_number(handle, volume.as_ptr(), &mut number),
                0
            );
            assert_eq!(number, 55.0);
            assert_eq!(
                player_ffi_state_get_number(handle, duration.as_ptr(), &mut number),
                -1
            );
            assert_eq!(player_ffi_state_set_time_pos(handle, 1.0), 1);
            assert_eq!(player_ffi_state_set_time_pos(handle, 1.0), 0);
            assert!(player_ffi_state_sequence(handle) > 0);
            player_ffi_state_destroy(handle);
        }
    }
}
