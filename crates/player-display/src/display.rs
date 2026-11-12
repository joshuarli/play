//! Linux Wayland window with Vulkan swapchain presentation.
//!
//! The window owns the whole display stack: the Wayland connection and
//! protocol objects, the Vulkan instance and surface, the libplacebo
//! device, renderer, and swapchain. Frames come in as decoded planes
//! and go out through `present_planes`.

use player_output::{
    render_to_target, ColorSpec, Instance, Overlay, PixelLayout, Plane, Renderer, Surface,
    Swapchain, Gpu,
};
use std::collections::VecDeque;
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
    PointerLeave,
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

impl App {
    fn new(width: i32, height: i32) -> Self {
        App {
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
        }
    }
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
        state: &mut Self,
        proxy: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_seat::Event::Capabilities { capabilities } = event else {
            return;
        };
        let capabilities = match capabilities {
            wayland_client::WEnum::Value(capabilities) => capabilities,
            wayland_client::WEnum::Unknown(bits) => wl_seat::Capability::from_bits_truncate(bits),
        };
        state.keyboard = follow_capability(
            state.keyboard.take(),
            capabilities.contains(wl_seat::Capability::Keyboard),
            || proxy.get_keyboard(qh, ()),
            |keyboard| {
                if keyboard.version() >= 3 {
                    keyboard.release();
                }
            },
        );
        let had_pointer = state.pointer.is_some();
        state.pointer = follow_capability(
            state.pointer.take(),
            capabilities.contains(wl_seat::Capability::Pointer),
            || proxy.get_pointer(qh, ()),
            |pointer| {
                if pointer.version() >= 3 {
                    pointer.release();
                }
            },
        );
        if had_pointer && state.pointer.is_none() {
            state.pending.push_back(InputEvent::PointerLeave);
        }
    }
}

fn follow_capability<T>(
    device: Option<T>,
    advertised: bool,
    get: impl FnOnce() -> T,
    release: impl FnOnce(T),
) -> Option<T> {
    match (device, advertised) {
        (None, true) => Some(get()),
        (Some(device), false) => {
            release(device);
            None
        }
        (device, _) => device,
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
        state.keyboard_event(event);
    }
}

