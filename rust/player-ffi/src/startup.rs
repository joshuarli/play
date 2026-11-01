use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;

use player_core::config::{parse_startup, PathListOp, StartTime, StartupOp};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlayerStartupSink {
    pub ctx: *mut c_void,
    pub add_file: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub add_playlist: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int>,
    pub set_fullscreen: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
    pub set_start: Option<unsafe extern "C" fn(*mut c_void, c_int, f64)>,
    pub append_path: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub set_paths:
        Option<unsafe extern "C" fn(*mut c_void, c_int, *const *const c_char, usize)>,
    pub add_remaining: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
}

/// # Safety
///
/// `argv` must contain `argc` valid NUL-terminated strings, `sink` must point to
/// a complete callback table, and `err` must have `err_cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_startup_parse(
    argc: c_int,
    argv: *const *const c_char,
    sink: *const PlayerStartupSink,
    err: *mut c_char,
    err_cap: usize,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        if argc < 0 || argv.is_null() || sink.is_null() {
            return Err("missing startup parser arguments".to_owned());
        }
        // SAFETY: sink is non-null and points to a valid callback table for the call.
        let sink = unsafe { *sink };
        if sink.add_file.is_none()
            || sink.add_playlist.is_none()
            || sink.set_fullscreen.is_none()
            || sink.set_start.is_none()
            || sink.append_path.is_none()
            || sink.set_paths.is_none()
            || sink.add_remaining.is_none()
        {
            return Err("startup sink is incomplete".to_owned());
        }
        let mut args = Vec::with_capacity(argc as usize);
        for index in 0..argc as usize {
            // SAFETY: argv contains argc entries.
            let pointer = unsafe { *argv.add(index) };
            if pointer.is_null() {
                return Err("null command line argument".to_owned());
            }
            // SAFETY: each argv entry is a NUL-terminated string.
            let text = unsafe { CStr::from_ptr(pointer) }
                .to_str()
                .map_err(|_| "command line argument is not UTF-8".to_owned())?;
            args.push(text.to_owned());
        }
        for operation in parse_startup(&args).map_err(|error| error.to_string())? {
            apply_operation(&sink, operation)?;
        }
        Ok(())
    }));
    match result {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            // SAFETY: err and err_cap come from the caller.
            unsafe { write_error(err, err_cap, &error) };
            -1
        }
        Err(_) => {
            // SAFETY: err and err_cap come from the caller.
            unsafe { write_error(err, err_cap, "panic in startup parser") };
            -1
        }
    }
}

const INCOMPLETE_SINK: &str = "startup sink is incomplete";

fn apply_operation(sink: &PlayerStartupSink, operation: StartupOp) -> Result<(), String> {
    match operation {
        StartupOp::AddFile(path) => {
            let path = c_string(&path)?;
            let callback = sink.add_file.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller and are valid for the call.
            unsafe { callback(sink.ctx, path.as_ptr()) };
        }
        StartupOp::AddPlaylist(path) => {
            let c_path = c_string(&path)?;
            let callback = sink.add_playlist.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller and are valid for the call.
            if unsafe { callback(sink.ctx, c_path.as_ptr()) } < 0 {
                return Err(format!("cannot read playlist '{path}'"));
            }
        }
        StartupOp::SetFullscreen(fullscreen) => {
            let callback = sink.set_fullscreen.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller and are valid for the call.
            unsafe { callback(sink.ctx, c_int::from(fullscreen)) };
        }
        StartupOp::SetStart(start) => {
            let (kind, position) = start_time_parts(&start);
            let callback = sink.set_start.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller and are valid for the call.
            unsafe { callback(sink.ctx, kind, position) };
        }
        StartupOp::AudioFiles(op) => apply_path_op(sink, false, &op)?,
        StartupOp::SubFiles(op) => apply_path_op(sink, true, &op)?,
        StartupOp::Forward(argument) => {
            let argument = c_string(&argument)?;
            let callback = sink.add_remaining.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller and are valid for the call.
            unsafe { callback(sink.ctx, argument.as_ptr()) };
        }
    }
    Ok(())
}

fn apply_path_op(sink: &PlayerStartupSink, is_sub: bool, op: &PathListOp) -> Result<(), String> {
    match op {
        PathListOp::Append(path) => {
            let path = c_string(path)?;
            let callback = sink.append_path.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller and are valid for the call.
            unsafe { callback(sink.ctx, c_int::from(is_sub), path.as_ptr()) };
        }
        PathListOp::Set(paths) => {
            let strings = paths
                .iter()
                .map(|path| c_string(path))
                .collect::<Result<Vec<_>, _>>()?;
            let pointers: Vec<*const c_char> = strings.iter().map(|path| path.as_ptr()).collect();
            let callback = sink.set_paths.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller; the string array and
            // pointers stay alive for the call.
            unsafe {
                callback(sink.ctx, c_int::from(is_sub), pointers.as_ptr(), pointers.len())
            };
        }
    }
    Ok(())
}

fn start_time_parts(start: &StartTime) -> (c_int, f64) {
    match start {
        StartTime::Unset => (0, 0.0),
        StartTime::Absolute(position) => (1, *position),
        StartTime::Relative(position) => (2, *position),
        StartTime::Percent(position) => (3, *position),
        StartTime::Chapter(chapter) => (4, *chapter as f64),
    }
}

fn c_string(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "argument contains NUL".to_owned())
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
