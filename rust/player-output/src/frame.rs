use crate::sys;
use crate::{Renderer, Vulkan};
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

pub fn render_frame(
    vk: &Vulkan,
    renderer: &Renderer,
    layout: PixelLayout,
    color: ColorSpec,
    planes: &[Plane<'_>],
    width: c_int,
    height: c_int,
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
        let fmt = vk
            .find_format(sys::PL_FMT_UNORM, spec.components, 8, sys::PL_FMT_CAP_SAMPLEABLE)
            .ok_or_else(|| "no sampleable texture format".to_string())?;
        let tex = vk.create_tex(plane.width, plane.height, fmt)?;
        if vk
            .upload_strided(tex, plane.width, plane.height, plane.stride, plane.data)
            .is_err()
        {
            vk.destroy_tex(tex);
            for done in texs {
                vk.destroy_tex(done);
            }
            return Err("plane upload failed".to_string());
        }
        texs.push(tex);
    }
    let target_fmt = vk.find_rgba8(sys::PL_FMT_CAP_RENDERABLE)?;
    let target = vk.create_render_target(width, height, target_fmt)?;
    let result = render_planes(renderer, layout, color, &texs, target, width, height);
    for tex in texs {
        vk.destroy_tex(tex);
    }
    let target = result?;
    let bytes = vk.download(target, width, height)?;
    vk.destroy_tex(target);
    Ok(bytes)
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
    renderer: &Renderer,
    layout: PixelLayout,
    color: ColorSpec,
    texs: &[sys::PlTex],
    target: sys::PlTex,
    width: c_int,
    height: c_int,
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
    let mut target_frame = crate::rgba_frame(target, width, height);
    // SAFETY: the renderer is live; infer fills defaulted metadata.
    unsafe { sys::pl_frames_infer(renderer.as_raw(), &mut frame, &mut target_frame) };
    // SAFETY: the library defaults are valid for any render.
    let params: sys::PlRenderParams = unsafe { sys::pl_render_default_params };
    // SAFETY: frames and textures are live for the call.
    if unsafe { sys::pl_render_image(renderer.as_raw(), &frame, &target_frame, &params) } {
        Ok(target)
    } else {
        Err("pl_render_image failed".to_string())
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

    fn setup() -> (Vulkan, Renderer) {
        let vk = Vulkan::create().expect("vulkan initializes headlessly");
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

    fn render_solid(y: u8, u: u8, v: u8) -> Vec<u8> {
        let (vk, renderer) = setup();
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
        )
        .expect("solid frame renders");
        assert_eq!(out.len(), 8 * 8 * 4);
        out
    }

    #[test]
    #[cfg_attr(target_os = "linux", ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\"")]
    fn yuv420_solid_colors_match_software_reference() {
        for (y, u, v) in [(54, 99, 255), (182, 30, 12), (18, 255, 117), (255, 128, 128), (0, 128, 128)] {
            let expected = bt709_full_to_rgb(y, u, v);
            let out = render_solid(y, u, v);
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

    #[test]
    #[cfg_attr(target_os = "linux", ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\"")]
    fn nv12_matches_yuv420() {
        let (vk, renderer) = setup();
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
        )
        .expect("nv12 renders");
        let planar = render_solid(54, 99, 255);
        let worst = nv
            .iter()
            .zip(planar.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(worst <= 2, "nv12 deviates from planar: {worst}");
    }

    #[test]
    #[cfg_attr(target_os = "linux", ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\"")]
    fn rgba8_passthrough_matches_input() {
        let (vk, renderer) = setup();
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
}
