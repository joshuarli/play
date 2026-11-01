use crate::value::NativeValue;

#[derive(Debug, Clone, PartialEq)]
pub enum CommandRequest {
    String(String),
    Args(Vec<String>),
    Native(NativeValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyFormat {
    None,
    Native,
    Bool,
    String,
    OsdString,
    Number,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScriptEvent {
    pub name: String,
    pub fields: Vec<(String, NativeValue)>,
}

impl ScriptEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            fields: Vec::new(),
        }
    }

    pub fn with_field(mut self, name: impl Into<String>, value: NativeValue) -> Self {
        self.fields.push((name.into(), value));
        self
    }

    pub fn shutdown() -> Self {
        Self::new("shutdown")
    }
}

pub trait PlayerBackend: Send + 'static {
    fn log(&mut self, level: &str, message: &str);
    fn command(&mut self, command: CommandRequest) -> Result<NativeValue, String>;
    fn get_property(&mut self, name: &str, format: PropertyFormat) -> Result<NativeValue, String>;
    fn set_property(&mut self, name: &str, value: NativeValue) -> Result<(), String>;
    fn del_property(&mut self, name: &str) -> Result<(), String>;
    fn observe_property(
        &mut self,
        id: u64,
        name: &str,
        format: PropertyFormat,
    ) -> Result<(), String>;
    fn unobserve_property(&mut self, id: u64) -> i32;
    fn request_event(&mut self, name: &str, enable: bool) -> bool;
    fn find_config_file(&mut self, name: &str) -> Option<String>;
    fn format_time(&mut self, seconds: f64, format: &str) -> Result<String, String>;
    fn input_set_section_mouse_area(
        &mut self,
        section: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    );
}
