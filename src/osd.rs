use std::cell::RefCell;
use std::ffi::c_void;

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

// SAFETY: CGColor is an opaque struct; the encoding matches CoreGraphics' pointer
// representation (^{CGColor=}) so msg_send! validates parameter types correctly.
unsafe impl RefEncode for CGColor {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGColor", &[]));
}

// SAFETY: CoreGraphics color/colorspace functions. All pointers passed to these
// functions are created and released in matching pairs within this module.
unsafe extern "C" {
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorCreate(space: *mut c_void, components: *const f64) -> *mut CGColor;
    fn CGColorRelease(color: *mut CGColor);
    fn CGColorSpaceRelease(space: *mut c_void);
}

/// RAII wrapper for a retained CGColorRef. Releases on drop.
pub(crate) struct OwnedCgColor(*mut CGColor);

impl OwnedCgColor {
    pub(crate) fn rgba(r: f64, g: f64, b: f64, a: f64) -> Self {
        // SAFETY: CGColorSpaceCreateDeviceRGB returns a valid color space with +1
        // refcount. CGColorCreate creates a color with the 4-component RGBA array.
        // We release the color space immediately (the color retains it internally).
        unsafe {
            let space = CGColorSpaceCreateDeviceRGB();
            let c = [r, g, b, a];
            let color = CGColorCreate(space, c.as_ptr());
            CGColorSpaceRelease(space);
            Self(color)
        }
    }

    pub(crate) fn as_ptr(&self) -> *mut CGColor {
        self.0
    }
}

impl Drop for OwnedCgColor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: self.0 was created by CGColorCreate in rgba().
            unsafe { CGColorRelease(self.0) };
        }
    }
}

// ── Typed CALayer wrappers ─────────────────────────────────────────
//
// Thin newtypes over raw `*mut AnyObject` that confine msg_send! to a
// small surface area. All pointers are owned (Retained::into_raw) and
// valid for the OSD lifetime (main thread, never freed until process exit).

/// A `CALayer` with typed accessors for the operations we use.
/// SAFETY invariant: the inner pointer is a valid ObjC CALayer (or subclass)
/// created via Retained::into_raw in init_layers(). It remains valid for the
/// process lifetime (layers are never deallocated).
struct Layer(*mut AnyObject);

impl Layer {
    fn set_opacity(&self, opacity: f32) {
        // SAFETY: self.0 is a valid CALayer per struct invariant.
        let _: () = unsafe { msg_send![self.0, setOpacity: opacity] };
    }

    fn set_frame(&self, frame: CGRect) {
        // SAFETY: self.0 is a valid CALayer per struct invariant.
        let _: () = unsafe { msg_send![self.0, setFrame: frame] };
    }

    fn frame(&self) -> CGRect {
        // SAFETY: self.0 is a valid CALayer per struct invariant.
        unsafe { msg_send![self.0, frame] }
    }

    fn bounds(&self) -> CGRect {
        // SAFETY: self.0 is a valid CALayer per struct invariant.
        unsafe { msg_send![self.0, bounds] }
    }
}

/// A `CATextLayer` with typed accessors.
/// SAFETY invariant: the inner Layer wraps a valid CATextLayer.
struct TextLayer(Layer);

impl TextLayer {
    fn set_opacity(&self, opacity: f32) {
        self.0.set_opacity(opacity);
    }

    fn set_frame(&self, frame: CGRect) {
        self.0.set_frame(frame);
    }

    fn set_string(&self, s: &objc2_foundation::NSString) {
        // SAFETY: self.0.0 is a valid CATextLayer; setString: accepts NSString.
        let _: () = unsafe { msg_send![self.0 .0, setString: s] };
    }

    fn set_attributed_string(&self, attr: &AnyObject) {
        // SAFETY: self.0.0 is a valid CATextLayer; setString: also accepts
        // NSAttributedString (CATextLayer renders either type).
        let _: () = unsafe { msg_send![self.0 .0, setString: attr] };
    }
}

