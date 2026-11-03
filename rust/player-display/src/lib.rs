#[cfg(target_os = "linux")]
pub mod display;

#[cfg(target_os = "linux")]
pub use display::{InputEvent, Window};
