use player_engine::video::PresentedFrame;
use player_output::{GpuApi, Overlay};

#[cfg(target_os = "linux")]
use player_display::{InputEvent, Window};
#[cfg(target_os = "macos")]
use player_display::{MacInput, MacWindow};

pub enum HostEvent {
    Key { name: String, pressed: bool },
    PointerMotion { x: f64, y: f64 },
    PointerButton { pressed: bool },
    Wheel { up: bool },
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
    pub fn open(title: &str, width: i32, height: i32, api: GpuApi) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            let GpuApi::Vulkan = api;
            let window = Window::open(title, width, height)?;
            Ok(HostWindow {
                window,
                presented: 0,
            })
        }
        #[cfg(target_os = "macos")]
        {
            let window = MacWindow::open(title, width, height, api)?;
            Ok(HostWindow {
                window,
                presented: 0,
            })
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (title, width, height, api);
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
                        (button == BTN_LEFT).then_some(HostEvent::PointerButton { pressed })
                    }
                    InputEvent::PointerAxis { vertical, value } => (vertical && value != 0.0)
                        .then_some(HostEvent::Wheel { up: value < 0.0 }),
                    InputEvent::PointerLeave => Some(HostEvent::PointerLeave),
                    _ => None,
                })
                .collect())
        }
        #[cfg(target_os = "macos")]
        {
            let mut events = Vec::new();
            for event in self.window.drain_input() {
                match event {
                    MacInput::Key {
                        key_code, pressed, ..
                    } => {
                        if let Some(name) = gui_key_name(key_code as u32) {
                            events.push(HostEvent::Key { name, pressed });
                        }
                    }
                    MacInput::PointerMotion { x, y } => {
                        events.push(HostEvent::PointerMotion { x, y });
                    }
                    MacInput::PointerButton { x, y, pressed } => {
                        events.push(HostEvent::PointerMotion { x, y });
                        events.push(HostEvent::PointerButton { pressed });
                    }
                    MacInput::Scroll { delta_y } => {
                        events.push(HostEvent::Wheel { up: delta_y > 0.0 });
                    }
                    MacInput::PointerLeave => events.push(HostEvent::PointerLeave),
                    MacInput::OpenFiles { paths, append } => {
                        events.push(HostEvent::OpenFiles { paths, append });
                    }
                }
            }
            Ok(events)
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
        let planes = frame.planes()?;
        self.window.present_planes(
            frame.layout,
            frame.color,
            &planes,
            overlay,
        )?;
        self.presented += 1;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
const BTN_LEFT: u32 = 0x110;

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