/// Progress bar: groups the five sub-layers into a single struct.
struct ProgressBar {
    bg: Layer,
    left_time: TextLayer,
    right_time: TextLayer,
    track: Layer,
    fill: Layer,
}

impl ProgressBar {
    fn set_visible(&self, visible: bool) {
        self.bg.set_opacity(if visible { 1.0 } else { 0.0 });
    }

    fn track_frame(&self) -> CGRect {
        self.track.frame()
    }

    fn set_fill_width(&self, origin: CGPoint, width: f64) {
        self.fill
            .set_frame(CGRect::new(origin, CGSize::new(width, TRACK_HEIGHT)));
    }

    fn set_left_time(&self, s: &str) {
        let ns = objc2_foundation::NSString::from_str(s);
        self.left_time.set_string(&ns);
    }

    fn set_right_time(&self, s: &str) {
        let ns = objc2_foundation::NSString::from_str(s);
        self.right_time.set_string(&ns);
    }
}

struct OsdInner {
    parent: Layer,
    title: TextLayer,
    message: TextLayer,
    subtitle: TextLayer,
    bar: ProgressBar,
    message_deadline_ms: u64,
    message_visible: bool,
    bar_visible: bool,
    bar_hide_deadline_ms: u64,
    current_us: i64,
    duration_us: i64,
    /// Cached second values to skip redundant NSString updates.
    last_left_secs: u64,
    last_right_secs: u64,
    /// After a click-seek, hold the bar position until the audio clock catches up.
    seek_hold_until_ms: u64,
    /// Cached NSShadow for subtitle attributed strings (created once).
    cached_sub_shadow: *mut AnyObject,
    /// Cached NSMutableParagraphStyle for subtitle attributed strings.
    cached_sub_para: *mut AnyObject,
    /// Whether the system cursor is currently hidden.
    cursor_hidden: bool,
    /// Deadline (ms) after which the cursor should be hidden.
    cursor_hide_deadline_ms: u64,
}

// OSD state is main-thread-only (timer, key handler, mouse handler all run on
// the main GCD queue). RefCell enforces this at the type level and avoids the
// Mutex lock/unlock overhead that fired every 16ms on the timer path.
std::thread_local! {
    static OSD: RefCell<Option<OsdInner>> = const { RefCell::new(None) };
}

use crate::time::now_ms;

fn disable_animations() {
    let cls = AnyClass::get(c"CATransaction").expect("CATransaction");
    // SAFETY: CATransaction class methods; begin starts a transaction,
    // setDisableActions: suppresses implicit layer animations.
    let _: () = unsafe { msg_send![cls, begin] };
    let _: () = unsafe { msg_send![cls, setDisableActions: Bool::YES] };
}

