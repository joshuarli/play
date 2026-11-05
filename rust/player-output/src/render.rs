use crate::sys;
use core::ffi::c_int;

pub struct Renderer {
    rr: sys::PlRenderer,
}

impl Renderer {
    pub(crate) fn as_raw(&self) -> sys::PlRenderer {
        self.rr
    }

    pub fn new(log: sys::PlLog, gpu: sys::PlGpu) -> Result<Self, String> {
        // SAFETY: log and gpu are live.
        let rr = unsafe { sys::pl_renderer_create(log, gpu) };
        if rr.is_null() {
            return Err("pl_renderer_create failed".to_string());
        }
        Ok(Renderer { rr })
    }

    pub fn render_rgba(
        &self,
        src: sys::PlTex,
        dst: sys::PlTex,
        width: c_int,
        height: c_int,
    ) -> Result<(), String> {
        let mut image = rgba_frame(src, width, height);
        let mut target = rgba_frame(dst, width, height);
        // SAFETY: the renderer is live; infer fills defaulted metadata.
        unsafe { sys::pl_frames_infer(self.rr, &mut image, &mut target) };
        // SAFETY: the library defaults are valid for any render.
        let params: sys::PlRenderParams = unsafe { sys::pl_render_default_params };
        // SAFETY: frames and textures are live for the call.
        if unsafe { sys::pl_render_image(self.rr, &image, &target, &params) } {
            Ok(())
        } else {
            Err("pl_render_image failed".to_string())
        }
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // SAFETY: the renderer is live and textures are already destroyed.
        unsafe { sys::pl_renderer_destroy(&mut self.rr) };
    }
}

pub fn rgba_frame(tex: sys::PlTex, width: c_int, height: c_int) -> sys::PlFrame {
    target_frame(tex, width, height, false)
}

pub fn target_frame(
    tex: sys::PlTex,
    width: c_int,
    height: c_int,
    flipped: bool,
) -> sys::PlFrame {
    sys::PlFrame {
        num_planes: 1,
        planes: [
            sys::PlPlane {
                texture: tex,
                address_mode: sys::PL_TEX_ADDRESS_CLAMP,
                flipped: false,
                components: 4,
                component_mapping: [0, 1, 2, 3],
                shift_x: 0.0,
                shift_y: 0.0,
            },
            empty_plane(),
            empty_plane(),
            empty_plane(),
        ],
        enhancement_layer: core::ptr::null(),
        field: sys::PL_FIELD_NONE,
        first_field: sys::PL_FIELD_NONE,
        prev: core::ptr::null(),
        next: core::ptr::null(),
        acquire: None,
        release: None,
        repr: sys::PlColorRepr {
            sys: sys::PL_COLOR_SYSTEM_RGB,
            levels: sys::PL_COLOR_LEVELS_FULL,
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
            y0: if flipped { height as f32 } else { 0.0 },
            x1: width as f32,
            y1: if flipped { 0.0 } else { height as f32 },
        },
        rotation: sys::PL_ROTATION_0,
        pixel_aspect_ratio: 1.0,
        overlays: core::ptr::null(),
        num_overlays: 0,
        film_grain: sys::PlFilmGrain { opaque: [0; 208] },
        user_data: core::ptr::null_mut(),
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
    use crate::Gpu;

    #[test]
    #[cfg_attr(
        target_os = "linux",
        ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\""
    )]
    fn rgba_renders_near_identity() {
        let vk = Gpu::vulkan().expect("vulkan initializes");
        let sample_fmt = vk
            .find_rgba8(sys::PL_FMT_CAP_SAMPLEABLE)
            .expect("sampleable rgba8");
        let target_fmt = vk
            .find_rgba8(sys::PL_FMT_CAP_RENDERABLE)
            .expect("renderable rgba8");
        let src = vk.create_tex(8, 8, sample_fmt).expect("src creates");
        let dst = vk
            .create_render_target(8, 8, target_fmt)
            .expect("target creates");
        let renderer = vk.create_renderer().expect("renderer creates");
        let mut gradient = vec![0u8; 8 * 8 * 4];
        for y in 0..8 {
            for x in 0..8 {
                let base = (y * 8 + x) * 4;
                gradient[base] = (x * 32) as u8;
                gradient[base + 1] = (y * 32) as u8;
                gradient[base + 2] = 128;
                gradient[base + 3] = 255;
            }
        }
        vk.upload(src, 8, 8, &gradient).expect("upload works");
        renderer
            .render_rgba(src, dst, 8, 8)
            .expect("render works");
        let back = vk.download(dst, 8, 8).expect("download works");
        assert_eq!(back.len(), gradient.len());
        let worst = back
            .iter()
            .zip(gradient.iter())
            .map(|(a, b)| a.abs_diff(*b))
            .max()
            .unwrap_or(0);
        assert!(worst <= 4, "render deviates: worst channel delta {worst}");
        vk.destroy_tex(src);
        vk.destroy_tex(dst);
    }
}
