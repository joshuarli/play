use std::cell::RefCell;
use std::fmt;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use mlua::{
    Error, Function, Lua, MultiValue, Result as LuaResult, Table, Value, Variadic,
};

use crate::backend::{CommandRequest, PlayerBackend, PropertyFormat, ScriptEvent};
use crate::new_interpreter;
use crate::value::{NativeValue, Sentinels};

const DEFAULTS: &str = include_str!("../../../player/lua/defaults.lua");
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_WAIT_SECONDS: f64 = 86_400.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    Lua(String),
    Script(String),
    Panic(String),
    PostClosed,
    ShutdownTimeout,
    Thread(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Lua(message) => write!(formatter, "Lua error: {message}"),
            HostError::Script(message) => write!(formatter, "script error: {message}"),
            HostError::Panic(message) => write!(formatter, "panic: {message}"),
            HostError::PostClosed => write!(formatter, "script event queue is closed"),
            HostError::ShutdownTimeout => write!(formatter, "script did not stop within 2 s"),
            HostError::Thread(message) => write!(formatter, "script thread error: {message}"),
        }
    }
}

impl std::error::Error for HostError {}

struct HostState<B: PlayerBackend> {
    backend: B,
    events: Receiver<ScriptEvent>,
    sentinels: Sentinels,
    start: Instant,
    script_name: String,
}

pub struct ScriptModule {
    pub name: String,
    pub source: String,
}

impl ScriptModule {
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
        }
    }
}

#[derive(Clone)]
pub struct ScriptEventSender {
    events: Sender<ScriptEvent>,
}

impl ScriptEventSender {
    pub fn post(&self, event: ScriptEvent) -> Result<(), HostError> {
        self.events.send(event).map_err(|_| HostError::PostClosed)
    }
}

pub struct ScriptHost {
    events: Sender<ScriptEvent>,
    done: Receiver<Result<(), HostError>>,
    thread: Option<JoinHandle<()>>,
    stopped: bool,
}

impl ScriptHost {
    pub fn spawn<B: PlayerBackend>(
        name: impl Into<String>,
        chunk_name: impl Into<String>,
        source: impl Into<String>,
        backend: B,
    ) -> Result<Self, HostError> {
        Self::spawn_with_modules(name, chunk_name, source, Vec::new(), backend)
    }

    pub fn spawn_with_modules<B: PlayerBackend>(
        name: impl Into<String>,
        chunk_name: impl Into<String>,
        source: impl Into<String>,
        modules: Vec<ScriptModule>,
        backend: B,
    ) -> Result<Self, HostError> {
        let name = name.into();
        let chunk_name = chunk_name.into();
        let source = source.into();
        let (event_tx, event_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let thread = thread::Builder::new()
            .name(format!("lua:{name}"))
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    run_script(&name, &chunk_name, &source, modules, backend, event_rx)
                }))
                .unwrap_or_else(|payload| Err(HostError::Panic(panic_message(&payload))));
                let _ = done_tx.send(result);
            })
            .map_err(|error| HostError::Thread(error.to_string()))?;
        Ok(Self {
            events: event_tx,
            done: done_rx,
            thread: Some(thread),
            stopped: false,
        })
    }

    pub fn event_sender(&self) -> ScriptEventSender {
        ScriptEventSender {
            events: self.events.clone(),
        }
    }

    pub fn post_event(&self, event: ScriptEvent) -> Result<(), HostError> {
        self.events.send(event).map_err(|_| HostError::PostClosed)
    }

    pub fn wait(&mut self) -> Result<(), HostError> {
        if self.stopped {
            return Ok(());
        }
        let result = self.done.recv().map_err(|_| {
            HostError::Thread("script thread exited without a result".into())
        })?;
        self.stopped = true;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        result
    }

    pub fn wait_timeout(&mut self, timeout: Duration) -> Result<(), HostError> {
        if self.stopped {
            return Ok(());
        }
        let result = match self.done.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => return Err(HostError::ShutdownTimeout),
            Err(RecvTimeoutError::Disconnected) => {
                return Err(HostError::Thread(
                    "script thread exited without a result".into(),
                ))
            }
        };
        self.stopped = true;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        result
    }

    pub fn stop(&mut self) -> Result<(), HostError> {
        if self.stopped {
            return Ok(());
        }
        self.stopped = true;
        let _ = self.events.send(ScriptEvent::shutdown());
        let result = match self.done.recv_timeout(SHUTDOWN_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(HostError::ShutdownTimeout),
            Err(RecvTimeoutError::Disconnected) => Err(HostError::Thread(
                "script thread exited without a result".into(),
            )),
        };
        if !matches!(result, Err(HostError::ShutdownTimeout)) {
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        } else {
            let _ = self.thread.take();
        }
        result
    }
}

