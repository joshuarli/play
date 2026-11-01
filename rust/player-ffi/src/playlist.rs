use std::collections::HashMap;
use std::ffi::{c_int, c_void};
use std::ptr;

use player_core::playlist::{EntryId, Playlist};

#[repr(C)]
pub struct PlayerPlaylist {
    _private: [u8; 0],
}

struct PlaylistHandle {
    playlist: Playlist,
    pointers: HashMap<EntryId, *mut c_void>,
    ids: HashMap<usize, EntryId>,
}

unsafe fn as_handle<'a>(handle: *mut PlayerPlaylist) -> Option<&'a mut PlaylistHandle> {
    if handle.is_null() {
        None
    } else {
        // SAFETY: handle was created by player_ffi_playlist_create and is live for the call.
        Some(unsafe { &mut *(handle as *mut PlaylistHandle) })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn player_ffi_playlist_create() -> *mut PlayerPlaylist {
    let handle = Box::new(PlaylistHandle {
        playlist: Playlist::new(),
        pointers: HashMap::new(),
        ids: HashMap::new(),
    });
    Box::into_raw(handle) as *mut PlayerPlaylist
}

/// # Safety
///
/// `handle` must be a live handle returned by `player_ffi_playlist_create` and
/// must not be used again after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_destroy(handle: *mut PlayerPlaylist) {
    if handle.is_null() {
        return;
    }
    // SAFETY: handle was created by player_ffi_playlist_create and is owned here.
    drop(unsafe { Box::from_raw(handle as *mut PlaylistHandle) });
}

/// # Safety
///
/// `handle` must be live and `entry` must be a caller-owned entry pointer that is
/// not already in this playlist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_insert_at(
    handle: *mut PlayerPlaylist,
    entry: *mut c_void,
    index: c_int,
) -> u64 {
    let Some(handle) = (unsafe { as_handle(handle) }) else {
        return 0;
    };
    if entry.is_null() {
        return 0;
    }
    let address = entry as usize;
    if handle.ids.contains_key(&address) {
        return 0;
    }
    let (id, _) = handle.playlist.insert_new_at(index.max(0) as usize);
    handle.pointers.insert(id, entry);
    handle.ids.insert(address, id);
    id
}

