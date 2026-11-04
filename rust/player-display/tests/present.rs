#![cfg(target_os = "linux")]

use player_display::Window;
use player_output::{ColorSpec, PixelLayout, Plane};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poison| poison.into_inner())
}

struct Compositor {
    child: Child,
    socket: String,
    dir: std::path::PathBuf,
}

impl Compositor {
    fn start() -> Self {
        let dir: std::path::PathBuf = std::env::temp_dir().join(format!("wl-present-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("runtime dir");
        let socket = "wl-player".to_string();
        let log = std::fs::File::create(dir.join("weston.log")).expect("weston log");
        let child = Command::new("weston")
            .args([
                "--backend=headless",
                &format!("--socket={socket}"),
                "--no-config",
                "--idle-time=0",
            ])
            .env("XDG_RUNTIME_DIR", &dir)
            .stdout(log.try_clone().expect("log clone"))
            .stderr(log)
            .spawn()
            .expect("weston binary runs headless compositors in the Linux container gate");
        let deadline = Instant::now() + Duration::from_secs(15);
        while !dir.join(&socket).exists() {
            assert!(Instant::now() < deadline, "weston did not open its socket");
            std::thread::sleep(Duration::from_millis(50));
        }
        Compositor { child, socket, dir }
    }

    fn socket(&self) -> &str {
        &self.socket
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn gradient(width: i32, height: i32, frame: u64) -> Vec<u8> {
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let base = (y as usize * width as usize + x as usize) * 4;
            rgba[base] = ((x + frame as i32) % 256) as u8;
            rgba[base + 1] = ((y * 2) % 256) as u8;
            rgba[base + 2] = 128;
            rgba[base + 3] = 255;
        }
    }
    rgba
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "static-pie test binaries cannot start the container Vulkan/Wayland stack; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
)]
fn wayland_window_presents_frames() {
    let _guard = serial();
    let compositor = Compositor::start();
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", compositor.dir.as_os_str());
        std::env::set_var("WAYLAND_DISPLAY", compositor.socket());
    }
    let icd = std::fs::read_dir("/usr/share/vulkan/icd.d")
        .expect("vulkan icd directory exists in the Linux container gate")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with("lvp_icd"))
                .unwrap_or(false)
        })
        .expect("lavapipe icd present in the Linux container gate");
    unsafe {
        std::env::set_var("VK_DRIVER_FILES", icd.as_os_str());
    }
    let mut window = Window::open("player", 320, 240).expect("window opens on weston");
    assert_eq!((window.width(), window.height()), (320, 240));
    for frame in 0..30u64 {
        let rgba = gradient(320, 240, frame);
        let planes = [Plane {
            data: &rgba,
            stride: 320 * 4,
            width: 320,
            height: 240,
        }];
        window
            .present_planes(
                PixelLayout::Rgba8,
                ColorSpec::bt709_full(),
                &planes,
                None,
            )
            .expect("frame presents");
        assert!(!window.poll().expect("events dispatch"), "compositor closed early");
    }
    assert_eq!(window.presented(), 30);
    drop(window);
}

fn open_window(title: &str, width: i32, height: i32) -> (Compositor, Window) {
    let compositor = Compositor::start();
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", compositor.dir.as_os_str());
        std::env::set_var("WAYLAND_DISPLAY", compositor.socket());
    }
    let window = Window::open(title, width, height).expect("window opens on weston");
    (compositor, window)
}

fn open_count() -> usize {
    std::fs::read_dir("/proc/self/fd")
        .expect("fd directory lists open descriptors")
        .count()
}

fn present_gray(window: &mut Window, width: i32, height: i32) {
    let rgba = vec![128u8; width as usize * height as usize * 4];
    let planes = [Plane {
        data: &rgba,
        stride: width as usize * 4,
        width,
        height,
    }];
    window
        .present_planes(
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            None,
        )
        .expect("frame presents");
    assert!(
        !window.poll().expect("events dispatch"),
        "compositor closed early"
    );
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "static-pie test binaries cannot start the container Vulkan/Wayland stack; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
)]
fn window_fullscreen_roundtrip() {
    let _guard = serial();
    let (_compositor, window) = open_window("player-fullscreen", 320, 240);
    let mut window = window;
    present_gray(&mut window, 320, 240);
    assert!(!window.fullscreen());
    let rest = (window.width(), window.height());
    window.set_fullscreen(true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.fullscreen() {
        let (w, h) = (window.width(), window.height());
        present_gray(&mut window, w, h);
        assert!(Instant::now() < deadline, "compositor never acked fullscreen");
    }
    assert_ne!(
        (window.width(), window.height()),
        rest,
        "fullscreen configure resized the swapchain"
    );
    let full = (window.width(), window.height());
    present_gray(&mut window, full.0, full.1);
    window.set_fullscreen(false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while window.fullscreen() {
        let (w, h) = (window.width(), window.height());
        present_gray(&mut window, w, h);
        assert!(Instant::now() < deadline, "compositor never left fullscreen");
    }
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "static-pie test binaries cannot start the container Vulkan/Wayland stack; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
)]
fn window_presents_with_overlay() {
    use player_output::{Overlay, OverlayRect};
    let _guard = serial();
    let (_compositor, window) = open_window("player-overlay", 320, 240);
    let mut window = window;
    let rgba = vec![64u8; 320 * 240 * 4];
    let planes = [Plane {
        data: &rgba,
        stride: 320 * 4,
        width: 320,
        height: 240,
    }];
    let white = vec![255u8; 32 * 16 * 4];
    let overlay = Overlay {
        data: &white,
        stride: 32 * 4,
        width: 32,
        height: 16,
        dst: OverlayRect {
            x0: 10.0,
            y0: 200.0,
            x1: 42.0,
            y1: 216.0,
        },
    };
    window
        .present_planes(
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            320,
            240,
            Some(&overlay),
        )
        .expect("frame with overlay presents");
    assert!(
        !window.poll().expect("events dispatch"),
        "compositor closed early"
    );
    assert_eq!(window.presented(), 1);
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "static-pie test binaries cannot start the container Vulkan/Wayland stack; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
)]
fn window_input_without_seat() {
    let _guard = serial();
    let (_compositor, window) = open_window("player-input", 320, 240);
    let mut window = window;
    assert!(
        !window.poll().expect("events dispatch"),
        "compositor closed early"
    );
    let events = window.drain_input().expect("input drains");
    assert!(events.is_empty(), "seatless compositor sends no input");
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "static-pie test binaries cannot start the container Vulkan/Wayland stack; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
)]
fn window_open_poll_drop_keeps_fds() {
    let _guard = serial();
    let compositor = Compositor::start();
    let before = open_count();
    let window = {
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", compositor.dir.as_os_str());
            std::env::set_var("WAYLAND_DISPLAY", compositor.socket());
        }
        Window::open("player-fds", 320, 240).expect("window opens on weston")
    };
    let mut window = window;
    for _ in 0..3 {
        assert!(
            !window.poll().expect("events dispatch"),
            "compositor closed early"
        );
        let events = window.drain_input().expect("input drains");
        assert!(events.is_empty());
    }
    drop(window);
    assert_eq!(open_count(), before, "window cycle leaks descriptors");
    drop(compositor);
}
