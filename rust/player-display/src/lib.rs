#[cfg(target_os = "linux")]
pub mod display;

#[cfg(target_os = "linux")]
pub use display::{InputEvent, Window};

#[cfg(target_os = "macos")]
pub mod mac;

#[cfg(target_os = "macos")]
pub use mac::MacInput;
#[cfg(target_os = "macos")]
pub use mac::MacWindow;
#[cfg(target_os = "macos")]
pub use mac::run_with_app_kit;
