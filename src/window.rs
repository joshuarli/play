use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use crossbeam_channel::Sender;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject};
use objc2::{MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSEvent, NSEventModifierFlags, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{MainThreadMarker, NSNotification, NSObject, NSObjectProtocol};

use crate::cmd::{Command, EndReason, UiUpdate, VideoFrame};
use crate::input::map_key;

// ── Global state ───────────────────────────────────────────────────────────

/// All main-thread state consolidated into a single struct.
/// Replaces the three separate thread-locals (FILE_STATE, WINDOW, END_REASON).
struct AppState {
    // Per-file state (replaced between playlist entries)
    cmd_tx: Sender<Command>,
    video_frame_rx: crossbeam_channel::Receiver<VideoFrame>,
    ui_update_rx: crossbeam_channel::Receiver<UiUpdate>,
    audio_clock: Arc<AtomicI64>,
    duration_us: i64,
    file_index: usize,
    file_count: usize,

    // Per-app state
    window: Option<Retained<NSWindow>>,
    end_reason: Option<EndReason>,
}

// All state is main-thread-only (timer, key handler, mouse handler, run_app
// all execute on the main GCD queue). RefCell enforces this at the type level
// and avoids Mutex lock/unlock overhead on every timer tick.
std::thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}
static INITIAL_SIZE: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();
static START_FULLSCREEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Replace per-file state, preserving the window reference.
fn set_file_state(
    cmd_tx: Sender<Command>,
    video_frame_rx: crossbeam_channel::Receiver<VideoFrame>,
    ui_update_rx: crossbeam_channel::Receiver<UiUpdate>,
    audio_clock: Arc<AtomicI64>,
    duration_us: i64,
    file_index: usize,
    file_count: usize,
) {
    APP_STATE.with(|s| {
        let mut s = s.borrow_mut();
        let window = s.as_mut().and_then(|state| state.window.take());
        *s = Some(AppState {
            cmd_tx,
            video_frame_rx,
            ui_update_rx,
            audio_clock,
            duration_us,
            file_index,
            file_count,
            window,
            end_reason: None,
        });
    });
}

/// Send a command to the player thread.
fn send_cmd(cmd: Command) {
    APP_STATE.with(|s| {
        let s = s.borrow();
        let Some(ref state) = *s else { return };
        // Ignore next/prev when already at playlist boundary
        if matches!(cmd, Command::NextFile) && state.file_index + 1 >= state.file_count {
            return;
        }
        if matches!(cmd, Command::PrevFile) && state.file_index == 0 {
            return;
        }
        let _ = state.cmd_tx.send(cmd);
    });
}

/// Stop the NSApp run loop so run_app() can return.
fn stop_app() {
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: stop: just sets a flag; the dummy event wakes the run loop
        // so run() returns immediately.
        app.stop(None);
        // Post a dummy event to ensure the run loop wakes up and exits
        post_dummy_event(mtm);
    }
}

