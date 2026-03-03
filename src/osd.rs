use std::ffi::c_void;
use std::sync::Mutex;

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};

// CoreGraphics color FFI
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorCreate(space: *mut c_void, components: *const f64) -> *mut c_void;
    fn CGColorRelease(color: *mut c_void);
    fn CGColorSpaceRelease(space: *mut c_void);
}

pub(crate) fn create_cgcolor(r: f64, g: f64, b: f64, a: f64) -> *mut c_void {
    unsafe {
        let space = CGColorSpaceCreateDeviceRGB();
        let c = [r, g, b, a];
        let color = CGColorCreate(space, c.as_ptr());
        CGColorSpaceRelease(space);
        color
    }
}

pub(crate) fn release_cgcolor(color: *mut c_void) {
    if !color.is_null() {
        unsafe { CGColorRelease(color) };
    }
}

struct OsdInner {
    message: *mut AnyObject,
    subtitle: *mut AnyObject,
    message_deadline_ms: u64,
    message_visible: bool,
}

unsafe impl Send for OsdInner {}

static OSD: Mutex<Option<OsdInner>> = Mutex::new(None);

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn disable_animations() {
    let cls = AnyClass::get(c"CATransaction").unwrap();
    let _: () = unsafe { msg_send![cls, begin] };
    let _: () = unsafe { msg_send![cls, setDisableActions: Bool::YES] };
}

fn commit_animations() {
    let cls = AnyClass::get(c"CATransaction").unwrap();
    let _: () = unsafe { msg_send![cls, commit] };
}

/// Create OSD + subtitle layers on the given parent layer. Main thread only.
pub fn init_layers(parent_ptr: *mut c_void, bounds: CGRect) {
    let parent = parent_ptr as *mut AnyObject;
    let cls = AnyClass::get(c"CATextLayer").expect("CATextLayer");
    let scale: f64 = unsafe { msg_send![parent, contentsScale] };

    // OSD message layer (bottom-left)
    let msg: Retained<AnyObject> = unsafe { msg_send![cls, new] };
    setup_text_layer(
        &msg,
        CGRect::new(
            CGPoint::new(12.0, 12.0),
            CGSize::new(bounds.size.width - 24.0, 30.0),
        ),
        16.0,
        scale,
        false,
    );
    let _: () = unsafe { msg_send![parent, addSublayer: &*msg] };

    // Subtitle layer (bottom-center)
    let sub: Retained<AnyObject> = unsafe { msg_send![cls, new] };
    setup_text_layer(
        &sub,
        CGRect::new(
            CGPoint::new(40.0, 50.0),
            CGSize::new(bounds.size.width - 80.0, 120.0),
        ),
        22.0,
        scale,
        true,
    );
    let _: () = unsafe { msg_send![parent, addSublayer: &*sub] };

    let msg_ptr = Retained::into_raw(msg) as *mut AnyObject;
    let sub_ptr = Retained::into_raw(sub) as *mut AnyObject;

    *OSD.lock().unwrap() = Some(OsdInner {
        message: msg_ptr,
        subtitle: sub_ptr,
        message_deadline_ms: 0,
        message_visible: false,
    });
}

fn setup_text_layer(
    layer: &AnyObject,
    frame: CGRect,
    font_size: f64,
    scale: f64,
    centered: bool,
) {
    let _: () = unsafe { msg_send![layer, setFrame: frame] };
    let _: () = unsafe { msg_send![layer, setFontSize: font_size] };
    let _: () = unsafe { msg_send![layer, setContentsScale: scale] };

    // White text
    let white = create_cgcolor(1.0, 1.0, 1.0, 1.0);
    let _: () = unsafe { msg_send![layer, setForegroundColor: white] };
    release_cgcolor(white);

    // Black shadow for outline effect
    let black = create_cgcolor(0.0, 0.0, 0.0, 1.0);
    let _: () = unsafe { msg_send![layer, setShadowColor: black] };
    release_cgcolor(black);
    let _: () = unsafe { msg_send![layer, setShadowOpacity: 1.0f32] };
    let zero = CGSize::new(0.0, 0.0);
    let _: () = unsafe { msg_send![layer, setShadowOffset: zero] };
    let _: () = unsafe { msg_send![layer, setShadowRadius: 2.0f64] };

    // Auto-resize width with parent
    let _: () = unsafe { msg_send![layer, setAutoresizingMask: 2u32] };

    if centered {
        let center = objc2_foundation::NSString::from_str("center");
        let _: () = unsafe { msg_send![layer, setAlignmentMode: &*center] };
        let _: () = unsafe { msg_send![layer, setWrapped: Bool::YES] };
    }

    // Hidden initially
    let _: () = unsafe { msg_send![layer, setOpacity: 0.0f32] };
}

/// Show a transient OSD message (bottom-left, fades after 2s). Main thread only.
pub fn show_message(text: &str) {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };

    let ns = objc2_foundation::NSString::from_str(text);
    disable_animations();
    let _: () = unsafe { msg_send![inner.message, setString: &*ns] };
    let _: () = unsafe { msg_send![inner.message, setOpacity: 1.0f32] };
    commit_animations();

    inner.message_deadline_ms = now_ms() + 2000;
    inner.message_visible = true;
}

/// Show or hide subtitle text (bottom-center). Main thread only.
pub fn show_subtitle(text: Option<&str>) {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };

    disable_animations();
    match text {
        Some(t) => {
            let ns = objc2_foundation::NSString::from_str(t);
            let _: () = unsafe { msg_send![inner.subtitle, setString: &*ns] };
            let _: () = unsafe { msg_send![inner.subtitle, setOpacity: 1.0f32] };
        }
        None => {
            let _: () = unsafe { msg_send![inner.subtitle, setOpacity: 0.0f32] };
        }
    }
    commit_animations();
}

/// Called on main thread to expire OSD messages.
pub fn tick() {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };

    if inner.message_visible && now_ms() >= inner.message_deadline_ms {
        disable_animations();
        let _: () = unsafe { msg_send![inner.message, setOpacity: 0.0f32] };
        commit_animations();
        inner.message_visible = false;
    }
}
