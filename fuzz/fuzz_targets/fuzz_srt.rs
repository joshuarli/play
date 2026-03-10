#![no_main]
use libfuzzer_sys::fuzz_target;
use play::subtitle::{parse_srt_content, SubtitleTrack};

fuzz_target!(|data: &str| {
    let entries = parse_srt_content(data);

    // Exercise the lookup path on every parsed result
    if !entries.is_empty() {
        let track = SubtitleTrack {
            label: String::new(),
            entries,
        };
        let _ = track.text_at(0);
        let _ = track.text_at(i64::MAX);
        let _ = track.text_at(i64::MIN);
    }
});
