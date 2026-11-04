use player_engine::video::PresentedFrame;
use player_output::{ColorSpec, Overlay, PixelLayout, Plane};

#[cfg(target_os = "linux")]
use player_display::{InputEvent, Window};
#[cfg(target_os = "macos")]
use player_display::{MacInput, MacWindow};

pub enum HostEvent {
    Key { name: String, pressed: bool },
    PointerMotion { x: f64, y: f64 },
    PointerButton { name: String, pressed: bool },
    PointerLeave,
    OpenFiles { paths: Vec<String>, append: bool },
}

pub struct HostWindow {
    #[cfg(target_os = "linux")]
    window: Window,
    #[cfg(target_os = "macos")]
    window: MacWindow,
    presented: u64,
}

impl HostWindow {
    pub fn open(title: &str, width: i32, height: i32) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            let window = Window::open(title, width, height)?;
            Ok(HostWindow {
                window,
                presented: 0,
            })
        }
        #[cfg(target_os = "macos")]
        {
            let window = MacWindow::open(title, width, height)?;
            Ok(HostWindow {
                window,
                presented: 0,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (title, width, height);
            Err("no window backend for this platform".to_owned())
        }
    }

    pub fn width(&self) -> i32 {
        self.window.width()
    }

    pub fn height(&self) -> i32 {
        self.window.height()
    }

    pub fn presented(&self) -> u64 {
        self.presented
    }

    pub fn fullscreen(&self) -> bool {
        self.window.fullscreen()
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.window.set_fullscreen(fullscreen);
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        self.window.resize(width, height);
    }

    pub fn poll(&mut self) -> Result<bool, String> {
        self.window.poll()
    }

    pub fn drain_input(&mut self) -> Result<Vec<HostEvent>, String> {
        #[cfg(target_os = "linux")]
        {
            let events = self.window.drain_input()?;
            Ok(events
                .into_iter()
                .filter_map(|event| match event {
                    InputEvent::Key { code, pressed } => gui_key_name(code).map(|name| {
                        HostEvent::Key { name, pressed }
                    }),
                    InputEvent::PointerMotion { x, y } => Some(HostEvent::PointerMotion { x, y }),
                    InputEvent::PointerButton { button, pressed } => {
                        (button == 0x110).then_some(HostEvent::PointerButton {
                            name: "MBTN_LEFT".to_owned(),
                            pressed,
                        })
                    }
                    InputEvent::PointerLeave => Some(HostEvent::PointerLeave),
                    _ => None,
                })
                .collect())
        }
        #[cfg(target_os = "macos")]
        {
            Ok(self
                .window
                .drain_input()
                .into_iter()
                .filter_map(|event| match event {
                    MacInput::Key {
                        key_code,
                        pressed,
                        ..
                    } => gui_key_name(key_code as u32).map(|name| HostEvent::Key { name, pressed }),
                    MacInput::PointerMotion { x, y } => Some(HostEvent::PointerMotion { x, y }),
                    MacInput::PointerLeave => Some(HostEvent::PointerLeave),
                    MacInput::OpenFiles { paths, append } => {
                        Some(HostEvent::OpenFiles { paths, append })
                    }
                })
                .collect())
        }
    }

    pub fn present(
        &mut self,
        frame: &PresentedFrame,
        overlay: Option<&Overlay<'_>>,
    ) -> Result<(), String> {
        if frame.width == 0 || frame.height == 0 {
            return Ok(());
        }
        let width = frame.width as i32;
        let height = frame.height as i32;
        let planes = [Plane {
            data: &frame.rgba,
            stride: frame.width as usize * 4,
            width,
            height,
        }];
        self.window.present_planes(
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            width,
            height,
            overlay,
        )?;
        self.presented += 1;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn gui_key_name(code: u32) -> Option<String> {
    let base = code & 0xffff;
    let name = match base {
        57 => "SPACE",
        16 => "q",
        1 => "ESCAPE",
        33 => "f",
        105 => "LEFT",
        106 => "RIGHT",
        103 => "UP",
        108 => "DOWN",
        50 => "m",
        24 => "o",
        36 => "j",
        52 => ".",
        _ => return None,
    };
    Some(name.to_owned())
}

#[cfg(target_os = "macos")]
fn gui_key_name(code: u32) -> Option<String> {
    let base = code & 0xffff;
    let name = match base {
        49 => "SPACE",
        12 => "q",
        53 => "ESCAPE",
        3 => "f",
        123 => "LEFT",
        124 => "RIGHT",
        126 => "UP",
        125 => "DOWN",
        46 => "m",
        31 => "o",
        38 => "j",
        47 => ".",
        _ => return None,
    };
    Some(name.to_owned())
}
