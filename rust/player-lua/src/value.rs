use mlua::{Error, Lua, Result, Table, Value};

const MAX_VALUE_DEPTH: usize = 64;

#[derive(Clone)]
pub struct Sentinels {
    pub unknown: Table,
    pub array: Table,
    pub map: Table,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NativeValue {
    None,
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<NativeValue>),
    Map(Vec<(String, NativeValue)>),
    Unknown,
}

impl NativeValue {
    pub fn from_lua(lua: &Lua, value: Value, sentinels: &Sentinels) -> Result<Self> {
        Self::from_lua_at(lua, value, sentinels, 0)
    }

    fn from_lua_at(
        lua: &Lua,
        value: Value,
        sentinels: &Sentinels,
        depth: usize,
    ) -> Result<Self> {
        match value {
            Value::Nil => Ok(NativeValue::None),
            Value::Boolean(value) => Ok(NativeValue::Bool(value)),
            Value::Integer(value) => Ok(NativeValue::Int(value)),
            Value::Number(value) => {
                if value.fract() == 0.0
                    && value >= i64::MIN as f64
                    && value <= i64::MAX as f64
                {
                    Ok(NativeValue::Int(value as i64))
                } else {
                    Ok(NativeValue::Double(value))
                }
            }
            Value::String(value) => match value.to_str() {
                Ok(text) => Ok(NativeValue::String(text.to_owned())),
                Err(_) => Ok(NativeValue::Bytes(value.as_bytes().to_vec())),
            },
            Value::Table(table) => {
                if depth >= MAX_VALUE_DEPTH {
                    return Err(Error::RuntimeError(format!(
                        "table nesting is too deep (limit {MAX_VALUE_DEPTH})"
                    )));
                }
                table_from_lua(lua, table, sentinels, depth + 1)
            }
            other => Err(Error::RuntimeError(format!(
                "disallowed Lua type found: {}",
                other.type_name()
            ))),
        }
    }

    pub fn to_lua(&self, lua: &Lua, sentinels: &Sentinels) -> Result<Value> {
        match self {
            NativeValue::None => Ok(Value::Nil),
            NativeValue::Bool(value) => Ok(Value::Boolean(*value)),
            NativeValue::Int(value) => Ok(Value::Number(*value as f64)),
            NativeValue::Double(value) => Ok(Value::Number(*value)),
            NativeValue::String(value) => Ok(Value::String(lua.create_string(value)?)),
            NativeValue::Bytes(value) => Ok(Value::String(lua.create_string(value)?)),
            NativeValue::Array(values) => {
                let table = lua.create_table_with_capacity(values.len(), 0)?;
                table.set_metatable(Some(sentinels.array.clone()))?;
                for (index, value) in values.iter().enumerate() {
                    table.raw_seti(index + 1, value.to_lua(lua, sentinels)?)?;
                }
                Ok(Value::Table(table))
            }
            NativeValue::Map(entries) => {
                let table = lua.create_table_with_capacity(0, entries.len())?;
                table.set_metatable(Some(sentinels.map.clone()))?;
                for (key, value) in entries {
                    table.raw_set(key.as_str(), value.to_lua(lua, sentinels)?)?;
                }
                Ok(Value::Table(table))
            }
            NativeValue::Unknown => {
                let table = lua.create_table()?;
                table.set_metatable(Some(sentinels.unknown.clone()))?;
                Ok(Value::Table(table))
            }
        }
    }
}

