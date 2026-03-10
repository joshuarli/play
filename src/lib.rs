#![allow(dead_code, unused_imports)]
//! Library entry point — exposes modules for fuzz testing.
//!
//! The player binary uses `main.rs`; this file exists so that `cargo fuzz`
//! targets can import the parsing and demuxing code directly.

pub mod cmd;
pub mod demux;
pub mod subtitle;
pub mod time;

// Private modules required by the public ones (transitive dependencies).
mod audio_out;
mod decode_audio;
mod decode_video;
mod input;
mod osd;
mod player;
mod sync;
mod terminal;
mod video_out;
mod window;