impl Drop for ScriptHost {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn run_script<B: PlayerBackend>(
    name: &str,
    chunk_name: &str,
    source: &str,
    modules: Vec<ScriptModule>,
    backend: B,
    events: Receiver<ScriptEvent>,
) -> Result<(), HostError> {
    let lua = new_interpreter().map_err(|error| HostError::Lua(error.to_string()))?;
    let sentinels = create_sentinels(&lua).map_err(|error| HostError::Lua(error.to_string()))?;
    let state = Rc::new(RefCell::new(HostState {
        backend,
        events,
        sentinels,
        start: Instant::now(),
        script_name: name.to_owned(),
    }));
    register_mp(&lua, &state)?;

    let defaults: Value = lua
        .load(DEFAULTS)
        .set_name("@mp.defaults")
        .eval()
        .map_err(|error| HostError::Lua(error.to_string()))?;
    let package: Table = lua
        .globals()
        .get("package")
        .map_err(|error| HostError::Lua(error.to_string()))?;
    let loaded: Table = package
        .get("loaded")
        .map_err(|error| HostError::Lua(error.to_string()))?;
    loaded
        .set(
            "mp.defaults",
            if defaults.is_nil() {
                Value::Boolean(true)
            } else {
                defaults
            },
        )
        .map_err(|error| HostError::Lua(error.to_string()))?;
    for module in modules {
        let value: Value = lua
            .load(&module.source)
            .set_name(format!("@{}", module.name))
            .eval()
            .map_err(|error| HostError::Lua(format!("{}: {error}", module.name)))?;
        loaded
            .set(
                module.name,
                if value.is_nil() {
                    Value::Boolean(true)
                } else {
                    value
                },
            )
            .map_err(|error| HostError::Lua(error.to_string()))?;
    }
    lua.load(source)
        .set_name(chunk_name)
        .exec()
        .map_err(|error| HostError::Script(error.to_string()))?;

    let event_loop: Function = lua
        .globals()
        .get("mp_event_loop")
        .map_err(|error| HostError::Script(format!("missing mp_event_loop: {error}")))?;
    event_loop
        .call::<()>(())
        .map_err(|error| HostError::Script(error.to_string()))
}

fn create_sentinels(lua: &Lua) -> LuaResult<Sentinels> {
    Ok(Sentinels {
        unknown: lua.create_table()?,
        array: lua.create_table()?,
        map: lua.create_table()?,
    })
}

fn register_mp<B: PlayerBackend>(
    lua: &Lua,
    state: &Rc<RefCell<HostState<B>>>,
) -> Result<(), HostError> {
    let (sentinels, script_name) = {
        let state = state.borrow();
        (state.sentinels.clone(), state.script_name.clone())
    };
    let mp = lua
        .create_table()
        .map_err(|error| HostError::Lua(error.to_string()))?;
    mp.set("script_name", script_name)
        .map_err(|error| HostError::Lua(error.to_string()))?;
    mp.set("UNKNOWN_TYPE", sentinels.unknown.clone())
        .map_err(|error| HostError::Lua(error.to_string()))?;
    mp.set("ARRAY", sentinels.array.clone())
        .map_err(|error| HostError::Lua(error.to_string()))?;
    mp.set("MAP", sentinels.map.clone())
        .map_err(|error| HostError::Lua(error.to_string()))?;
    lua.globals()
        .set("mp", mp.clone())
        .map_err(|error| HostError::Lua(error.to_string()))?;

    let utils = lua
        .create_table()
        .map_err(|error| HostError::Lua(error.to_string()))?;
    mp.set("utils", utils.clone())
        .map_err(|error| HostError::Lua(error.to_string()))?;
    let package: Table = lua
        .globals()
        .get("package")
        .map_err(|error| HostError::Lua(error.to_string()))?;
    let loaded: Table = package
        .get("loaded")
        .map_err(|error| HostError::Lua(error.to_string()))?;
    register_utils(lua, &utils, &sentinels)?;
    loaded
        .set("mp.utils", utils)
        .map_err(|error| HostError::Lua(error.to_string()))?;

    register_log(lua, &mp, state)?;
    register_properties(lua, &mp, state, &sentinels)?;
    register_commands(lua, &mp, state, &sentinels)?;
    register_events(lua, &mp, state, &sentinels)?;
    Ok(())
}

fn register_utils(
    lua: &Lua,
    utils: &Table,
    sentinels: &Sentinels,
) -> Result<(), HostError> {
    let sentinels_clone = sentinels.clone();
    register_fn(
        lua,
        utils,
        "readdir",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(path) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("path is required".into()));
                };
                let filter = match values.get(1) {
                    None | Some(Value::Nil) => "normal".to_owned(),
                    Some(value) => value_to_string(value).ok_or_else(|| {
                        Error::RuntimeError("filter must be a string".into())
                    })?,
                };
                let mut result = MultiValue::new();
                match crate::utils::readdir(&path, &filter) {
                    Ok(value) => result.push_back(value.to_lua(lua, &sentinels_clone)?),
                    Err(_) => {
                        result.push_back(Value::Nil);
                        result.push_back(Value::String(lua.create_string("error")?));
                    }
                }
                Ok(result)
            })
        },
    )?;

    let sentinels_clone = sentinels.clone();
    register_fn(
        lua,
        utils,
        "file_info",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(path) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("path is required".into()));
                };
                let mut result = MultiValue::new();
                match crate::utils::file_info(&path) {
                    Ok(value) => result.push_back(value.to_lua(lua, &sentinels_clone)?),
                    Err(_) => {
                        result.push_back(Value::Nil);
                        result.push_back(Value::String(lua.create_string("error")?));
                    }
                }
                Ok(result)
            })
        },
    )?;

    register_fn(
        lua,
        utils,
        "split_path",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(path) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("path is required".into()));
                };
                let (directory, filename) = crate::utils::split_path(&path);
                let mut result = MultiValue::new();
                result.push_back(Value::String(lua.create_string(&directory)?));
                result.push_back(Value::String(lua.create_string(&filename)?));
                Ok(result)
            })
        },
    )?;

    let sentinels_clone = sentinels.clone();
    register_fn(
        lua,
        utils,
        "parse_json",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(text) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("text is required".into()));
                };
                let trailing = values
                    .get(1)
                    .map(|value| !matches!(value, Value::Nil | Value::Boolean(false)))
                    .unwrap_or(false);
                let mut result = MultiValue::new();
                match crate::utils::parse_json(&text, trailing) {
                    Ok((value, remainder)) => {
                        result.push_back(value.to_lua(lua, &sentinels_clone)?);
                        result.push_back(Value::Nil);
                        result.push_back(Value::String(lua.create_string(remainder)?));
                    }
                    Err(_) => {
                        result.push_back(Value::Nil);
                        result.push_back(Value::String(lua.create_string("error")?));
                        result.push_back(Value::String(lua.create_string(&text)?));
                    }
                }
                Ok(result)
            })
        },
    )?;

    let sentinels_clone = sentinels.clone();
    register_fn(
        lua,
        utils,
        "format_json",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let value = match values.first() {
                    Some(value) => NativeValue::from_lua(lua, value.clone(), &sentinels_clone)?,
                    None => NativeValue::None,
                };
                let mut result = MultiValue::new();
                match crate::utils::format_json(&value) {
                    Ok(text) => result.push_back(Value::String(lua.create_string(&text)?)),
                    Err(_) => {
                        result.push_back(Value::Nil);
                        result.push_back(Value::String(lua.create_string("error")?));
                    }
                }
                Ok(result)
            })
        },
    )
}