fn commit_animations() {
    let cls = AnyClass::get(c"CATransaction").expect("CATransaction");
    // SAFETY: CATransaction commit ends the current transaction.
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
///
/// # Safety contract
/// `parent_ptr` must be a valid CALayer. All msg_send! calls in this function
/// target standard CALayer/CATextLayer methods. Layer pointers are leaked via
/// Retained::into_raw and stored in OsdInner for the process lifetime.
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
    let black = OwnedCgColor::rgba(0.0, 0.0, 0.0, 1.0);
    let _: () = unsafe { msg_send![&*sub, setShadowColor: black.as_ptr()] };
    let _: () = unsafe { msg_send![&*sub, setShadowOpacity: 1.0f32] };
    let zero = CGSize::new(0.0, 0.0);
    let _: () = unsafe { msg_send![&*sub, setShadowOffset: zero] };
    let _: () = unsafe { msg_send![&*sub, setShadowRadius: 2.0f64] };
    let center_align = objc2_foundation::NSString::from_str("center");
    let _: () = unsafe { msg_send![&*sub, setAlignmentMode: &*center_align] };
    let _: () = unsafe { msg_send![&*sub, setWrapped: Bool::YES] };
    let _: () = unsafe { msg_send![&*sub, setOpacity: 0.0f32] };
    let _: () = unsafe { msg_send![parent, addSublayer: &*sub] };

    // Filename title (top-left, shows/hides with progress bar)
    let title: Retained<AnyObject> = unsafe { msg_send![text_cls, new] };
    let title_frame = CGRect::new(
        CGPoint::new(12.0, bounds.size.height - 36.0),
        CGSize::new(bounds.size.width - 24.0, 20.0),
    );
    setup_text_layer(&title, title_frame, 13.0, scale, false);
    // Pin to top (kCALayerWidthSizable | kCALayerMinYMargin)
    let _: () = unsafe { msg_send![&*title, setAutoresizingMask: 10u32] };
    let _: () = unsafe { msg_send![parent, addSublayer: &*title] };

    // ── Progress bar ───────────────────────────────────────────────────────
    let bar_w = bounds.size.width;

    // Background container
    let bar_bg: Retained<AnyObject> = unsafe { msg_send![layer_cls, new] };
    let bar_frame = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(bar_w, BAR_HEIGHT));
    let _: () = unsafe { msg_send![&*bar_bg, setFrame: bar_frame] };
    let bg_color = OwnedCgColor::rgba(0.0, 0.0, 0.0, 0.6);
    let _: () = unsafe { msg_send![&*bar_bg, setBackgroundColor: bg_color.as_ptr()] };
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
    let track_color = OwnedCgColor::rgba(1.0, 1.0, 1.0, 0.2);
    let _: () = unsafe { msg_send![&*bar_track, setBackgroundColor: track_color.as_ptr()] };
    let _: () = unsafe { msg_send![&*bar_track, setCornerRadius: TRACK_RADIUS] };
    let _: () = unsafe { msg_send![&*bar_track, setAutoresizingMask: 2u32] };
    let _: () = unsafe { msg_send![&*bar_bg, addSublayer: &*bar_track] };

    // Track fill
    let bar_fill: Retained<AnyObject> = unsafe { msg_send![layer_cls, new] };
    let fill_frame = CGRect::new(
        CGPoint::new(track_x(), track_y),
        CGSize::new(0.0, TRACK_HEIGHT),
    );
    let _: () = unsafe { msg_send![&*bar_fill, setFrame: fill_frame] };
    let fill_color = OwnedCgColor::rgba(1.0, 1.0, 1.0, 0.85);
    let _: () = unsafe { msg_send![&*bar_fill, setBackgroundColor: fill_color.as_ptr()] };
    let _: () = unsafe { msg_send![&*bar_fill, setCornerRadius: TRACK_RADIUS] };
    let _: () = unsafe { msg_send![&*bar_bg, addSublayer: &*bar_fill] };

    // Create cached subtitle style objects (shadow + paragraph style)
    // SAFETY: Standard AppKit classes; objects are leaked via into_raw for
    // the process lifetime. All msg_send! calls target documented methods.
    let cached_sub_shadow: *mut AnyObject = unsafe {
        let shadow_cls = AnyClass::get(c"NSShadow").expect("NSShadow");
        let shadow: Retained<AnyObject> = msg_send![shadow_cls, new];
        let color_cls = AnyClass::get(c"NSColor").expect("NSColor");
        let black_ns: Retained<AnyObject> = msg_send![color_cls, blackColor];
        let _: () = msg_send![&*shadow, setShadowColor: &*black_ns];
        let zero = CGSize::new(0.0, 0.0);
        let _: () = msg_send![&*shadow, setShadowOffset: zero];
        let _: () = msg_send![&*shadow, setShadowBlurRadius: 2.0f64];
        Retained::into_raw(shadow)
    };
    let cached_sub_para: *mut AnyObject = unsafe {
        let para_cls = AnyClass::get(c"NSMutableParagraphStyle").expect("NSMutableParagraphStyle");
        let para: Retained<AnyObject> = msg_send![para_cls, new];
        let _: () = msg_send![&*para, setAlignment: 2i64]; // NSTextAlignmentCenter
        Retained::into_raw(para)
    };

    OSD.with(|osd| {
        *osd.borrow_mut() = Some(OsdInner {
            parent: Layer(parent),
            title: TextLayer(Layer(Retained::into_raw(title))),
            message: TextLayer(Layer(Retained::into_raw(msg))),
            subtitle: TextLayer(Layer(Retained::into_raw(sub))),
            bar: ProgressBar {
                bg: Layer(Retained::into_raw(bar_bg)),
                left_time: TextLayer(Layer(Retained::into_raw(bar_left))),
                right_time: TextLayer(Layer(Retained::into_raw(bar_right))),
                track: Layer(Retained::into_raw(bar_track)),
                fill: Layer(Retained::into_raw(bar_fill)),
            },
            message_deadline_ms: 0,
            message_visible: false,
            bar_visible: false,
            bar_hide_deadline_ms: 0,
            current_us: 0,
            duration_us: 0,
            last_left_secs: u64::MAX,
            last_right_secs: u64::MAX,
            seek_hold_until_ms: 0,
            cached_sub_shadow,
            cached_sub_para,
            cursor_hidden: false,
            cursor_hide_deadline_ms: now_ms() + 2000,
        });
    });
}

