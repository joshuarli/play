use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use player_tui::{Control, Cover, Player, Snapshot, Track};

use crate::mpv::{
    MPV_FORMAT_DOUBLE, MPV_FORMAT_FLAG, MPV_EVENT_SHUTDOWN, MpvHandle, mpv_command,
    mpv_error_string, mpv_free, mpv_get_property, mpv_get_property_string,
    mpv_wait_event,
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlayerTuiCallbacks {
    pub ctx: *mut c_void,
    pub cover_art: Option<
        unsafe extern "C" fn(
            ctx: *mut c_void,
            path: *const c_char,
            size: c_int,
            out: *mut u8,
            cap: usize,
        ) -> c_int,
    >,
}

struct TuiApi {
    client: *mut MpvHandle,
    callbacks: PlayerTuiCallbacks,
}

impl TuiApi {
    fn get_string(&self, name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        // SAFETY: client is a live mpv handle and name is NUL-terminated.
        let value = unsafe { mpv_get_property_string(self.client, name.as_ptr()) };
        if value.is_null() {
            return None;
        }
        // SAFETY: mpv returns a NUL-terminated string owned by the caller.
        let text = unsafe { CStr::from_ptr(value) }.to_string_lossy().into_owned();
        // SAFETY: value was allocated by mpv.
        unsafe { mpv_free(value.cast()) };
        if text.is_empty() { None } else { Some(text) }
    }

    fn get_double(&self, name: &str) -> Option<f64> {
        let name = CString::new(name).ok()?;
        let mut value = 0.0f64;
        // SAFETY: client is live and value is a writable double.
        let result = unsafe {
            mpv_get_property(
                self.client,
                name.as_ptr(),
                MPV_FORMAT_DOUBLE,
                (&mut value as *mut f64).cast(),
            )
        };
        (result >= 0).then_some(value)
    }

    fn get_flag(&self, name: &str) -> Option<bool> {
        let name = CString::new(name).ok()?;
        let mut value: c_int = 0;
        // SAFETY: client is live and value is a writable int.
        let result = unsafe {
            mpv_get_property(
                self.client,
                name.as_ptr(),
                MPV_FORMAT_FLAG,
                (&mut value as *mut c_int).cast(),
            )
        };
        (result >= 0).then_some(value != 0)
    }

    fn command(&self, args: &[&str]) -> Result<(), String> {
        let owned = args
            .iter()
            .map(|arg| CString::new(*arg))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "command contains a NUL".to_owned())?;
        let mut pointers: Vec<*const c_char> = owned.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(ptr::null());
        // SAFETY: client is live and pointers is a NULL-terminated string array.
        let result = unsafe { mpv_command(self.client, pointers.as_ptr()) };
        if result < 0 {
            Err(self.error(result))
        } else {
            Ok(())
        }
    }

    fn error(&self, code: c_int) -> String {
        // SAFETY: mpv_error_string returns a static string for any code.
        let pointer = unsafe { mpv_error_string(code) };
        if pointer.is_null() {
            format!("mpv error {code}")
        } else {
            // SAFETY: the returned pointer is NUL-terminated.
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned()
        }
    }
}

impl Player for TuiApi {
    fn snapshot(&mut self) -> Snapshot {
        let path = self.get_string("path").unwrap_or_default();
        let media_title = self.get_string("media-title");
        let title = media_title
            .filter(|title| !title.is_empty())
            .or_else(|| self.get_string("metadata/by-key/Title"))
            .or_else(|| {
                std::path::Path::new(&path)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "NO FILE".to_owned());
        let artist = ["Artist", "album_artist", "AlbumArtist"]
            .iter()
            .find_map(|key| self.get_string(&format!("metadata/by-key/{key}")))
            .unwrap_or_default();
        let album = self.get_string("metadata/by-key/Album").unwrap_or_default();
        let year = ["Date", "Year"]
            .iter()
            .find_map(|key| self.get_string(&format!("metadata/by-key/{key}")))
            .map(|date| date.chars().take(4).collect::<String>())
            .unwrap_or_default();
        let format = self
            .get_string("file-format")
            .or_else(|| {
                std::path::Path::new(&path)
                    .extension()
                    .map(|extension| extension.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let duration = self.get_double("duration").unwrap_or(0.0);
        let pos = self.get_double("time-pos").unwrap_or(0.0);
        let paused = self.get_flag("pause").unwrap_or(false);
        let muted = self.get_flag("mute").unwrap_or(false);
        let volume = self.get_double("volume").unwrap_or(100.0) / 100.0;
        let playing = !paused && !path.is_empty();
        Snapshot {
            path,
            track: Track {
                title,
                artist,
                album,
                year,
                format,
            },
            pos,
            duration,
            playing,
            volume: volume.clamp(0.0, 1.0),
            muted,
        }
    }

    fn control(&mut self, control: Control) -> Result<(), String> {
        match control {
            Control::Pause => self.command(&["cycle", "pause"]),
            Control::Seek(delta) => self.command(&["seek", &delta.to_string()]),
            Control::Volume(delta) => self.command(&["add", "volume", &delta.to_string()]),
            Control::Mute => self.command(&["cycle", "mute"]),
            Control::Next => self.command(&["playlist-next"]),
            Control::Previous => self.command(&["playlist-prev"]),
            Control::Restart => self.command(&["seek", "0", "absolute"]),
            Control::Quit => self.command(&["quit"]),
        }
    }

    fn cover_art(&mut self, path: &str) -> Option<Cover> {
        let callback = self.callbacks.cover_art?;
        let path = CString::new(path).ok()?;
        let size = player_tui::COVER_SIZE;
        let mut pixels = vec![0u8; size * size * 3];
        // SAFETY: the callback and context come from the C caller; the output
        // buffer is writable and the path is NUL-terminated.
        let result = unsafe {
            callback(
                self.callbacks.ctx,
                path.as_ptr(),
                size as c_int,
                pixels.as_mut_ptr(),
                pixels.len(),
            )
        };
        if result == 0 {
            Cover::new(pixels)
        } else {
            None
        }
    }

    fn shutdown(&mut self) -> bool {
        // SAFETY: client is live; a zero timeout polls without blocking.
        let event = unsafe { mpv_wait_event(self.client, 0.0) };
        !event.is_null() && unsafe { (*event).event_id } == MPV_EVENT_SHUTDOWN
    }
}

/// # Safety
///
/// `client` must be a live mpv handle, `callbacks` must point to a complete
/// callback table, and `err` must have `err_cap` writable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_tui_run(
    client: *mut c_void,
    callbacks: *const PlayerTuiCallbacks,
    err: *mut c_char,
    err_cap: usize,
) -> c_int {
    let callbacks_copy = if callbacks.is_null() {
        None
    } else {
        // SAFETY: callbacks is non-null and points to a valid callback table.
        Some(unsafe { *callbacks })
    };
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        let callbacks = callbacks_copy.ok_or_else(|| "missing TUI callbacks".to_owned())?;
        if client.is_null() {
            return Err("missing TUI client".to_owned());
        }
        let mut api = TuiApi {
            client: client.cast::<MpvHandle>(),
            callbacks,
        };
        player_tui::run(&mut api)
    }));
    match result {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            unsafe { write_error(err, err_cap, &error) };
            eprintln!("player: {error}");
            -1
        }
        Err(_) => {
            let message = "panic in the terminal UI";
            unsafe { write_error(err, err_cap, message) };
            eprintln!("player: {message}");
            -1
        }
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