fn table_from_lua(lua: &Lua, table: Table, sentinels: &Sentinels, depth: usize) -> Result<NativeValue> {
    let tag = table
        .metatable()
        .and_then(|meta| meta.raw_get::<Option<String>>("type").ok().flatten());

    if tag.as_deref() == Some("UNKNOWN_TYPE") {
        return Ok(NativeValue::Unknown);
    }

    let mut length = 0usize;
    loop {
        let index = length + 1;
        if matches!(table.raw_get::<Value>(index)?, Value::Nil) {
            break;
        }
        length = index;
    }

    let mut entries = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        entries.push((key, value));
    }

    let is_array = match tag.as_deref() {
        Some("ARRAY") => true,
        Some("MAP") => false,
        _ => entries.len() == length,
    };

    if is_array {
        let mut values = Vec::with_capacity(length);
        for index in 1..=length {
            let value = table.raw_get::<Value>(index)?;
            values.push(NativeValue::from_lua_at(lua, value, sentinels, depth)?);
        }
        Ok(NativeValue::Array(values))
    } else {
        let mut values = Vec::with_capacity(entries.len());
        for (key, value) in entries {
            let Value::String(key) = key else {
                return Err(Error::RuntimeError(format!(
                    "key must be a string, but got {}",
                    key.type_name()
                )));
            };
            values.push((
                key.to_str()?.to_owned(),
                NativeValue::from_lua_at(lua, value, sentinels, depth)?,
            ));
        }
        values.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(NativeValue::Map(values))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentinels(lua: &Lua) -> Sentinels {
        let array = lua.create_table().unwrap();
        array.set("type", "ARRAY").unwrap();
        let map = lua.create_table().unwrap();
        map.set("type", "MAP").unwrap();
        let unknown = lua.create_table().unwrap();
        unknown.set("type", "UNKNOWN_TYPE").unwrap();
        Sentinels { unknown, array, map }
    }

    #[test]
    fn round_trips_scalars() {
        let lua = Lua::new();
        let sentinels = sentinels(&lua);
        for value in [
            NativeValue::None,
            NativeValue::Bool(true),
            NativeValue::Int(42),
            NativeValue::Double(1.5),
            NativeValue::String("text".into()),
            NativeValue::Unknown,
        ] {
            let lua_value = value.to_lua(&lua, &sentinels).unwrap();
            assert_eq!(
                NativeValue::from_lua(&lua, lua_value, &sentinels).unwrap(),
                value
            );
        }
    }

    #[test]
    fn byte_strings_round_trip() {
        let lua = Lua::new();
        let sentinels = sentinels(&lua);
        let value = NativeValue::Bytes(vec![0, 159, 146, 150, 255]);
        let lua_value = value.to_lua(&lua, &sentinels).unwrap();
        assert_eq!(
            NativeValue::from_lua(&lua, lua_value, &sentinels).unwrap(),
            value
        );
    }

    #[test]
    fn array_and_map_keep_their_identity() {
        let lua = Lua::new();
        let sentinels = sentinels(&lua);
        let array = NativeValue::Array(vec![NativeValue::Int(1), NativeValue::Int(2)]);
        let array_lua = array.to_lua(&lua, &sentinels).unwrap();
        let Value::Table(table) = &array_lua else {
            panic!("array did not become a table");
        };
        assert_eq!(
            table.metatable().unwrap().raw_get::<String>("type").unwrap(),
            "ARRAY"
        );
        assert_eq!(NativeValue::from_lua(&lua, array_lua, &sentinels).unwrap(), array);

        let map = NativeValue::Map(vec![
            ("a".into(), NativeValue::Int(1)),
            ("b".into(), NativeValue::Array(Vec::new())),
        ]);
        let map_lua = map.to_lua(&lua, &sentinels).unwrap();
        let Value::Table(table) = &map_lua else {
            panic!("map did not become a table");
        };
        assert_eq!(
            table.metatable().unwrap().raw_get::<String>("type").unwrap(),
            "MAP"
        );
        assert_eq!(NativeValue::from_lua(&lua, map_lua, &sentinels).unwrap(), map);
    }

    #[test]
    fn rejects_non_string_map_keys() {
        let lua = Lua::new();
        let sentinels = sentinels(&lua);
        let table = lua.create_table().unwrap();
        table.set(1, 1).unwrap();
        table.set(2, 2).unwrap();
        table.set(true, 3).unwrap();
        let error = NativeValue::from_lua(&lua, Value::Table(table), &sentinels).unwrap_err();
        assert!(error.to_string().contains("key must be a string"));
    }

    #[test]
    fn rejects_cyclic_and_deep_tables() {
        let lua = Lua::new();
        let sentinels = sentinels(&lua);
        let cyclic = lua.create_table().unwrap();
        cyclic.set("self", cyclic.clone()).unwrap();
        let error = NativeValue::from_lua(&lua, Value::Table(cyclic), &sentinels).unwrap_err();
        assert!(error.to_string().contains("table nesting is too deep"));

        let mut deep = lua.create_table().unwrap();
        for _ in 0..MAX_VALUE_DEPTH + 1 {
            let parent = lua.create_table().unwrap();
            parent.set("child", deep).unwrap();
            deep = parent;
        }
        let error = NativeValue::from_lua(&lua, Value::Table(deep), &sentinels).unwrap_err();
        assert!(error.to_string().contains("table nesting is too deep"));
    }
}
