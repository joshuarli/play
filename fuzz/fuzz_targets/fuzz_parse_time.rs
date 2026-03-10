#![no_main]
use libfuzzer_sys::fuzz_target;
use play::time::{format_time, parse_time};

fuzz_target!(|data: &str| {
    // parse_time must never panic on any input
    if let Ok(us) = parse_time(data) {
        // Round-trip: format the result (exercises format_time too)
        let _ = format_time(us);
    }
});
