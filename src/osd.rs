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
    // Progress bar layers
    bar_bg: *mut AnyObject,        // container: semi-transparent background
    bar_left_time: *mut AnyObject,  // CATextLayer: left timestamp
    bar_right_time: *mut AnyObject, // CATextLayer: right timestamp
    bar_track: *mut AnyObject,      // CALayer: unfilled track
    bar_fill: *mut AnyObject,       // CALayer: filled track
    message_deadline_ms: u64,
    message_visible: bool,
    bar_visible: bool,
    bar_hide_deadline_ms: u64,
    current_us: i64,
    duration_us: i64,
    /// After a click-seek, hold the bar position until the audio clock catches up.
    seek_hold_until_ms: u64,
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

// ── Progress bar layout constants ──────────────────────────────────────────

const BAR_HEIGHT: f64 = 36.0;
const BAR_FONT_SIZE: f64 = 14.0;
const BAR_PAD: f64 = 12.0;
/// Width reserved for "HH:MM:SS" label.
const TIME_LABEL_W: f64 = 80.0;
const TRACK_HEIGHT: f64 = 4.0;
const TRACK_RADIUS: f64 = 2.0;

/// X where the track starts.
fn track_x() -> f64 {
    BAR_PAD + TIME_LABEL_W + BAR_PAD
}

/// Track width for a given bar width.
fn track_width(bar_w: f64) -> f64 {
    (bar_w - 2.0 * (BAR_PAD + TIME_LABEL_W + BAR_PAD)).max(0.0)
}

// ── Layer creation ─────────────────────────────────────────────────────────

