#[cfg(test)]
type OpenGpu = fn() -> Result<context::Gpu, String>;

#[cfg(test)]
macro_rules! gpu_tests {
    ($($name:ident),* $(,)?) => {
        mod vulkan {
            $(
                #[test]
                #[cfg_attr(target_os = "linux", ignore = "static-pie test binaries cannot call the container Vulkan loader; run with RUSTFLAGS=\"-C target-feature=-crt-static\"")]
                fn $name() {
                    super::$name(crate::Gpu::vulkan)
                }
            )*
        }

        #[cfg(target_os = "macos")]
        mod metal {
            $(
                #[test]
                fn $name() {
                    super::$name(crate::Gpu::metal)
                }
            )*
        }
    };
}

#[cfg(target_os = "macos")]
mod iosurface;
mod context;
mod frame;
mod instance;
mod render;
mod swapchain;
mod sys;

pub use context::{with_autorelease_pool, Gpu, GpuApi};
pub use frame::{
    render_frame, render_to_target, ColorSpec, FrameSpec, Overlay, OverlayRect, PixelLayout, Plane,
    RenderTarget, VideoGeometry,
};
pub use instance::{Instance, Surface};
pub use render::target_frame;
pub use swapchain::{Swapchain, SwapFrame};
pub use sys::{
    PL_COLOR_LEVELS_FULL, PL_COLOR_LEVELS_LIMITED, PL_COLOR_SYSTEM_BT_2020_NC,
    PL_COLOR_SYSTEM_BT_601, PL_COLOR_SYSTEM_BT_709,
};
pub use render::{rgba_frame, Renderer};

#[cfg(target_os = "macos")]
pub use iosurface::NativeSurface;
#[cfg(target_os = "macos")]
pub use frame::{render_surface_to_target, render_surface_frame};
