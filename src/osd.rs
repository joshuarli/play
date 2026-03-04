use std::ffi::c_void;
use std::sync::Mutex;

use objc2::encode::{Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};

// Opaque CGColor type with correct ObjC encoding so msg_send! validates as ^{CGColor=}
#[repr(C)]
pub(crate) struct CGColor {
    _private: [u8; 0],
}

unsafe impl RefEncode for CGColor {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGColor", &[]));
}

// CoreGraphics color FFI
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorCreate(space: *mut c_void, components: *const f64) -> *mut CGColor;
    fn CGColorRelease(color: *mut CGColor);
    fn CGColorSpaceRelease(space: *mut c_void);
}

pub(crate) fn create_cgcolor(r: f64, g: f64, b: f64, a: f64) -> *mut CGColor {
    unsafe {
        let space = CGColorSpaceCreateDeviceRGB();
        let c = [r, g, b, a];
        let color = CGColorCreate(space, c.as_ptr());
        CGColorSpaceRelease(space);
        color
    }
}

pub(crate) fn release_cgcolor(color: *mut CGColor) {
    if !color.is_null() {
        unsafe { CGColorRelease(color) };
    }
}

struct OsdInner {
    parent: *mut AnyObject,
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

    // Subtitle layer — frame and font set dynamically in show_subtitle
    let sub: Retained<AnyObject> = unsafe { msg_send![cls, new] };
    let _: () = unsafe { msg_send![&*sub, setContentsScale: scale] };
    let black = create_cgcolor(0.0, 0.0, 0.0, 0.8);
    let _: () = unsafe { msg_send![&*sub, setShadowColor: black] };
    release_cgcolor(black);
    let _: () = unsafe { msg_send![&*sub, setShadowOpacity: 1.0f32] };
    let zero = CGSize::new(0.0, 0.0);
    let _: () = unsafe { msg_send![&*sub, setShadowOffset: zero] };
    let _: () = unsafe { msg_send![&*sub, setShadowRadius: 1.0f64] };
    let center = objc2_foundation::NSString::from_str("center");
    let _: () = unsafe { msg_send![&*sub, setAlignmentMode: &*center] };
    let _: () = unsafe { msg_send![&*sub, setWrapped: Bool::YES] };
    let _: () = unsafe { msg_send![&*sub, setOpacity: 0.0f32] };
    let _: () = unsafe { msg_send![parent, addSublayer: &*sub] };

    let msg_ptr = Retained::into_raw(msg) as *mut AnyObject;
    let sub_ptr = Retained::into_raw(sub) as *mut AnyObject;

    *OSD.lock().unwrap() = Some(OsdInner {
        parent,
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

/// Build an NSAttributedString with mpv-style outlined text for subtitles.
fn build_sub_string(text: &str, font_size: f64) -> Retained<AnyObject> {
    unsafe {
        let font_cls = AnyClass::get(c"NSFont").unwrap();
        let font: Retained<AnyObject> = msg_send![font_cls, systemFontOfSize: font_size];

        let color_cls = AnyClass::get(c"NSColor").unwrap();
        let white: Retained<AnyObject> = msg_send![color_cls, whiteColor];
        let black: Retained<AnyObject> = msg_send![color_cls, blackColor];

        // Negative NSStrokeWidth = fill + stroke (outline around each glyph)
        let num_cls = AnyClass::get(c"NSNumber").unwrap();
        let stroke_w: Retained<AnyObject> = msg_send![num_cls, numberWithDouble: -4.0f64];

        // Center-aligned paragraph style
        let para_cls = AnyClass::get(c"NSMutableParagraphStyle").unwrap();
        let para: Retained<AnyObject> = msg_send![para_cls, new];
        let _: () = msg_send![&*para, setAlignment: 2i64]; // NSTextAlignmentCenter (macOS)

        let dict_cls = AnyClass::get(c"NSMutableDictionary").unwrap();
        let dict: Retained<AnyObject> = msg_send![dict_cls, new];

        let k = objc2_foundation::NSString::from_str("NSFont");
        let _: () = msg_send![&*dict, setObject: &*font, forKey: &*k];
        let k = objc2_foundation::NSString::from_str("NSColor");
        let _: () = msg_send![&*dict, setObject: &*white, forKey: &*k];
        let k = objc2_foundation::NSString::from_str("NSStrokeColor");
        let _: () = msg_send![&*dict, setObject: &*black, forKey: &*k];
        let k = objc2_foundation::NSString::from_str("NSStrokeWidth");
        let _: () = msg_send![&*dict, setObject: &*stroke_w, forKey: &*k];
        let k = objc2_foundation::NSString::from_str("NSParagraphStyle");
        let _: () = msg_send![&*dict, setObject: &*para, forKey: &*k];

        let ns_text = objc2_foundation::NSString::from_str(text);
        let raw: *mut AnyObject =
            msg_send![AnyClass::get(c"NSAttributedString").unwrap(), alloc];
        let raw: *mut AnyObject =
            msg_send![raw, initWithString: &*ns_text, attributes: &*dict];
        Retained::from_raw(raw).unwrap()
    }
}

/// Show or hide subtitle text (bottom-center). Main thread only.
pub fn show_subtitle(text: Option<&str>) {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };

    disable_animations();
    match text {
        Some(t) => {
            // Query parent bounds for dynamic font sizing
            let bounds: CGRect = unsafe { msg_send![inner.parent, bounds] };
            let h = bounds.size.height;
            let w = bounds.size.width;

            // Scaled to ~2/3 of mpv sub-scale=0.6 (33 * 2/3 ≈ 22 at 720p ref)
            let font_size = (h * 22.0 / 720.0).max(10.0);
            let margin_y = h * 22.0 / 720.0;
            let margin_x = w * 0.05;
            let layer_h = font_size * 4.0;

            let frame = CGRect::new(
                CGPoint::new(margin_x, margin_y),
                CGSize::new(w - margin_x * 2.0, layer_h),
            );
            let _: () = unsafe { msg_send![inner.subtitle, setFrame: frame] };

            let attr = build_sub_string(t, font_size);
            let _: () = unsafe { msg_send![inner.subtitle, setString: &*attr] };
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
