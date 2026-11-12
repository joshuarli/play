# HE-AAC reference fixtures

The two MP4 files contain two seconds of synthetic stereo audio at 48 kHz. The left channel combines 440 Hz and 9 kHz tones at amplitude 0.15. The right channel contains a 660 Hz tone at amplitude 0.2.

FFmpeg 9.0.2 with Apple's `aac_at` encoder generated the files. `he-aac.m4a` uses profile 4 at 48 kbit/s. `he-aac-v2.m4a` uses profile 28 at 32 kbit/s. The original configurations use backward-compatible SBR/PS signaling. The regression also supplies equivalent hierarchical configurations to decode the same packets.

FFmpeg 9.0.2's native AAC decoder generated the little-endian float reference files. Each reference contains the first 8192 stereo frames after container priming removal. The raw decoder output includes 2112 priming frames before that reference window.
