use crate::sys;
use core::ffi::c_void;

pub struct Instance {
    raw: sys::VkInstance,
}

impl Instance {
    pub fn new() -> Result<Self, String> {
        let app_name = c"player";
        let engine_name = c"player-output";
        let app_info = sys::VkApplicationInfo {
            s_type: sys::VK_STRUCTURE_TYPE_APPLICATION_INFO,
            next: core::ptr::null(),
            app_name: app_name.as_ptr(),
            app_version: 0,
            engine_name: engine_name.as_ptr(),
            engine_version: 0,
            api_version: sys::VK_API_VERSION_1_2,
        };
        let surface_ext = c"VK_KHR_surface";
        #[cfg(target_os = "macos")]
        let platform_ext = c"VK_EXT_metal_surface";
        #[cfg(not(target_os = "macos"))]
        let platform_ext = c"VK_KHR_wayland_surface";
        let extensions = [surface_ext.as_ptr(), platform_ext.as_ptr()];
        #[cfg(target_os = "macos")]
        let portability_ext = c"VK_KHR_portability_enumeration";
        #[cfg(target_os = "macos")]
        let extensions = [extensions[0], extensions[1], portability_ext.as_ptr()];
        let create = sys::VkInstanceCreateInfo {
            s_type: sys::VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
            next: core::ptr::null(),
            #[cfg(target_os = "macos")]
            flags: sys::VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR,
            #[cfg(not(target_os = "macos"))]
            flags: 0,
            app_info: &app_info,
            layer_count: 0,
            layers: core::ptr::null(),
            extension_count: extensions.len() as u32,
            extensions: extensions.as_ptr(),
        };
        let mut raw: sys::VkInstance = core::ptr::null_mut();
        // SAFETY: the create info outlives the call; the allocator is unused.
        let status = unsafe { sys::vkCreateInstance(&create, core::ptr::null(), &mut raw) };
        if status != sys::VK_SUCCESS || raw.is_null() {
            return Err(format!("vkCreateInstance failed: {status}"));
        }
        Ok(Instance { raw })
    }

    pub fn create_surface(
        &self,
        display: *mut c_void,
        surface: *mut c_void,
    ) -> Result<Surface, String> {
        let create = sys::VkWaylandSurfaceCreateInfoKHR {
            s_type: sys::VK_STRUCTURE_TYPE_WAYLAND_SURFACE_CREATE_INFO_KHR,
            next: core::ptr::null(),
            flags: 0,
            display,
            surface,
        };
        let mut raw: sys::VkSurfaceKHR = 0;
        // SAFETY: the instance and Wayland objects are live.
        let status = unsafe {
            sys::vkCreateWaylandSurfaceKHR(self.raw, &create, core::ptr::null(), &mut raw)
        };
        if status != sys::VK_SUCCESS || raw == 0 {
            return Err(format!("vkCreateWaylandSurfaceKHR failed: {status}"));
        }
        Ok(Surface {
            instance: self.raw,
            raw,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn create_metal_surface(&self, layer: *mut c_void) -> Result<Surface, String> {
        let create = sys::VkMetalSurfaceCreateInfoEXT {
            s_type: sys::VK_STRUCTURE_TYPE_METAL_SURFACE_CREATE_INFO_EXT,
            next: core::ptr::null(),
            flags: 0,
            layer,
        };
        let mut raw: sys::VkSurfaceKHR = 0;
        // SAFETY: the instance and Metal layer are live.
        let status = unsafe {
            sys::vkCreateMetalSurfaceEXT(self.raw, &create, core::ptr::null(), &mut raw)
        };
        if status != sys::VK_SUCCESS || raw == 0 {
            return Err(format!("vkCreateMetalSurfaceEXT failed: {status}"));
        }
        Ok(Surface {
            instance: self.raw,
            raw,
        })
    }

    pub(crate) fn raw(&self) -> sys::VkInstance {
        self.raw
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        // SAFETY: all device resources are destroyed before this runs.
        unsafe { sys::vkDestroyInstance(self.raw, core::ptr::null()) };
    }
}

pub struct Surface {
    instance: sys::VkInstance,
    raw: sys::VkSurfaceKHR,
}

impl Surface {
    pub(crate) fn raw(&self) -> sys::VkSurfaceKHR {
        self.raw
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // SAFETY: the swapchain is gone and the device is still live;
        // the surface must die before both the device and the instance.
        unsafe { sys::vkDestroySurfaceKHR(self.instance, self.raw, core::ptr::null()) };
    }
}
