use std::ffi::{c_char, c_int, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use player_core::command::{parse_args, parse_string, Command};

/// # Safety
///
/// `argv` must contain `argc` valid NUL-terminated strings, and `err` must have
/// `err_cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_command_check_strv(
    argc: c_int,
    argv: *const *const c_char,
    err: *mut c_char,
    err_cap: usize,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<c_int, String> {
        if argc < 0 || (argc > 0 && argv.is_null()) {
            return Err("missing command arguments".to_owned());
        }
        let mut args = Vec::with_capacity(argc as usize);
        for index in 0..argc as usize {
            // SAFETY: argv contains argc entries.
            let pointer = unsafe { *argv.add(index) };
            if pointer.is_null() {
                return Err("null command argument".to_owned());
            }
            // SAFETY: each argv entry is a NUL-terminated string.
            let text = unsafe { CStr::from_ptr(pointer) }
                .to_str()
                .map_err(|_| "command argument is not UTF-8".to_owned())?;
            args.push(text.to_owned());
        }
        check(parse_args(&args))
    }));
    finish(result, err, err_cap)
}

/// # Safety
///
/// `text` must be a valid NUL-terminated string, and `err` must have `err_cap`
/// writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_command_check_str(
    text: *const c_char,
    err: *mut c_char,
    err_cap: usize,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<c_int, String> {
        if text.is_null() {
            return Err("missing command text".to_owned());
        }
        // SAFETY: text is a NUL-terminated string.
        let text = unsafe { CStr::from_ptr(text) }
            .to_str()
            .map_err(|_| "command text is not UTF-8".to_owned())?;
        if has_unquoted_semicolon(text) {
            return Ok(0);
        }
        check(parse_string(text))
    }));
    finish(result, err, err_cap)
}

fn check(result: Result<Option<Command>, player_core::command::CommandError>) -> Result<c_int, String> {
    match result {
        Ok(Some(_)) => Ok(1),
        Ok(None) => Ok(0),
        Err(error) => Err(error.to_string()),
    }
}

fn finish(result: Result<Result<c_int, String>, Box<dyn std::any::Any + Send>>, err: *mut c_char, err_cap: usize) -> c_int {
    match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            // SAFETY: err and err_cap come from the caller.
            unsafe { write_error(err, err_cap, &error) };
            -1
        }
        Err(_) => {
            // SAFETY: err and err_cap come from the caller.
            unsafe { write_error(err, err_cap, "panic in command checker") };
            -1
        }
    }
}

fn has_unquoted_semicolon(text: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for character in text.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            ';' if !quoted => return true,
            _ => {}
        }
    }
    false
}

unsafe fn write_error(err: *mut c_char, cap: usize, message: &str) {
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