fn post_dummy_event(mtm: MainThreadMarker) {
    use objc2_app_kit::NSEventType;
    let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(
        NSEventType::ApplicationDefined,
        CGPoint::new(0.0, 0.0),
        NSEventModifierFlags::empty(),
        0.0,
        0,
        None,
        0,
        0,
        0,
    );
    if let Some(event) = event {
        NSApplication::sharedApplication(mtm).postEvent_atStart(&event, true);
    }
}

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
            // SAFETY: NSScreen class and mainScreen/visibleFrame are standard
            // AppKit APIs available on all macOS versions we target.
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

            // SAFETY: Standard NSWindow initialization with valid parameters.
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    rect,
                    style,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            // SAFETY: We manage the window lifetime ourselves via Retained.
            unsafe { window.setReleasedWhenClosed(false) };
            window.setTitle(&objc2_foundation::NSString::from_str("play"));
            window.center();
            window.setDelegate(Some(ProtocolObject::from_ref(self)));

            if let Some(view) = window.contentView() {
                view.setWantsLayer(true);

                crate::video_out::init_display(vw, vh);

                if let Some(layer) = view.layer() {
                    // SAFETY: All msg_send! calls target standard CALayer
                    // properties/methods. display_layer is a valid
                    // AVSampleBufferDisplayLayer from video_out::init_display_layer.
                    let black = crate::osd::create_cgcolor(0.0, 0.0, 0.0, 1.0);
                    let _: () = unsafe { msg_send![&*layer, setBackgroundColor: black] };
                    crate::osd::release_cgcolor(black);

                    // Add display layer
                    if let Some(display_ptr) = crate::video_out::display_layer_ptr() {
                        let display_layer = display_ptr as *mut AnyObject;
                        let _: () = unsafe { msg_send![&*layer, addSublayer: display_layer] };
                        let bounds: CGRect = unsafe { msg_send![&*layer, bounds] };
                        let _: () = unsafe { msg_send![display_layer, setFrame: bounds] };
                        let mask: u32 = 18; // kCALayerWidthSizable | kCALayerHeightSizable
                        let _: () = unsafe { msg_send![display_layer, setAutoresizingMask: mask] };
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

            APP_STATE.with(|s| {
                if let Some(ref mut state) = *s.borrow_mut() {
                    state.window = Some(window);
                }
            });

            install_key_monitor();
            install_mouse_monitor();
            start_main_timer();
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _: &NSNotification) {
            APP_STATE.with(|s| {
                if let Some(ref mut state) = *s.borrow_mut() {
                    state.end_reason = Some(EndReason::Quit);
                }
            });
            send_cmd(Command::Quit);
            stop_app();
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        // SAFETY: Standard NSObject alloc/init pattern.
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
        let chars = event
            .charactersIgnoringModifiers()
            .map(|s| s.to_string())
            .unwrap_or_default();

        if let Some(cmd) = map_key(key_code, shift, &chars) {
            // Handle fullscreen directly on main thread
            if matches!(cmd, Command::ToggleFullscreen) {
                APP_STATE.with(|s| {
                    if let Some(ref state) = *s.borrow()
                        && let Some(ref win) = state.window
                    {
                        win.toggleFullScreen(None);
                    }
                });
                return std::ptr::null_mut();
            }
            let is_quit = matches!(cmd, Command::Quit);
            let is_seek = matches!(cmd, Command::SeekRelative { .. });
            send_cmd(cmd);
            if is_seek {
                crate::osd::show_bar();
            }
            if is_quit {
                APP_STATE.with(|s| {
                    if let Some(ref mut state) = *s.borrow_mut() {
                        state.end_reason = Some(EndReason::Quit);
                    }
                });
                stop_app();
            }
            return std::ptr::null_mut();
        }
        event_ptr.as_ptr()
    });

    // SAFETY: addLocalMonitorForEventsMatchingMask:handler: installs a block
    // that intercepts key events before they reach the responder chain. The
    // monitor is leaked (never removed) — it lives for the process lifetime.
    unsafe {
        let _monitor = NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler);
    }
}

// ── Mouse event handling ───────────────────────────────────────────────────

fn install_mouse_monitor() {
    use block2::RcBlock;
    use objc2_app_kit::NSEventMask;
    use std::ptr::NonNull;

    let mask = NSEventMask::MouseMoved | NSEventMask::LeftMouseDown | NSEventMask::LeftMouseDragged;

    let handler = RcBlock::new(|event_ptr: NonNull<NSEvent>| -> *mut NSEvent {
        let event = unsafe { event_ptr.as_ref() };
        let event_type = event.r#type();

        use objc2_app_kit::NSEventType;
        match event_type {
            NSEventType::MouseMoved => {
                crate::osd::show_bar();
            }
            NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged => {
                let location = event.locationInWindow();
                if location.y <= crate::osd::bar_height()
                    && let Some(fraction) = crate::osd::bar_fraction_at_x(location.x)
                {
                    let duration = APP_STATE
                        .with(|s| s.borrow().as_ref().map(|s| s.duration_us))
                        .unwrap_or(0);
                    let target_us = (fraction * duration as f64) as i64;
                    send_cmd(Command::SeekAbsolute { target_us });
                    crate::osd::seek_bar(target_us, duration);
                }
            }
            _ => {}
        }

        event_ptr.as_ptr()
    });

    // SAFETY: Same as key monitor — installs a block for mouse events.
    unsafe {
        let _monitor = NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler);
    }
}

// ── Timer for video/UI updates ─────────────────────────────────────────────