impl App {
    fn keyboard_event(&mut self, event: wl_keyboard::Event) {
        match event {
            wl_keyboard::Event::Keymap { fd, .. } => {
                drop(fd);
            }
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                self.pending.push_back(InputEvent::Key {
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
            wl_pointer::Event::Leave { .. } => {
                state.pending.push_back(InputEvent::PointerLeave);
            }
            _ => {}
        }
    }
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
    // the GPU device, then the instance, and only then the Wayland
    // protocol objects.
    swapchain: Swapchain,
    #[allow(dead_code)]
    surface: Surface,
    renderer: Renderer,
    gpu: Gpu,
    #[allow(dead_code)]
    instance: Instance,
    queue: EventQueue<App>,
    app: App,
    _shell: Shell,
    width: i32,
    height: i32,
    windowed_size: (i32, i32),
    constraint: Option<(i32, i32)>,
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
        let mut app = App::new(width, height);
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
        let gpu = Gpu::vulkan_for_surface(&instance, &surface)?;
        let renderer = gpu.create_renderer()?;
        let width = app_width(&app, width);
        let height = app_height(&app, height);
        let swapchain = Swapchain::vulkan(&gpu, &surface, width, height)?;
        eprintln!("vo/gpu-next/wayland: Registered output {width}x{height}");
        Ok(Window {
            swapchain,
            surface,
            renderer,
            gpu,
            instance,
            queue,
            app,
            _shell: shell,
            width,
            height,
            windowed_size: (width, height),
            constraint: None,
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

    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        if fullscreen {
            if !self.app.fullscreen {
                self.windowed_size = (self.width, self.height);
            }
            self._shell.toplevel.set_fullscreen(None);
            self._shell.surface.commit();
        } else {
            self._shell.toplevel.unset_fullscreen();
            self._shell.surface.commit();
            self.resize(self.windowed_size.0, self.windowed_size.1);
        }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        if self.constraint != Some((width, height)) {
            self.constraint = Some((width, height));
        }
        self._shell.toplevel.set_min_size(width, height);
        self._shell.toplevel.set_max_size(width, height);
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
        if let Some((target_width, target_height)) = self.constraint
            && (self.width, self.height) == (target_width, target_height)
        {
            self._shell.toplevel.set_min_size(0, 0);
            self._shell.toplevel.set_max_size(0, 0);
            self._shell.surface.commit();
            self.constraint = None;
        }
        Ok(false)
    }

    pub fn present_planes(
        &mut self,
        layout: PixelLayout,
        color: ColorSpec,
        planes: &[Plane<'_>],
        overlay: Option<&Overlay>,
    ) -> Result<(), String> {
        let frame = self.swapchain.start_frame()?;
        if let Err(message) = render_to_target(
            &self.gpu,
            &self.renderer,
            layout,
            color,
            planes,
            frame.fbo,
            self.width,
            self.height,
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
    app.seat = Some(seat);
    Ok(())
}

fn app_width(app: &App, fallback: i32) -> i32 {
    if app.width > 0 { app.width } else { fallback }
}

fn app_height(app: &App, fallback: i32) -> i32 {
    if app.height > 0 { app.height } else { fallback }
}

#[cfg(test)]
mod seat_tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::thread::JoinHandle;

    const KEYBOARD: u32 = 2;
    const POINTER: u32 = 1;

    #[derive(Debug, PartialEq)]
    struct Request {
        object: u32,
        opcode: u16,
        args: Vec<u32>,
    }

    fn words(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|word| u32::from_ne_bytes(word.try_into().unwrap()))
            .collect()
    }

    fn message(object: u32, opcode: u16, args: &[u32]) -> Vec<u8> {
        let size = (8 + args.len() * 4) as u32;
        let mut bytes = Vec::new();
        bytes.extend(object.to_ne_bytes());
        bytes.extend((size << 16 | opcode as u32).to_ne_bytes());
        for arg in args {
            bytes.extend(arg.to_ne_bytes());
        }
        bytes
    }

    fn string_words(text: &str) -> Vec<u32> {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(0);
        let length = bytes.len() as u32;
        bytes.resize(bytes.len().div_ceil(4) * 4, 0);
        let mut out = vec![length];
        out.extend(words(&bytes));
        out
    }

    fn compositor_sending_capabilities_after_each_sync(mut stream: UnixStream, capabilities: Vec<u32>) -> JoinHandle<Vec<Request>> {
        std::thread::spawn(move || {
            let mut requests = Vec::new();
            let mut buffer = Vec::new();
            let mut registry = 0;
            let mut seat = 0;
            let mut pending = capabilities.into_iter();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).unwrap_or(0);
                if read == 0 {
                    return requests;
                }
                buffer.extend_from_slice(&chunk[..read]);
                while buffer.len() >= 8 {
                    let header = words(&buffer[..8]);
                    let size = (header[1] >> 16) as usize;
                    if buffer.len() < size {
                        break;
                    }
                    let request = Request {
                        object: header[0],
                        opcode: (header[1] & 0xffff) as u16,
                        args: words(&buffer[8..size]),
                    };
                    buffer.drain(..size);
                    let mut reply = Vec::new();
                    match (request.object, request.opcode) {
                        (1, 1) => {
                            registry = request.args[0];
                            let mut global = vec![1];
                            global.extend(string_words("wl_seat"));
                            global.push(7);
                            reply.extend(message(registry, 0, &global));
                        }
                        (1, 0) => {
                            let callback = request.args[0];
                            if seat != 0 {
                                if let Some(value) = pending.next() {
                                    reply.extend(message(seat, 0, &[value]));
                                }
                            }
                            reply.extend(message(callback, 0, &[0]));
                            reply.extend(message(1, 1, &[callback]));
                        }
                        (object, 0) if object == registry => {
                            seat = *request.args.last().unwrap();
                        }
                        _ => {}
                    }
                    stream.write_all(&reply).unwrap();
                    requests.push(request);
                }
            }
        })
    }

    fn run_seat(capabilities: Vec<u32>) -> (Vec<Request>, u32) {
        let (client, server) = UnixStream::pair().unwrap();
        let fake = compositor_sending_capabilities_after_each_sync(server, capabilities.clone());
        let conn = Connection::from_socket(client).unwrap();
        let (globals, mut queue) =
            wayland_client::globals::registry_queue_init::<App>(&conn).unwrap();
        let qh = queue.handle();
        let mut app = App::new(1, 1);
        bind_seat(&globals, &qh, &mut app).unwrap();
        for _ in 0..=capabilities.len() {
            queue.roundtrip(&mut app).unwrap();
        }
        let seat = app.seat.as_ref().unwrap().id().protocol_id();
        drop(app);
        drop(queue);
        drop(conn);
        (fake.join().unwrap(), seat)
    }

    #[test]
    fn keyboard_only_seat_gets_no_pointer() {
        let (requests, seat) = run_seat(vec![KEYBOARD]);
        let on_seat: Vec<u16> = requests
            .iter()
            .filter(|request| request.object == seat)
            .map(|request| request.opcode)
            .collect();
        assert!(on_seat.contains(&1), "keyboard requested: {requests:?}");
        assert!(!on_seat.contains(&0), "pointer requested from a keyboard-only seat: {requests:?}");
    }

    #[test]
    fn lost_pointer_capability_releases_the_pointer() {
        let (requests, seat) = run_seat(vec![KEYBOARD | POINTER, KEYBOARD]);
        let pointer = requests
            .iter()
            .find(|request| request.object == seat && request.opcode == 0)
            .map(|request| request.args[0])
            .expect("pointer requested once advertised");
        assert!(
            requests.iter().any(|request| request.object == pointer && request.opcode == 1),
            "pointer released after the capability went away: {requests:?}"
        );
    }
}


#[cfg(test)]
mod keyboard_tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    #[test]
    fn keymap_event_releases_its_single_descriptor_owner() {
        let (mut peer, keymap) = UnixStream::pair().unwrap();
        peer.set_nonblocking(true).unwrap();
        let fd: OwnedFd = keymap.into();
        let mut app = App::new(32, 32);
        app.keyboard_event(wl_keyboard::Event::Keymap {
            format: wayland_client::WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
            fd,
            size: 0,
        });
        assert_eq!(peer.read(&mut [0]).unwrap(), 0);
        assert!(app.pending.is_empty());
    }
}
