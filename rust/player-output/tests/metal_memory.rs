#![cfg(target_os = "macos")]

use player_output::{render_frame, ColorSpec, Gpu, PixelLayout, Plane};

fn resident_mb() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().expect("rss parses") / 1024.0
}

#[test]
fn metal_frames_on_a_plain_thread_do_not_accumulate_memory() {
    let growth = std::thread::spawn(|| {
        let gpu = Gpu::metal().expect("metal initializes headlessly");
        let renderer = gpu.create_renderer().expect("renderer creates");
        let rgba = vec![90u8; 512 * 512 * 4];
        let planes = [Plane {
            data: &rgba,
            stride: 512 * 4,
            width: 512,
            height: 512,
        }];
        let render = || {
            render_frame(&gpu, &renderer, PixelLayout::Rgba8, ColorSpec::bt709_full(), &planes, 512, 512, None)
                .expect("frame renders");
        };
        for _ in 0..20 {
            render();
        }
        let before = resident_mb();
        for _ in 0..300 {
            render();
        }
        resident_mb() - before
    })
    .join()
    .expect("render thread finishes");
    assert!(growth < 64.0, "300 Metal frames grew resident memory by {growth:.0} MB");
}