/// Configure a CATextLayer with common properties.
/// SAFETY: `layer` must be a valid CATextLayer. All msg_send! calls target
/// standard CATextLayer/CALayer properties.
fn setup_text_layer(layer: &AnyObject, frame: CGRect, font_size: f64, scale: f64, centered: bool) {
    let _: () = unsafe { msg_send![layer, setFrame: frame] };
    let _: () = unsafe { msg_send![layer, setFontSize: font_size] };
    let _: () = unsafe { msg_send![layer, setContentsScale: scale] };

    let white = OwnedCgColor::rgba(1.0, 1.0, 1.0, 1.0);
    let _: () = unsafe { msg_send![layer, setForegroundColor: white.as_ptr()] };

    let black = OwnedCgColor::rgba(0.0, 0.0, 0.0, 1.0);
    let _: () = unsafe { msg_send![layer, setShadowColor: black.as_ptr()] };
    let _: () = unsafe { msg_send![layer, setShadowOpacity: 1.0f32] };
    let zero = CGSize::new(0.0, 0.0);
    let _: () = unsafe { msg_send![layer, setShadowOffset: zero] };
    let _: () = unsafe { msg_send![layer, setShadowRadius: 2.0f64] };

    let _: () = unsafe { msg_send![layer, setAutoresizingMask: 2u32] };

    if centered {
        let center = objc2_foundation::NSString::from_str("center");
        let _: () = unsafe { msg_send![layer, setAlignmentMode: &*center] };
        let _: () = unsafe { msg_send![layer, setWrapped: Bool::YES] };
    }

    let _: () = unsafe { msg_send![layer, setOpacity: 0.0f32] };
}

/// Configure a timestamp text layer inside the progress bar.
/// SAFETY: `layer` must be a valid CATextLayer. All msg_send! calls target
/// standard CATextLayer/CALayer properties.
fn setup_bar_text_layer(layer: &AnyObject, frame: CGRect, scale: f64, right_align: bool) {
    let _: () = unsafe { msg_send![layer, setFrame: frame] };
    let _: () = unsafe { msg_send![layer, setContentsScale: scale] };
    let _: () = unsafe { msg_send![layer, setFontSize: BAR_FONT_SIZE] };
    let font_name = objc2_foundation::NSString::from_str("Menlo");
    let font_ptr: *const c_void = &*font_name as *const _ as *const c_void;
    let _: () = unsafe { msg_send![layer, setFont: font_ptr] };
    let white = OwnedCgColor::rgba(1.0, 1.0, 1.0, 0.9);
    let _: () = unsafe { msg_send![layer, setForegroundColor: white.as_ptr()] };
    if right_align {
        let align = objc2_foundation::NSString::from_str("right");
        let _: () = unsafe { msg_send![layer, setAlignmentMode: &*align] };
    }
}

/// Set the filename shown at the top of the window when the progress bar is visible.
pub fn set_title(text: &str) {
    OSD.with(|osd| {
        let osd = osd.borrow();
        let Some(ref inner) = *osd else { return };
        let ns = objc2_foundation::NSString::from_str(text);
        disable_animations();
        inner.title.set_string(&ns);
        commit_animations();
    });
}

