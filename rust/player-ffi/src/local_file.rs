use std::ffi::{c_char, c_int, CStr};
use std::path::Path;
use std::ptr;

use player_core::local_file::{LocalFile, SeekWhence};

#[repr(C)]
pub struct PlayerLocalFile {
    _private: [u8; 0],
}

struct FileHandle {
    file: LocalFile,
}

unsafe fn as_file<'a>(handle: *mut PlayerLocalFile) -> Option<&'a mut FileHandle> {
    if handle.is_null() {
        None
    } else {
        // SAFETY: handle was created by player_ffi_local_file_open and is live.
        Some(unsafe { &mut *(handle as *mut FileHandle) })
    }
}

/// # Safety
///
/// `path` must be a valid NUL-terminated string, and `err` must have `cap`
/// writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_local_file_open(
    path: *const c_char,
    write: c_int,
    err: *mut c_char,
    cap: usize,
) -> *mut PlayerLocalFile {
    let result = (|| -> Result<*mut PlayerLocalFile, String> {
        if path.is_null() {
            return Err("missing path".to_owned());
        }
        // SAFETY: path is a NUL-terminated string.
        let path = unsafe { CStr::from_ptr(path) }
            .to_str()
            .map_err(|_| "path is not UTF-8".to_owned())?;
        let file = if write != 0 {
            LocalFile::open_write(Path::new(path))
        } else {
            LocalFile::open_read(Path::new(path))
        }
        .map_err(|error| error.to_string())?;
        Ok(Box::into_raw(Box::new(FileHandle { file })) as *mut PlayerLocalFile)
    })();
    match result {
        Ok(handle) => handle,
        Err(error) => {
            // SAFETY: err and cap come from the caller.
            unsafe { write_error(err, cap, &error) };
            ptr::null_mut()
        }
    }
}

/// # Safety
///
/// `path` must be a valid NUL-terminated string, and `err` must have `cap`
/// writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_local_file_is_directory(
    path: *const c_char,
    err: *mut c_char,
    cap: usize,
) -> c_int {
    if path.is_null() {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "missing path") };
        return -1;
    }
    // SAFETY: path is a NUL-terminated string.
    let Ok(path) = unsafe { CStr::from_ptr(path) }.to_str() else {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "path is not UTF-8") };
        return -1;
    };
    match LocalFile::is_directory(Path::new(path)) {
        Ok(value) => value as c_int,
        Err(error) => {
            // SAFETY: err and cap come from the caller.
            unsafe { write_error(err, cap, &error.to_string()) };
            -1
        }
    }
}

/// # Safety
///
/// `handle` must be live, `buffer` must have `length` writable bytes, `out_read`
/// must be writable, and `err` must have `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_local_file_read(
    handle: *mut PlayerLocalFile,
    buffer: *mut u8,
    length: usize,
    out_read: *mut usize,
    err: *mut c_char,
    cap: usize,
) -> c_int {
    let Some(handle) = (unsafe { as_file(handle) }) else {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "invalid file handle") };
        return -1;
    };
    if buffer.is_null() || out_read.is_null() {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "missing buffer") };
        return -1;
    }
    // SAFETY: buffer has length writable bytes.
    let slice = unsafe { std::slice::from_raw_parts_mut(buffer, length) };
    match handle.file.read(slice) {
        Ok(read) => {
            // SAFETY: out_read is writable.
            unsafe { *out_read = read };
            0
        }
        Err(error) => {
            // SAFETY: err and cap come from the caller.
            unsafe { write_error(err, cap, &error.to_string()) };
            -1
        }
    }
}

/// # Safety
///
/// `handle` must be live, `buffer` must have `length` readable bytes, `out_written`
/// must be writable, and `err` must have `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_local_file_write(
    handle: *mut PlayerLocalFile,
    buffer: *const u8,
    length: usize,
    out_written: *mut usize,
    err: *mut c_char,
    cap: usize,
) -> c_int {
    let Some(handle) = (unsafe { as_file(handle) }) else {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "invalid file handle") };
        return -1;
    };
    if buffer.is_null() || out_written.is_null() {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "missing buffer") };
        return -1;
    }
    // SAFETY: buffer has length readable bytes.
    let slice = unsafe { std::slice::from_raw_parts(buffer, length) };
    match handle.file.write(slice) {
        Ok(written) => {
            // SAFETY: out_written is writable.
            unsafe { *out_written = written };
            0
        }
        Err(error) => {
            // SAFETY: err and cap come from the caller.
            unsafe { write_error(err, cap, &error.to_string()) };
            -1
        }
    }
}

/// # Safety
///
/// `handle` must be live, `out_position` must be writable, and `err` must have
/// `cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_local_file_seek(
    handle: *mut PlayerLocalFile,
    offset: i64,
    whence: c_int,
    out_position: *mut u64,
    err: *mut c_char,
    cap: usize,
) -> c_int {
    let Some(handle) = (unsafe { as_file(handle) }) else {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "invalid file handle") };
        return -1;
    };
    if out_position.is_null() {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "missing output position") };
        return -1;
    }
    let whence = match whence {
        0 => SeekWhence::Start,
        1 => SeekWhence::Current,
        2 => SeekWhence::End,
        _ => {
            // SAFETY: err and cap come from the caller.
            unsafe { write_error(err, cap, "invalid seek origin") };
            return -1;
        }
    };
    match handle.file.seek(offset, whence) {
        Ok(position) => {
            // SAFETY: out_position is writable.
            unsafe { *out_position = position };
            0
        }
        Err(error) => {
            // SAFETY: err and cap come from the caller.
            unsafe { write_error(err, cap, &error.to_string()) };
            -1
        }
    }
}

/// # Safety
///
/// `handle` must be live, `out_size` must be writable, and `err` must have `cap`
/// writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_local_file_size(
    handle: *mut PlayerLocalFile,
    out_size: *mut u64,
    err: *mut c_char,
    cap: usize,
) -> c_int {
    let Some(handle) = (unsafe { as_file(handle) }) else {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "invalid file handle") };
        return -1;
    };
    if out_size.is_null() {
        // SAFETY: err and cap come from the caller.
        unsafe { write_error(err, cap, "missing output size") };
        return -1;
    }
    match handle.file.length() {
        Ok(size) => {
            // SAFETY: out_size is writable.
            unsafe { *out_size = size };
            0
        }
        Err(error) => {
            // SAFETY: err and cap come from the caller.
            unsafe { write_error(err, cap, &error.to_string()) };
            -1
        }
    }
}

/// # Safety
///
/// `handle` must be a live handle and must not be used again after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_local_file_destroy(handle: *mut PlayerLocalFile) {
    if handle.is_null() {
        return;
    }
    // SAFETY: handle was created by player_ffi_local_file_open and is owned here.
    drop(unsafe { Box::from_raw(handle as *mut FileHandle) });
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