fn start_main_timer() {
    use dispatch2::{DispatchObject, DispatchQueue, DispatchSource, DispatchTime};

    let queue = DispatchQueue::main();
    let timer_type: dispatch2::dispatch_source_type_t =
        &raw const dispatch2::_dispatch_source_type_timer as *mut _;
    // SAFETY: DispatchSource::new creates a GCD timer source on the main queue.
    let source = unsafe { DispatchSource::new(timer_type, 0, 0, Some(queue)) };

    // 16ms interval (~60Hz), 2ms leeway — AVSampleBufferDisplayLayer's
    // CMTimebase handles frame presentation timing, so we only need to
    // feed frames fast enough to keep its queue non-empty.
    let interval_ns: u64 = 16_000_000;
    let leeway_ns: u64 = 2_000_000;
    source.set_timer(DispatchTime::NOW, interval_ns, leeway_ns);

    // Counter for periodic drift correction (~1s)
    let drift_counter = std::cell::Cell::new(0u32);
    let handler = block2::RcBlock::new(move || {
        // Borrow state immutably for processing, collecting any EOF signal.
        let (eof, progress) = APP_STATE.with(|s| {
            let s = s.borrow();
            let Some(ref state) = *s else {
                return (None, (0i64, 0i64));
            };

            let eof = process_pending_ui_updates(state);
            process_pending_frames(state);

            let c = drift_counter.get() + 1;
            if c >= 62 {
                drift_counter.set(0);
                crate::video_out::sync_timebase(state.audio_clock.load(Ordering::Relaxed));
            } else {
                drift_counter.set(c);
            }

            let current = state.audio_clock.load(Ordering::Relaxed);
            (eof, (current, state.duration_us))
        });

        // Apply EOF (needs mutable borrow, separate from above)
        if let Some(reason) = eof {
            APP_STATE.with(|s| {
                if let Some(ref mut state) = *s.borrow_mut() {
                    state.end_reason = Some(reason);
                }
            });
            stop_app();
        }

        crate::osd::tick(progress);
    });

    // SAFETY: set_event_handler_with_block sets the timer's handler block.
    // The block and source are leaked (std::mem::forget) to keep the timer
    // alive for the process lifetime.
    unsafe {
        source.set_event_handler_with_block(
            &*handler as *const block2::DynBlock<dyn Fn()> as *mut block2::DynBlock<dyn Fn()>,
        );
    }
    source.resume();
    std::mem::forget(source);
    std::mem::forget(handler);
}

fn process_pending_frames(state: &AppState) {
    for _ in 0..8 {
        match state.video_frame_rx.try_recv() {
            Ok(frame) => {
                crate::video_out::enqueue_frame(frame);
            }
            Err(_) => break,
        }
    }
}

/// Process UI updates. Returns Some(EndReason) if EOF was received (caller
/// must set end_reason after releasing the borrow).
fn process_pending_ui_updates(state: &AppState) -> Option<EndReason> {
    let mut eof = None;
    while let Ok(update) = state.ui_update_rx.try_recv() {
        match update {
            UiUpdate::Osd(text) => crate::osd::show_message(&text),
            UiUpdate::SubtitleText(text) => crate::osd::show_subtitle(text.as_deref()),
            UiUpdate::VideoSize { width, height } => {
                log::debug!("Video size: {width}x{height}");
            }
            UiUpdate::Paused(paused) => {
                if !paused {
                    crate::video_out::sync_timebase(state.audio_clock.load(Ordering::Relaxed));
                }
                crate::video_out::set_playback_rate(if paused { 0.0 } else { 1.0 });
            }
            UiUpdate::SeekFlush(pts_us) => {
                crate::video_out::flush_and_seek(pts_us);
            }
            UiUpdate::EndOfFile(reason) => {
                let _ = state.cmd_tx.send(Command::Quit);
                eof = Some(reason);
            }
        }
    }
    eof
}

// ── Public API ─────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub fn run_app(
    cmd_tx: Sender<Command>,
    video_frame_rx: crossbeam_channel::Receiver<VideoFrame>,
    ui_update_rx: crossbeam_channel::Receiver<UiUpdate>,
    video_width: u32,
    video_height: u32,
    fullscreen: bool,
    audio_clock: Arc<AtomicI64>,
    duration_us: i64,
    title: &str,
    first_run: bool,
    file_index: usize,
    file_count: usize,
) -> EndReason {
    set_file_state(
        cmd_tx,
        video_frame_rx,
        ui_update_rx,
        audio_clock,
        duration_us,
        file_index,
        file_count,
    );

    let mtm = MainThreadMarker::new().expect("must run on main thread");
    let app = NSApplication::sharedApplication(mtm);

    if first_run {
        INITIAL_SIZE.set((video_width, video_height)).ok();
        START_FULLSCREEN.set(fullscreen).ok();

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
    } else {
        // Subsequent files: reset display for new resolution, update title
        crate::video_out::reset_for_new_file();
        APP_STATE.with(|s| {
            if let Some(ref state) = *s.borrow()
                && let Some(ref win) = state.window
            {
                win.setTitle(&objc2_foundation::NSString::from_str(title));
            }
        });
    }

    app.run();

    APP_STATE
        .with(|s| {
            s.borrow_mut()
                .as_mut()
                .and_then(|state| state.end_reason.take())
        })
        .unwrap_or(EndReason::Quit)
}
