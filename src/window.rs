use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};

use crossbeam_channel::Sender;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSEvent, NSEventModifierFlags, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSNotification, NSObject, NSObjectProtocol};

use crate::cmd::{Command, UiUpdate, VideoFrame};
use crate::input::map_key;

// ── Global state ───────────────────────────────────────────────────────────

static CMD_TX: OnceLock<Sender<Command>> = OnceLock::new();
static VIDEO_FRAME_RX: OnceLock<crossbeam_channel::Receiver<VideoFrame>> = OnceLock::new();
static UI_UPDATE_RX: OnceLock<crossbeam_channel::Receiver<UiUpdate>> = OnceLock::new();
// Window is only accessed from main thread; use a thread-local instead of OnceLock
use std::cell::RefCell;
std::thread_local! {
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}
static INITIAL_SIZE: OnceLock<(u32, u32)> = OnceLock::new();
static START_FULLSCREEN: OnceLock<bool> = OnceLock::new();
static AUDIO_CLOCK: OnceLock<Arc<AtomicI64>> = OnceLock::new();

// ── AppDelegate ────────────────────────────────────────────────────────────

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "PlayAppDelegate"]
    #[derive(Debug)]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, notification: &NSNotification) {
            let mtm = self.mtm();
            let app = notification
                .object()
                .unwrap()
                .downcast::<NSApplication>()
                .unwrap();

            let (vw, vh) = INITIAL_SIZE.get().copied().unwrap_or((960, 540));

            // Cap window to 80% of screen
            let screen_cls = AnyClass::get(c"NSScreen").unwrap();
            let screen: Retained<AnyObject> = unsafe { msg_send![screen_cls, mainScreen] };
            let sf: CGRect = unsafe { msg_send![&*screen, visibleFrame] };
            let max_w = sf.size.width * 0.8;
            let max_h = sf.size.height * 0.8;
            let scale = (max_w / vw as f64).min(max_h / vh as f64).min(1.0);
            let w = (vw as f64 * scale).round();
            let h = (vh as f64 * scale).round();

            let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w, h));
            let style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable;

            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    rect,
                    style,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            unsafe { window.setReleasedWhenClosed(false) };
            window.setTitle(&objc2_foundation::NSString::from_str("play"));
            window.center();
            window.setDelegate(Some(ProtocolObject::from_ref(self)));

            if let Some(view) = window.contentView() {
                view.setWantsLayer(true);

                crate::video_out::init_display_layer(vw, vh);

                if let Some(layer) = view.layer() {
                    // Black background
                    let black = crate::osd::create_cgcolor(0.0, 0.0, 0.0, 1.0);
                    let _: () = unsafe { msg_send![&*layer, setBackgroundColor: black] };
                    crate::osd::release_cgcolor(black);

                    // Add display layer
                    if let Some(display_ptr) = crate::video_out::display_layer_ptr() {
                        let display_layer = display_ptr as *mut AnyObject;
                        let _: () =
                            unsafe { msg_send![&*layer, addSublayer: display_layer] };
                        let bounds: CGRect = unsafe { msg_send![&*layer, bounds] };
                        let _: () =
                            unsafe { msg_send![display_layer, setFrame: bounds] };
                        let mask: u32 = 18; // kCALayerWidthSizable | kCALayerHeightSizable
                        let _: () = unsafe {
                            msg_send![display_layer, setAutoresizingMask: mask]
                        };
                    }

                    // Init OSD + subtitle layers
                    let bounds: CGRect = unsafe { msg_send![&*layer, bounds] };
                    let layer_ptr = &*layer as *const _ as *mut c_void;
                    crate::osd::init_layers(layer_ptr, bounds);
                }
            }

            // Become a regular app and grab focus before showing the window
            app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);

            window.makeKeyAndOrderFront(None);

            if START_FULLSCREEN.get().copied().unwrap_or(false) {
                window.toggleFullScreen(None);
            }

            WINDOW.with(|w| *w.borrow_mut() = Some(window));

            install_key_monitor();
            start_main_timer();
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _: &NSNotification) {
            if let Some(tx) = CMD_TX.get() {
                let _ = tx.send(Command::Quit);
            }
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        unsafe { msg_send![Self::alloc(mtm), init] }
    }
}

// ── Key event handling ─────────────────────────────────────────────────────

fn install_key_monitor() {
    use block2::RcBlock;
    use objc2_app_kit::NSEventMask;
    use std::ptr::NonNull;

    let mask = NSEventMask::KeyDown;

    let handler = RcBlock::new(|event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { event_ptr.as_ref() };
        let key_code = event.keyCode();
        let mods = event.modifierFlags();
        let shift = mods.contains(NSEventModifierFlags::Shift);
        let chars = event.charactersIgnoringModifiers()
            .map(|s| s.to_string())
            .unwrap_or_default();

        if let Some(cmd) = map_key(key_code, shift, &chars) {
            // Handle fullscreen directly on main thread
            if matches!(cmd, Command::ToggleFullscreen) {
                WINDOW.with(|w| {
                    if let Some(ref win) = *w.borrow() {
                        win.toggleFullScreen(None);
                    }
                });
                return std::ptr::null_mut();
            }
            let is_quit = matches!(cmd, Command::Quit);
            if let Some(tx) = CMD_TX.get() {
                let _ = tx.send(cmd);
            }
            if is_quit {
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).terminate(None);
                }
            }
            return std::ptr::null_mut();
        }
        event_ptr.as_ptr()
    });

    unsafe {
        let _monitor = NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &*handler);
    }
}

