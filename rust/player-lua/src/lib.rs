use mlua::{Lua, LuaOptions, Result, StdLib};

pub mod backend;
pub mod host;
pub mod queue;
pub mod utils;
pub mod value;

pub use backend::{CommandRequest, PlayerBackend, PropertyFormat, ScriptEvent};
pub use host::{HostError, ScriptEventSender, ScriptHost, ScriptModule, SHUTDOWN_TIMEOUT};
pub use queue::Posted;
pub use value::{NativeValue, Sentinels};

pub fn new_interpreter() -> Result<Lua> {
    Lua::new_with(StdLib::ALL_SAFE, LuaOptions::default())
}

pub fn run_script(lua: &Lua, name: &str, source: &str) -> Result<()> {
    lua.load(source).set_name(name).exec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpreter_is_lua_52() {
        let lua = new_interpreter().unwrap();
        let version: String = lua.globals().get("_VERSION").unwrap();
        assert_eq!(version, "Lua 5.2");
    }

    #[test]
    fn opens_the_standard_libraries_the_builtin_modules_use() {
        let lua = new_interpreter().unwrap();
        for name in ["io", "os", "package", "string", "table", "math"] {
            assert!(
                lua.globals().get::<mlua::Table>(name).is_ok(),
                "missing standard library {name}"
            );
        }
    }

    #[test]
    fn loads_and_runs_a_script() {
        let lua = new_interpreter().unwrap();
        run_script(
            &lua,
            "fixture.lua",
            "local values = {} for i = 1, 6 do values[i] = i * 7 end result = values[6]",
        )
        .unwrap();
        let result: i64 = lua.globals().get("result").unwrap();
        assert_eq!(result, 42);
    }

    #[test]
    fn reports_script_errors_with_the_chunk_name() {
        let lua = new_interpreter().unwrap();
        let error = run_script(&lua, "broken.lua", "error('boom')").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("broken.lua"), "{message}");
        assert!(message.contains("boom"), "{message}");
    }
}
