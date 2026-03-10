#![no_main]
use libfuzzer_sys::fuzz_target;
use std::sync::atomic::{AtomicUsize, Ordering};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fuzz_target!(|data: &[u8]| {
    // Skip tiny inputs that can't be valid containers
    if data.len() < 8 {
        return;
    }

    // Write fuzz data to a unique temp file
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("play_fuzz_probe_{n}"));
    if std::fs::write(&path, data).is_err() {
        return;
    }

    // probe() must never panic — errors are fine, panics are not
    let _ = play::demux::probe(&path);

    let _ = std::fs::remove_file(&path);
});
