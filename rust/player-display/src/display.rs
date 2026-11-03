//! Linux Wayland window with Vulkan swapchain presentation.
//!
//! The window owns the whole display stack: the Wayland connection and
//! protocol objects, the Vulkan instance and surface, the libplacebo
//! device, renderer, and swapchain. Frames come in as decoded planes
//! and go out through `present_planes`.

use player_output::{
    render_to_target, ColorSpec, Instance, Overlay, PixelLayout, Plane, Renderer, Surface,
    Swapchain, Vulkan,
};
use std::collections::VecDeque;
use std::os::fd::{AsRawFd, FromRawFd};
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_compositor, wl_keyboard, wl_pointer, wl_registry, wl_seat, wl_surface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(10);

pub enum InputEvent {
    Key { code: u32, pressed: bool },
    PointerButton { button: u32, pressed: bool },
    PointerAxis { vertical: bool, value: f64 },
    PointerMotion { x: f64, y: f64 },
}

struct App {
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    seat: Option<wl_seat::WlSeat>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pending: VecDeque<InputEvent>,
    configured: bool,
    closed: bool,
    fullscreen: bool,
    width: i32,
    height: i32,
}

impl Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &wayland_client::globals::GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(
        _state: &mut Self,
        proxy: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            proxy.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(
        state: &mut Self,
        proxy: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            proxy.ack_configure(serial);
            state.configured = true;
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for App {
    fn event(
        state: &mut Self,
        _proxy: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Configure {
                width,
                height,
                states,
            } => {
                if width > 0 && height > 0 {
                    state.width = width;
                    state.height = height;
                }
                state.fullscreen = is_fullscreen(&states);
            }
            xdg_toplevel::Event::Close => {
                state.closed = true;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        _state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        state: &mut Self,
        _proxy: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap { fd, .. } => {
                drop_keymap(fd.as_raw_fd());
            }
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                state.pending.push_back(InputEvent::Key {
                    code: key,
                    pressed: translate_key_state(key_state),
                });
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(
        state: &mut Self,
        _proxy: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                state.pending.push_back(InputEvent::PointerButton {
                    button,
                    pressed: translate_button_state(button_state),
                });
            }
            wl_pointer::Event::Axis { axis, value, .. } => {
                state.pending.push_back(InputEvent::PointerAxis {
                    vertical: translate_axis(axis),
                    value,
                });
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                state.pending.push_back(InputEvent::PointerMotion {
                    x: surface_x,
                    y: surface_y,
                });
            }
            _ => {}
        }
    }
}

fn drop_keymap(fd: std::os::fd::RawFd) {
    let owned = unsafe { std::fs::File::from_raw_fd(fd) };
    drop(owned);
}

fn translate_key_state(state: wayland_client::WEnum<wl_keyboard::KeyState>) -> bool {
    u32::from(state) == wl_keyboard::KeyState::Pressed as u32
}

fn translate_button_state(state: wayland_client::WEnum<wl_pointer::ButtonState>) -> bool {
    u32::from(state) == wl_pointer::ButtonState::Pressed as u32
}

fn translate_axis(axis: wayland_client::WEnum<wl_pointer::Axis>) -> bool {
    u32::from(axis) == wl_pointer::Axis::VerticalScroll as u32
}

fn is_fullscreen(states: &[u8]) -> bool {
    states.contains(&(xdg_toplevel::State::Fullscreen as u8))
}

#[cfg(test)]
mod protocol_tests {
    use super::*;
    use wayland_client::WEnum;

    #[test]
    fn key_press_release_roundtrip() {
        assert!(translate_key_state(WEnum::Value(
            wl_keyboard::KeyState::Pressed
        )));
        assert!(!translate_key_state(WEnum::Value(
            wl_keyboard::KeyState::Released
        )));
        assert!(!translate_key_state(WEnum::Unknown(7)));
    }

    #[test]
    fn button_and_axis_orientation() {
        assert!(translate_button_state(WEnum::Value(
            wl_pointer::ButtonState::Pressed
        )));
        assert!(!translate_button_state(WEnum::Value(
            wl_pointer::ButtonState::Released
        )));
        assert!(translate_axis(WEnum::Value(
            wl_pointer::Axis::VerticalScroll
        )));
        assert!(!translate_axis(WEnum::Value(
            wl_pointer::Axis::HorizontalScroll
        )));
    }

    #[test]
    fn fullscreen_state_detection() {
        assert!(is_fullscreen(&[
            xdg_toplevel::State::Fullscreen as u8
        ]));
        assert!(!is_fullscreen(&[]));
        assert!(!is_fullscreen(&[
            xdg_toplevel::State::Maximized as u8,
            xdg_toplevel::State::Activated as u8,
        ]));
    }
}

delegate_noop!(App: ignore wl_compositor::WlCompositor);
delegate_noop!(App: ignore wl_surface::WlSurface);

pub struct Window {
    // Destruction order is declaration order: the swapchain, then the
    // surface while the device is still live, then the renderer, then
    // the Vulkan device, then the instance, and only then the Wayland
    // protocol objects.
    swapchain: Swapchain,
    #[allow(dead_code)]
    surface: Surface,
    renderer: Renderer,
    vulkan: Vulkan,
    #[allow(dead_code)]
    instance: Instance,
    queue: EventQueue<App>,
    app: App,
    _shell: Shell,
    width: i32,
    height: i32,
    presented: u64,
}

struct Shell {
    surface: wl_surface::WlSurface,
    _xdg_surface: xdg_surface::XdgSurface,
    toplevel: xdg_toplevel::XdgToplevel,
}

impl Window {
    pub fn open(title: &str, width: i32, height: i32) -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|err| format!("wayland connect: {err:?}"))?;
        let (globals, mut queue) =
            wayland_client::globals::registry_queue_init::<App>(&conn)
                .map_err(|err| format!("wayland registry: {err}"))?;
        let qh = queue.handle();
        let mut app = App {
            wm_base: None,
            seat: None,
            keyboard: None,
            pointer: None,
            pending: VecDeque::new(),
            configured: false,
            closed: false,
            fullscreen: false,
            width,
            height,
        };
        let compositor: wl_compositor::WlCompositor = globals
            .bind(&qh, 1..=1, ())
            .map_err(|err| format!("bind compositor: {err}"))?;
        let wm_base: xdg_wm_base::XdgWmBase = globals
            .bind(&qh, 1..=1, ())
            .map_err(|err| format!("bind xdg_wm_base: {err}"))?;
        let surface = compositor.create_surface(&qh, ());
        let xdg_surface = wm_base.get_xdg_surface(&surface, &qh, ());
        let toplevel = xdg_surface.get_toplevel(&qh, ());
        toplevel.set_title(title.to_string());
        toplevel.set_app_id("player".to_string());
        surface.commit();
        bind_seat(&globals, &qh, &mut app)?;
        let shell = Shell {
            surface,
            _xdg_surface: xdg_surface,
            toplevel,
        };
        let deadline = Instant::now() + CONFIGURE_TIMEOUT;
        while !app.configured {
            if Instant::now() > deadline {
                return Err("wayland surface was never configured".to_string());
            }
            queue
                .roundtrip(&mut app)
                .map_err(|err| format!("wayland configure: {err}"))?;
        }
        app.wm_base = Some(wm_base);
        let display = conn.backend().display_ptr();
        if display.is_null() {
            return Err("wayland backend has no display pointer".to_string());
        }
        let raw_surface = shell.surface.id().as_ptr() as *mut core::ffi::c_void;
        if raw_surface.is_null() {
            return Err("wayland surface has no proxy pointer".to_string());
        }
        let instance = Instance::new()?;
        let surface = instance.create_surface(display as *mut _, raw_surface)?;
        let vulkan = Vulkan::create_for_surface(&instance, &surface)?;
        let renderer = vulkan.create_renderer()?;
        let width = app_width(&app, width);
        let height = app_height(&app, height);
        let swapchain = Swapchain::open(&vulkan, &surface, width, height)?;
        Ok(Window {
            swapchain,
            surface,
            renderer,
            vulkan,
            instance,
            queue,
            app,
            _shell: shell,
            width,
            height,
            presented: 0,
        })
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    pub fn presented(&self) -> u64 {
        self.presented
    }

    pub fn fullscreen(&self) -> bool {
        self.app.fullscreen
    }

    pub fn set_fullscreen(&self, fullscreen: bool) {
        if fullscreen {
            self._shell.toplevel.set_fullscreen(None);
        } else {
            self._shell.toplevel.unset_fullscreen();
        }
        self._shell.surface.commit();
    }

    pub fn drain_input(&mut self) -> Result<Vec<InputEvent>, String> {
        self.pump()?;
        Ok(self.app.pending.drain(..).collect())
    }

    fn pump(&mut self) -> Result<(), String> {
        self.queue
            .flush()
            .map_err(|err| format!("wayland flush: {err}"))?;
        if let Some(guard) = self.queue.prepare_read() {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(err))
                    if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(format!("wayland read: {err}")),
            }
        }
        self.queue
            .dispatch_pending(&mut self.app)
            .map_err(|err| format!("wayland dispatch: {err}"))?;
        Ok(())
    }

    pub fn poll(&mut self) -> Result<bool, String> {
        self.pump()?;
        if self.app.closed {
            return Ok(true);
        }
        let width = app_width(&self.app, self.width);
        let height = app_height(&self.app, self.height);
        if (width, height) != (self.width, self.height) {
            self.swapchain.resize(width, height)?;
            self.width = width;
            self.height = height;
        }
        Ok(false)
    }

    pub fn present_planes(
        &mut self,
        layout: PixelLayout,
        color: ColorSpec,
        planes: &[Plane<'_>],
        width: i32,
        height: i32,
        overlay: Option<&Overlay>,
    ) -> Result<(), String> {
        let frame = self.swapchain.start_frame()?;
        if let Err(message) = render_to_target(
            &self.vulkan,
            &self.renderer,
            layout,
            color,
            planes,
            frame.fbo,
            width,
            height,
            frame.flipped,
            overlay,
        ) {
            let _ = self.swapchain.submit();
            return Err(message);
        }
        self.swapchain.submit()?;
        self.presented += 1;
        Ok(())
    }
}

fn bind_seat(
    globals: &wayland_client::globals::GlobalList,
    qh: &QueueHandle<App>,
    app: &mut App,
) -> Result<(), String> {
    let version = globals
        .contents()
        .clone_list()
        .iter()
        .find(|global| global.interface == "wl_seat")
        .map(|global| global.version.min(7));
    let Some(version) = version else {
        return Ok(());
    };
    let seat: wl_seat::WlSeat = globals
        .bind(qh, version..=version, ())
        .map_err(|err| format!("bind seat: {err}"))?;
    app.keyboard = Some(seat.get_keyboard(qh, ()));
    app.pointer = Some(seat.get_pointer(qh, ()));
    app.seat = Some(seat);
    Ok(())
}

fn app_width(app: &App, fallback: i32) -> i32 {
    if app.width > 0 { app.width } else { fallback }
}

fn app_height(app: &App, fallback: i32) -> i32 {
    if app.height > 0 { app.height } else { fallback }
}
