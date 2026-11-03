#import <Cocoa/Cocoa.h>
#import <QuartzCore/CAMetalLayer.h>

// Narrow v1 bridge between Rust player-display and AppKit. The window
// owns a layer-hosting view whose CAMetalLayer backs the Vulkan
// surface; all rendering stays on the Rust side. Event input beyond
// close detection belongs to a later slice.

@interface PlayerMacCloseWatcher : NSObject <NSWindowDelegate>
@property(nonatomic, assign) BOOL closed;
@end

@implementation PlayerMacCloseWatcher
- (void)windowWillClose:(NSNotification *)notification {
    (void)notification;
    self.closed = YES;
}
@end

struct PlayerMacWindow {
    NSWindow *window;
    NSView *view;
    CAMetalLayer *layer;
    PlayerMacCloseWatcher *watcher;
};

void *player_mac_window_open(const char *title, int width, int height) {
    NSApplication *app = [NSApplication sharedApplication];
    [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    [app activateIgnoringOtherApps:YES];

    NSRect frame = NSMakeRect(0, 0, width > 0 ? width : 320, height > 0 ? height : 240);
    NSWindow *window = [[NSWindow alloc]
        initWithContentRect:frame
                  styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable |
                            NSWindowStyleMaskResizable | NSWindowStyleMaskMiniaturizable
            backing:NSBackingStoreBuffered
                      defer:NO];
    if (!window) {
        return NULL;
    }
    [window setTitle:[NSString stringWithUTF8String:title ? title : "player"]];
    [window center];

    NSView *view = [[NSView alloc] initWithFrame:frame];
    if (!view) {
        return NULL;
    }
    CAMetalLayer *layer = [CAMetalLayer layer];
    if (!layer) {
        return NULL;
    }
    layer.contentsScale = window.backingScaleFactor;
    view.layer = layer;
    view.wantsLayer = YES;
    view.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    window.contentView = view;

    PlayerMacCloseWatcher *watcher = [[PlayerMacCloseWatcher alloc] init];
    window.delegate = watcher;

    [window makeKeyAndOrderFront:nil];

    struct PlayerMacWindow *handle =
        (struct PlayerMacWindow *)calloc(1, sizeof(struct PlayerMacWindow));
    if (!handle) {
        return NULL;
    }
    handle->window = window;
    handle->view = view;
    handle->layer = layer;
    handle->watcher = watcher;
    return handle;
}

void *player_mac_window_layer(void *handle) {
    CAMetalLayer *layer = ((struct PlayerMacWindow *)handle)->layer;
    return (__bridge void *)layer;
}

int player_mac_window_poll(void *handle) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    NSApplication *app = [NSApplication sharedApplication];
    NSEvent *event;
    while ((event = [app nextEventMatchingMask:NSEventMaskAny
                                     untilDate:[NSDate distantPast]
                                        inMode:NSDefaultRunLoopMode
                                       dequeue:YES])) {
        [app sendEvent:event];
    }
    [app updateWindows];
    return win->watcher.closed ? 1 : 0;
}

void player_mac_window_size(void *handle, int *width, int *height) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    NSSize size = win->view.frame.size;
    CGFloat scale = win->window.backingScaleFactor;
    if (width) {
        *width = (int)(size.width * scale);
    }
    if (height) {
        *height = (int)(size.height * scale);
    }
}

int player_mac_window_fullscreen(void *handle) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    return (win->window.styleMask & NSWindowStyleMaskFullScreen) != 0 ? 1 : 0;
}

void player_mac_window_set_fullscreen(void *handle, int fullscreen) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    BOOL is_full = (win->window.styleMask & NSWindowStyleMaskFullScreen) != 0;
    if ((fullscreen != 0) != is_full) {
        [win->window toggleFullScreen:nil];
    }
}

void player_mac_window_close(void *handle) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    win->window.delegate = nil;
    [win->window close];
    free(win);
}
