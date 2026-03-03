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

#[cfg(test)]
mod tests {
    use super::*;

    // Virtual key codes
    const KVK_SPACE: u16 = 49;
    const KVK_LEFT: u16 = 123;
    const KVK_RIGHT: u16 = 124;
    const KVK_DOWN: u16 = 125;
    const KVK_UP: u16 = 126;
    const KVK_RETURN: u16 = 36;
    const KVK_DELETE: u16 = 51;

    #[test]
    fn space_play_pause() {
        assert_eq!(map_key(KVK_SPACE, false, ""), Some(Command::PlayPause));
    }

    #[test]
    fn left_arrow_seek_back_5s() {
        assert_eq!(
            map_key(KVK_LEFT, false, ""),
            Some(Command::SeekRelative { seconds: -5.0, exact: false })
        );
    }

    #[test]
    fn right_arrow_seek_forward_5s() {
        assert_eq!(
            map_key(KVK_RIGHT, false, ""),
            Some(Command::SeekRelative { seconds: 5.0, exact: false })
        );
    }

    #[test]
    fn shift_left_seek_back_1s_exact() {
        assert_eq!(
            map_key(KVK_LEFT, true, ""),
            Some(Command::SeekRelative { seconds: -1.0, exact: true })
        );
    }

    #[test]
    fn shift_right_seek_forward_1s_exact() {
        assert_eq!(
            map_key(KVK_RIGHT, true, ""),
            Some(Command::SeekRelative { seconds: 1.0, exact: true })
        );
    }

    #[test]
    fn shift_up_seek_forward_60s() {
        assert_eq!(
            map_key(KVK_UP, true, ""),
            Some(Command::SeekRelative { seconds: 60.0, exact: false })
        );
    }

    #[test]
    fn shift_down_seek_back_60s() {
        assert_eq!(
            map_key(KVK_DOWN, true, ""),
            Some(Command::SeekRelative { seconds: -60.0, exact: false })
        );
    }

    #[test]
    fn up_down_volume() {
        assert_eq!(map_key(KVK_UP, false, ""), Some(Command::VolumeUp));
        assert_eq!(map_key(KVK_DOWN, false, ""), Some(Command::VolumeDown));
    }

    #[test]
    fn return_next_delete_prev() {
        assert_eq!(map_key(KVK_RETURN, false, ""), Some(Command::NextFile));
        assert_eq!(map_key(KVK_DELETE, false, ""), Some(Command::PrevFile));
    }

    #[test]
    fn char_commands() {
        assert_eq!(map_key(0, false, "q"), Some(Command::Quit));
        assert_eq!(map_key(0, false, "f"), Some(Command::ToggleFullscreen));
        assert_eq!(map_key(0, false, "a"), Some(Command::CycleAudioTrack));
        assert_eq!(map_key(0, false, "s"), Some(Command::CycleSubtitle));
        assert_eq!(map_key(0, false, "+"), Some(Command::AudioDelayIncrease));
        assert_eq!(map_key(0, false, "="), Some(Command::AudioDelayIncrease));
        assert_eq!(map_key(0, false, "-"), Some(Command::AudioDelayDecrease));
        assert_eq!(map_key(0, false, ">"), Some(Command::NextFile));
        assert_eq!(map_key(0, false, "."), Some(Command::NextFile));
        assert_eq!(map_key(0, false, "<"), Some(Command::PrevFile));
        assert_eq!(map_key(0, false, ","), Some(Command::PrevFile));
    }

    #[test]
    fn unknown_key_returns_none() {
        assert_eq!(map_key(0, false, "z"), None);
        assert_eq!(map_key(200, false, ""), None);
    }
}
