use crate::sys;
use crate::{Gpu, Renderer};
use core::ffi::c_int;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelLayout {
    Yuv420,
    Yuv422,
    Yuv444,
    Yuv420p10,
    Yuv422p10,
    Yuv444p10,
    P010,
    Nv12,
    Nv21,
    Rgb24,
    Rgba8,
}

impl PixelLayout {
    fn storage_depth(self) -> c_int {
        match self {
            Self::Yuv420p10 | Self::Yuv422p10 | Self::Yuv444p10 | Self::P010 => 16,
            _ => 8,
        }
    }

    fn color_depth(self) -> c_int {
        if self.storage_depth() == 16 { 10 } else { 8 }
    }
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoGeometry {
    pub pixel_aspect: f64,
    pub rotation: u16,
}

impl Default for VideoGeometry {
    fn default() -> Self { Self { pixel_aspect: 1.0, rotation: 0 } }
}

impl VideoGeometry {
    pub fn display_size(self, width: u32, height: u32) -> (u32, u32) {
        let width = (width as f64 * self.pixel_aspect).round().clamp(1.0, u32::MAX as f64) as u32;
        if self.rotation == 90 || self.rotation == 270 { (height, width) } else { (width, height) }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct FrameSpec {
    pub layout: PixelLayout,
    pub color: ColorSpec,
    pub geometry: VideoGeometry,
}

impl FrameSpec {
    pub fn new(layout: PixelLayout, color: ColorSpec) -> Self {
        Self { layout, color, geometry: VideoGeometry::default() }
    }

    pub fn with_geometry(self, geometry: VideoGeometry) -> Self {
        Self { geometry, ..self }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RenderTarget {
    pub tex: sys::PlTex,
    pub width: c_int,
    pub height: c_int,
    pub flipped: bool,
}

pub fn render_frame(
    gpu: &Gpu,
    renderer: &Renderer,
    spec: FrameSpec,
    planes: &[Plane<'_>],
    (width, height): (c_int, c_int),
    overlay: Option<&Overlay>,
) -> Result<Vec<u8>, String> {
    crate::with_autorelease_pool(|| {
        render_frame_in_pool(gpu, renderer, spec, planes, (width, height), overlay)
    })
}

fn render_frame_in_pool(
    gpu: &Gpu,
    renderer: &Renderer,
    spec: FrameSpec,
    planes: &[Plane<'_>],
    (width, height): (c_int, c_int),
    overlay: Option<&Overlay>,
) -> Result<Vec<u8>, String> {
    let layout = spec.layout;
    let indexed = plane_specs(layout)?;
    if planes.len() != indexed.len() {
        return Err(format!(
            "layout {layout:?} needs {} planes, got {}",
            indexed.len(),
            planes.len()
        ));
    }
    validate_planes(layout, planes, &indexed)?;
    let mut texs: Vec<sys::PlTex> = Vec::with_capacity(indexed.len());
    for (plane, spec) in planes.iter().zip(indexed.iter()) {
        let fmt = gpu
            .find_format(sys::PL_FMT_UNORM, spec.components, layout.storage_depth(), sys::PL_FMT_CAP_SAMPLEABLE)
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
        spec,
        &texs,
        (planes[0].width, planes[0].height),
        RenderTarget { tex: target, width, height, flipped: false },
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
    spec: FrameSpec,
    planes: &[Plane<'_>],
    target: RenderTarget,
    overlay: Option<&Overlay>,
) -> Result<(), String> {
    let layout = spec.layout;
    let indexed = plane_specs(layout)?;
    if planes.len() != indexed.len() {
        return Err(format!(
            "layout {layout:?} needs {} planes, got {}",
            indexed.len(),
            planes.len()
        ));
    }
    validate_planes(layout, planes, &indexed)?;
    let mut texs: Vec<sys::PlTex> = Vec::with_capacity(indexed.len());
    for (plane, spec) in planes.iter().zip(indexed.iter()) {
        let fmt = gpu
            .find_format(sys::PL_FMT_UNORM, spec.components, layout.storage_depth(), sys::PL_FMT_CAP_SAMPLEABLE)
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
    let result = render_planes(gpu, renderer, spec, &texs, source, target, overlay);
    for tex in texs {
        gpu.destroy_tex(tex);
    }
    result?;
    Ok(())
}

#[cfg(target_os = "macos")]
pub fn render_surface_to_target(
    gpu: &Gpu, renderer: &Renderer, surface: &crate::NativeSurface,
    spec: FrameSpec, source: (i32, i32), target: RenderTarget, overlay: Option<&Overlay>,
) -> Result<bool, String> {
    crate::with_autorelease_pool(|| {
        let Some(textures) = gpu.import_surface(surface, spec.layout)? else { return Ok(false); };
        render_planes(gpu, renderer, spec, &textures, source, target, overlay)?;
        Ok(true)
    })
}

#[cfg(target_os = "macos")]
pub fn render_surface_frame(
    gpu: &Gpu, renderer: &Renderer, surface: &crate::NativeSurface,
    spec: FrameSpec, source: (i32, i32), output: (i32, i32),
) -> Result<Option<Vec<u8>>, String> {
    crate::with_autorelease_pool(|| {
        let Some(textures) = gpu.import_surface(surface, spec.layout)? else { return Ok(None); };
        let format = gpu.find_rgba8(sys::PL_FMT_CAP_RENDERABLE)?;
        let target = gpu.create_render_target(output.0, output.1, format)?;
        let render_target = RenderTarget { tex: target, width: output.0, height: output.1, flipped: false };
        let result = render_planes(gpu, renderer, spec, &textures, source, render_target, None)
            .and_then(|_| gpu.download(target, output.0, output.1));
        gpu.destroy_tex(target);
        result.map(Some)
    })
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
        PixelLayout::Yuv420 | PixelLayout::Yuv422 | PixelLayout::Yuv444
        | PixelLayout::Yuv420p10 | PixelLayout::Yuv422p10 | PixelLayout::Yuv444p10 => {
            Ok(vec![one(0), one(1), one(2)])
        }
        PixelLayout::Nv12 | PixelLayout::P010 => Ok(vec![
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

fn validate_planes(layout: PixelLayout, planes: &[Plane<'_>], specs: &[PlaneSpec]) -> Result<(), String> {
    for (plane, spec) in planes.iter().zip(specs) {
        let width = usize::try_from(plane.width).map_err(|_| "invalid plane width")?;
        let height = usize::try_from(plane.height).map_err(|_| "invalid plane height")?;
        let row = width.checked_mul(spec.components as usize)
            .and_then(|n| n.checked_mul((layout.storage_depth() / 8) as usize))
            .ok_or("plane row size overflow")?;
        let size = plane.stride.checked_mul(height).ok_or("plane size overflow")?;
        if width == 0 || height == 0 || plane.stride < row || plane.data.len() < size {
            return Err("invalid plane dimensions, stride, or buffer length".to_string());
        }
    }
    Ok(())
}

fn render_planes(
    gpu: &Gpu,
    renderer: &Renderer,
    spec: FrameSpec,
    texs: &[sys::PlTex],
    (width, height): (c_int, c_int),
    target: RenderTarget,
    overlay: Option<&Overlay>,
) -> Result<sys::PlTex, String> {
    let FrameSpec { layout, color, geometry } = spec;
    let RenderTarget { tex: target, width: target_width, height: target_height, flipped } = target;
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
                sample_depth: layout.storage_depth(),
                color_depth: layout.color_depth(),
                bit_shift: if layout == PixelLayout::P010 { 6 } else { 0 },
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
        rotation: (geometry.rotation / 90) as c_int,
        pixel_aspect_ratio: geometry.pixel_aspect as f32,
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
    target_frame.crop = fit_crop((width, height), (target_width, target_height), flipped, geometry);
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
    geometry: VideoGeometry,
) -> sys::PlRect2df {
    let (width, height) = if geometry.rotation == 90 || geometry.rotation == 270 {
        (height as f32, width as f32 * geometry.pixel_aspect as f32)
    } else {
        (width as f32 * geometry.pixel_aspect as f32, height as f32)
    };
    let scale = (target_width as f32 / width.max(1.0))
        .min(target_height as f32 / height.max(1.0));
    let fitted_width = width * scale;
    let fitted_height = height * scale;
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

    #[test]
    fn anamorphic_rotation_fits_in_display_coordinates() {
        let geometry = VideoGeometry { pixel_aspect: 2.0, rotation: 90 };
        assert_eq!(geometry.display_size(64, 32), (32, 128));
        let crop = fit_crop((64, 32), (200, 200), false, geometry);
        assert_eq!((crop.x0, crop.y0, crop.x1, crop.y1), (75.0, 0.0, 125.0, 200.0));
        let plain = VideoGeometry { pixel_aspect: 2.0, rotation: 0 };
        let crop = fit_crop((64, 32), (200, 200), false, plain);
        assert_eq!((crop.x0, crop.y0, crop.x1, crop.y1), (0.0, 75.0, 200.0, 125.0));
    }

    fn anamorphic_rotation_transposes_and_rotates_pixels(open: OpenGpu) {
        let (gpu, renderer) = setup(open);
        let mut pixels = vec![0u8; 64 * 32 * 4];
        for y in 0..32 {
            for x in 0..64 {
                let color = match (x < 32, y < 16) {
                    (true, true) => [255, 0, 0, 255],
                    (false, true) => [0, 255, 0, 255],
                    (true, false) => [0, 0, 255, 255],
                    (false, false) => [255, 255, 255, 255],
                };
                pixels[(y * 64 + x) * 4..][..4].copy_from_slice(&color);
            }
        }
        let planes = [Plane { data: &pixels, stride: 256, width: 64, height: 32 }];
        let image = render_frame(&gpu, &renderer,
            FrameSpec::new(PixelLayout::Rgba8, ColorSpec::bt709_full())
                .with_geometry(VideoGeometry { pixel_aspect: 2.0, rotation: 270 }),
            &planes, (32, 128), None).unwrap();
        let at = |x: usize, y: usize| &image[(y * 32 + x) * 4..][..3];
        assert!(at(8, 16)[1] > 200 && at(8, 16)[0] < 30, "top left: {:?}", at(8, 16));
        assert!(at(8, 112)[0] > 200 && at(8, 112)[1] < 30, "bottom left: {:?}", at(8, 112));
    }

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
        let out = render_frame(&vk, &renderer, FrameSpec::new(PixelLayout::Yuv420, ColorSpec::bt709_full()), &planes, (w, h), None)
        .expect("solid frame renders");
        assert_eq!(out.len(), 8 * 8 * 4);
        out
    }

    fn yuv420_solid_colors_match_software_reference(open: OpenGpu) {
        for (y, u, v) in [(54, 99, 255), (182, 30, 12), (18, 255, 117), (255, 128, 128), (0, 128, 128)] {
            let expected = bt709_full_to_rgb(y, u, v);
            let out = render_solid(open, y, u, v);
            let worst = out
                .as_chunks::<4>()
                .0
                .iter()
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
            FrameSpec::new(PixelLayout::Rgba8, ColorSpec::bt709_full()),
            &planes,
            RenderTarget { tex: target, width: 64, height: 64, flipped: false },
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

    fn yuv420p10_padded_samples_match_eight_bit_reference(open: OpenGpu) {
        let (gpu, renderer) = setup(open);
        let values = [216u16, 396, 1020];
        let dimensions = [(8, 8), (4, 4), (4, 4)];
        let data: Vec<Vec<u8>> = values.iter().zip(dimensions).map(|(value, (w, h))| {
            let stride = w * 2 + 4;
            let mut bytes = vec![0u8; stride * h];
            for row in bytes.chunks_exact_mut(stride) {
                for pixel in row[..w * 2].as_chunks_mut::<2>().0.iter_mut() {
                    pixel.copy_from_slice(&value.to_le_bytes());
                }
            }
            bytes
        }).collect();
        let planes: Vec<Plane<'_>> = data.iter().zip(dimensions).map(|(bytes, (w, h))| Plane {
            data: bytes,
            stride: w * 2 + 4,
            width: w as c_int,
            height: h as c_int,
        }).collect();
        let out = render_frame(&gpu, &renderer, FrameSpec::new(PixelLayout::Yuv420p10, ColorSpec::bt709_full()), &planes, (8, 8), None).expect("10-bit frame renders");
        let reference = render_solid(open, 54, 99, 255);
        let worst = out.iter().zip(reference.iter()).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 4, "10-bit samples differ by {worst}");
    }

    fn p010_packing_matches_planar_reference(open: OpenGpu) {
        let (gpu, renderer) = setup(open);
        let y: Vec<u8> = (0..64).flat_map(|_| (216u16 << 6).to_le_bytes()).collect();
        let uv: Vec<u8> = (0..16).flat_map(|_| [(396u16 << 6).to_le_bytes(), (1020u16 << 6).to_le_bytes()].concat()).collect();
        let planes = [Plane { data: &y, stride: 16, width: 8, height: 8 },
            Plane { data: &uv, stride: 16, width: 4, height: 4 }];
        let output = render_frame(&gpu, &renderer, FrameSpec::new(PixelLayout::P010, ColorSpec::bt709_full()), &planes, (8, 8), None).unwrap();
        let reference = render_solid(open, 54, 99, 255);
        let worst = output.iter().zip(reference.iter()).map(|(a,b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 4, "P010 differs by {worst}");
    }

    #[test]
    fn rejects_short_ten_bit_rows_and_buffers() {
        let specs = plane_specs(PixelLayout::Yuv420p10).unwrap();
        let data = [0u8; 16];
        let short_row = [Plane { data: &data, stride: 7, width: 4, height: 2 }];
        assert!(validate_planes(PixelLayout::Yuv420p10, &short_row, &specs).is_err());
        let short_buffer = [Plane { data: &data[..15], stride: 8, width: 4, height: 2 }];
        assert!(validate_planes(PixelLayout::Yuv420p10, &short_buffer, &specs).is_err());
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
        let nv = render_frame(&vk, &renderer, FrameSpec::new(PixelLayout::Nv12, ColorSpec::bt709_full()), &planes, (w, h), None)
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
        let plain = render_frame(&vk, &renderer, FrameSpec::new(PixelLayout::Rgba8, ColorSpec::bt709_full()), &planes, (64, 64), None)
        .expect("base renders");
        let mut white = vec![255u8; 16 * 16 * 4];
        for pixel in white.as_chunks_mut::<4>().0.iter_mut() {
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
        let covered = render_frame(&vk, &renderer, FrameSpec::new(PixelLayout::Rgba8, ColorSpec::bt709_full()), &planes, (64, 64), Some(&overlay))
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
        let out = render_frame(&vk, &renderer, FrameSpec::new(PixelLayout::Rgba8, ColorSpec::bt709_full()), &planes, (8, 8), None)
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
        let plain = render_frame(&vk, &renderer, FrameSpec::new(PixelLayout::Rgba8, ColorSpec::bt709_full()), &planes, (64, 64), None)
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
        let covered = render_frame(&vk, &renderer, FrameSpec::new(PixelLayout::Rgba8, ColorSpec::bt709_full()), &planes, (64, 64), Some(&overlay))
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
        anamorphic_rotation_transposes_and_rotates_pixels,
        yuv420_solid_colors_match_software_reference,
        presented_frame_fits_the_target_with_letterbox_bars,
        nv12_matches_yuv420,
        yuv420p10_padded_samples_match_eight_bit_reference,
        p010_packing_matches_planar_reference,
        overlay_alpha_blends_with_the_base,
        rgba8_passthrough_matches_input,
        overlay_covers_only_its_rect,
    );

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
        for (i, pixel) in ink.as_chunks_mut::<4>().0.iter_mut().enumerate() {
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
            render_frame(&gpu, &renderer, FrameSpec::new(layout, ColorSpec::bt709_full()), planes, (w as c_int, h as c_int), Some(&overlay))
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