fn register_fn(
    lua: &Lua,
    table: &Table,
    name: &str,
    function: impl Fn(&Lua, Variadic<Value>) -> LuaResult<MultiValue> + 'static,
) -> Result<(), HostError> {
    let function = lua
        .create_function(function)
        .map_err(|error| HostError::Lua(error.to_string()))?;
    table
        .set(name, function)
        .map_err(|error| HostError::Lua(error.to_string()))
}

fn register_log<B: PlayerBackend>(
    lua: &Lua,
    mp: &Table,
    state: &Rc<RefCell<HostState<B>>>,
) -> Result<(), HostError> {
    let state = state.clone();
    register_fn(
        lua,
        mp,
        "log",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(level) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("log level is required".into()));
                };
                match level.as_str() {
                    "fatal" | "error" | "warn" | "info" | "v" | "verbose" | "debug"
                    | "trace" => {}
                    other => {
                        return Err(Error::RuntimeError(format!(
                            "Invalid log level '{other}'"
                        )))
                    }
                }
                let tostring: Function = lua.globals().get("tostring")?;
                let mut parts = Vec::with_capacity(values.len().saturating_sub(1));
                for value in values.iter().skip(1) {
                    let text: mlua::LuaString = tostring.call(value.clone())?;
                    parts.push(text.to_str()?.to_owned());
                }
                state
                    .borrow_mut()
                    .backend
                    .log(&level, &parts.join(" "));
                Ok(MultiValue::new())
            })
        },
    )
}

fn register_properties<B: PlayerBackend>(
    lua: &Lua,
    mp: &Table,
    state: &Rc<RefCell<HostState<B>>>,
    sentinels: &Sentinels,
) -> Result<(), HostError> {
    for (name, format, osd) in [
        ("get_property", PropertyFormat::String, false),
        ("get_property_osd", PropertyFormat::OsdString, true),
        ("get_property_bool", PropertyFormat::Bool, false),
        ("get_property_number", PropertyFormat::Number, false),
        ("get_property_native", PropertyFormat::Native, false),
    ] {
        let state = state.clone();
        let sentinels = sentinels.clone();
        register_fn(
            lua,
            mp,
            name,
            move |lua, values: Variadic<Value>| {
                guard(|| {
                    let (property, default) = property_args(&values)?;
                    let result = state
                        .borrow_mut()
                        .backend
                        .get_property(&property, format);
                    value_or_error(lua, &sentinels, result, default, osd)
                })
            },
        )?;
    }

    for (name, format) in [
        ("set_property", PropertyFormat::String),
        ("set_property_native", PropertyFormat::Native),
        ("set_property_number", PropertyFormat::Number),
    ] {
        let state = state.clone();
        let sentinels = sentinels.clone();
        register_fn(
            lua,
            mp,
            name,
            move |lua, values: Variadic<Value>| {
                guard(|| {
                    let Some(property) = values.first().and_then(value_to_string) else {
                        return Err(Error::RuntimeError("property name is required".into()));
                    };
                    let Some(value) = values.get(1) else {
                        return Err(Error::RuntimeError("property value is required".into()));
                    };
                    let value = match format {
                        PropertyFormat::String => {
                            let Some(value) = lua.coerce_string(value.clone())? else {
                                return Err(Error::RuntimeError("string expected".into()));
                            };
                            NativeValue::String(value.to_str()?.to_owned())
                        }
                        PropertyFormat::Number => {
                            let Some(value) = lua.coerce_number(value.clone())? else {
                                return Err(Error::RuntimeError("number expected".into()));
                            };
                            number_value(value)
                        }
                        _ => NativeValue::from_lua(lua, value.clone(), &sentinels)?,
                    };
                    let result = state
                        .borrow_mut()
                        .backend
                        .set_property(&property, value);
                    ok_or_error(lua, result)
                })
            },
        )?;
    }

    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "del_property",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(property) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("property name is required".into()));
                };
                let result = state_clone.borrow_mut().backend.del_property(&property);
                ok_or_error(lua, result)
            })
        },
    )?;

    for (name, observe) in [
        ("raw_observe_property", true),
        ("raw_unobserve_property", false),
    ] {
        let state = state.clone();
        register_fn(
            lua,
            mp,
            name,
            move |lua, values: Variadic<Value>| {
                guard(|| {
                    let Some(id) = values.first().and_then(value_as_number) else {
                        return Err(Error::RuntimeError("observation id is required".into()));
                    };
                    let id = id as u64;
                    if observe {
                        let Some(property) = values.get(1).and_then(value_to_string) else {
                            return Err(Error::RuntimeError(
                                "property name is required".into(),
                            ));
                        };
                        let format = property_format(values.get(2))?;
                        let result = state
                            .borrow_mut()
                            .backend
                            .observe_property(id, &property, format);
                        ok_or_error(lua, result)
                    } else {
                        let count = state.borrow_mut().backend.unobserve_property(id);
                        let mut result = MultiValue::new();
                        result.push_back(Value::Number(count as f64));
                        Ok(result)
                    }
                })
            },
        )?;
    }

    Ok(())
}

