use crate::sys;
use crate::{Surface, Vulkan};
use core::ffi::c_int;

pub struct Swapchain {
    sw: sys::PlSwapchain,
}

pub struct SwapFrame {
    pub fbo: sys::PlTex,
    pub flipped: bool,
}

impl Swapchain {
    pub fn open(vk: &Vulkan, surface: &Surface, width: c_int, height: c_int) -> Result<Self, String> {
        let params = sys::PlVulkanSwapchainParams {
            surface: surface.raw(),
            present_mode: sys::VK_PRESENT_MODE_MAILBOX_KHR,
            swapchain_depth: 3,
            allow_suboptimal: false,
            disable_10bit_sdr: false,
            alpha_bits: 0,
            color_bits: 0,
        };
        // SAFETY: the Vulkan device is live and the surface belongs to it.
        let sw = unsafe { sys::pl_vulkan_create_swapchain(vk.as_raw(), &params) };
        if sw.is_null() {
            return Err("pl_vulkan_create_swapchain failed".to_string());
        }
        let mut chain = Swapchain { sw };
        chain.resize(width, height)?;
        Ok(chain)
    }

    pub fn resize(&mut self, width: c_int, height: c_int) -> Result<(), String> {
        // SAFETY: the swapchain is live; the caller sizes it from the
        // surface configure when the compositor leaves the size open.
        let mut width = width;
        let mut height = height;
        if !unsafe { sys::pl_swapchain_resize(self.sw, &mut width, &mut height) } {
            return Err("pl_swapchain_resize failed".to_string());
        }
        Ok(())
    }

    pub fn start_frame(&mut self) -> Result<SwapFrame, String> {
        // SAFETY: the swapchain frame is valid until submit.
        let mut frame: sys::PlSwapchainFrame = unsafe { core::mem::zeroed() };
        // SAFETY: the swapchain is live.
        if !unsafe { sys::pl_swapchain_start_frame(self.sw, &mut frame) } {
            return Err("pl_swapchain_start_frame failed".to_string());
        }
        if frame.fbo.is_null() {
            return Err("swapchain returned no framebuffer".to_string());
        }
        Ok(SwapFrame {
            fbo: frame.fbo,
            flipped: frame.flipped,
        })
    }

    pub fn submit(&mut self) -> Result<(), String> {
        // SAFETY: the current frame was rendered.
        if !unsafe { sys::pl_swapchain_submit_frame(self.sw) } {
            return Err("pl_swapchain_submit_frame failed".to_string());
        }
        unsafe { sys::pl_swapchain_swap_buffers(self.sw) };
        Ok(())
    }
}

impl Drop for Swapchain {
    fn drop(&mut self) {
        // SAFETY: the swapchain is live; the renderer and device below it
        // drop after this runs.
        unsafe { sys::pl_swapchain_destroy(&mut self.sw) };
    }
}
