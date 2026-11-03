#![cfg(target_os = "linux")]

use player_display::Window;
use player_output::{ColorSpec, PixelLayout, Plane};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

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
            .present_planes(PixelLayout::Rgba8, ColorSpec::bt709_full(), &planes, 320, 240)
            .expect("frame presents");
        assert!(!window.poll().expect("events dispatch"), "compositor closed early");
    }
    assert_eq!(window.presented(), 30);
    drop(window);
}
