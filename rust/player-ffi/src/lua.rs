use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use player_lua::{
    HostError, NativeValue, PlayerBackend, PropertyFormat, ScriptEvent, ScriptHost,
    ScriptModule, SHUTDOWN_TIMEOUT,
};

const MPV_FORMAT_NONE: c_int = 0;
const MPV_FORMAT_STRING: c_int = 1;
const MPV_FORMAT_OSD_STRING: c_int = 2;
const MPV_FORMAT_FLAG: c_int = 3;
const MPV_FORMAT_INT64: c_int = 4;
const MPV_FORMAT_DOUBLE: c_int = 5;
const MPV_FORMAT_NODE: c_int = 6;
const MPV_FORMAT_NODE_ARRAY: c_int = 7;
const MPV_FORMAT_NODE_MAP: c_int = 8;
const MPV_FORMAT_BYTE_ARRAY: c_int = 9;

const MPV_EVENT_NONE: c_int = 0;
const MPV_EVENT_SHUTDOWN: c_int = 1;

const MSGL_ERR: c_int = 1;
const MSGL_WARN: c_int = 2;

const MAX_NODE_DEPTH: usize = 64;

const CLIENT_CLOSED: &str = "script client is closed";
const CLIENT_CLOSED_CODE: c_int = -1;
const EVENT_POLL_SECONDS: f64 = 0.2;

const ASSDRAW: &str = include_str!("../../../player/lua/assdraw.lua");
const OPTIONS: &str = include_str!("../../../player/lua/options.lua");
const INPUT: &str = include_str!("../../../player/lua/input.lua");
const OSC: &str = include_str!("../../../player/lua/osc.lua");

#[repr(C)]
pub struct MpvHandle {
    _private: [u8; 0],
}

#[repr(C)]
pub union MpvNodeValue {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double_: f64,
    pub list: *mut MpvNodeList,
    pub ba: *mut MpvByteArray,
}

#[repr(C)]
pub struct MpvNode {
    pub value: MpvNodeValue,
    pub format: c_int,
}

#[repr(C)]
pub struct MpvNodeList {
    pub num: c_int,
    pub values: *mut MpvNode,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
pub struct MpvByteArray {
    pub data: *mut c_void,
    pub size: usize,
}

#[repr(C)]
pub struct MpvEvent {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PlayerLuaCallbacks {
    pub ctx: *mut c_void,
    pub log: Option<unsafe extern "C" fn(*mut c_void, c_int, *const c_char)>,
    pub find_config_file:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_char>,
    pub free_string: Option<unsafe extern "C" fn(*mut c_void, *mut c_char)>,
    pub input_set_section_mouse_area: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int, c_int, c_int, c_int),
    >,
}

unsafe extern "C" {
    fn mpv_command_string(ctx: *mut MpvHandle, args: *const c_char) -> c_int;
    fn mpv_command(ctx: *mut MpvHandle, args: *const *const c_char) -> c_int;
    fn mpv_command_node(ctx: *mut MpvHandle, args: *mut MpvNode, result: *mut MpvNode) -> c_int;
    fn mpv_get_property(
        ctx: *mut MpvHandle,
        name: *const c_char,
        format: c_int,
        data: *mut c_void,
    ) -> c_int;
    fn mpv_set_property(
        ctx: *mut MpvHandle,
        name: *const c_char,
        format: c_int,
        data: *mut c_void,
    ) -> c_int;
    fn mpv_del_property(ctx: *mut MpvHandle, name: *const c_char) -> c_int;
    fn mpv_observe_property(
        ctx: *mut MpvHandle,
        reply_userdata: u64,
        name: *const c_char,
        format: c_int,
    ) -> c_int;
    fn mpv_unobserve_property(ctx: *mut MpvHandle, registered: u64) -> c_int;
    fn mpv_request_event(ctx: *mut MpvHandle, event: c_int, enable: c_int) -> c_int;
    fn mpv_event_name(event: c_int) -> *const c_char;
    fn mpv_error_string(error: c_int) -> *const c_char;
    fn mpv_wait_event(ctx: *mut MpvHandle, timeout: f64) -> *mut MpvEvent;
    fn mpv_event_to_node(dst: *mut MpvNode, event: *mut MpvEvent) -> c_int;
    fn mpv_free_node_contents(node: *mut MpvNode);
    fn mpv_free(data: *mut c_void);
}

impl MpvNode {
    fn none() -> Self {
        Self {
            value: MpvNodeValue { int64: 0 },
            format: MPV_FORMAT_NONE,
        }
    }
}

#[derive(Clone, Copy)]
struct Live {
    client: *mut MpvHandle,
    callbacks: PlayerLuaCallbacks,
}

// SAFETY: the handle and callback context are only used while the gate lock is
// held, and the gate is closed before the C script thread releases them.
unsafe impl Send for Live {}

struct ClientGate {
    closing: AtomicBool,
    live: Mutex<Option<Live>>,
}

struct GateGuard<'a> {
    live: Live,
    _lock: MutexGuard<'a, Option<Live>>,
}

