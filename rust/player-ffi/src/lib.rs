use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};

use player_core::input::{classify_input, InputError, InputPath};

mod command;
mod local_file;
mod lua;
mod playlist;
mod state;
mod startup;

pub const PLAYER_FFI_INPUT_LOCAL: i32 = 0;
pub const PLAYER_FFI_INPUT_REJECTED: i32 = 1;
pub const PLAYER_FFI_INPUT_INVALID: i32 = 2;
pub const PLAYER_FFI_INPUT_BUFFER_TOO_SMALL: i32 = 3;

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_abi_version() -> u32 {
    player_core::abi_version()
}

/// # Safety
///
/// `path` must be a valid NUL-terminated string, and each output buffer must
/// have the capacity passed to the function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_classify_input(
    path: *const c_char,
    allow_file_uri: i32,
    out_path: *mut c_char,
    out_path_cap: usize,
    err: *mut c_char,
    err_cap: usize,
) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if path.is_null() {
            return (PLAYER_FFI_INPUT_INVALID, None);
        }
        // SAFETY: the caller guarantees a NUL-terminated C string.
        let Ok(input) = unsafe { CStr::from_ptr(path) }.to_str() else {
            return (PLAYER_FFI_INPUT_INVALID, Some("input is not UTF-8".to_string()));
        };
        match classify_input(input, allow_file_uri != 0) {
            Ok(InputPath::Local(resolved)) => {
                let Some(resolved) = resolved.to_str() else {
                    return (PLAYER_FFI_INPUT_INVALID, Some("path is not UTF-8".to_string()));
                };
                // SAFETY: the output buffer and capacity come from the C caller.
                if unsafe { copy_cstr(out_path, out_path_cap, resolved) } {
                    (PLAYER_FFI_INPUT_LOCAL, None)
                } else {
                    (PLAYER_FFI_INPUT_BUFFER_TOO_SMALL,
                     Some("output path buffer is too small".to_string()))
                }
            }
            Err(error @ (InputError::Empty | InputError::InvalidFileUri)) => {
                (PLAYER_FFI_INPUT_INVALID, Some(error.to_string()))
            }
            Err(error) => (PLAYER_FFI_INPUT_REJECTED, Some(error.to_string())),
        }
    }));

    let (code, message) = match result {
        Ok(value) => value,
        Err(_) => (PLAYER_FFI_INPUT_INVALID, Some("panic in classifier".to_string())),
    };
    if let Some(message) = message {
        // SAFETY: the error buffer and capacity come from the C caller.
        unsafe { copy_cstr(err, err_cap, &message) };
    }
    code
}

unsafe fn copy_cstr(dst: *mut c_char, cap: usize, value: &str) -> bool {
    if dst.is_null() || cap == 0 {
        return false;
    }
    let bytes = value.as_bytes();
    if bytes.len() + 1 > cap {
        return false;
    }
    // SAFETY: dst has at least bytes.len() + 1 bytes of writable capacity.
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst as *mut u8, bytes.len());
        *dst.add(bytes.len()) = 0;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn ffi_reports_core_version() {
        assert_eq!(player_ffi_abi_version(), 1);
    }

    #[test]
    fn ffi_classifies_local_path() {
        let input = CString::new("/tmp/a:b.mp4").unwrap();
        let mut out = [0u8; 256];
        let mut err = [0u8; 256];
        let rc = unsafe {
            player_ffi_classify_input(
                input.as_ptr(), 0, out.as_mut_ptr() as *mut c_char, out.len(),
                err.as_mut_ptr() as *mut c_char, err.len())
        };
        assert_eq!(rc, PLAYER_FFI_INPUT_LOCAL);
        assert_eq!(unsafe { CStr::from_ptr(out.as_ptr() as *const c_char) }
                       .to_str().unwrap(), "/tmp/a:b.mp4");
    }

    #[test]
    fn ffi_rejects_remote_scheme() {
        let input = CString::new("https://example.com/a.mp4").unwrap();
        let mut out = [0u8; 64];
        let mut err = [0u8; 256];
        let rc = unsafe {
            player_ffi_classify_input(
                input.as_ptr(), 0, out.as_mut_ptr() as *mut c_char, out.len(),
                err.as_mut_ptr() as *mut c_char, err.len())
        };
        assert_eq!(rc, PLAYER_FFI_INPUT_REJECTED);
        assert!(unsafe { CStr::from_ptr(err.as_ptr() as *const c_char) }
                    .to_str().unwrap().contains("remote scheme"));
    }
}