fn register_commands<B: PlayerBackend>(
    lua: &Lua,
    mp: &Table,
    state: &Rc<RefCell<HostState<B>>>,
    sentinels: &Sentinels,
) -> Result<(), HostError> {
    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "command",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(command) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("command is required".into()));
                };
                let result = state_clone
                    .borrow_mut()
                    .backend
                    .command(CommandRequest::String(command));
                ok_or_error(lua, result.map(|_| ()))
            })
        },
    )?;

    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "commandv",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let mut args = Vec::with_capacity(values.len());
                for value in values.iter() {
                    let Some(text) = value_to_string(value) else {
                        return Err(Error::RuntimeError("argument is not a string".into()));
                    };
                    args.push(text);
                }
                let result = state_clone
                    .borrow_mut()
                    .backend
                    .command(CommandRequest::Args(args));
                ok_or_error(lua, result.map(|_| ()))
            })
        },
    )?;

    let state_clone = state.clone();
    let sentinels_clone = sentinels.clone();
    register_fn(
        lua,
        mp,
        "command_native",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(value) = values.first() else {
                    return Err(Error::RuntimeError("command is required".into()));
                };
                let command = NativeValue::from_lua(lua, value.clone(), &sentinels_clone)?;
                let default = values.get(1).cloned();
                let result = state_clone
                    .borrow_mut()
                    .backend
                    .command(CommandRequest::Native(command));
                command_result(lua, &sentinels_clone, result, default)
            })
        },
    )
}

fn register_events<B: PlayerBackend>(
    lua: &Lua,
    mp: &Table,
    state: &Rc<RefCell<HostState<B>>>,
    sentinels: &Sentinels,
) -> Result<(), HostError> {
    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "request_event",
        move |_lua, values: Variadic<Value>| {
            guard(|| {
                let Some(name) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("event name is required".into()));
                };
                let enable = values
                    .get(1)
                    .map(|value| !matches!(value, Value::Nil | Value::Boolean(false)))
                    .unwrap_or(false);
                let enabled = state_clone
                    .borrow_mut()
                    .backend
                    .request_event(&name, enable);
                let mut result = MultiValue::new();
                result.push_back(Value::Boolean(enabled));
                Ok(result)
            })
        },
    )?;

    let state_clone = state.clone();
    let sentinels_clone = sentinels.clone();
    register_fn(
        lua,
        mp,
        "raw_wait_event",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let timeout = match values.first() {
                    None | Some(Value::Nil) => 1e20,
                    Some(value) => value_as_number(value).ok_or_else(|| {
                        Error::RuntimeError("timeout must be a number".into())
                    })?,
                };
                let event = {
                    let state = state_clone.borrow();
                    wait_for_event(&state.events, timeout)
                };
                event_to_lua(lua, &sentinels_clone, &event)
            })
        },
    )?;

    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "get_time",
        move |_lua, _values: Variadic<Value>| {
            guard(|| {
                let now = state_clone.borrow().start.elapsed().as_secs_f64();
                let mut result = MultiValue::new();
                result.push_back(Value::Number(now));
                Ok(result)
            })
        },
    )?;

    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "find_config_file",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(name) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("file name is required".into()));
                };
                let mut result = MultiValue::new();
                match state_clone.borrow_mut().backend.find_config_file(&name) {
                    Some(path) => result.push_back(Value::String(lua.create_string(&path)?)),
                    None => result.push_back(Value::Nil),
                }
                Ok(result)
            })
        },
    )?;

    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "format_time",
        move |lua, values: Variadic<Value>| {
            guard(|| {
                let Some(seconds) = values.first().and_then(value_as_number) else {
                    return Err(Error::RuntimeError("time value is required".into()));
                };
                let format = match values.get(1) {
                    None | Some(Value::Nil) => "%H:%M:%S".to_owned(),
                    Some(value) => value_to_string(value).ok_or_else(|| {
                        Error::RuntimeError("time format must be a string".into())
                    })?,
                };
                match state_clone
                    .borrow_mut()
                    .backend
                    .format_time(seconds, &format)
                {
                    Ok(text) => {
                        let mut result = MultiValue::new();
                        result.push_back(Value::String(lua.create_string(&text)?));
                        Ok(result)
                    }
                    Err(error) => Err(Error::RuntimeError(error)),
                }
            })
        },
    )?;

    let state_clone = state.clone();
    register_fn(
        lua,
        mp,
        "input_set_section_mouse_area",
        move |_lua, values: Variadic<Value>| {
            guard(|| {
                let Some(section) = values.first().and_then(value_to_string) else {
                    return Err(Error::RuntimeError("section is required".into()));
                };
                let coordinate = |index: usize| {
                    values
                        .get(index)
                        .and_then(value_as_number)
                        .ok_or_else(|| Error::RuntimeError("coordinate is required".into()))
                };
                let (x0, y0, x1, y1) = (
                    coordinate(1)?,
                    coordinate(2)?,
                    coordinate(3)?,
                    coordinate(4)?,
                );
                state_clone
                    .borrow_mut()
                    .backend
                    .input_set_section_mouse_area(&section, x0, y0, x1, y1);
                Ok(MultiValue::new())
            })
        },
    )
}

