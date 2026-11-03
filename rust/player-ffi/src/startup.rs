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
    pub set_tui: Option<unsafe extern "C" fn(*mut c_void, c_int)>,
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
            || sink.set_tui.is_none()
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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlayerHostSteps {
    pub ctx: *mut c_void,
    pub create: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
    pub init: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *mut c_char) -> c_int>,
    pub play: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
    pub finish: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, c_int) -> c_int>,
    pub destroy: Option<unsafe extern "C" fn(*mut c_void, *mut c_void)>,
}

/// Run the whole player lifecycle: create, initialize, play, finish, destroy.
///
/// The host owns every step implementation; Rust owns the order. Returns the
/// process exit code: 1 when the arguments or the step table are unusable.
///
/// # Safety
///
/// `argv` must contain `argc` valid NUL-terminated strings, and `steps` must
/// point to a complete step table whose functions are valid for the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_run(
    argc: c_int,
    argv: *const *const c_char,
    steps: *const PlayerHostSteps,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<c_int, ()> {
        if argc < 0 || argv.is_null() || steps.is_null() {
            return Err(());
        }
        // SAFETY: steps is non-null and points to a valid table for the call.
        let steps = unsafe { *steps };
        let (Some(create), Some(init), Some(play), Some(finish), Some(destroy)) =
            (steps.create, steps.init, steps.play, steps.finish, steps.destroy)
        else {
            return Err(());
        };
        // SAFETY: create is valid; the context stays alive for the call.
        let mpctx = unsafe { create(steps.ctx) };
        if mpctx.is_null() {
            return Ok(1);
        }
        // Skip the program name, mirroring the previous C entry point.
        // SAFETY: argv contains argc valid entries.
        let options = if argc > 1 && !(unsafe { *argv }).is_null() {
            unsafe { argv.add(1) as *mut *mut c_char }
        } else {
            ptr::null_mut()
        };
        // SAFETY: init, play, finish, and destroy are valid; mpctx is live
        // until destroy runs, and options borrows argv for the call.
        let status = unsafe { init(steps.ctx, mpctx, options) };
        if status == 0 {
            unsafe { play(steps.ctx, mpctx) };
        }
        let code = unsafe { finish(steps.ctx, mpctx, status) };
        unsafe { destroy(steps.ctx, mpctx) };
        Ok(code)
    }));
    match result {
        Ok(Ok(code)) => code,
        Ok(Err(())) => 1,
        Err(_) => 1,
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
        StartupOp::SetTui(tui) => {
            let callback = sink.set_tui.ok_or(INCOMPLETE_SINK)?;
            // SAFETY: the callback and ctx come from the caller and are valid for the call.
            unsafe { callback(sink.ctx, c_int::from(tui)) };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;
    use std::sync::{LazyLock, Mutex};

    static CALLS: LazyLock<Mutex<Vec<&'static str>>> =
        LazyLock::new(|| Mutex::new(Vec::new()));
    static INIT_STATUS: std::sync::atomic::AtomicI32 =
        std::sync::atomic::AtomicI32::new(0);
    static SERIAL: Mutex<()> = Mutex::new(());

    fn calls() -> std::sync::MutexGuard<'static, Vec<&'static str>> {
        CALLS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn log(call: &'static str) {
        calls().push(call);
    }

    unsafe extern "C" fn stub_create(_ctx: *mut c_void) -> *mut c_void {
        log("create");
        0x1234 as *mut c_void
    }

    unsafe extern "C" fn stub_init(
        _ctx: *mut c_void,
        mpctx: *mut c_void,
        options: *mut *mut c_char,
    ) -> c_int {
        assert_eq!(mpctx as usize, 0x1234);
        // SAFETY: the test passes a NUL-terminated options array.
        let first = unsafe { CStr::from_ptr(*options) };
        assert_eq!(first.to_str().unwrap(), "a.mkv");
        log("init");
        INIT_STATUS.load(std::sync::atomic::Ordering::SeqCst)
    }

    unsafe extern "C" fn stub_play(_ctx: *mut c_void, mpctx: *mut c_void) {
        assert_eq!(mpctx as usize, 0x1234);
        log("play");
    }

    unsafe extern "C" fn stub_finish(
        _ctx: *mut c_void,
        mpctx: *mut c_void,
        status: c_int,
    ) -> c_int {
        assert_eq!(mpctx as usize, 0x1234);
        assert_eq!(status, INIT_STATUS.load(std::sync::atomic::Ordering::SeqCst));
        log("finish");
        7
    }

    unsafe extern "C" fn stub_destroy(_ctx: *mut c_void, mpctx: *mut c_void) {
        assert_eq!(mpctx as usize, 0x1234);
        log("destroy");
    }

    fn steps() -> PlayerHostSteps {
        PlayerHostSteps {
            ctx: ptr::null_mut(),
            create: Some(stub_create),
            init: Some(stub_init),
            play: Some(stub_play),
            finish: Some(stub_finish),
            destroy: Some(stub_destroy),
        }
    }

    fn run_with(args: &[&str], init_status: i32) -> (c_int, std::sync::MutexGuard<'static, ()>) {
        let serial = SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        INIT_STATUS.store(init_status, std::sync::atomic::Ordering::SeqCst);
        calls().clear();
        let owned: Vec<CString> = args
            .iter()
            .map(|arg| CString::new(*arg).unwrap())
            .collect();
        let mut pointers: Vec<*const c_char> =
            owned.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(ptr::null());
        let table = steps();
        // SAFETY: the argument vector and step table stay live for the call.
        let code = unsafe {
            player_ffi_run(
                args.len() as c_int,
                pointers.as_ptr(),
                &table as *const PlayerHostSteps,
            )
        };
        (code, serial)
    }

    #[test]
    fn run_sequences_full_lifecycle() {
        let (code, _serial) = run_with(&["player", "a.mkv"], 0);
        assert_eq!(code, 7);
        assert_eq!(
            *calls(),
            ["create", "init", "play", "finish", "destroy"]
        );
    }

    #[test]
    fn run_skips_play_when_init_fails() {
        let (code, _serial) = run_with(&["player", "a.mkv"], -1);
        assert_eq!(code, 7);
        assert_eq!(
            *calls(),
            ["create", "init", "finish", "destroy"]
        );
    }

    #[test]
    fn run_rejects_bad_input() {
        let table = steps();
        // SAFETY: the table is valid; the argument pointer is null.
        assert_eq!(
            unsafe { player_ffi_run(1, ptr::null(), &table as *const PlayerHostSteps) },
            1
        );
        let owned = CString::new("player").unwrap();
        let pointers = [owned.as_ptr(), ptr::null()];
        let mut broken = steps();
        broken.play = None;
        // SAFETY: arguments and table stay live for the call.
        assert_eq!(
            unsafe {
                player_ffi_run(
                    1,
                    pointers.as_ptr(),
                    &broken as *const PlayerHostSteps,
                )
            },
            1
        );
    }
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