/// Show a transient OSD message (bottom-left, fades after 2s). Main thread only.
pub fn show_message(text: &str) {
    OSD.with(|osd| {
        let mut osd = osd.borrow_mut();
        let Some(ref mut inner) = *osd else { return };

        let ns = objc2_foundation::NSString::from_str(text);
        disable_animations();
        inner.message.set_string(&ns);
        inner.message.set_opacity(1.0);
        commit_animations();

        inner.message_deadline_ms = now_ms() + 2000;
        inner.message_visible = true;
    });
}

/// Build an NSAttributedString for subtitles.
fn build_sub_string(
    text: &str,
    font_size: f64,
    shadow: *mut AnyObject,
    para: *mut AnyObject,
) -> Retained<AnyObject> {
    // SAFETY: All msg_send! calls target standard AppKit/Foundation classes
    // (NSFont, NSColor, NSMutableDictionary, NSAttributedString). shadow and
    // para are valid objects created in init_layers() and kept alive for the
    // process lifetime via Retained::into_raw.
    unsafe {
        let font_cls = AnyClass::get(c"NSFont").expect("NSFont");
        let font: Retained<AnyObject> = msg_send![font_cls, systemFontOfSize: font_size];

        let color_cls = AnyClass::get(c"NSColor").expect("NSColor");
        let white: Retained<AnyObject> = msg_send![color_cls, whiteColor];

        let dict_cls = AnyClass::get(c"NSMutableDictionary").expect("NSMutableDictionary");
        let dict: Retained<AnyObject> = msg_send![dict_cls, new];

        let k = objc2_foundation::NSString::from_str("NSFont");
        let _: () = msg_send![&*dict, setObject: &*font, forKey: &*k];
        let k = objc2_foundation::NSString::from_str("NSColor");
        let _: () = msg_send![&*dict, setObject: &*white, forKey: &*k];
        let k = objc2_foundation::NSString::from_str("NSShadow");
        let _: () = msg_send![&*dict, setObject: shadow, forKey: &*k];
        let k = objc2_foundation::NSString::from_str("NSParagraphStyle");
        let _: () = msg_send![&*dict, setObject: para, forKey: &*k];

        let ns_text = objc2_foundation::NSString::from_str(text);
        let raw: *mut AnyObject = msg_send![
            AnyClass::get(c"NSAttributedString").expect("NSAttributedString"),
            alloc
        ];
        let raw: *mut AnyObject = msg_send![raw, initWithString: &*ns_text, attributes: &*dict];
        Retained::from_raw(raw).expect("NSAttributedString initWithString:attributes: returned nil")
    }
}

/// Show or hide subtitle text (bottom-center). Main thread only.
pub fn show_subtitle(text: Option<&str>) {
    OSD.with(|osd| {
        let mut osd = osd.borrow_mut();
        let Some(ref mut inner) = *osd else { return };

        disable_animations();
        match text {
            Some(t) => {
                let bounds = inner.parent.bounds();
                let h = bounds.size.height;
                let w = bounds.size.width;

                let font_size = (h * 22.0 / 720.0).max(10.0);
                let margin_y = h * 22.0 / 720.0;
                let margin_x = w * 0.05;
                let layer_h = font_size * 4.0;

                let frame = CGRect::new(
                    CGPoint::new(margin_x, margin_y),
                    CGSize::new(w - margin_x * 2.0, layer_h),
                );
                inner.subtitle.set_frame(frame);

                let attr =
                    build_sub_string(t, font_size, inner.cached_sub_shadow, inner.cached_sub_para);
                inner.subtitle.set_attributed_string(&attr);
                inner.subtitle.set_opacity(1.0);
            }
            None => {
                inner.subtitle.set_opacity(0.0);
            }
        }
        commit_animations();
    });
}