fn wait_for_event(events: &Receiver<ScriptEvent>, timeout: f64) -> ScriptEvent {
    let duration = if timeout.is_finite() && timeout > 0.0 {
        Duration::from_secs_f64(timeout.min(MAX_WAIT_SECONDS))
    } else {
        Duration::ZERO
    };
    match events.recv_timeout(duration) {
        Ok(event) => event,
        Err(RecvTimeoutError::Timeout) => ScriptEvent::new("none"),
        Err(RecvTimeoutError::Disconnected) => ScriptEvent::shutdown(),
    }
}

fn event_to_lua(
    lua: &Lua,
    sentinels: &Sentinels,
    event: &ScriptEvent,
) -> LuaResult<MultiValue> {
    let table = lua.create_table()?;
    table.set("event", event.name.as_str())?;
    for (key, value) in &event.fields {
        table.set(key.as_str(), value.to_lua(lua, sentinels)?)?;
    }
    let mut result = MultiValue::new();
    result.push_back(Value::Table(table));
    Ok(result)
}

fn value_or_error(
    lua: &Lua,
    sentinels: &Sentinels,
    result: Result<NativeValue, String>,
    default: Option<Value>,
    osd: bool,
) -> LuaResult<MultiValue> {
    match result {
        Ok(value) => {
            let mut result = MultiValue::new();
            result.push_back(value.to_lua(lua, sentinels)?);
            Ok(result)
        }
        Err(error) => {
            let default = match default {
                Some(Value::Nil) | None if osd => {
                    Value::String(lua.create_string("")?)
                }
                Some(value) => value,
                None => Value::Nil,
            };
            let mut result = MultiValue::new();
            result.push_back(default);
            result.push_back(Value::String(lua.create_string(&error)?));
            Ok(result)
        }
    }
}

fn command_result(
    lua: &Lua,
    sentinels: &Sentinels,
    result: Result<NativeValue, String>,
    default: Option<Value>,
) -> LuaResult<MultiValue> {
    match result {
        Ok(value) => {
            let mut result = MultiValue::new();
            result.push_back(value.to_lua(lua, sentinels)?);
            Ok(result)
        }
        Err(error) => {
            let mut result = MultiValue::new();
            result.push_back(default.unwrap_or(Value::Nil));
            result.push_back(Value::String(lua.create_string(&error)?));
            Ok(result)
        }
    }
}

fn ok_or_error(lua: &Lua, result: Result<(), String>) -> LuaResult<MultiValue> {
    match result {
        Ok(()) => {
            let mut result = MultiValue::new();
            result.push_back(Value::Boolean(true));
            Ok(result)
        }
        Err(error) => {
            let mut result = MultiValue::new();
            result.push_back(Value::Nil);
            result.push_back(Value::String(lua.create_string(&error)?));
            Ok(result)
        }
    }
}

fn property_args(values: &Variadic<Value>) -> LuaResult<(String, Option<Value>)> {
    let Some(property) = values.first().and_then(value_to_string) else {
        return Err(Error::RuntimeError("property name is required".into()));
    };
    Ok((property, values.get(1).cloned()))
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => text.to_str().ok().map(|text| text.to_owned()),
        Value::Integer(value) => Some(value.to_string()),
        Value::Number(value) => Some(format_number(*value)),
        _ => None,
    }
}

fn value_as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(value) => Some(*value as f64),
        Value::Number(value) => Some(*value),
        _ => None,
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        value.to_string()
    }
}

fn number_value(value: f64) -> NativeValue {
    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        NativeValue::Int(value as i64)
    } else {
        NativeValue::Double(value)
    }
}

fn property_format(value: Option<&Value>) -> LuaResult<PropertyFormat> {
    match value {
        None | Some(Value::Nil) => Ok(PropertyFormat::None),
        Some(Value::String(text)) => match &*text.to_str()? {
            "none" => Ok(PropertyFormat::None),
            "native" => Ok(PropertyFormat::Native),
            "bool" => Ok(PropertyFormat::Bool),
            "string" => Ok(PropertyFormat::String),
            "number" => Ok(PropertyFormat::Number),
            other => Err(Error::RuntimeError(format!(
                "invalid property format '{other}'"
            ))),
        },
        _ => Err(Error::RuntimeError("invalid property format".into())),
    }
}