impl std::ops::Deref for GateGuard<'_> {
    type Target = Live;

    fn deref(&self) -> &Live {
        &self.live
    }
}

impl ClientGate {
    fn open(client: *mut MpvHandle, callbacks: PlayerLuaCallbacks) -> Arc<Self> {
        Arc::new(Self {
            closing: AtomicBool::new(false),
            live: Mutex::new(Some(Live { client, callbacks })),
        })
    }

    fn enter(&self) -> Result<GateGuard<'_>, String> {
        if self.closing.load(Ordering::SeqCst) {
            return Err(CLIENT_CLOSED.to_owned());
        }
        let lock = self
            .live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *lock {
            Some(live) => Ok(GateGuard { live, _lock: lock }),
            None => Err(CLIENT_CLOSED.to_owned()),
        }
    }

    fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let mut lock = self
            .live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *lock = None;
    }
}

struct ClientBackend {
    gate: Arc<ClientGate>,
}

impl ClientBackend {
    fn error(&self, code: c_int) -> String {
        // SAFETY: mpv_error_string returns a static string for any code.
        let ptr = unsafe { mpv_error_string(code) };
        if ptr.is_null() {
            format!("mpv error {code}")
        } else {
            // SAFETY: the returned pointer is a valid NUL-terminated string.
            unsafe { CStr::from_ptr(ptr) }
                .to_string_lossy()
                .into_owned()
        }
    }

    fn check(&self, code: c_int) -> Result<(), String> {
        if code < 0 {
            Err(self.error(code))
        } else {
            Ok(())
        }
    }

    fn event_id(&self, name: &str) -> Option<c_int> {
        let target = c_string(name).ok()?;
        for id in 0..=255 {
            // SAFETY: mpv_event_name accepts any integer and returns NULL or a static string.
            let ptr = unsafe { mpv_event_name(id) };
            if ptr.is_null() {
                continue;
            }
            // SAFETY: the returned pointer is a valid NUL-terminated string.
            if unsafe { CStr::from_ptr(ptr) }.to_bytes() == target.as_bytes() {
                return Some(id);
            }
        }
        None
    }
}

impl PlayerBackend for ClientBackend {
    fn log(&mut self, level: &str, message: &str) {
        let Ok(live) = self.gate.enter() else {
            return;
        };
        let Some(callback) = live.callbacks.log else {
            return;
        };
        let Some(level) = log_level(level) else {
            return;
        };
        let message = CString::new(message.replace('\0', " ")).unwrap_or_default();
        // SAFETY: the callback and ctx come from the C caller and remain valid for the
        // script lifetime; the message pointer is valid for the call.
        unsafe { callback(live.callbacks.ctx, level, message.as_ptr()) };
    }

