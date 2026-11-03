use std::ffi::{c_char, CStr};
use std::sync::Mutex;

use player_core::event::EventBus;

#[repr(C)]
pub struct PlayerEventBus {
    _private: [u8; 0],
}

struct EventBusHandle {
    bus: Mutex<EventBus>,
}

fn lock_bus<'a>(handle: &'a EventBusHandle) -> std::sync::MutexGuard<'a, EventBus> {
    handle.bus.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

unsafe fn as_bus<'a>(handle: *mut PlayerEventBus) -> Option<&'a EventBusHandle> {
    if handle.is_null() {
        None
    } else {
        // SAFETY: handle was created by player_ffi_event_create and is live for the call.
        Some(unsafe { &*(handle as *mut EventBusHandle) })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_event_create() -> *mut PlayerEventBus {
    Box::into_raw(Box::new(EventBusHandle {
        bus: Mutex::new(EventBus::new()),
    })) as *mut PlayerEventBus
}

/// # Safety
///
/// `handle` must be a live event bus and must not be used again after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_event_destroy(handle: *mut PlayerEventBus) {
    if handle.is_null() {
        return;
    }
    // SAFETY: handle was created by player_ffi_event_create and is owned here.
    drop(unsafe { Box::from_raw(handle as *mut EventBusHandle) });
}

/// # Safety
///
/// `handle` must be live and `name` must be a valid NUL-terminated string.
/// Returns the event sequence number, or 0 when the handle or name is invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_event_push(
    handle: *mut PlayerEventBus,
    name: *const c_char,
) -> u64 {
    let Some(handle) = (unsafe { as_bus(handle) }) else {
        return 0;
    };
    if name.is_null() {
        return 0;
    }
    // SAFETY: name is a NUL-terminated string.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return 0;
    };
    lock_bus(handle).push(name)
}

/// # Safety
///
/// `handle` must be live. Returns the next sequence number, or 0 when invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_event_next_sequence(handle: *mut PlayerEventBus) -> u64 {
    let Some(handle) = (unsafe { as_bus(handle) }) else {
        return 0;
    };
    lock_bus(handle).next_sequence()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn event_bus_ffi_pushes_and_counts() {
        let handle = player_ffi_event_create();
        let pause = CString::new("pause").unwrap();
        // SAFETY: the handle and name are valid for the calls.
        unsafe {
            assert_eq!(player_ffi_event_next_sequence(handle), 1);
            assert_eq!(player_ffi_event_push(handle, pause.as_ptr()), 1);
            assert_eq!(player_ffi_event_push(handle, pause.as_ptr()), 2);
            assert_eq!(player_ffi_event_next_sequence(handle), 3);
            assert_eq!(player_ffi_event_push(std::ptr::null_mut(), pause.as_ptr()), 0);
            player_ffi_event_destroy(handle);
        }
    }
}