/// Called on main thread to expire OSD messages, auto-hide bar, and
/// update the progress bar position.
pub fn tick(progress: (i64, i64)) {
    OSD.with(|osd| {
        let mut osd = osd.borrow_mut();
        let Some(ref mut inner) = *osd else { return };

        let now = now_ms();

        if !inner.cursor_hidden && now >= inner.cursor_hide_deadline_ms {
            hide_cursor();
            inner.cursor_hidden = true;
        }

        if !inner.message_visible && !inner.bar_visible {
            return;
        }

        if inner.message_visible && now >= inner.message_deadline_ms {
            disable_animations();
            inner.message.set_opacity(0.0);
            commit_animations();
            inner.message_visible = false;
        }

        if inner.bar_visible && now >= inner.bar_hide_deadline_ms {
            disable_animations();
            inner.bar.set_visible(false);
            inner.title.set_opacity(0.0);
            commit_animations();
            inner.bar_visible = false;
        }

        let (current_us, duration_us) = progress;
        if inner.bar_visible && now >= inner.seek_hold_until_ms {
            inner.current_us = current_us;
            inner.duration_us = duration_us;
            render_bar(inner);
        }
    });
}

/// Seek via the progress bar: snap position, show bar, hold against timer updates.
pub fn seek_bar(target_us: i64, duration_us: i64) {
    OSD.with(|osd| {
        let mut osd = osd.borrow_mut();
        let Some(ref mut inner) = *osd else { return };
        inner.current_us = target_us;
        inner.duration_us = duration_us;
        inner.seek_hold_until_ms = now_ms() + 500;
        set_bar_visible(inner);
    });
}

fn hide_cursor() {
    let cls = AnyClass::get(c"NSCursor").expect("NSCursor");
    let _: () = unsafe { msg_send![cls, hide] };
}

fn unhide_cursor() {
    let cls = AnyClass::get(c"NSCursor").expect("NSCursor");
    let _: () = unsafe { msg_send![cls, unhide] };
}

/// Reset the cursor-hide timer and unhide if hidden. Called on mouse movement.
pub fn show_cursor() {
    OSD.with(|osd| {
        let mut osd = osd.borrow_mut();
        let Some(ref mut inner) = *osd else { return };
        if inner.cursor_hidden {
            unhide_cursor();
            inner.cursor_hidden = false;
        }
        inner.cursor_hide_deadline_ms = now_ms() + 2000;
    });
}

/// Show the progress bar and reset the auto-hide timer. Main thread only.
pub fn show_bar() {
    OSD.with(|osd| {
        let mut osd = osd.borrow_mut();
        let Some(ref mut inner) = *osd else { return };
        set_bar_visible(inner);
    });
}

fn set_bar_visible(inner: &mut OsdInner) {
    if !inner.bar_visible {
        disable_animations();
        inner.bar.set_visible(true);
        inner.title.set_opacity(0.7);
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
    OSD.with(|osd| {
        let osd = osd.borrow();
        let inner = osd.as_ref()?;

        let track_frame = inner.bar.track_frame();
        let start = track_frame.origin.x;
        let end = start + track_frame.size.width;
        if track_frame.size.width <= 0.0 {
            return None;
        }

        Some(((x - start) / (end - start)).clamp(0.0, 1.0))
    })
}

fn render_bar(inner: &mut OsdInner) {
    let fraction = if inner.duration_us > 0 {
        (inner.current_us as f64 / inner.duration_us as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let track_frame = inner.bar.track_frame();
    let fill_w = fraction * track_frame.size.width;

    disable_animations();

    let left_secs = inner.current_us.unsigned_abs() / 1_000_000;
    let right_secs = inner.duration_us.unsigned_abs() / 1_000_000;
    if left_secs != inner.last_left_secs || right_secs != inner.last_right_secs {
        inner.last_left_secs = left_secs;
        inner.last_right_secs = right_secs;
        inner
            .bar
            .set_left_time(&crate::time::format_time(inner.current_us));
        inner
            .bar
            .set_right_time(&crate::time::format_time(inner.duration_us));
    }

    inner.bar.set_fill_width(track_frame.origin, fill_w);

    commit_animations();
}
