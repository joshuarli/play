use std::ffi::c_void;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

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

/// Per-file state that gets replaced between playlist entries.
struct FileState {
    cmd_tx: Sender<Command>,
    video_frame_rx: crossbeam_channel::Receiver<VideoFrame>,
    ui_update_rx: crossbeam_channel::Receiver<UiUpdate>,
    audio_clock: Arc<AtomicI64>,
    duration_us: i64,
    file_index: usize,
    file_count: usize,
}

static FILE_STATE: Mutex<Option<FileState>> = Mutex::new(None);

// Window is only accessed from main thread; use a thread-local instead of Mutex
use std::cell::RefCell;
std::thread_local! {
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
}
static INITIAL_SIZE: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();
static START_FULLSCREEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Stored EndReason from EndOfFile or Quit, read after app.run() returns.
static END_REASON: Mutex<Option<EndReason>> = Mutex::new(None);

/// Replace all per-file state atomically (single lock).
fn set_file_state(state: FileState) {
    *FILE_STATE.lock().unwrap() = Some(state);
    *END_REASON.lock().unwrap() = None;
}

/// Send a command to the player thread.
fn send_cmd(cmd: Command) {
    if let Some(ref state) = *FILE_STATE.lock().unwrap() {
        // Ignore next/prev when already at playlist boundary
        if matches!(cmd, Command::NextFile) && state.file_index + 1 >= state.file_count {
            return;
        }
        if matches!(cmd, Command::PrevFile) && state.file_index == 0 {
            return;
        }
        let _ = state.cmd_tx.send(cmd);
    }
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

            WINDOW.with(|w| *w.borrow_mut() = Some(window));

            install_key_monitor();
            install_mouse_monitor();
            start_main_timer();
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _: &NSNotification) {
            *END_REASON.lock().unwrap() = Some(EndReason::Quit);
            send_cmd(Command::Quit);
            stop_app();
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
        let chars = event
            .charactersIgnoringModifiers()
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
            let is_seek = matches!(cmd, Command::SeekRelative { .. });
            send_cmd(cmd);
            if is_seek {
                crate::osd::show_bar();
            }
            if is_quit {
                *END_REASON.lock().unwrap() = Some(EndReason::Quit);
                stop_app();
            }
            return std::ptr::null_mut();
        }
        event_ptr.as_ptr()
    });

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
                    let duration = FILE_STATE
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|s| s.duration_us)
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
    let source = unsafe { DispatchSource::new(timer_type, 0, 0, Some(queue)) };

    // 4ms interval (~240Hz), 500μs leeway
    let interval_ns: u64 = 4_000_000;
    let leeway_ns: u64 = 500_000;
    source.set_timer(DispatchTime::NOW, interval_ns, leeway_ns);

    // Counters for periodic work
    let drift_counter = std::cell::Cell::new(0u32);
    let progress_counter = std::cell::Cell::new(0u32);
    let handler = block2::RcBlock::new(move || {
        let guard = FILE_STATE.lock().unwrap();
        let Some(ref state) = *guard else { return };

        process_pending_ui_updates(state);
        process_pending_frames(state);

        let c = drift_counter.get() + 1;
        if c >= 250 {
            drift_counter.set(0);
            crate::video_out::sync_timebase(state.audio_clock.load(Ordering::Relaxed));
        } else {
            drift_counter.set(c);
        }

        let progress = {
            let p = progress_counter.get() + 1;
            if p >= 60 {
                progress_counter.set(0);
                let current = state.audio_clock.load(Ordering::Relaxed);
                Some((current, state.duration_us))
            } else {
                progress_counter.set(p);
                None
            }
        };

        drop(guard);
        crate::osd::tick(progress);
    });

    unsafe {
        source.set_event_handler_with_block(
            &*handler as *const block2::DynBlock<dyn Fn()> as *mut block2::DynBlock<dyn Fn()>,
        );
    }
    source.resume();
    std::mem::forget(source);
    std::mem::forget(handler);
}

fn process_pending_frames(state: &FileState) {
    for _ in 0..4 {
        match state.video_frame_rx.try_recv() {
            Ok(frame) => {
                let flush = frame.seek_flush;
                crate::video_out::enqueue_frame(frame);
                // After a seek-flush frame, yield so the compositor can
                // present it before the next flush replaces it.
                if flush {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}

fn process_pending_ui_updates(state: &FileState) {
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
                *END_REASON.lock().unwrap() = Some(reason);
                let _ = state.cmd_tx.send(Command::Quit);
                stop_app();
            }
        }
    }
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
    set_file_state(FileState {
        cmd_tx,
        video_frame_rx,
        ui_update_rx,
        audio_clock,
        duration_us,
        file_index,
        file_count,
    });

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
        WINDOW.with(|w| {
            if let Some(ref win) = *w.borrow() {
                win.setTitle(&objc2_foundation::NSString::from_str(title));
            }
        });
    }

    app.run();

    END_REASON.lock().unwrap().take().unwrap_or(EndReason::Quit)
}
