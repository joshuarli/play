#![cfg(target_os = "linux")]

use player_audio::sink::SinkKind;
use player_engine::SchedulerEngine;
use player_output::{ColorSpec, PixelLayout, Plane};
use std::time::{Duration, Instant};

fn fixture(name: &str) -> String {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    root.join(".work/fixtures")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

struct SwapCompositor {
    child: std::process::Child,
    socket: String,
    dir: std::path::PathBuf,
}

impl SwapCompositor {
    fn start() -> Self {
        let dir: std::path::PathBuf =
            std::env::temp_dir().join(format!("wl-swap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("runtime dir");
        let socket = "wl-player".to_string();
        let log = std::fs::File::create(dir.join("weston.log")).expect("weston log");
        let child = std::process::Command::new("weston")
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
        SwapCompositor {
            child,
            socket,
            dir,
        }
    }
}

impl Drop for SwapCompositor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
#[cfg_attr(
    target_os = "linux",
    ignore = "static-pie test binaries cannot start the container Vulkan/Wayland stack; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
)]
fn plays_video_into_wayland_window() {
    let compositor = SwapCompositor::start();
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", compositor.dir.as_os_str());
        std::env::set_var("WAYLAND_DISPLAY", compositor.socket.as_str());
    }
    let mut window =
        player_display::Window::open("player-swap", 320, 240).expect("window opens on weston");
    let engine =
        SchedulerEngine::load(&fixture("h264.mp4"), SinkKind::Null).expect("engine loads video");
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut delivered = 0u64;
    let status = loop {
        for frame in engine.drain_frames() {
            let planes = [Plane {
                data: &frame.rgba,
                stride: frame.width as usize * 4,
                width: frame.width as i32,
                height: frame.height as i32,
            }];
            window
                .present_planes(
                    PixelLayout::Rgba8,
                    ColorSpec::bt709_full(),
                    &planes,
                    frame.width as i32,
                    frame.height as i32,
                    None,
                )
                .expect("frame presents");
            delivered += 1;
        }
        assert!(
            !window.poll().expect("events dispatch"),
            "compositor closed early"
        );
        let status = engine.snapshot();
        if status.ended || status.error.is_some() {
            break status;
        }
        assert!(Instant::now() < deadline, "timed out waiting: {status:?}");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(status.error.is_none(), "unexpected error: {:?}", status.error);
    assert!(delivered > 0, "no frames reached the window");
    assert_eq!(
        window.presented(),
        delivered,
        "every drained frame presents exactly once"
    );
}