/// # Safety
///
/// `handle` must be live and `entry` must be an entry in this playlist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_remove(
    handle: *mut PlayerPlaylist,
    entry: *mut c_void,
) -> c_int {
    let Some(handle) = (unsafe { as_handle(handle) }) else {
        return 0;
    };
    let Some(id) = handle.ids.remove(&(entry as usize)) else {
        return 0;
    };
    handle.pointers.remove(&id);
    if handle.playlist.remove(id).is_some() {
        1
    } else {
        0
    }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_count(handle: *mut PlayerPlaylist) -> c_int {
    unsafe { as_handle(handle) }
        .map(|handle| handle.playlist.count() as c_int)
        .unwrap_or(0)
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_entry_at(
    handle: *mut PlayerPlaylist,
    index: c_int,
) -> *mut c_void {
    let Some(handle) = (unsafe { as_handle(handle) }) else {
        return ptr::null_mut();
    };
    if index < 0 {
        return ptr::null_mut();
    }
    handle
        .playlist
        .entry_at(index as usize)
        .and_then(|id| handle.pointers.get(&id).copied())
        .unwrap_or(ptr::null_mut())
}

/// # Safety
///
/// `handle` must be live and `entry` must be an entry pointer or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_index_of(
    handle: *mut PlayerPlaylist,
    entry: *mut c_void,
) -> c_int {
    let Some(handle) = (unsafe { as_handle(handle) }) else {
        return -1;
    };
    handle
        .ids
        .get(&(entry as usize))
        .and_then(|id| handle.playlist.index_of(*id))
        .map(|index| index as c_int)
        .unwrap_or(-1)
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_first(handle: *mut PlayerPlaylist) -> *mut c_void {
    entry_for(unsafe { as_handle(handle) }, |playlist| playlist.first())
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_last(handle: *mut PlayerPlaylist) -> *mut c_void {
    entry_for(unsafe { as_handle(handle) }, |playlist| playlist.last())
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_next(
    handle: *mut PlayerPlaylist,
    direction: c_int,
) -> *mut c_void {
    entry_for(unsafe { as_handle(handle) }, |playlist| playlist.next(direction))
}

/// # Safety
///
/// `handle` must be live and `entry` must be an entry in this playlist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_entry_rel(
    handle: *mut PlayerPlaylist,
    entry: *mut c_void,
    direction: c_int,
) -> *mut c_void {
    let Some(handle) = (unsafe { as_handle(handle) }) else {
        return ptr::null_mut();
    };
    let Some(id) = handle.ids.get(&(entry as usize)).copied() else {
        return ptr::null_mut();
    };
    handle
        .playlist
        .entry_rel(id, direction)
        .and_then(|id| handle.pointers.get(&id).copied())
        .unwrap_or(ptr::null_mut())
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_current(handle: *mut PlayerPlaylist) -> *mut c_void {
    entry_for(unsafe { as_handle(handle) }, |playlist| playlist.current())
}

/// # Safety
///
/// `handle` must be live and `entry` must be an entry pointer or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_set_current(
    handle: *mut PlayerPlaylist,
    entry: *mut c_void,
) {
    let Some(handle) = (unsafe { as_handle(handle) }) else {
        return;
    };
    let current = if entry.is_null() {
        None
    } else {
        handle.ids.get(&(entry as usize)).copied()
    };
    handle.playlist.set_current(current);
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_current_was_replaced(
    handle: *mut PlayerPlaylist,
) -> c_int {
    unsafe { as_handle(handle) }
        .map(|handle| handle.playlist.current_was_replaced() as c_int)
        .unwrap_or(0)
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_set_current_was_replaced(
    handle: *mut PlayerPlaylist,
    value: c_int,
) {
    if let Some(handle) = unsafe { as_handle(handle) } {
        handle.playlist.set_current_was_replaced(value != 0);
    }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_completed(handle: *mut PlayerPlaylist) -> c_int {
    unsafe { as_handle(handle) }
        .map(|handle| handle.playlist.completed() as c_int)
        .unwrap_or(0)
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_set_completed(
    handle: *mut PlayerPlaylist,
    value: c_int,
) {
    if let Some(handle) = unsafe { as_handle(handle) } {
        handle.playlist.set_completed(value != 0);
    }
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_started(handle: *mut PlayerPlaylist) -> c_int {
    unsafe { as_handle(handle) }
        .map(|handle| handle.playlist.started() as c_int)
        .unwrap_or(0)
}

/// # Safety
///
/// `handle` must be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_set_started(
    handle: *mut PlayerPlaylist,
    value: c_int,
) {
    if let Some(handle) = unsafe { as_handle(handle) } {
        handle.playlist.set_started(value != 0);
    }
}

/// # Safety
///
/// `handle` must be live; `entry` and `at` must be entries in this playlist or
/// `at` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_playlist_move(
    handle: *mut PlayerPlaylist,
    entry: *mut c_void,
    at: *mut c_void,
) -> c_int {
    let Some(handle) = (unsafe { as_handle(handle) }) else {
        return 0;
    };
    let Some(id) = handle.ids.get(&(entry as usize)).copied() else {
        return 0;
    };
    let at = if at.is_null() {
        None
    } else {
        handle.ids.get(&(at as usize)).copied()
    };
    handle.playlist.move_entry(id, at) as c_int
}

fn entry_for(
    handle: Option<&mut PlaylistHandle>,
    lookup: impl FnOnce(&Playlist) -> Option<EntryId>,
) -> *mut c_void {
    let Some(handle) = handle else {
        return ptr::null_mut();
    };
    lookup(&handle.playlist)
        .and_then(|id| handle.pointers.get(&id).copied())
        .unwrap_or(ptr::null_mut())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_ffi_maps_entries_and_current() {
        let handle = player_ffi_playlist_create();
        assert!(!handle.is_null());
        let first = 0x1000usize as *mut c_void;
        let second = 0x2000usize as *mut c_void;
        let third = 0x3000usize as *mut c_void;
        // SAFETY: the handles are live and the fake entry pointers are only used as keys.
        unsafe {
            let first_id = player_ffi_playlist_insert_at(handle, first, 0);
            let second_id = player_ffi_playlist_insert_at(handle, second, 1);
            let third_id = player_ffi_playlist_insert_at(handle, third, 1);
            assert!(first_id > 0 && second_id > 0 && third_id > 0);
            assert_eq!(player_ffi_playlist_count(handle), 3);
            assert_eq!(player_ffi_playlist_entry_at(handle, 0), first);
            assert_eq!(player_ffi_playlist_entry_at(handle, 1), third);
            assert_eq!(player_ffi_playlist_index_of(handle, third), 1);
            assert_eq!(player_ffi_playlist_first(handle), first);
            assert_eq!(player_ffi_playlist_last(handle), second);
            player_ffi_playlist_set_current(handle, third);
            assert_eq!(player_ffi_playlist_current(handle), third);
            assert_eq!(player_ffi_playlist_next(handle, 1), second);
            assert_eq!(player_ffi_playlist_entry_rel(handle, third, -1), first);
            assert_eq!(player_ffi_playlist_move(handle, third, first), 1);
            assert_eq!(player_ffi_playlist_entry_at(handle, 0), third);
            assert_eq!(player_ffi_playlist_remove(handle, third), 1);
            assert_eq!(player_ffi_playlist_count(handle), 2);
            player_ffi_playlist_destroy(handle);
        }
    }
}