    fn command(&mut self, command: player_lua::CommandRequest) -> Result<NativeValue, String> {
        let live = self.gate.enter()?;
        match command {
            player_lua::CommandRequest::String(text) => {
                let text = c_string(&text)?;
                // SAFETY: client is a valid mpv handle; text is NUL-terminated.
                let code = unsafe { mpv_command_string(live.client, text.as_ptr()) };
                self.check(code)?;
                Ok(NativeValue::None)
            }
            player_lua::CommandRequest::Args(args) => {
                let strings = args
                    .iter()
                    .map(|arg| c_string(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut pointers: Vec<*const c_char> =
                    strings.iter().map(|arg| arg.as_ptr()).collect();
                pointers.push(ptr::null());
                // SAFETY: client is valid and pointers is a NULL-terminated string array
                // that stays alive for the call.
                let code = unsafe { mpv_command(live.client, pointers.as_ptr()) };
                self.check(code)?;
                Ok(NativeValue::None)
            }
            player_lua::CommandRequest::Native(value) => {
                let mut storage = NodeStorage::default();
                let mut node = build_node(&value, &mut storage)?;
                let mut result = MpvNode::none();
                // SAFETY: client is valid, node is a fully initialized tree that stays
                // alive for the call, and result is writable.
                let code = unsafe { mpv_command_node(live.client, &mut node, &mut result) };
                if code < 0 {
                    return Err(self.error(code));
                }
                // SAFETY: result was initialized by a successful mpv_command_node.
                let converted = unsafe { node_to_native(&result, 0) };
                // SAFETY: result is owned by this call and must be released.
                unsafe { mpv_free_node_contents(&mut result) };
                converted
            }
        }
    }

    fn get_property(&mut self, name: &str, format: PropertyFormat) -> Result<NativeValue, String> {
        let name = c_string(name)?;
        let live = self.gate.enter()?;
        match format {
            PropertyFormat::String | PropertyFormat::OsdString => {
                let mpv_format = if format == PropertyFormat::OsdString {
                    MPV_FORMAT_OSD_STRING
                } else {
                    MPV_FORMAT_STRING
                };
                let mut value: *mut c_char = ptr::null_mut();
                // SAFETY: client and name are valid; value is writable and mpv allocates it.
                let code = unsafe {
                    mpv_get_property(
                        live.client,
                        name.as_ptr(),
                        mpv_format,
                        &mut value as *mut _ as *mut c_void,
                    )
                };
                if code < 0 {
                    return Err(self.error(code));
                }
                let text = if value.is_null() {
                    String::new()
                } else {
                    // SAFETY: mpv returned a valid NUL-terminated string.
                    unsafe { CStr::from_ptr(value) }
                        .to_string_lossy()
                        .into_owned()
                };
                if !value.is_null() {
                    // SAFETY: value was allocated by mpv_get_property.
                    unsafe { mpv_free(value as *mut c_void) };
                }
                Ok(NativeValue::String(text))
            }
            PropertyFormat::Bool => {
                let mut value: c_int = 0;
                // SAFETY: client and name are valid; value is writable.
                let code = unsafe {
                    mpv_get_property(
                        live.client,
                        name.as_ptr(),
                        MPV_FORMAT_FLAG,
                        &mut value as *mut _ as *mut c_void,
                    )
                };
                self.check(code)?;
                Ok(NativeValue::Bool(value != 0))
            }
            PropertyFormat::Number => {
                let mut value: f64 = 0.0;
                // SAFETY: client and name are valid; value is writable.
                let code = unsafe {
                    mpv_get_property(
                        live.client,
                        name.as_ptr(),
                        MPV_FORMAT_DOUBLE,
                        &mut value as *mut _ as *mut c_void,
                    )
                };
                self.check(code)?;
                Ok(number_value(value))
            }
            PropertyFormat::Native | PropertyFormat::None => {
                let mut node = MpvNode::none();
                // SAFETY: client and name are valid; node is writable.
                let code = unsafe {
                    mpv_get_property(
                        live.client,
                        name.as_ptr(),
                        MPV_FORMAT_NODE,
                        &mut node as *mut _ as *mut c_void,
                    )
                };
                if code < 0 {
                    return Err(self.error(code));
                }
                // SAFETY: node was initialized by a successful mpv_get_property.
                let converted = unsafe { node_to_native(&node, 0) };
                // SAFETY: node is owned by this call and must be released.
                unsafe { mpv_free_node_contents(&mut node) };
                converted
            }
        }
    }

    fn set_property(&mut self, name: &str, value: NativeValue) -> Result<(), String> {
        let name = c_string(name)?;
        let live = self.gate.enter()?;
        match value {
            NativeValue::String(text) => {
                let text = c_string(&text)?;
                let mut pointer = text.as_ptr() as *mut c_char;
                // SAFETY: client and name are valid; MPV_FORMAT_STRING expects a pointer
                // to a string pointer, which stays valid for the call.
                let code = unsafe {
                    mpv_set_property(
                        live.client,
                        name.as_ptr(),
                        MPV_FORMAT_STRING,
                        &mut pointer as *mut _ as *mut c_void,
                    )
                };
                self.check(code)
            }
            NativeValue::Int(value) => {
                let mut value = value;
                // SAFETY: client and name are valid; value is a valid int64.
                let code = unsafe {
                    mpv_set_property(
                        live.client,
                        name.as_ptr(),
                        MPV_FORMAT_INT64,
                        &mut value as *mut _ as *mut c_void,
                    )
                };
                self.check(code)
            }
            NativeValue::Double(value) => {
                let mut value = value;
                // SAFETY: client and name are valid; value is a valid double.
                let code = unsafe {
                    mpv_set_property(
                        live.client,
                        name.as_ptr(),
                        MPV_FORMAT_DOUBLE,
                        &mut value as *mut _ as *mut c_void,
                    )
                };
                self.check(code)
            }
            value => {
                let mut storage = NodeStorage::default();
                let mut node = build_node(&value, &mut storage)?;
                // SAFETY: client and name are valid; node is a complete tree for the call.
                let code = unsafe {
                    mpv_set_property(
                        live.client,
                        name.as_ptr(),
                        MPV_FORMAT_NODE,
                        &mut node as *mut _ as *mut c_void,
                    )
                };
                self.check(code)
            }
        }
    }

    fn del_property(&mut self, name: &str) -> Result<(), String> {
        let name = c_string(name)?;
        let live = self.gate.enter()?;
        // SAFETY: client and name are valid.
        let code = unsafe { mpv_del_property(live.client, name.as_ptr()) };
        self.check(code)
    }

    fn observe_property(
        &mut self,
        id: u64,
        name: &str,
        format: PropertyFormat,
    ) -> Result<(), String> {
        let name = c_string(name)?;
        let live = self.gate.enter()?;
        let format = match format {
            PropertyFormat::None => MPV_FORMAT_NONE,
            PropertyFormat::Native => MPV_FORMAT_NODE,
            PropertyFormat::Bool => MPV_FORMAT_FLAG,
            PropertyFormat::String | PropertyFormat::OsdString => MPV_FORMAT_STRING,
            PropertyFormat::Number => MPV_FORMAT_DOUBLE,
        };
        // SAFETY: client and name are valid.
        let code = unsafe { mpv_observe_property(live.client, id, name.as_ptr(), format) };
        self.check(code)
    }

    fn unobserve_property(&mut self, id: u64) -> i32 {
        let Ok(live) = self.gate.enter() else {
            return CLIENT_CLOSED_CODE;
        };
        // SAFETY: client is valid; id was returned by an observation call.
        unsafe { mpv_unobserve_property(live.client, id) }
    }

    fn request_event(&mut self, name: &str, enable: bool) -> bool {
        let Some(id) = self.event_id(name) else {
            return false;
        };
        let Ok(live) = self.gate.enter() else {
            return false;
        };
        // SAFETY: client is valid and id names a client event.
        unsafe { mpv_request_event(live.client, id, enable as c_int) >= 0 }
    }

    fn find_config_file(&mut self, name: &str) -> Option<String> {
        let live = self.gate.enter().ok()?;
        let callback = live.callbacks.find_config_file?;
        let name = c_string(name).ok()?;
        // SAFETY: the callback and ctx come from the C caller and remain valid for the
        // script lifetime; name is NUL-terminated.
        let pointer = unsafe { callback(live.callbacks.ctx, name.as_ptr()) };
        if pointer.is_null() {
            return None;
        }
        // SAFETY: the callback returned a valid NUL-terminated string.
        let value = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        if let Some(free) = live.callbacks.free_string {
            // SAFETY: free_string releases exactly the pointer returned by the callback.
            unsafe { free(live.callbacks.ctx, pointer) };
        }
        Some(value)
    }

    fn format_time(&mut self, seconds: f64, format: &str) -> Result<String, String> {
        format_time_fmt(format, seconds)
    }

    fn input_set_section_mouse_area(
        &mut self,
        section: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) {
        let Ok(live) = self.gate.enter() else {
            return;
        };
        let Some(callback) = live.callbacks.input_set_section_mouse_area else {
            return;
        };
        let section = CString::new(section.replace('\0', " ")).unwrap_or_default();
        // SAFETY: the callback and ctx come from the C caller and remain valid for the
        // script lifetime; section is NUL-terminated.
        unsafe {
            callback(
                live.callbacks.ctx,
                section.as_ptr(),
                x0 as c_int,
                y0 as c_int,
                x1 as c_int,
                y1 as c_int,
            )
        };
    }
}

#[derive(Default)]
struct NodeStorage {
    strings: Vec<CString>,
    node_arrays: Vec<Vec<MpvNode>>,
    key_arrays: Vec<Vec<CString>>,
    key_pointers: Vec<Vec<*mut c_char>>,
    // Box keeps each list at a stable address while the storage vectors grow.
    #[allow(clippy::vec_box)]
    lists: Vec<Box<MpvNodeList>>,
}

fn build_node(value: &NativeValue, storage: &mut NodeStorage) -> Result<MpvNode, String> {
    Ok(match value {
        NativeValue::None => MpvNode::none(),
        NativeValue::Bool(value) => MpvNode {
            value: MpvNodeValue {
                flag: if *value { 1 } else { 0 },
            },
            format: MPV_FORMAT_FLAG,
        },
        NativeValue::Int(value) => MpvNode {
            value: MpvNodeValue { int64: *value },
            format: MPV_FORMAT_INT64,
        },
        NativeValue::Double(value) => MpvNode {
            value: MpvNodeValue { double_: *value },
            format: MPV_FORMAT_DOUBLE,
        },
        NativeValue::String(value) => {
            let value = c_string(value)?;
            let pointer = value.as_ptr() as *mut c_char;
            storage.strings.push(value);
            MpvNode {
                value: MpvNodeValue { string: pointer },
                format: MPV_FORMAT_STRING,
            }
        }
        NativeValue::Array(values) => {
            let mut nodes = Vec::with_capacity(values.len());
            for value in values {
                nodes.push(build_node(value, storage)?);
            }
            let list = Box::new(MpvNodeList {
                num: nodes.len() as c_int,
                values: nodes.as_mut_ptr(),
                keys: ptr::null_mut(),
            });
            storage.node_arrays.push(nodes);
            let index = storage.lists.len();
            storage.lists.push(list);
            let list = storage.lists[index].as_mut() as *mut MpvNodeList;
            MpvNode {
                value: MpvNodeValue { list },
                format: MPV_FORMAT_NODE_ARRAY,
            }
        }
        NativeValue::Map(entries) => {
            let mut nodes = Vec::with_capacity(entries.len());
            let mut keys = Vec::with_capacity(entries.len());
            for (key, value) in entries {
                keys.push(c_string(key)?);
                nodes.push(build_node(value, storage)?);
            }
            let mut key_pointers: Vec<*mut c_char> =
                keys.iter().map(|key| key.as_ptr() as *mut c_char).collect();
            let list = Box::new(MpvNodeList {
                num: nodes.len() as c_int,
                values: nodes.as_mut_ptr(),
                keys: key_pointers.as_mut_ptr(),
            });
            storage.node_arrays.push(nodes);
            storage.key_arrays.push(keys);
            storage.key_pointers.push(key_pointers);
            let index = storage.lists.len();
            storage.lists.push(list);
            let list = storage.lists[index].as_mut() as *mut MpvNodeList;
            MpvNode {
                value: MpvNodeValue { list },
                format: MPV_FORMAT_NODE_MAP,
            }
        }
        NativeValue::Unknown => {
            return Err("cannot send an unknown native value".to_owned())
        }
    })
}

unsafe fn node_to_native(node: *const MpvNode, depth: usize) -> Result<NativeValue, String> {
    if node.is_null() {
        return Err("null native node".to_owned());
    }
    if depth > MAX_NODE_DEPTH {
        return Err("native node nesting is too deep".to_owned());
    }
    // SAFETY: node is non-null and points to a valid node for this call.
    let node = unsafe { &*node };
    match node.format {
        MPV_FORMAT_NONE => Ok(NativeValue::None),
        MPV_FORMAT_STRING | MPV_FORMAT_OSD_STRING => {
            // SAFETY: string nodes carry a valid NUL-terminated pointer or NULL.
            let pointer = unsafe { node.value.string };
            if pointer.is_null() {
                Ok(NativeValue::String(String::new()))
            } else {
                // SAFETY: pointer is a valid NUL-terminated string.
                Ok(NativeValue::String(
                    unsafe { CStr::from_ptr(pointer) }
                        .to_string_lossy()
                        .into_owned(),
                ))
            }
        }
        MPV_FORMAT_FLAG => {
            // SAFETY: flag nodes carry a c_int.
            Ok(NativeValue::Bool(unsafe { node.value.flag } != 0))
        }
        MPV_FORMAT_INT64 => {
            // SAFETY: int64 nodes carry an i64.
            Ok(NativeValue::Int(unsafe { node.value.int64 }))
        }
        MPV_FORMAT_DOUBLE => {
            // SAFETY: double nodes carry an f64.
            Ok(NativeValue::Double(unsafe { node.value.double_ }))
        }
        MPV_FORMAT_NODE_ARRAY => {
            // SAFETY: array nodes carry a list pointer or NULL.
            let list = unsafe { node.value.list };
            if list.is_null() {
                return Ok(NativeValue::Array(Vec::new()));
            }
            // SAFETY: list points to a valid node list for this call.
            let list = unsafe { &*list };
            if list.num < 0 {
                return Err("negative native array length".to_owned());
            }
            let mut values = Vec::with_capacity(list.num as usize);
            for index in 0..list.num as usize {
                // SAFETY: the list contains num values.
                let child = unsafe { list.values.add(index) };
                values.push(unsafe { node_to_native(child, depth + 1)? });
            }
            Ok(NativeValue::Array(values))
        }
        MPV_FORMAT_NODE_MAP => {
            // SAFETY: map nodes carry a list pointer or NULL.
            let list = unsafe { node.value.list };
            if list.is_null() {
                return Ok(NativeValue::Map(Vec::new()));
            }
            // SAFETY: list points to a valid node list for this call.
            let list = unsafe { &*list };
            if list.num < 0 {
                return Err("negative native map length".to_owned());
            }
            if list.num > 0 && (list.keys.is_null() || list.values.is_null()) {
                return Err("native map is missing keys or values".to_owned());
            }
            let mut entries = Vec::with_capacity(list.num as usize);
            for index in 0..list.num as usize {
                // SAFETY: the list contains num keys and values.
                let key = unsafe { *list.keys.add(index) };
                if key.is_null() {
                    return Err("native map has a null key".to_owned());
                }
                // SAFETY: key is a valid NUL-terminated string.
                let key = unsafe { CStr::from_ptr(key) }
                    .to_string_lossy()
                    .into_owned();
                // SAFETY: the list contains num values.
                let child = unsafe { list.values.add(index) };
                entries.push((key, unsafe { node_to_native(child, depth + 1)? }));
            }
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            Ok(NativeValue::Map(entries))
        }
        MPV_FORMAT_BYTE_ARRAY => {
            // SAFETY: byte-array nodes carry a byte-array pointer or NULL.
            let array = unsafe { node.value.ba };
            if array.is_null() {
                return Ok(NativeValue::String(String::new()));
            }
            // SAFETY: array points to a valid byte array for this call.
            let array = unsafe { &*array };
            if array.size == 0 {
                return Ok(NativeValue::String(String::new()));
            }
            if array.data.is_null() {
                return Err("native byte array has a null data pointer".to_owned());
            }
            // SAFETY: data points to size readable bytes.
            let bytes = unsafe { slice::from_raw_parts(array.data as *const u8, array.size) };
            Ok(NativeValue::String(
                String::from_utf8_lossy(bytes).into_owned(),
            ))
        }
        _ => Ok(NativeValue::Unknown),
    }
}

fn event_from_native(value: NativeValue) -> Option<ScriptEvent> {
    let NativeValue::Map(entries) = value else {
        return None;
    };
    let mut name = None;
    let mut fields = Vec::new();
    for (key, value) in entries {
        if key == "event" {
            if let NativeValue::String(value) = value {
                name = Some(value);
            }
        } else {
            fields.push((key, value));
        }
    }
    name.map(|name| ScriptEvent { name, fields })
}

fn pump_events(
    client: *mut MpvHandle,
    host: &mut ScriptHost,
    callbacks: &PlayerLuaCallbacks,
) -> Result<(), String> {
    let sender = host.event_sender();
    let outcome = loop {
        if let Some(result) = host.poll() {
            break result;
        }
        // SAFETY: client is a valid mpv handle owned by this thread, which is the only
        // thread that waits for events on it.
        let event = unsafe { mpv_wait_event(client, EVENT_POLL_SECONDS) };
        if event.is_null() {
            continue;
        }
        // SAFETY: mpv_wait_event returned a valid event for this call.
        let event_id = unsafe { (*event).event_id };
        if event_id == MPV_EVENT_NONE {
            continue;
        }
        let shutdown = event_id == MPV_EVENT_SHUTDOWN;
        let mut node = MpvNode::none();
        // SAFETY: event is valid and event_to_node fills a node owned by us.
        let code = unsafe { mpv_event_to_node(&mut node, event) };
        if code >= 0 {
            // SAFETY: node was initialized by mpv_event_to_node.
            let converted = unsafe { node_to_native(&node, 0) };
            // SAFETY: node is owned by this call and must be released.
            unsafe { mpv_free_node_contents(&mut node) };
            match converted {
                Ok(value) => match event_from_native(value) {
                    Some(event) => {
                        if sender.post(event).is_err() {
                            break host.wait_timeout(SHUTDOWN_TIMEOUT);
                        }
                    }
                    None => log_message(
                        callbacks,
                        MSGL_WARN,
                        "dropping a Lua event with no event name",
                    ),
                },
                Err(error) => log_message(
                    callbacks,
                    MSGL_ERR,
                    &format!("cannot convert a Lua event: {error}"),
                ),
            }
        }
        if shutdown {
            break host.wait_timeout(SHUTDOWN_TIMEOUT);
        }
    };
    outcome.map_err(|error| match error {
        HostError::ShutdownTimeout => {
            format!("script did not stop within {} s and was abandoned", SHUTDOWN_TIMEOUT.as_secs())
        }
        other => other.to_string(),
    })
}

/// # Safety
///
/// `client` must be a valid mpv handle for the script, `name`, `filename`, and
/// `path` must be valid NUL-terminated strings, and `callbacks` must point to a
/// callback table that stays valid until the function returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn player_ffi_lua_run(
    client: *mut c_void,
    name: *const c_char,
    filename: *const c_char,
    path: *const c_char,
    callbacks: *const PlayerLuaCallbacks,
) -> c_int {
    if callbacks.is_null() {
        return -1;
    }
    // SAFETY: the caller guarantees a valid callback struct for this call.
    let callbacks_copy = unsafe { *callbacks };
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
        if client.is_null() || name.is_null() || filename.is_null() {
            return Err("missing script host arguments".to_owned());
        }
        // SAFETY: the caller guarantees NUL-terminated strings.
        let name = unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: the caller guarantees a NUL-terminated filename.
        let filename = unsafe { CStr::from_ptr(filename) }
            .to_string_lossy()
            .into_owned();
        let _ = path;
        let source = match builtin_source(&filename) {
            Some(source) => source.to_owned(),
            None => std::fs::read_to_string(&filename)
                .map_err(|error| format!("cannot read {filename}: {error}"))?,
        };
        let modules = vec![
            ScriptModule::new("mp.assdraw", ASSDRAW),
            ScriptModule::new("mp.options", OPTIONS),
            ScriptModule::new("mp.input", INPUT),
        ];
        let client = client.cast::<MpvHandle>();
        let gate = ClientGate::open(client, callbacks_copy);
        let backend = ClientBackend { gate: gate.clone() };
        let mut host = ScriptHost::spawn_with_modules(
            name.clone(),
            format!("@{filename}"),
            source,
            modules,
            backend,
        )
        .map_err(|error| error.to_string())?;
        let result = pump_events(client, &mut host, &callbacks_copy);
        gate.close();
        result
    }));
    match result {
        Ok(Ok(())) => 0,
        Ok(Err(error)) => {
            log_message(&callbacks_copy, MSGL_ERR, &error);
            -1
        }
        Err(payload) => {
            let message = if let Some(text) = payload.downcast_ref::<&str>() {
                (*text).to_owned()
            } else if let Some(text) = payload.downcast_ref::<String>() {
                text.clone()
            } else {
                "panic in the Lua host".to_owned()
            };
            log_message(
                &callbacks_copy,
                MSGL_ERR,
                &format!("Lua host panic: {message}"),
            );
            -1
        }
    }
}