fn guard<T>(callback: impl FnOnce() -> LuaResult<T>) -> LuaResult<T> {
    match catch_unwind(AssertUnwindSafe(callback)) {
        Ok(result) => result,
        Err(payload) => Err(Error::RuntimeError(format!(
            "panic in Lua host callback: {}",
            panic_message(&payload)
        ))),
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Default)]
    struct Recorded {
        commands: Vec<CommandRequest>,
        properties: HashMap<String, NativeValue>,
        observed: Vec<(u64, String, PropertyFormat)>,
        unobserved: Vec<u64>,
        requested_events: Vec<(String, bool)>,
        logs: Vec<(String, String)>,
        mouse_areas: Vec<(String, f64, f64, f64, f64)>,
    }

    #[derive(Clone)]
    struct MockBackend {
        recorded: Arc<Mutex<Recorded>>,
    }

    impl MockBackend {
        fn new() -> (Self, Arc<Mutex<Recorded>>) {
            let recorded = Arc::new(Mutex::new(Recorded::default()));
            (
                Self {
                    recorded: recorded.clone(),
                },
                recorded,
            )
        }
    }

    impl PlayerBackend for MockBackend {
        fn log(&mut self, level: &str, message: &str) {
            self.recorded
                .lock()
                .unwrap()
                .logs
                .push((level.to_owned(), message.to_owned()));
        }

        fn command(&mut self, command: CommandRequest) -> Result<NativeValue, String> {
            let result = match &command {
                CommandRequest::Native(NativeValue::Map(entries)) => {
                    let name = entries.iter().find(|(key, _)| key == "name");
                    match name {
                        Some((_, NativeValue::String(name))) if name == "expand-text" => {
                            NativeValue::Map(vec![(
                                "value".to_owned(),
                                NativeValue::String("expanded".to_owned()),
                            )])
                        }
                        _ => NativeValue::None,
                    }
                }
                _ => NativeValue::None,
            };
            self.recorded.lock().unwrap().commands.push(command);
            Ok(result)
        }

        fn get_property(
            &mut self,
            name: &str,
            format: PropertyFormat,
        ) -> Result<NativeValue, String> {
            let recorded = self.recorded.lock().unwrap();
            let Some(value) = recorded.properties.get(name) else {
                return Err(format!("property '{name}' not found"));
            };
            match format {
                PropertyFormat::String | PropertyFormat::OsdString => match value {
                    NativeValue::String(_) => Ok(value.clone()),
                    NativeValue::Bool(value) => Ok(NativeValue::String(
                        if *value { "yes" } else { "no" }.to_owned(),
                    )),
                    NativeValue::Int(value) => Ok(NativeValue::String(value.to_string())),
                    NativeValue::Double(value) => Ok(NativeValue::String(value.to_string())),
                    _ => Err(format!("property '{name}' is not a string")),
                },
                PropertyFormat::Bool => match value {
                    NativeValue::Bool(_) => Ok(value.clone()),
                    _ => Err(format!("property '{name}' is not a boolean")),
                },
                PropertyFormat::Number => match value {
                    NativeValue::Int(_) | NativeValue::Double(_) => Ok(value.clone()),
                    _ => Err(format!("property '{name}' is not a number")),
                },
                _ => Ok(value.clone()),
            }
        }

        fn set_property(&mut self, name: &str, value: NativeValue) -> Result<(), String> {
            self.recorded
                .lock()
                .unwrap()
                .properties
                .insert(name.to_owned(), value);
            Ok(())
        }

        fn del_property(&mut self, name: &str) -> Result<(), String> {
            self.recorded.lock().unwrap().properties.remove(name);
            Ok(())
        }

        fn observe_property(
            &mut self,
            id: u64,
            name: &str,
            format: PropertyFormat,
        ) -> Result<(), String> {
            self.recorded
                .lock()
                .unwrap()
                .observed
                .push((id, name.to_owned(), format));
            Ok(())
        }

        fn unobserve_property(&mut self, id: u64) -> i32 {
            self.recorded.lock().unwrap().unobserved.push(id);
            1
        }

        fn request_event(&mut self, name: &str, enable: bool) -> bool {
            self.recorded
                .lock()
                .unwrap()
                .requested_events
                .push((name.to_owned(), enable));
            true
        }

        fn find_config_file(&mut self, _name: &str) -> Option<String> {
            None
        }

        fn format_time(&mut self, seconds: f64, _format: &str) -> Result<String, String> {
            let seconds = seconds.max(0.0) as u64;
            Ok(format!(
                "{:02}:{:02}:{:02}",
                seconds / 3600,
                (seconds / 60) % 60,
                seconds % 60
            ))
        }

        fn input_set_section_mouse_area(
            &mut self,
            section: &str,
            x0: f64,
            y0: f64,
            x1: f64,
            y1: f64,
        ) {
            self.recorded
                .lock()
                .unwrap()
                .mouse_areas
                .push((section.to_owned(), x0, y0, x1, y1));
        }
    }

    const FIXTURE: &str = r#"
local observed = nil
local observe
observe = function(_, value)
    observed = value
    mp.commandv("record", "property", tostring(value))
end
mp.observe_property("time-pos", "number", observe)

mp.register_event("fixture-event", function(event)
    mp.commandv("record", "event", tostring(event.value))
end)

mp.add_timeout(0.01, function()
    mp.commandv("record", "timer")
    mp.osd_message("fixture timer")
end)

mp.add_key_binding("x", "fixture-binding", function()
    mp.commandv("record", "binding")
end)
mp.set_mouse_area(0, 0, 100, 100)

mp.register_script_message("fixture-message", function(value)
    mp.commandv("record", "message", value)
    assert(mp.get_property("fixture-property", "missing") == "missing")
    mp.set_property("fixture-property", "set-by-script")
    assert(mp.get_property("fixture-property") == "set-by-script")
    mp.set_property_number("volume", 55)
    assert(mp.get_property_number("volume") == 55)
    mp.set_property_native("fixture-native", {answer = 42})
    local native = mp.get_property_native("fixture-native")
    assert(native.answer == 42)
    assert(mp.get_property_bool("paused") == false)
    assert(type(mp.get_property_osd("playback-time")) == "string")
    mp.set_property("temporary", "value")
    mp.del_property("temporary")
    assert(mp.get_property("temporary", "gone") == "gone")
    assert(observed == 42, "property observation missing")
    mp.unobserve_property(observe)
    local expanded = mp.command_native({name = "expand-text", text = "value"})
    assert(expanded.value == "expanded")
    local overlay = mp.create_osd_overlay("ass-events")
    overlay.data = "{\\an7}fixture"
    overlay:update()
    mp.command("fixture-string-command")
    local width, height = mp.get_osd_size()
    assert(width == 1920 and height == 1080)
    local directory, filename = mp.utils.split_path("/fixture/file.mkv")
    assert(directory == "/fixture" and filename == "file.mkv")
    local encoded = mp.utils.format_json({answer = 42})
    local decoded = mp.utils.parse_json(encoded)
    assert(decoded.answer == 42)
    mp.commandv("record", "complete")
    exit()
end)