// ── Timer for video/UI updates ─────────────────────────────────────────────

fn start_main_timer() {
    use dispatch2::{DispatchObject, DispatchQueue, DispatchSource, DispatchTime};

    let queue = DispatchQueue::main();
    let timer_type: dispatch2::dispatch_source_type_t =
        &raw const dispatch2::_dispatch_source_type_timer as *mut _;
    let source = unsafe { DispatchSource::new(timer_type, 0, 0, Some(queue)) };

    // 8ms interval (~120Hz), 1ms leeway
    let interval_ns: u64 = 8_000_000;
    let leeway_ns: u64 = 1_000_000;
    source.set_timer(DispatchTime::NOW, interval_ns, leeway_ns);

    // Drift correction counter: sync timebase to audio clock ~once per second
    let drift_counter = std::cell::Cell::new(0u32);
    let handler = block2::RcBlock::new(move || {
        process_pending_ui_updates();
        process_pending_frames();
        crate::osd::tick();

        // Sync timebase to audio ~once per second (every 125 ticks at 8ms)
        let c = drift_counter.get() + 1;
        if c >= 125 {
            drift_counter.set(0);
            if let Some(clock) = AUDIO_CLOCK.get() {
                crate::video_out::sync_timebase(clock.load(Ordering::Relaxed));
            }
        } else {
            drift_counter.set(c);
        }
    });

    unsafe {
        source.set_event_handler_with_block(
            &*handler as *const block2::DynBlock<dyn Fn()>
                as *mut block2::DynBlock<dyn Fn()>,
        );
    }
    source.resume();
    std::mem::forget(source);
    std::mem::forget(handler);
}

fn process_pending_frames() {
    let Some(rx) = VIDEO_FRAME_RX.get() else {
        return;
    };
    for _ in 0..4 {
        match rx.try_recv() {
            Ok(frame) => crate::video_out::enqueue_frame(frame),
            Err(_) => break,
        }
    }
}

fn process_pending_ui_updates() {
    let Some(rx) = UI_UPDATE_RX.get() else {
        return;
    };
    while let Ok(update) = rx.try_recv() {
        match update {
            UiUpdate::Osd(text) => crate::osd::show_message(&text),
            UiUpdate::SubtitleText(text) => crate::osd::show_subtitle(text.as_deref()),
            UiUpdate::VideoSize { width, height } => {
                log::debug!("Video size: {width}x{height}");
            }
            UiUpdate::Paused(paused) => {
                if !paused {
                    // Sync timebase to audio clock before resuming so they agree
                    if let Some(clock) = AUDIO_CLOCK.get() {
                        crate::video_out::sync_timebase(clock.load(Ordering::Relaxed));
                    }
                }
                crate::video_out::set_playback_rate(if paused { 0.0 } else { 1.0 });
            }
            UiUpdate::SeekFlush(pts_us) => {
                crate::video_out::flush_and_seek(pts_us);
            }
            UiUpdate::EndOfFile => {
                if let Some(tx) = CMD_TX.get() {
                    let _ = tx.send(Command::Quit);
                }
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).terminate(None);
                }
            }
        }
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

pub fn run_app(
    cmd_tx: Sender<Command>,
    video_frame_rx: crossbeam_channel::Receiver<VideoFrame>,
    ui_update_rx: crossbeam_channel::Receiver<UiUpdate>,
    video_width: u32,
    video_height: u32,
    fullscreen: bool,
    audio_clock: Arc<AtomicI64>,
) {
    CMD_TX.set(cmd_tx).unwrap();
    VIDEO_FRAME_RX.set(video_frame_rx).unwrap();
    UI_UPDATE_RX.set(ui_update_rx).unwrap();
    INITIAL_SIZE.set((video_width, video_height)).unwrap();
    START_FULLSCREEN.set(fullscreen).ok();
    AUDIO_CLOCK.set(audio_clock).ok();

    let mtm = MainThreadMarker::new().expect("must run on main thread");
    let app = NSApplication::sharedApplication(mtm);

    // Minimal menu
    {
        use objc2_app_kit::{NSMenu, NSMenuItem};
        let menu_bar = NSMenu::new(mtm);
        let app_menu_item = NSMenuItem::new(mtm);
        menu_bar.addItem(&app_menu_item);
        app.setMainMenu(Some(&menu_bar));
    }

    let delegate = AppDelegate::new(mtm);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

    app.run();
}
