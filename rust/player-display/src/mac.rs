use player_output::{
    render_to_target, ColorSpec, Instance, Overlay, PixelLayout, Plane, Renderer, Surface,
    Swapchain, Vulkan,
};
use std::ffi::{c_char, c_int, c_void, CStr, CString};

unsafe extern "C" {
    fn player_mac_window_open(
        title: *const c_char,
        width: c_int,
        height: c_int,
    ) -> *mut c_void;
    fn player_mac_window_layer(handle: *mut c_void) -> *mut c_void;
    fn player_mac_window_poll(handle: *mut c_void) -> c_int;
    fn player_mac_window_size(handle: *mut c_void, width: *mut c_int, height: *mut c_int);
    fn player_mac_window_resize(handle: *mut c_void, width: c_int, height: c_int);
    fn player_mac_window_fullscreen(handle: *mut c_void) -> c_int;
    fn player_mac_window_set_fullscreen(handle: *mut c_void, fullscreen: c_int);
    fn player_mac_window_next_input(
        handle: *mut c_void,
        kind: *mut c_int,
        key_code: *mut c_int,
        flags: *mut c_int,
        pressed: *mut c_int,
        repeat: *mut c_int,
        append: *mut c_int,
        x: *mut f64,
        y: *mut f64,
        path: *mut c_char,
        path_cap: c_int,
    ) -> c_int;
    fn player_mac_window_close(handle: *mut c_void);
}

struct Bridge {
    handle: *mut c_void,
}

impl Bridge {
    fn open(title: &str, width: i32, height: i32) -> Result<Self, String> {
        let name = CString::new(title).map_err(|_| "window title has a nul byte".to_string())?;
        // SAFETY: the title outlives the call; a null handle means AppKit refused.
        let handle =
            unsafe { player_mac_window_open(name.as_ptr(), width as c_int, height as c_int) };
        if handle.is_null() {
            return Err("AppKit refused the window".to_string());
        }
        Ok(Bridge { handle })
    }

    fn poll_closed(&mut self) -> bool {
        // SAFETY: the handle is live; close detection only reads the watcher flag.
        unsafe { player_mac_window_poll(self.handle) != 0 }
    }

    fn size(&self) -> (i32, i32) {
        let mut width = 0;
        let mut height = 0;
        // SAFETY: the handle is live and the out-pointers are valid.
        unsafe { player_mac_window_size(self.handle, &mut width, &mut height) };
        (width as i32, height as i32)
    }

    fn resize(&self, width: i32, height: i32) {
        // SAFETY: the handle is live.
        unsafe { player_mac_window_resize(self.handle, width as c_int, height as c_int) };
    }

    fn fullscreen(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { player_mac_window_fullscreen(self.handle) != 0 }
    }

    fn set_fullscreen(&self, fullscreen: bool) {
        // SAFETY: the handle is live.
        unsafe { player_mac_window_set_fullscreen(self.handle, fullscreen as c_int) };
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        // SAFETY: the handle came from open and outlives every GPU object.
        unsafe { player_mac_window_close(self.handle) };
    }
}

pub enum MacInput {
    Key {
        key_code: i32,
        modifiers: u32,
        pressed: bool,
        repeat: bool,
    },
    PointerMotion {
        x: f64,
        y: f64,
    },
    PointerLeave,
    PointerButton {
        x: f64,
        y: f64,
        pressed: bool,
    },
    Scroll {
        delta_y: f64,
    },
    OpenFiles {
        paths: Vec<String>,
        append: bool,
    },
}

pub struct MacWindow {
    swapchain: Swapchain,
    surface: Surface,
    renderer: Renderer,
    vulkan: Vulkan,
    instance: Instance,
    bridge: Bridge,
    width: i32,
    height: i32,
    presented: u64,
}

impl MacWindow {
    pub fn open(title: &str, width: i32, height: i32) -> Result<Self, String> {
        let bridge = Bridge::open(title, width, height)?;
        // SAFETY: the bridge owns the layer for the window lifetime.
        let layer = unsafe { player_mac_window_layer(bridge.handle) };
        if layer.is_null() {
            return Err("window has no metal layer".to_string());
        }
        let instance = Instance::new()?;
        let surface = instance.create_metal_surface(layer)?;
        let vulkan = Vulkan::create_for_surface(&instance, &surface)?;
        let renderer = vulkan.create_renderer()?;
        let mut swapchain = Swapchain::open(&vulkan, &surface, width, height)?;
        let (mut width, mut height) = (width, height);
        let (w, h) = bridge.size();
        if w > 0 && h > 0 {
            if (w, h) != (width, height) {
                swapchain.resize(w, h)?;
            }
            width = w;
            height = h;
        }
        Ok(MacWindow {
            swapchain,
            surface,
            renderer,
            vulkan,
            instance,
            bridge,
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
        self.bridge.fullscreen()
    }

    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.bridge.set_fullscreen(fullscreen);
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        self.bridge.resize(width, height);
    }

    pub fn drain_input(&mut self) -> Vec<MacInput> {
        let mut events = Vec::new();
        loop {
            let mut kind = 0;
            let mut key_code = 0;
            let mut flags = 0;
            let mut pressed = 0;
            let mut repeat = 0;
            let mut append = 0;
            let mut x = 0.0;
            let mut y = 0.0;
            let mut path = [0 as c_char; 4096];
            // SAFETY: the handle is live and the out-pointers are valid.
            let present = unsafe {
                player_mac_window_next_input(
                    self.bridge.handle,
                    &mut kind,
                    &mut key_code,
                    &mut flags,
                    &mut pressed,
                    &mut repeat,
                    &mut append,
                    &mut x,
                    &mut y,
                    path.as_mut_ptr(),
                    path.len() as c_int,
                )
            };
            if present == 0 {
                break;
            }
            match kind {
                0 => events.push(MacInput::Key {
                    key_code,
                    modifiers: flags as u32,
                    pressed: pressed != 0,
                    repeat: repeat != 0,
                }),
                1 => events.push(MacInput::PointerMotion { x, y }),
                2 => events.push(MacInput::PointerLeave),
                4 => events.push(MacInput::PointerButton {
                    x,
                    y,
                    pressed: pressed != 0,
                }),
                5 => events.push(MacInput::Scroll { delta_y: y }),
                3 => {
                    // SAFETY: the bridge wrote a NUL-terminated path.
                    let name = unsafe { CStr::from_ptr(path.as_ptr()) }
                        .to_string_lossy()
                        .into_owned();
                    if !name.is_empty() {
                        events.push(MacInput::OpenFiles {
                            paths: vec![name],
                            append: append != 0,
                        });
                    }
                }
                _ => {}
            }
        }
        events
    }

    pub fn poll(&mut self) -> Result<bool, String> {
        if self.bridge.poll_closed() {
            return Ok(true);
        }
        let (width, height) = self.bridge.size();
        if width > 0 && height > 0 && (width, height) != (self.width, self.height) {
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