mp.commandv("record", "ready")
"#;

    const ERROR_FIXTURE: &str = r#"error("fixture failure")"#;
    const TIMER_ERROR_FIXTURE: &str = r#"
mp.add_timeout(0.01, function() error("timer failure") end)
mp.commandv("record", "ready")
"#;

    fn has_record(recorded: &Recorded, kind: &str) -> bool {
        recorded.commands.iter().any(|command| match command {
            CommandRequest::Args(args) => {
                args.first().map(String::as_str) == Some("record")
                    && args.get(1).map(String::as_str) == Some(kind)
            }
            _ => false,
        })
    }

    fn has_command(recorded: &Recorded, name: &str) -> bool {
        recorded.commands.iter().any(|command| match command {
            CommandRequest::String(command) => command == name,
            CommandRequest::Args(args) => args.first().map(String::as_str) == Some(name),
            CommandRequest::Native(_) => false,
        })
    }

    fn has_native_command(recorded: &Recorded, name: &str) -> bool {
        recorded.commands.iter().any(|command| match command {
            CommandRequest::Native(NativeValue::Map(entries)) => entries.iter().any(
                |(key, value)| key == "name" && value == &NativeValue::String(name.to_owned()),
            ),
            _ => false,
        })
    }

    fn record_args(recorded: &Recorded, kind: &str) -> Option<Vec<String>> {
        recorded.commands.iter().find_map(|command| match command {
            CommandRequest::Args(args)
                if args.first().map(String::as_str) == Some("record")
                    && args.get(1).map(String::as_str) == Some(kind) =>
            {
                Some(args.clone())
            }
            _ => None,
        })
    }

    fn command_args(recorded: &Recorded, name: &str) -> Option<Vec<String>> {
        recorded.commands.iter().find_map(|command| match command {
            CommandRequest::Args(args) if args.first().map(String::as_str) == Some(name) => {
                Some(args.clone())
            }
            _ => None,
        })
    }

    fn wait_for(
        recorded: &Arc<Mutex<Recorded>>,
        description: &str,
        predicate: impl Fn(&Recorded) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if predicate(&recorded.lock().unwrap()) {
                return;
            }
            if Instant::now() >= deadline {
                panic!(
                    "timed out waiting for {description}; recorded {:#?}",
                    recorded.lock().unwrap()
                );
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn fixture_host() -> (ScriptHost, Arc<Mutex<Recorded>>) {
        let (backend, recorded) = MockBackend::new();
        {
            let mut recorded = recorded.lock().unwrap();
            recorded
                .properties
                .insert("paused".to_owned(), NativeValue::Bool(false));
            recorded.properties.insert(
                "playback-time".to_owned(),
                NativeValue::Double(12.5),
            );
            recorded.properties.insert(
                "osd-dimensions".to_owned(),
                NativeValue::Map(vec![
                    ("w".to_owned(), NativeValue::Int(1920)),
                    ("h".to_owned(), NativeValue::Int(1080)),
                    ("aspect".to_owned(), NativeValue::Double(1.777)),
                    ("ml".to_owned(), NativeValue::Int(0)),
                    ("mt".to_owned(), NativeValue::Int(0)),
                    ("mr".to_owned(), NativeValue::Int(0)),
                    ("mb".to_owned(), NativeValue::Int(0)),
                ]),
            );
        }
        let host = ScriptHost::spawn("fixture", "@fixture.lua", FIXTURE, backend)
            .expect("spawn script host");
        (host, recorded)
    }

    #[test]
    fn lua_host_fixture_parses() {
        let lua = new_interpreter().unwrap();
        lua.load(FIXTURE)
            .set_name("@fixture.lua")
            .into_function()
            .expect("fixture parses");
        lua.load(TIMER_ERROR_FIXTURE)
            .set_name("@timer-error.lua")
            .into_function()
            .expect("timer error fixture parses");
    }

    #[test]
    fn lua_host_records_commands_properties_events_and_bindings() {
        let (mut host, recorded) = fixture_host();
        wait_for(&recorded, "ready", |recorded| {
            has_record(recorded, "ready")
        });

        let observation_id = recorded.lock().unwrap().observed[0].0;
        host.post_event(
            ScriptEvent::new("property-change")
                .with_field("id", NativeValue::Int(observation_id as i64))
                .with_field("name", NativeValue::String("time-pos".to_owned()))
                .with_field("data", NativeValue::Int(42)),
        )
        .expect("post property change");
        wait_for(&recorded, "property callback", |recorded| {
            has_record(recorded, "property")
        });

        host.post_event(
            ScriptEvent::new("fixture-event")
                .with_field("value", NativeValue::String("event-data".to_owned())),
        )
        .expect("post fixture event");
        wait_for(&recorded, "event callback", |recorded| {
            has_record(recorded, "event")
        });

        host.post_event(ScriptEvent::new("client-message").with_field(
            "args",
            NativeValue::Array(vec![
                NativeValue::String("key-binding".to_owned()),
                NativeValue::String("fixture-binding".to_owned()),
                NativeValue::String("d".to_owned()),
            ]),
        ))
        .expect("post key binding");
        wait_for(&recorded, "binding callback", |recorded| {
            has_record(recorded, "binding")
        });

        wait_for(&recorded, "timer callback", |recorded| {
            has_record(recorded, "timer")
        });

        host.post_event(ScriptEvent::new("client-message").with_field(
            "args",
            NativeValue::Array(vec![
                NativeValue::String("fixture-message".to_owned()),
                NativeValue::String("payload".to_owned()),
            ]),
        ))
        .expect("post fixture message");
        wait_for(&recorded, "complete", |recorded| {
            has_record(recorded, "complete")
        });

        host.stop().expect("script exits cleanly");

        let recorded = recorded.lock().unwrap();
        assert_eq!(record_args(&recorded, "ready"), Some(vec!["record".to_owned(), "ready".to_owned()]));
        assert_eq!(
            record_args(&recorded, "property"),
            Some(vec!["record".to_owned(), "property".to_owned(), "42".to_owned()])
        );
        assert_eq!(
            record_args(&recorded, "event"),
            Some(vec!["record".to_owned(), "event".to_owned(), "event-data".to_owned()])
        );
        assert_eq!(
            record_args(&recorded, "binding"),
            Some(vec!["record".to_owned(), "binding".to_owned()])
        );
        assert_eq!(
            record_args(&recorded, "timer"),
            Some(vec!["record".to_owned(), "timer".to_owned()])
        );
        assert_eq!(
            record_args(&recorded, "message"),
            Some(vec!["record".to_owned(), "message".to_owned(), "payload".to_owned()])
        );
        assert_eq!(
            record_args(&recorded, "complete"),
            Some(vec!["record".to_owned(), "complete".to_owned()])
        );
        assert_eq!(
            command_args(&recorded, "show-text"),
            Some(vec!["show-text".to_owned(), "fixture timer".to_owned(), "-1".to_owned()])
        );
        let kinds: Vec<String> = recorded
            .commands
            .iter()
            .filter_map(|command| match command {
                CommandRequest::Args(args)
                    if args.first().map(String::as_str) == Some("record") =>
                {
                    args.get(1).cloned()
                }
                _ => None,
            })
            .collect();
        let position = |kind: &str| {
            kinds
                .iter()
                .position(|value| value == kind)
                .unwrap_or_else(|| panic!("missing {kind} in {kinds:?}"))
        };
        assert_eq!(position("ready"), 0);
        assert!(position("property") < position("event"));
        assert!(position("event") < position("binding"));
        assert!(position("binding") < position("message"));
        assert!(position("message") < position("complete"));
        assert!(position("timer") < position("complete"));
        assert!(has_command(&recorded, "show-text"));
        assert!(has_command(&recorded, "define-section"));
        assert!(has_command(&recorded, "fixture-string-command"));
        assert!(has_native_command(&recorded, "osd-overlay"));
        assert_eq!(
            recorded.properties.get("fixture-property"),
            Some(&NativeValue::String("set-by-script".to_owned()))
        );
        assert_eq!(
            recorded.properties.get("volume"),
            Some(&NativeValue::Int(55))
        );
        assert_eq!(
            recorded.properties.get("fixture-native"),
            Some(&NativeValue::Map(vec![(
                "answer".to_owned(),
                NativeValue::Int(42)
            )]))
        );
        assert!(!recorded.properties.contains_key("temporary"));
        assert_eq!(recorded.unobserved, vec![observation_id]);
        assert!(recorded
            .requested_events
            .contains(&("property-change".to_owned(), true)));
        assert!(recorded
            .requested_events
            .contains(&("fixture-event".to_owned(), true)));
        assert_eq!(recorded.mouse_areas.len(), 1);
        assert_eq!(
            &recorded.mouse_areas[0],
            &("input_dispatch_fixture".to_owned(), 0.0, 0.0, 100.0, 100.0)
        );
    }

    #[test]
    fn lua_host_reports_a_script_error_without_hanging() {
        let (backend, _recorded) = MockBackend::new();
        let mut host = ScriptHost::spawn("error", "@error.lua", ERROR_FIXTURE, backend)
            .expect("spawn script host");
        let started = Instant::now();
        let result = host.stop();
        assert!(started.elapsed() < SHUTDOWN_TIMEOUT);
        match result {
            Err(HostError::Script(message)) => {
                assert!(message.contains("fixture failure"), "{message}");
            }
            other => panic!("expected a script error, got {other:?}"),
        }
    }

    #[test]
    fn lua_host_reports_a_callback_error_without_hanging() {
        let (backend, recorded) = MockBackend::new();
        let mut host = ScriptHost::spawn(
            "timer-error",
            "@timer-error.lua",
            TIMER_ERROR_FIXTURE,
            backend,
        )
        .expect("spawn script host");
        wait_for(&recorded, "ready", |recorded| {
            has_record(recorded, "ready")
        });
        thread::sleep(Duration::from_millis(50));
        let started = Instant::now();
        let result = host.stop();
        assert!(started.elapsed() < SHUTDOWN_TIMEOUT);
        match result {
            Err(HostError::Script(message)) => {
                assert!(message.contains("timer failure"), "{message}");
            }
            other => panic!("expected a timer error, got {other:?}"),
        }
    }
}
