#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayResume {
    Playing,
    Draining,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeekResume {
    Playing,
    Paused(PlayResume),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Opening,
    Buffering,
    Playing,
    Paused { resume: PlayResume },
    Seeking { resume: SeekResume },
    Draining,
    Ended,
    Error,
    Closing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Open,
    Opened,
    Buffered,
    Pause,
    Resume,
    Seek,
    SeekDone,
    Eof,
    Drained,
    Fail,
    Close,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rejected {
    pub state: State,
    pub event: Event,
}

impl Rejected {
    fn of(state: &State, event: Event) -> Self {
        Rejected {
            state: state.clone(),
            event,
        }
    }
}

pub struct Machine {
    state: State,
    generation: u64,
    error: Option<String>,
}

impl Machine {
    pub fn new() -> Self {
        Machine {
            state: State::Idle,
            generation: 0,
            error: None,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    pub fn apply(&mut self, event: Event) -> Result<(), Rejected> {
        let next = transition(&self.state, event).ok_or_else(|| Rejected::of(&self.state, event))?;
        if event == Event::Open {
            self.generation += 1;
            self.error = None;
        }
        self.state = next;
        Ok(())
    }
}

fn transition(state: &State, event: Event) -> Option<State> {
    match (state, event) {
        (State::Idle, Event::Open) => Some(State::Opening),
        (State::Idle, Event::Close) => Some(State::Closing),

        (State::Opening, Event::Opened) => Some(State::Buffering),
        (State::Opening, Event::Fail) => Some(State::Error),
        (State::Opening, Event::Close) => Some(State::Closing),

        (State::Buffering, Event::Buffered) => Some(State::Playing),
        (State::Buffering, Event::Fail) => Some(State::Error),
        (State::Buffering, Event::Close) => Some(State::Closing),

        (State::Playing, Event::Pause) => Some(State::Paused {
            resume: PlayResume::Playing,
        }),
        (State::Playing, Event::Seek) => Some(State::Seeking {
            resume: SeekResume::Playing,
        }),
        (State::Playing, Event::Eof) => Some(State::Draining),
        (State::Playing, Event::Fail) => Some(State::Error),
        (State::Playing, Event::Close) => Some(State::Closing),

        (State::Paused { resume }, Event::Resume) => Some(match resume {
            PlayResume::Playing => State::Playing,
            PlayResume::Draining => State::Draining,
        }),
        (State::Paused { resume }, Event::Seek) => Some(State::Seeking {
            resume: SeekResume::Paused(*resume),
        }),
        (State::Paused { .. }, Event::Fail) => Some(State::Error),
        (State::Paused { .. }, Event::Close) => Some(State::Closing),

        (State::Seeking { resume }, Event::SeekDone) => Some(match resume {
            SeekResume::Playing => State::Playing,
            SeekResume::Paused(inner) => State::Paused { resume: *inner },
        }),
        (State::Seeking { resume }, Event::Seek) => Some(State::Seeking { resume: *resume }),
        (State::Seeking { .. }, Event::Fail) => Some(State::Error),
        (State::Seeking { .. }, Event::Close) => Some(State::Closing),

        (State::Draining, Event::Pause) => Some(State::Paused {
            resume: PlayResume::Draining,
        }),
        (State::Draining, Event::Drained) => Some(State::Ended),
        (State::Draining, Event::Seek) => Some(State::Seeking {
            resume: SeekResume::Playing,
        }),
        (State::Draining, Event::Fail) => Some(State::Error),
        (State::Draining, Event::Close) => Some(State::Closing),

        (State::Ended, Event::Open) => Some(State::Opening),
        (State::Ended, Event::Seek) => Some(State::Seeking {
            resume: SeekResume::Playing,
        }),
        (State::Ended, Event::Close) => Some(State::Closing),

        (State::Error, Event::Close) => Some(State::Closing),

        (State::Closing, Event::Closed) => Some(State::Idle),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drive(events: &[Event]) -> Machine {
        let mut machine = Machine::new();
        for event in events {
            machine.apply(*event).expect("legal edge");
        }
        machine
    }

    #[test]
    fn happy_path_to_ended() {
        let machine = drive(&[
            Event::Open,
            Event::Opened,
            Event::Buffered,
            Event::Pause,
            Event::Resume,
            Event::Eof,
            Event::Drained,
        ]);
        assert_eq!(*machine.state(), State::Ended);
    }

    #[test]
    fn seek_while_playing_resumes_playing() {
        let machine = drive(&[
            Event::Open,
            Event::Opened,
            Event::Buffered,
            Event::Seek,
            Event::SeekDone,
        ]);
        assert_eq!(*machine.state(), State::Playing);
    }

    #[test]
    fn seek_while_paused_resumes_paused() {
        let machine = drive(&[
            Event::Open,
            Event::Opened,
            Event::Buffered,
            Event::Pause,
            Event::Seek,
        ]);
        assert_eq!(
            *machine.state(),
            State::Seeking {
                resume: SeekResume::Paused(PlayResume::Playing)
            }
        );
        let mut machine = machine;
        machine.apply(Event::SeekDone).expect("legal edge");
        assert_eq!(
            *machine.state(),
            State::Paused {
                resume: PlayResume::Playing
            }
        );
    }

    #[test]
    fn reseek_keeps_original_resume_target() {
        let machine = drive(&[
            Event::Open,
            Event::Opened,
            Event::Buffered,
            Event::Pause,
            Event::Seek,
            Event::Seek,
            Event::SeekDone,
        ]);
        assert_eq!(
            *machine.state(),
            State::Paused {
                resume: PlayResume::Playing
            }
        );
    }

    #[test]
    fn pause_during_drain_out_resumes_draining() {
        let machine = drive(&[
            Event::Open,
            Event::Opened,
            Event::Buffered,
            Event::Eof,
            Event::Pause,
        ]);
        assert_eq!(
            *machine.state(),
            State::Paused {
                resume: PlayResume::Draining
            }
        );
        let mut machine = machine;
        machine.apply(Event::Resume).expect("legal edge");
        assert_eq!(*machine.state(), State::Draining);
    }

    #[test]
    fn seek_from_draining_and_ended_restarts_playback() {
        for tail in [Event::Eof, Event::Drained] {
            let mut machine = drive(&[Event::Open, Event::Opened, Event::Buffered, Event::Eof]);
            if tail == Event::Drained {
                machine.apply(Event::Drained).expect("legal edge");
            }
            machine.apply(Event::Seek).expect("legal seek");
            assert_eq!(
                *machine.state(),
                State::Seeking {
                    resume: SeekResume::Playing
                }
            );
        }
    }

    #[test]
    fn any_state_reaches_closing_then_idle() {
        let paths: &[&[Event]] = &[
            &[Event::Open],
            &[Event::Open, Event::Opened],
            &[Event::Open, Event::Opened, Event::Buffered],
            &[
                Event::Open,
                Event::Opened,
                Event::Buffered,
                Event::Pause,
            ],
            &[Event::Open, Event::Opened, Event::Buffered, Event::Seek],
            &[Event::Open, Event::Opened, Event::Buffered, Event::Eof],
            &[
                Event::Open,
                Event::Opened,
                Event::Buffered,
                Event::Eof,
                Event::Drained,
            ],
            &[Event::Open, Event::Fail],
        ];
        for path in paths {
            let mut machine = drive(path);
            machine.apply(Event::Close).expect("close from anywhere");
            assert_eq!(*machine.state(), State::Closing);
            machine.apply(Event::Closed).expect("closed");
            assert_eq!(*machine.state(), State::Idle);
        }
    }

    #[test]
    fn fail_reaches_error_and_only_close_leaves() {
        let mut machine = drive(&[Event::Open, Event::Opened, Event::Buffered]);
        machine.apply(Event::Fail).expect("fail");
        assert_eq!(*machine.state(), State::Error);
        for event in [
            Event::Open,
            Event::Resume,
            Event::Seek,
            Event::SeekDone,
            Event::Closed,
        ] {
            assert!(machine.apply(event).is_err(), "reject {event:?}");
        }
        machine.apply(Event::Close).expect("close from error");
    }

    #[test]
    fn open_advances_generation_and_clears_error() {
        let mut machine = drive(&[
            Event::Open,
            Event::Opened,
            Event::Buffered,
            Event::Eof,
            Event::Drained,
        ]);
        let first = machine.generation();
        machine.set_error("stale");
        machine.apply(Event::Open).expect("advance after ended");
        assert_eq!(*machine.state(), State::Opening);
        assert_eq!(machine.generation(), first + 1);
        assert_eq!(machine.error(), None);
    }

    #[test]
    fn illegal_edges_rejected() {
        let cases: &[(&[Event], Event)] = &[
            (&[], Event::Pause),
            (&[], Event::Resume),
            (&[], Event::Seek),
            (&[], Event::Buffered),
            (&[Event::Open], Event::Buffered),
            (&[Event::Open], Event::Pause),
            (
                &[Event::Open, Event::Opened, Event::Buffered],
                Event::Buffered,
            ),
            (
                &[Event::Open, Event::Opened, Event::Buffered],
                Event::Opened,
            ),
            (
                &[Event::Open, Event::Opened, Event::Buffered, Event::Pause],
                Event::Eof,
            ),
            (
                &[Event::Open, Event::Opened, Event::Buffered, Event::Pause],
                Event::Pause,
            ),
            (
                &[Event::Open, Event::Opened, Event::Buffered],
                Event::Resume,
            ),
            (
                &[
                    Event::Open,
                    Event::Opened,
                    Event::Buffered,
                    Event::Eof,
                    Event::Drained,
                ],
                Event::Pause,
            ),
            (&[Event::Open, Event::Fail], Event::SeekDone),
            (&[Event::Close], Event::Close),
            (&[Event::Close], Event::Open),
        ];
        for (path, event) in cases {
            let mut machine = drive(path);
            let before = machine.state().clone();
            assert!(machine.apply(*event).is_err(), "reject {event:?} in {before:?}");
            assert_eq!(*machine.state(), before);
        }
    }
}
