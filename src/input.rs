use crate::cmd::Command;

/// Map an NSEvent key code + modifiers to a Command.
/// `key_code` is the virtual key code from NSEvent.
/// `shift` is true if Shift is held.
/// `chars` is the characters string from the event.
pub fn map_key(key_code: u16, shift: bool, chars: &str) -> Option<Command> {
    // Virtual key codes (from Carbon HIToolbox/Events.h)
    const KVK_SPACE: u16 = 49;
    const KVK_LEFT: u16 = 123;
    const KVK_RIGHT: u16 = 124;
    const KVK_DOWN: u16 = 125;
    const KVK_UP: u16 = 126;
    const KVK_RETURN: u16 = 36;
    const KVK_DELETE: u16 = 51; // backspace

    match key_code {
        KVK_SPACE => Some(Command::PlayPause),
        KVK_LEFT if shift => Some(Command::SeekRelative {
            seconds: -1.0,
            exact: true,
        }),
        KVK_RIGHT if shift => Some(Command::SeekRelative {
            seconds: 1.0,
            exact: true,
        }),
        KVK_UP if shift => Some(Command::SeekRelative {
            seconds: 60.0,
            exact: false,
        }),
        KVK_DOWN if shift => Some(Command::SeekRelative {
            seconds: -60.0,
            exact: false,
        }),
        KVK_LEFT => Some(Command::SeekRelative {
            seconds: -5.0,
            exact: false,
        }),
        KVK_RIGHT => Some(Command::SeekRelative {
            seconds: 5.0,
            exact: false,
        }),
        KVK_UP => Some(Command::VolumeUp),
        KVK_DOWN => Some(Command::VolumeDown),
        KVK_RETURN => Some(Command::NextFile),
        KVK_DELETE => Some(Command::PrevFile),
        _ => map_char(chars),
    }
}

fn map_char(chars: &str) -> Option<Command> {
    match chars {
        "q" => Some(Command::Quit),
        "f" => Some(Command::ToggleFullscreen),
        "a" => Some(Command::CycleAudioTrack),
        "s" => Some(Command::CycleSubtitle),
        "+" | "=" => Some(Command::AudioDelayIncrease),
        "-" => Some(Command::AudioDelayDecrease),
        ">" | "." => Some(Command::NextFile),
        "<" | "," => Some(Command::PrevFile),
        _ => None,
    }
}