fn log_message(callbacks: &PlayerLuaCallbacks, level: c_int, message: &str) {
    let Some(log) = callbacks.log else {
        return;
    };
    let message = CString::new(message.replace('\0', " ")).unwrap_or_default();
    // SAFETY: callbacks and ctx stay valid until the C script thread returns.
    unsafe { log(callbacks.ctx, level, message.as_ptr()) };
}

fn builtin_source(filename: &str) -> Option<&'static str> {
    match filename {
        "@osc.lua" => Some(OSC),
        _ => None,
    }
}

fn c_string(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "string contains NUL".to_owned())
}

fn number_value(value: f64) -> NativeValue {
    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        NativeValue::Int(value as i64)
    } else {
        NativeValue::Double(value)
    }
}

fn log_level(level: &str) -> Option<c_int> {
    match level {
        "fatal" => Some(0),
        "error" => Some(1),
        "warn" => Some(2),
        "info" => Some(3),
        "v" | "verbose" => Some(5),
        "debug" => Some(6),
        "trace" => Some(7),
        _ => None,
    }
}

fn format_time_fmt(format: &str, time: f64) -> Result<String, String> {
    if time == i64::MIN as f64 {
        return Ok("unknown".to_owned());
    }
    let sign = if time < 0.0 { "-" } else { "" };
    let time = time.abs();
    let mut whole = time as i64;
    let mut milliseconds = ((time - whole as f64) * 1000.0).round() as i64;
    if milliseconds >= 1000 {
        milliseconds -= 1000;
        whole += 1;
    }
    let total_seconds = whole;
    let total_minutes = total_seconds / 60;
    let hours = total_seconds / 3600;
    let seconds_in_hour = total_seconds - hours * 3600;
    let minutes = seconds_in_hour / 60;
    let seconds = seconds_in_hour - minutes * 60;
    let mut output = String::new();
    let mut chars = format.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('h') => output.push_str(&format!("{sign}{hours}")),
            Some('H') => output.push_str(&format!("{sign}{hours:02}")),
            Some('m') => output.push_str(&format!("{sign}{total_minutes}")),
            Some('M') => output.push_str(&format!("{minutes:02}")),
            Some('s') => output.push_str(&format!("{sign}{total_seconds}")),
            Some('S') => output.push_str(&format!("{seconds:02}")),
            Some('T') => output.push_str(&format!("{milliseconds:03}")),
            Some('f') => output.push_str(&format!("{time:.6}")),
            Some('%') => output.push('%'),
            _ => return Err(format!("Invalid time format string '{format}'")),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_time_like_the_c_helper() {
        assert_eq!(format_time_fmt("%H:%M:%S", 3661.5).unwrap(), "01:01:01");
        assert_eq!(format_time_fmt("%M:%S", 61.0).unwrap(), "01:01");
        assert_eq!(format_time_fmt("%s.%T", 5.25).unwrap(), "5.250");
        assert_eq!(format_time_fmt("%H:%M:%S", -61.0).unwrap(), "-00:01:01");
        assert!(format_time_fmt("%Q", 1.0).is_err());
    }

    #[test]
    fn builds_and_reads_native_nodes() {
        let value = NativeValue::Map(vec![
            ("answer".to_owned(), NativeValue::Int(42)),
            (
                "list".to_owned(),
                NativeValue::Array(vec![
                    NativeValue::String("a".to_owned()),
                    NativeValue::Double(1.5),
                ]),
            ),
        ]);
        let mut storage = NodeStorage::default();
        let node = build_node(&value, &mut storage).unwrap();
        // SAFETY: node and its storage are alive for the conversion.
        let converted = unsafe { node_to_native(&node, 0) }.unwrap();
        assert_eq!(converted, value);
    }

    #[test]
    fn rejects_unknown_nodes_for_commands() {
        let mut storage = NodeStorage::default();
        assert!(build_node(&NativeValue::Unknown, &mut storage).is_err());
    }
}
