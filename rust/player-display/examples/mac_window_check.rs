#[cfg(target_os = "macos")]
use player_display::MacWindow;
#[cfg(target_os = "macos")]
use player_output::{ColorSpec, PixelLayout, Plane};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

#[cfg(not(target_os = "macos"))]
fn main() {}

#[cfg(target_os = "macos")]
fn gray(window: &mut MacWindow, width: i32, height: i32) {
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
        "window closed early"
    );
}

#[cfg(target_os = "macos")]
fn main() {
    let mut window = MacWindow::open("player", 320, 240).expect("window opens");
    assert_eq!((window.width(), window.height()), (640, 480));
    assert!(!window.fullscreen());
    for _ in 0..30 {
        let (w, h) = (window.width(), window.height());
        gray(&mut window, w, h);
    }
    assert_eq!(window.presented(), 30);
    window.set_fullscreen(true);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !window.fullscreen() {
        assert!(Instant::now() < deadline, "fullscreen never engaged");
        let (w, h) = (window.width(), window.height());
        gray(&mut window, w, h);
    }
    let settle = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < settle {
        let (w, h) = (window.width(), window.height());
        gray(&mut window, w, h);
        std::thread::sleep(Duration::from_millis(16));
    }
    eprintln!("settled at {}x{}", window.width(), window.height());
    window.set_fullscreen(false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while window.fullscreen() {
        assert!(Instant::now() < deadline, "fullscreen never left");
        let (w, h) = (window.width(), window.height());
        gray(&mut window, w, h);
    }
    println!("mac-window-check: PASS presented={}", window.presented());
}
