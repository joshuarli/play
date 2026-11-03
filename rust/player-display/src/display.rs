//! Linux Wayland window with Vulkan swapchain presentation.
//!
//! The window owns the whole display stack: the Wayland connection and
//! protocol objects, the Vulkan instance and surface, the libplacebo
//! device, renderer, and swapchain. Frames come in as decoded planes
//! and go out through `present_planes`.

use player_output::{
    render_to_target, ColorSpec, Instance, PixelLayout, Plane, Renderer, Surface, Swapchain,
    Vulkan,
};
use std::time::{Duration, Instant};

use wayland_client::protocol::{wl_compositor, wl_registry, wl_surface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(10);

struct App {
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    configured: bool,
    closed: bool,
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
            xdg_toplevel::Event::Configure { width, height, .. } => {
                if width > 0 && height > 0 {
                    state.width = width;
                    state.height = height;
                }
            }
            xdg_toplevel::Event::Close => {
                state.closed = true;
            }
            _ => {}
        }
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
    _toplevel: xdg_toplevel::XdgToplevel,
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
            configured: false,
            closed: false,
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
        let shell = Shell {
            surface,
            _xdg_surface: xdg_surface,
            _toplevel: toplevel,
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

    pub fn poll(&mut self) -> Result<bool, String> {
        self.queue
            .dispatch_pending(&mut self.app)
            .map_err(|err| format!("wayland dispatch: {err}"))?;
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
        ) {
            let _ = self.swapchain.submit();
            return Err(message);
        }
        self.swapchain.submit()?;
        self.presented += 1;
        Ok(())
    }
}

fn app_width(app: &App, fallback: i32) -> i32 {
    if app.width > 0 { app.width } else { fallback }
}

fn app_height(app: &App, fallback: i32) -> i32 {
    if app.height > 0 { app.height } else { fallback }
}
