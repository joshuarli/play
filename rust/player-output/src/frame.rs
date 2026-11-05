use crate::sys;
use crate::{Gpu, Renderer};
use core::ffi::c_int;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelLayout {
    Yuv420,
    Yuv422,
    Yuv444,
    Nv12,
    Nv21,
    Rgb24,
    Rgba8,
}

#[derive(Clone, Copy, Debug)]
pub struct ColorSpec {
    pub system: c_int,
    pub levels: c_int,
}

impl ColorSpec {
    pub fn bt709_full() -> ColorSpec {
        ColorSpec {
            system: sys::PL_COLOR_SYSTEM_BT_709,
            levels: sys::PL_COLOR_LEVELS_FULL,
        }
    }
}

pub struct Plane<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub width: c_int,
    pub height: c_int,
}

pub struct OverlayRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

pub struct Overlay<'a> {
    pub data: &'a [u8],
    pub stride: usize,
    pub width: c_int,
    pub height: c_int,
    pub dst: OverlayRect,
}

pub fn render_frame(
    gpu: &Gpu,
    renderer: &Renderer,
    layout: PixelLayout,
    color: ColorSpec,
    planes: &[Plane<'_>],
    width: c_int,
    height: c_int,
    overlay: Option<&Overlay>,
) -> Result<Vec<u8>, String> {
    crate::with_autorelease_pool(|| {
        render_frame_in_pool(gpu, renderer, layout, color, planes, width, height, overlay)
    })
}

fn render_frame_in_pool(
    gpu: &Gpu,
    renderer: &Renderer,
    layout: PixelLayout,
    color: ColorSpec,
    planes: &[Plane<'_>],
    width: c_int,
    height: c_int,
    overlay: Option<&Overlay>,
) -> Result<Vec<u8>, String> {
    let indexed = plane_specs(layout)?;
    if planes.len() != indexed.len() {
        return Err(format!(
            "layout {layout:?} needs {} planes, got {}",
            indexed.len(),
            planes.len()
        ));
    }
    let mut texs: Vec<sys::PlTex> = Vec::with_capacity(indexed.len());
    for (plane, spec) in planes.iter().zip(indexed.iter()) {
        let fmt = gpu
            .find_format(sys::PL_FMT_UNORM, spec.components, 8, sys::PL_FMT_CAP_SAMPLEABLE)
            .ok_or_else(|| "no sampleable texture format".to_string())?;
        let tex = gpu.create_tex(plane.width, plane.height, fmt)?;
        if gpu
            .upload_strided(tex, plane.width, plane.height, plane.stride, plane.data)
            .is_err()
        {
            gpu.destroy_tex(tex);
            for done in texs {
                gpu.destroy_tex(done);
            }
            return Err("plane upload failed".to_string());
        }
        texs.push(tex);
    }
    let target_fmt = gpu.find_rgba8(sys::PL_FMT_CAP_RENDERABLE)?;
    let target = gpu.create_render_target(width, height, target_fmt)?;
    let result = render_planes(
        gpu,
        renderer,
        layout,
        color,
        &texs,
        (width, height),
        target,
        (width, height),
        false,
        overlay,
    );
    for tex in texs {
        gpu.destroy_tex(tex);
    }
    let target = result?;
    let bytes = gpu.download(target, width, height)?;
    gpu.destroy_tex(target);
    Ok(bytes)
}

pub fn render_to_target(
    gpu: &Gpu,
    renderer: &Renderer,
    layout: PixelLayout,
    color: ColorSpec,
    planes: &[Plane<'_>],
    target: sys::PlTex,
    target_width: c_int,
    target_height: c_int,
    flipped: bool,
    overlay: Option<&Overlay>,
) -> Result<(), String> {
    let indexed = plane_specs(layout)?;
    if planes.len() != indexed.len() {
        return Err(format!(
            "layout {layout:?} needs {} planes, got {}",
            indexed.len(),
            planes.len()
        ));
    }
    let mut texs: Vec<sys::PlTex> = Vec::with_capacity(indexed.len());
    for (plane, spec) in planes.iter().zip(indexed.iter()) {
        let fmt = gpu
            .find_format(sys::PL_FMT_UNORM, spec.components, 8, sys::PL_FMT_CAP_SAMPLEABLE)
            .ok_or_else(|| "no sampleable texture format".to_string())?;
        let tex = gpu.create_tex(plane.width, plane.height, fmt)?;
        if gpu
            .upload_strided(tex, plane.width, plane.height, plane.stride, plane.data)
            .is_err()
        {
            gpu.destroy_tex(tex);
            for done in texs {
                gpu.destroy_tex(done);
            }
            return Err("plane upload failed".to_string());
        }
        texs.push(tex);
    }
    let source = planes
        .first()
        .map(|plane| (plane.width, plane.height))
        .ok_or_else(|| "no planes to render".to_string())?;
    let result = render_planes(
        gpu,
        renderer,
        layout,
        color,
        &texs,
        source,
        target,
        (target_width, target_height),
        flipped,
        overlay,
    );
    for tex in texs {
        gpu.destroy_tex(tex);
    }
    result?;
    Ok(())
}

struct PlaneSpec {
    components: c_int,
    mapping: [c_int; 4],
}

fn plane_specs(layout: PixelLayout) -> Result<Vec<PlaneSpec>, String> {
    let one = |index: c_int| PlaneSpec {
        components: 1,
        mapping: [index, 0, 0, 0],
    };
    match layout {
        PixelLayout::Yuv420 | PixelLayout::Yuv422 | PixelLayout::Yuv444 => {
            Ok(vec![one(0), one(1), one(2)])
        }
        PixelLayout::Nv12 => Ok(vec![
            one(0),
            PlaneSpec {
                components: 2,
                mapping: [1, 2, 0, 0],
            },
        ]),
        PixelLayout::Nv21 => Ok(vec![
            one(0),
            PlaneSpec {
                components: 2,
                mapping: [2, 1, 0, 0],
            },
        ]),
        PixelLayout::Rgb24 => Ok(vec![PlaneSpec {
            components: 3,
            mapping: [0, 1, 2, 0],
        }]),
        PixelLayout::Rgba8 => Ok(vec![PlaneSpec {
            components: 4,
            mapping: [0, 1, 2, 3],
        }]),
    }
}

fn render_planes(
    gpu: &Gpu,
    renderer: &Renderer,
    layout: PixelLayout,
    color: ColorSpec,
    texs: &[sys::PlTex],
    (width, height): (c_int, c_int),
    target: sys::PlTex,
    (target_width, target_height): (c_int, c_int),
    flipped: bool,
    overlay: Option<&Overlay>,
) -> Result<sys::PlTex, String> {
    let specs = plane_specs(layout)?;
    let mut frame = sys::PlFrame {
        num_planes: texs.len() as c_int,
        planes: [empty_plane(), empty_plane(), empty_plane(), empty_plane()],
        enhancement_layer: core::ptr::null(),
        field: sys::PL_FIELD_NONE,
        first_field: sys::PL_FIELD_NONE,
        prev: core::ptr::null(),
        next: core::ptr::null(),
        acquire: None,
        release: None,
        repr: sys::PlColorRepr {
            sys: match layout {
                PixelLayout::Rgb24 | PixelLayout::Rgba8 => sys::PL_COLOR_SYSTEM_RGB,
                _ => color.system,
            },
            levels: match layout {
                PixelLayout::Rgb24 | PixelLayout::Rgba8 => sys::PL_COLOR_LEVELS_FULL,
                _ => color.levels,
            },
            alpha: sys::PL_ALPHA_INDEPENDENT,
            bits: sys::PlBitEncoding {
                sample_depth: 8,
                color_depth: 8,
                bit_shift: 0,
            },
            dovi: core::ptr::null(),
        },
        color: sys::PlColorSpace {
            primaries: sys::PL_COLOR_PRIM_BT_709,
            transfer: sys::PL_COLOR_TRC_SRGB,
            hdr: sys::PlHdrMetadata { opaque: [0; 148] },
        },
        icc: core::ptr::null_mut(),
        profile: sys::PlIccProfile {
            data: core::ptr::null(),
            len: 0,
            signature: 0,
        },
        lut: core::ptr::null(),
        lut_type: 0,
        crop: sys::PlRect2df {
            x0: 0.0,
            y0: 0.0,
            x1: width as f32,
            y1: height as f32,
        },
        rotation: sys::PL_ROTATION_0,
        pixel_aspect_ratio: 1.0,
        overlays: core::ptr::null(),
        num_overlays: 0,
        film_grain: sys::PlFilmGrain { opaque: [0; 208] },
        user_data: core::ptr::null_mut(),
    };
    let mut overlay_tex = core::ptr::null_mut();
    let mut overlay_part = sys::PlOverlayPart {
        src: sys::PlRect2df {
            x0: 0.0,
            y0: 0.0,
            x1: 0.0,
            y1: 0.0,
        },
        dst: sys::PlRect2df {
            x0: 0.0,
            y0: 0.0,
            x1: 0.0,
            y1: 0.0,
        },
        color: [0.0; 4],
    };
    let mut overlay_image = sys::PlOverlay {
        tex: core::ptr::null_mut(),
        mode: sys::PL_OVERLAY_NORMAL,
        coords: sys::PL_OVERLAY_COORDS_DST_FRAME,
        repr: overlay_repr(),
        color: overlay_color(),
        parts: core::ptr::null(),
        num_parts: 0,
    };
    if let Some(image) = overlay {
        let format = gpu
            .find_format(sys::PL_FMT_UNORM, 4, 8, sys::PL_FMT_CAP_SAMPLEABLE)
            .ok_or_else(|| "no sampleable overlay format".to_string())?;
        let tex = gpu.create_tex(image.width, image.height, format)?;
        if gpu
            .upload_strided(tex, image.width, image.height, image.stride, image.data)
            .is_err()
        {
            gpu.destroy_tex(tex);
            return Err("overlay upload failed".to_string());
        }
        overlay_part.src.x1 = image.width as f32;
        overlay_part.src.y1 = image.height as f32;
        overlay_part.dst = sys::PlRect2df {
            x0: image.dst.x0,
            y0: image.dst.y0,
            x1: image.dst.x1,
            y1: image.dst.y1,
        };
        overlay_image.tex = tex;
        overlay_image.parts = &overlay_part;
        overlay_image.num_parts = 1;
        overlay_tex = tex;
        frame.overlays = &overlay_image;
        frame.num_overlays = 1;
    }
    for (slot, (tex, spec)) in frame.planes.iter_mut().zip(texs.iter().zip(specs.iter())).take(4) {
        *slot = sys::PlPlane {
            texture: *tex,
            address_mode: sys::PL_TEX_ADDRESS_CLAMP,
            flipped: false,
            components: spec.components,
            component_mapping: spec.mapping,
            shift_x: 0.0,
            shift_y: 0.0,
        };
    }
    let mut target_frame = crate::render::target_frame(target, target_width, target_height, flipped);
    target_frame.crop = fit_crop((width, height), (target_width, target_height), flipped);
    // SAFETY: the renderer is live; infer fills defaulted metadata.
    unsafe { sys::pl_frames_infer(renderer.as_raw(), &mut frame, &mut target_frame) };
    // SAFETY: the library defaults are valid for any render.
    let params: sys::PlRenderParams = unsafe { sys::pl_render_default_params };
    // SAFETY: frames and textures are live for the call.
    if unsafe { sys::pl_render_image(renderer.as_raw(), &frame, &target_frame, &params) } {
        if !overlay_tex.is_null() {
            gpu.destroy_tex(overlay_tex);
        }
        Ok(target)
    } else {
        if !overlay_tex.is_null() {
            gpu.destroy_tex(overlay_tex);
        }
        Err("pl_render_image failed".to_string())
    }
}

fn fit_crop(
    (width, height): (c_int, c_int),
    (target_width, target_height): (c_int, c_int),
    flipped: bool,
) -> sys::PlRect2df {
    let scale = (target_width as f32 / width.max(1) as f32)
        .min(target_height as f32 / height.max(1) as f32);
    let fitted_width = width as f32 * scale;
    let fitted_height = height as f32 * scale;
    let x0 = ((target_width as f32 - fitted_width) / 2.0).round();
    let y0 = ((target_height as f32 - fitted_height) / 2.0).round();
    let (x1, y1) = (x0 + fitted_width.round(), y0 + fitted_height.round());
    if flipped {
        sys::PlRect2df { x0, y0: y1, x1, y1: y0 }
    } else {
        sys::PlRect2df { x0, y0, x1, y1 }
    }
}

fn overlay_repr() -> sys::PlColorRepr {
    sys::PlColorRepr {
        sys: sys::PL_COLOR_SYSTEM_RGB,
        levels: sys::PL_COLOR_LEVELS_FULL,
        alpha: sys::PL_ALPHA_INDEPENDENT,
        bits: sys::PlBitEncoding {
            sample_depth: 8,
            color_depth: 8,
            bit_shift: 0,
        },
        dovi: core::ptr::null(),
    }
}

fn overlay_color() -> sys::PlColorSpace {
    sys::PlColorSpace {
        primaries: sys::PL_COLOR_PRIM_BT_709,
        transfer: sys::PL_COLOR_TRC_SRGB,
        hdr: sys::PlHdrMetadata { opaque: [0; 148] },
    }
}

fn empty_plane() -> sys::PlPlane {
    sys::PlPlane {
        texture: core::ptr::null_mut(),
        address_mode: sys::PL_TEX_ADDRESS_CLAMP,
        flipped: false,
        components: 0,
        component_mapping: [0; 4],
        shift_x: 0.0,
        shift_y: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpenGpu;

    fn setup(open: OpenGpu) -> (Gpu, Renderer) {
        let vk = open().expect("gpu initializes headlessly");
        let renderer = vk.create_renderer().expect("renderer creates");
        (vk, renderer)
    }

    fn solid_yuv420(y: u8, u: u8, v: u8, w: c_int, h: c_int) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let cw = (w + 1) as usize / 2;
        let ch = (h + 1) as usize / 2;
        (
            vec![y; w as usize * h as usize],
            vec![u; cw * ch],
            vec![v; cw * ch],
        )
    }

    fn bt709_full_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
        let y = y as f64;
        let u = u as f64 - 128.0;
        let v = v as f64 - 128.0;
        let clamp = |x: f64| x.round().clamp(0.0, 255.0) as u8;
        [
            clamp(y + 1.5748 * v),
            clamp(y - 0.1873 * u - 0.4681 * v),
            clamp(y + 1.8556 * u),
        ]
    }

    fn render_solid(open: OpenGpu, y: u8, u: u8, v: u8) -> Vec<u8> {
        let (vk, renderer) = setup(open);
        let w = 8;
        let h = 8;
        let (yp, up, vp) = solid_yuv420(y, u, v, w, h);
        let planes = [
            Plane {
                data: &yp,
                stride: w as usize,
                width: w,
                height: h,
            },
            Plane {
                data: &up,
                stride: (w / 2) as usize,
                width: w / 2,
                height: h / 2,
            },
            Plane {
                data: &vp,
                stride: (w / 2) as usize,
                width: w / 2,
                height: h / 2,
            },
        ];
        let out = render_frame(
            &vk,
            &renderer,
            PixelLayout::Yuv420,
            ColorSpec::bt709_full(),
            &planes,
            w,
            h,
            None,
        )
        .expect("solid frame renders");
        assert_eq!(out.len(), 8 * 8 * 4);
        out
    }

    fn yuv420_solid_colors_match_software_reference(open: OpenGpu) {
        for (y, u, v) in [(54, 99, 255), (182, 30, 12), (18, 255, 117), (255, 128, 128), (0, 128, 128)] {
            let expected = bt709_full_to_rgb(y, u, v);
            let out = render_solid(open, y, u, v);
            let worst = out
                .chunks_exact(4)
                .map(|px| {
                    (px[0].abs_diff(expected[0]))
                        .max(px[1].abs_diff(expected[1]))
                        .max(px[2].abs_diff(expected[2]))
                })
                .max()
                .unwrap_or(0);
            assert!(worst <= 6, "solid yuv({y},{u},{v}): worst channel delta {worst}");
        }
    }

    fn presented_frame_fits_the_target_with_letterbox_bars(open: OpenGpu) {
        let (vk, renderer) = setup(open);
        let rgba = [255u8; 32 * 16 * 4];
        let planes = [Plane {
            data: &rgba,
            stride: 32 * 4,
            width: 32,
            height: 16,
        }];
        let format = vk.find_rgba8(sys::PL_FMT_CAP_RENDERABLE).expect("renderable format");
        let target = vk.create_render_target(64, 64, format).expect("target allocates");
        render_to_target(
            &vk,
            &renderer,
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            target,
            64,
            64,
            false,
            None,
        )
        .expect("frame renders");
        let pixels = vk.download(target, 64, 64).expect("target downloads");
        vk.destroy_tex(target);
        let luma = |x: usize, y: usize| pixels[(y * 64 + x) * 4];
        for x in [0, 31, 63] {
            assert!(luma(x, 4) < 16, "top bar at x={x} is {}", luma(x, 4));
            assert!(luma(x, 59) < 16, "bottom bar at x={x} is {}", luma(x, 59));
            for y in [17, 32, 46] {
                assert!(luma(x, y) > 240, "picture at ({x},{y}) is {}", luma(x, y));
            }
        }
    }

    fn nv12_matches_yuv420(open: OpenGpu) {
        let (vk, renderer) = setup(open);
        let w = 8;
        let h = 8;
        let (yp, up, vp) = solid_yuv420(54, 99, 255, w, h);
        let mut uv = Vec::with_capacity(4 * 4 * 2);
        for i in 0..16 {
            uv.push(up[i]);
            uv.push(vp[i]);
        }
        let planes = [
            Plane {
                data: &yp,
                stride: w as usize,
                width: w,
                height: h,
            },
            Plane {
                data: &uv,
                stride: w as usize,
                width: w / 2,
                height: h / 2,
            },
        ];
        let nv = render_frame(
            &vk,
            &renderer,
            PixelLayout::Nv12,
            ColorSpec::bt709_full(),
            &planes,
            w,
            h,
            None,
        )
        .expect("nv12 renders");
        let planar = render_solid(open, 54, 99, 255);
        let worst = nv
            .iter()
            .zip(planar.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(worst <= 2, "nv12 deviates from planar: {worst}");
    }

    fn overlay_alpha_blends_with_the_base(open: OpenGpu) {
        let (vk, renderer) = setup(open);
        let base = vec![64u8; 64 * 64 * 4];
        let planes = [Plane {
            data: &base,
            stride: 64 * 4,
            width: 64,
            height: 64,
        }];
        let plain = render_frame(
            &vk,
            &renderer,
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            64,
            64,
            None,
        )
        .expect("base renders");
        let mut white = vec![255u8; 16 * 16 * 4];
        for pixel in white.chunks_exact_mut(4) {
            pixel[3] = 128;
        }
        let overlay = Overlay {
            data: &white,
            stride: 16 * 4,
            width: 16,
            height: 16,
            dst: OverlayRect {
                x0: 24.0,
                y0: 24.0,
                x1: 40.0,
                y1: 40.0,
            },
        };
        let covered = render_frame(
            &vk,
            &renderer,
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            64,
            64,
            Some(&overlay),
        )
        .expect("overlay renders");
        let at = |bytes: &[u8], x: usize, y: usize| bytes[(y * 64 + x) * 4];
        let before = at(&plain, 32, 32);
        let after = at(&covered, 32, 32);
        assert!(
            (100..=200).contains(&after),
            "half alpha did not blend: {before} -> {after}"
        );
        assert_eq!(at(&plain, 4, 4), at(&covered, 4, 4));
    }

    fn rgba8_passthrough_matches_input(open: OpenGpu) {
        let (vk, renderer) = setup(open);
        let mut input = vec![0u8; 8 * 8 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let base = (y * 8 + x) * 4;
                input[base] = (x * 32) as u8;
                input[base + 1] = (y * 32) as u8;
                input[base + 2] = 200;
                input[base + 3] = 255;
            }
        }
        let planes = [Plane {
            data: &input,
            stride: 32,
            width: 8,
            height: 8,
        }];
        let out = render_frame(
            &vk,
            &renderer,
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            8,
            8,
            None,
        )
        .expect("rgba renders");
        let worst = out
            .iter()
            .zip(input.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(worst <= 4, "rgba deviates: {worst}");
    }

    fn overlay_covers_only_its_rect(open: OpenGpu) {
        let (vk, renderer) = setup(open);
        let base = vec![128u8; 64 * 64 * 4];
        let planes = [Plane {
            data: &base,
            stride: 64 * 4,
            width: 64,
            height: 64,
        }];
        let plain = render_frame(
            &vk,
            &renderer,
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            64,
            64,
            None,
        )
        .expect("base renders");
        let white = vec![255u8; 16 * 16 * 4];
        let overlay = Overlay {
            data: &white,
            stride: 16 * 4,
            width: 16,
            height: 16,
            dst: OverlayRect {
                x0: 24.0,
                y0: 24.0,
                x1: 40.0,
                y1: 40.0,
            },
        };
        let covered = render_frame(
            &vk,
            &renderer,
            PixelLayout::Rgba8,
            ColorSpec::bt709_full(),
            &planes,
            64,
            64,
            Some(&overlay),
        )
        .expect("overlay renders");
        let at = |bytes: &[u8], x: usize, y: usize| bytes[(y * 64 + x) * 4];
        assert_eq!(at(&covered, 4, 4), at(&plain, 4, 4));
        assert_eq!(at(&covered, 60, 60), at(&plain, 60, 60));
        let before = at(&plain, 32, 32);
        let after = at(&covered, 32, 32);
        assert!(
            after.abs_diff(before) > 100,
            "overlay pixel barely moved: {before} -> {after}"
        );
        assert!(after > 200, "overlay pixel not near white: {after}");
    }

    gpu_tests!(
        yuv420_solid_colors_match_software_reference,
        presented_frame_fits_the_target_with_letterbox_bars,
        nv12_matches_yuv420,
        overlay_alpha_blends_with_the_base,
        rgba8_passthrough_matches_input,
        overlay_covers_only_its_rect,
    );

    #[cfg(target_os = "macos")]
    #[test]
    fn metal_frames_on_a_plain_thread_do_not_accumulate_memory() {
        fn resident_mb() -> f64 {
            let out = std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
                .expect("ps runs");
            String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().expect("rss parses") / 1024.0
        }
        let growth = std::thread::spawn(|| {
            let (gpu, renderer) = setup(Gpu::metal);
            let rgba = vec![90u8; 512 * 512 * 4];
            let planes = [Plane {
                data: &rgba,
                stride: 512 * 4,
                width: 512,
                height: 512,
            }];
            let mut render = || {
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

    #[cfg(target_os = "macos")]
    #[test]
    fn metal_matches_vulkan() {
        let (w, h) = (64usize, 32usize);
        let (cw, ch) = (w / 2, h / 2);
        let luma: Vec<u8> = (0..w * h).map(|i| ((i % w) * 3 + (i / w) * 2) as u8).collect();
        let cb: Vec<u8> = (0..cw * ch).map(|i| (64 + (i % cw) * 4) as u8).collect();
        let cr: Vec<u8> = (0..cw * ch).map(|i| (200 - (i / cw) * 6) as u8).collect();
        let cbcr: Vec<u8> = cb.iter().zip(cr.iter()).flat_map(|(b, r)| [*b, *r]).collect();
        let mut ink = vec![0u8; 16 * 8 * 4];
        for (i, pixel) in ink.chunks_exact_mut(4).enumerate() {
            pixel.copy_from_slice(&[255, (i * 2) as u8, 40, (i * 2) as u8]);
        }
        let overlay = Overlay {
            data: &ink,
            stride: 16 * 4,
            width: 16,
            height: 8,
            dst: OverlayRect {
                x0: 10.0,
                y0: 6.0,
                x1: 42.0,
                y1: 22.0,
            },
        };
        fn plane(data: &[u8], stride: usize, width: usize, height: usize) -> Plane<'_> {
            Plane {
                data,
                stride,
                width: width as c_int,
                height: height as c_int,
            }
        }
        let cases = [
            (
                PixelLayout::Yuv420,
                vec![plane(&luma, w, w, h), plane(&cb, cw, cw, ch), plane(&cr, cw, cw, ch)],
            ),
            (PixelLayout::Nv12, vec![plane(&luma, w, w, h), plane(&cbcr, w, cw, ch)]),
        ];
        let render = |open: OpenGpu, layout: PixelLayout, planes: &[Plane<'_>]| {
            let (gpu, renderer) = setup(open);
            render_frame(
                &gpu,
                &renderer,
                layout,
                ColorSpec::bt709_full(),
                planes,
                w as c_int,
                h as c_int,
                Some(&overlay),
            )
            .expect("frame renders")
        };
        for (layout, planes) in &cases {
            let vulkan = render(Gpu::vulkan, *layout, planes);
            let metal = render(Gpu::metal, *layout, planes);
            assert_eq!(vulkan.len(), metal.len());
            let (worst, at) = vulkan
                .iter()
                .zip(metal.iter())
                .enumerate()
                .map(|(i, (a, b))| (a.abs_diff(*b), i))
                .max()
                .unwrap_or((0, 0));
            let pixel = at / 4;
            assert!(
                worst <= 2,
                "{layout:?}: Metal differs from Vulkan by {worst} at ({}, {}) channel {}",
                pixel % w,
                pixel / w,
                at % 4
            );
        }
    }
}