/// Create OSD + subtitle layers on the given parent layer. Main thread only.
pub fn init_layers(parent_ptr: *mut c_void, bounds: CGRect) {
    let parent = parent_ptr as *mut AnyObject;
    let text_cls = AnyClass::get(c"CATextLayer").expect("CATextLayer");
    let layer_cls = AnyClass::get(c"CALayer").expect("CALayer");
    let scale: f64 = unsafe { msg_send![parent, contentsScale] };

    // OSD message layer (bottom-left)
    let msg: Retained<AnyObject> = unsafe { msg_send![text_cls, new] };
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
    let sub: Retained<AnyObject> = unsafe { msg_send![text_cls, new] };
    let _: () = unsafe { msg_send![&*sub, setContentsScale: scale] };
    let black = create_cgcolor(0.0, 0.0, 0.0, 1.0);
    let _: () = unsafe { msg_send![&*sub, setShadowColor: black] };
    release_cgcolor(black);
    let _: () = unsafe { msg_send![&*sub, setShadowOpacity: 1.0f32] };
    let zero = CGSize::new(0.0, 0.0);
    let _: () = unsafe { msg_send![&*sub, setShadowOffset: zero] };
    let _: () = unsafe { msg_send![&*sub, setShadowRadius: 2.0f64] };
    let center_align = objc2_foundation::NSString::from_str("center");
    let _: () = unsafe { msg_send![&*sub, setAlignmentMode: &*center_align] };
    let _: () = unsafe { msg_send![&*sub, setWrapped: Bool::YES] };
    let _: () = unsafe { msg_send![&*sub, setOpacity: 0.0f32] };
    let _: () = unsafe { msg_send![parent, addSublayer: &*sub] };

    // ── Progress bar ───────────────────────────────────────────────────────
    let bar_w = bounds.size.width;

    // Background container
    let bar_bg: Retained<AnyObject> = unsafe { msg_send![layer_cls, new] };
    let bar_frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(bar_w, BAR_HEIGHT));
    let _: () = unsafe { msg_send![&*bar_bg, setFrame: bar_frame] };
    let bg_color = create_cgcolor(0.0, 0.0, 0.0, 0.6);
    let _: () = unsafe { msg_send![&*bar_bg, setBackgroundColor: bg_color] };
    release_cgcolor(bg_color);
    // Auto-resize width with parent
    let _: () = unsafe { msg_send![&*bar_bg, setAutoresizingMask: 2u32] };
    let _: () = unsafe { msg_send![&*bar_bg, setOpacity: 0.0f32] };
    let _: () = unsafe { msg_send![parent, addSublayer: &*bar_bg] };

    // Left timestamp
    let bar_left: Retained<AnyObject> = unsafe { msg_send![text_cls, new] };
    let text_y = (BAR_HEIGHT - BAR_FONT_SIZE * 1.3) / 2.0;
    let left_frame = CGRect::new(
        CGPoint::new(BAR_PAD, text_y),
        CGSize::new(TIME_LABEL_W, BAR_FONT_SIZE * 1.3),
    );
    setup_bar_text_layer(&bar_left, left_frame, scale, false);
    let _: () = unsafe { msg_send![&*bar_bg, addSublayer: &*bar_left] };

    // Right timestamp
    let bar_right: Retained<AnyObject> = unsafe { msg_send![text_cls, new] };
    let right_x = bar_w - BAR_PAD - TIME_LABEL_W;
    let right_frame = CGRect::new(
        CGPoint::new(right_x, text_y),
        CGSize::new(TIME_LABEL_W, BAR_FONT_SIZE * 1.3),
    );
    setup_bar_text_layer(&bar_right, right_frame, scale, true);
    // Auto-position right label relative to right edge when bar resizes
    // autoresizingMask: kCALayerMinXMargin (1 << 0) = 1
    let _: () = unsafe { msg_send![&*bar_right, setAutoresizingMask: 1u32] };
    let _: () = unsafe { msg_send![&*bar_bg, addSublayer: &*bar_right] };

    // Track background (unfilled)
    let bar_track: Retained<AnyObject> = unsafe { msg_send![layer_cls, new] };
    let tw = track_width(bar_w);
    let track_y = (BAR_HEIGHT - TRACK_HEIGHT) / 2.0;
    let track_frame = CGRect::new(
        CGPoint::new(track_x(), track_y),
        CGSize::new(tw, TRACK_HEIGHT),
    );
    let _: () = unsafe { msg_send![&*bar_track, setFrame: track_frame] };
    let track_color = create_cgcolor(1.0, 1.0, 1.0, 0.2);
    let _: () = unsafe { msg_send![&*bar_track, setBackgroundColor: track_color] };
    release_cgcolor(track_color);
    let _: () = unsafe { msg_send![&*bar_track, setCornerRadius: TRACK_RADIUS] };
    // Auto-resize width: kCALayerWidthSizable (1 << 1) = 2
    let _: () = unsafe { msg_send![&*bar_track, setAutoresizingMask: 2u32] };
    let _: () = unsafe { msg_send![&*bar_bg, addSublayer: &*bar_track] };

    // Track fill
    let bar_fill: Retained<AnyObject> = unsafe { msg_send![layer_cls, new] };
    let fill_frame = CGRect::new(
        CGPoint::new(track_x(), track_y),
        CGSize::new(0.0, TRACK_HEIGHT),
    );
    let _: () = unsafe { msg_send![&*bar_fill, setFrame: fill_frame] };
    let fill_color = create_cgcolor(1.0, 1.0, 1.0, 0.85);
    let _: () = unsafe { msg_send![&*bar_fill, setBackgroundColor: fill_color] };
    release_cgcolor(fill_color);
    let _: () = unsafe { msg_send![&*bar_fill, setCornerRadius: TRACK_RADIUS] };
    let _: () = unsafe { msg_send![&*bar_bg, addSublayer: &*bar_fill] };

    // Store raw pointers
    let msg_ptr = Retained::into_raw(msg) as *mut AnyObject;
    let sub_ptr = Retained::into_raw(sub) as *mut AnyObject;

    *OSD.lock().unwrap() = Some(OsdInner {
        parent,
        message: msg_ptr,
        subtitle: sub_ptr,
        bar_bg: Retained::into_raw(bar_bg) as *mut AnyObject,
        bar_left_time: Retained::into_raw(bar_left) as *mut AnyObject,
        bar_right_time: Retained::into_raw(bar_right) as *mut AnyObject,
        bar_track: Retained::into_raw(bar_track) as *mut AnyObject,
        bar_fill: Retained::into_raw(bar_fill) as *mut AnyObject,
        message_deadline_ms: 0,
        message_visible: false,
        bar_visible: false,
        bar_hide_deadline_ms: 0,
        current_us: 0,
        duration_us: 0,
        seek_hold_until_ms: 0,
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

/// Configure a timestamp text layer inside the progress bar.
fn setup_bar_text_layer(layer: &AnyObject, frame: CGRect, scale: f64, right_align: bool) {
    let _: () = unsafe { msg_send![layer, setFrame: frame] };
    let _: () = unsafe { msg_send![layer, setContentsScale: scale] };
    let _: () = unsafe { msg_send![layer, setFontSize: BAR_FONT_SIZE] };
    // Monospace font
    let font_name = objc2_foundation::NSString::from_str("Menlo");
    let font_ptr: *const c_void = &*font_name as *const _ as *const c_void;
    let _: () = unsafe { msg_send![layer, setFont: font_ptr] };
    // White text
    let white = create_cgcolor(1.0, 1.0, 1.0, 0.9);
    let _: () = unsafe { msg_send![layer, setForegroundColor: white] };
    release_cgcolor(white);
    if right_align {
        let align = objc2_foundation::NSString::from_str("right");
        let _: () = unsafe { msg_send![layer, setAlignmentMode: &*align] };
    }
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

/// Build an NSAttributedString for subtitles.
/// Uses NSShadow attribute for one outline pass; the CATextLayer's own
/// shadow provides a second pass, combining into a thick crisp outline.
fn build_sub_string(text: &str, font_size: f64) -> Retained<AnyObject> {
    unsafe {
        let font_cls = AnyClass::get(c"NSFont").unwrap();
        let font: Retained<AnyObject> =
            msg_send![font_cls, systemFontOfSize: font_size];

        let color_cls = AnyClass::get(c"NSColor").unwrap();
        let white: Retained<AnyObject> = msg_send![color_cls, whiteColor];
        let black: Retained<AnyObject> = msg_send![color_cls, blackColor];

        // Text-level shadow (rendered by Core Text, independent of CALayer shadow)
        let shadow_cls = AnyClass::get(c"NSShadow").unwrap();
        let shadow: Retained<AnyObject> = msg_send![shadow_cls, new];
        let _: () = msg_send![&*shadow, setShadowColor: &*black];
        let zero = CGSize::new(0.0, 0.0);
        let _: () = msg_send![&*shadow, setShadowOffset: zero];
        let _: () = msg_send![&*shadow, setShadowBlurRadius: 2.0f64];

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
        let k = objc2_foundation::NSString::from_str("NSShadow");
        let _: () = msg_send![&*dict, setObject: &*shadow, forKey: &*k];
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

/// Called on main thread to expire OSD messages and progress bar.
pub fn tick() {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };

    if inner.message_visible && now_ms() >= inner.message_deadline_ms {
        disable_animations();
        let _: () = unsafe { msg_send![inner.message, setOpacity: 0.0f32] };
        commit_animations();
        inner.message_visible = false;
    }

    if inner.bar_visible && now_ms() >= inner.bar_hide_deadline_ms {
        disable_animations();
        let _: () = unsafe { msg_send![inner.bar_bg, setOpacity: 0.0f32] };
        commit_animations();
        inner.bar_visible = false;
    }
}

/// Update the progress bar with current playback position. Main thread only.
/// Called periodically by the timer — skipped if a seek hold is active.
pub fn update_progress(current_us: i64, duration_us: i64) {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };
    // Don't let timer overwrite a recent click-seek position
    if now_ms() < inner.seek_hold_until_ms {
        return;
    }
    inner.current_us = current_us;
    inner.duration_us = duration_us;
    if inner.bar_visible {
        render_bar(inner);
    }
}

/// Seek via the progress bar: snap position, show bar, hold against timer updates.
pub fn seek_bar(target_us: i64, duration_us: i64) {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };
    inner.current_us = target_us;
    inner.duration_us = duration_us;
    // Hold for 500ms so the timer doesn't snap it back before the audio clock catches up
    inner.seek_hold_until_ms = now_ms() + 500;
    set_bar_visible(inner);
}

/// Show the progress bar and reset the auto-hide timer. Main thread only.
pub fn show_bar() {
    let mut osd = OSD.lock().unwrap();
    let Some(ref mut inner) = *osd else { return };
    set_bar_visible(inner);
}

fn set_bar_visible(inner: &mut OsdInner) {
    if !inner.bar_visible {
        disable_animations();
        let _: () = unsafe { msg_send![inner.bar_bg, setOpacity: 1.0f32] };
        commit_animations();
        inner.bar_visible = true;
    }
    inner.bar_hide_deadline_ms = now_ms() + 2000;
    render_bar(inner);
}

/// Height of the progress bar in points (for mouse hit testing).
pub fn bar_height() -> f64 {
    BAR_HEIGHT
}

/// Map a click x-coordinate (in window points) to a 0.0–1.0 fraction within
/// the bar's track region. Returns None if the bar isn't initialized.
pub fn bar_fraction_at_x(x: f64) -> Option<f64> {
    let osd = OSD.lock().unwrap();
    let inner = osd.as_ref()?;

    // Read the track layer's actual frame (handles window resize)
    let track_frame: CGRect = unsafe { msg_send![inner.bar_track, frame] };
    let start = track_frame.origin.x;
    let end = start + track_frame.size.width;
    if track_frame.size.width <= 0.0 {
        return None;
    }

    Some(((x - start) / (end - start)).clamp(0.0, 1.0))
}

fn render_bar(inner: &OsdInner) {
    use crate::time::format_time;

    let left = format_time(inner.current_us);
    let right = format_time(inner.duration_us);

    let fraction = if inner.duration_us > 0 {
        (inner.current_us as f64 / inner.duration_us as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Read current track width (handles window resize via autoresizingMask)
    let track_frame: CGRect = unsafe { msg_send![inner.bar_track, frame] };
    let fill_w = fraction * track_frame.size.width;

    disable_animations();

    // Update timestamps
    let ns_left = objc2_foundation::NSString::from_str(&left);
    let _: () = unsafe { msg_send![inner.bar_left_time, setString: &*ns_left] };
    let ns_right = objc2_foundation::NSString::from_str(&right);
    let _: () = unsafe { msg_send![inner.bar_right_time, setString: &*ns_right] };

    // Update fill width (pixel-smooth)
    let fill_frame = CGRect::new(
        track_frame.origin,
        CGSize::new(fill_w, TRACK_HEIGHT),
    );
    let _: () = unsafe { msg_send![inner.bar_fill, setFrame: fill_frame] };

    commit_animations();
}
