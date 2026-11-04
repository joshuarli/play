#import <Cocoa/Cocoa.h>
#import <QuartzCore/CAMetalLayer.h>

// Narrow v1 bridge between Rust player-display and AppKit. The window
// owns a layer-hosting view whose CAMetalLayer backs the Vulkan
// surface; all rendering stays on the Rust side. Keyboard and pointer
// events queue for the Rust host.

@interface PlayerMacCloseWatcher : NSObject <NSWindowDelegate>
@property(nonatomic, assign) BOOL closed;
@end

@implementation PlayerMacCloseWatcher
- (void)windowWillClose:(NSNotification *)notification {
    (void)notification;
    self.closed = YES;
}
@end

enum {
    PLAYER_MAC_EVENT_KEY = 0,
    PLAYER_MAC_EVENT_MOUSE_MOVE = 1,
    PLAYER_MAC_EVENT_MOUSE_LEAVE = 2,
    PLAYER_MAC_EVENT_OPEN_FILES = 3,
};

#define PLAYER_MAC_PATH_MAX 4096

struct PlayerMacInputEvent {
    int kind;
    int key_code;
    int flags;
    int pressed;
    int repeat;
    int append;
    double x;
    double y;
    char path[PLAYER_MAC_PATH_MAX];
};

#define PLAYER_MAC_INPUT_QUEUE 64

struct PlayerMacWindow {
    NSWindow *window;
    NSView *view;
    CAMetalLayer *layer;
    PlayerMacCloseWatcher *watcher;
    struct PlayerMacInputEvent input[PLAYER_MAC_INPUT_QUEUE];
    int input_head;
    int input_tail;
};

static void player_mac_push_input(struct PlayerMacWindow *win,
                                  struct PlayerMacInputEvent event) {
    int next = (win->input_head + 1) % PLAYER_MAC_INPUT_QUEUE;
    if (next == win->input_tail) {
        return;
    }
    win->input[win->input_head] = event;
    win->input_head = next;
}

static struct PlayerMacWindow *player_mac_main_window;

static void player_mac_enqueue_files(NSArray<NSURL *> *urls, BOOL append) {
    if (!player_mac_main_window) {
        return;
    }
    for (NSUInteger index = 0; index < urls.count; index++) {
        NSURL *url = urls[index];
        if (!url.isFileURL) {
            continue;
        }
        const char *path = url.path.UTF8String;
        if (!path) {
            continue;
        }
        struct PlayerMacInputEvent event = {0};
        event.kind = PLAYER_MAC_EVENT_OPEN_FILES;
        event.append = (append || index > 0) ? 1 : 0;
        strlcpy(event.path, path, PLAYER_MAC_PATH_MAX);
        player_mac_push_input(player_mac_main_window, event);
    }
}

@interface PlayerMacAppDelegate : NSObject <NSApplicationDelegate>
@end

@implementation PlayerMacAppDelegate
- (void)application:(NSApplication *)sender openFiles:(NSArray<NSString *> *)filenames {
    NSMutableArray<NSURL *> *urls = [NSMutableArray arrayWithCapacity:filenames.count];
    for (NSString *name in filenames) {
        [urls addObject:[NSURL fileURLWithPath:name]];
    }
    player_mac_enqueue_files(urls, NO);
    [sender replyToOpenOrPrint:NSApplicationDelegateReplySuccess];
}

- (void)application:(NSApplication *)sender openURLs:(NSArray<NSURL *> *)urls {
    player_mac_enqueue_files(urls, NO);
    [sender replyToOpenOrPrint:NSApplicationDelegateReplySuccess];
}
@end

static PlayerMacAppDelegate *player_mac_delegate;

static void player_mac_install_delegate(void) {
    if (player_mac_delegate) {
        return;
    }
    player_mac_delegate = [[PlayerMacAppDelegate alloc] init];
    [NSApplication sharedApplication].delegate = player_mac_delegate;
}

@interface PlayerMacView : NSView
@property(nonatomic, assign) struct PlayerMacWindow *owner;
@property(nonatomic, strong) NSTrackingArea *playerTrackingArea;
@end

@implementation PlayerMacView
- (BOOL)acceptsFirstResponder {
    return YES;
}

- (BOOL)becomeFirstResponder {
    return YES;
}

- (void)updateTrackingAreas {
    [super updateTrackingAreas];
    if (self.playerTrackingArea) {
        [self removeTrackingArea:self.playerTrackingArea];
    }
    NSTrackingAreaOptions options = NSTrackingMouseMoved | NSTrackingMouseEnteredAndExited |
                                    NSTrackingActiveInKeyWindow | NSTrackingInVisibleRect;
    self.playerTrackingArea =
        [[NSTrackingArea alloc] initWithRect:NSZeroRect
                                     options:options
                                       owner:self
                                    userInfo:nil];
    [self addTrackingArea:self.playerTrackingArea];
}

- (void)keyDown:(NSEvent *)event {
    if (self.owner) {
        player_mac_push_input(self.owner, (struct PlayerMacInputEvent){
            .kind = PLAYER_MAC_EVENT_KEY,
            .key_code = (int)event.keyCode,
            .flags = (int)event.modifierFlags,
            .pressed = 1,
            .repeat = event.isARepeat ? 1 : 0,
        });
    }
}

- (void)keyUp:(NSEvent *)event {
    if (self.owner) {
        player_mac_push_input(self.owner, (struct PlayerMacInputEvent){
            .kind = PLAYER_MAC_EVENT_KEY,
            .key_code = (int)event.keyCode,
            .flags = (int)event.modifierFlags,
            .pressed = 0,
        });
    }
}

- (void)mouseMoved:(NSEvent *)event {
    if (!self.owner) {
        return;
    }
    NSPoint point = [self convertPoint:event.locationInWindow fromView:nil];
    CGFloat scale = self.window.backingScaleFactor;
    player_mac_push_input(self.owner, (struct PlayerMacInputEvent){
        .kind = PLAYER_MAC_EVENT_MOUSE_MOVE,
        .x = point.x * scale,
        .y = (self.bounds.size.height - point.y) * scale,
    });
}

- (void)mouseExited:(NSEvent *)event {
    (void)event;
    if (self.owner) {
        player_mac_push_input(self.owner, (struct PlayerMacInputEvent){
            .kind = PLAYER_MAC_EVENT_MOUSE_LEAVE,
        });
    }
}

- (NSDragOperation)draggingEntered:(id<NSDraggingInfo>)sender {
    (void)sender;
    return NSDragOperationCopy;
}

- (BOOL)performDragOperation:(id<NSDraggingInfo>)sender {
    NSArray<NSURL *> *urls = [sender.draggingPasteboard
        readObjectsForClasses:@[ [NSURL class] ]
                      options:@{NSPasteboardURLReadingFileURLsOnlyKey : @YES}];
    if (urls.count == 0) {
        return NO;
    }
    player_mac_enqueue_files(urls, NO);
    return YES;
}
@end

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

    NSView *view = [[PlayerMacView alloc] initWithFrame:frame];
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
    [view registerForDraggedTypes:@[ NSPasteboardTypeFileURL ]];

    PlayerMacCloseWatcher *watcher = [[PlayerMacCloseWatcher alloc] init];
    window.delegate = watcher;

    [window setAcceptsMouseMovedEvents:YES];
    [window makeKeyAndOrderFront:nil];
    [window makeFirstResponder:view];

    NSMenu *menuBar = [[NSMenu alloc] init];
    NSMenuItem *appMenuItem = [[NSMenuItem alloc] init];
    [menuBar addItem:appMenuItem];
    [NSApp setMainMenu:menuBar];
    NSMenu *appMenu = [[NSMenu alloc] init];
    [appMenu addItemWithTitle:@"Quit player"
                       action:@selector(performClose:)
                keyEquivalent:@"q"];
    appMenuItem.submenu = appMenu;

    struct PlayerMacWindow *handle =
        (struct PlayerMacWindow *)calloc(1, sizeof(struct PlayerMacWindow));
    if (!handle) {
        return NULL;
    }
    handle->window = window;
    handle->view = view;
    handle->layer = layer;
    handle->watcher = watcher;
    ((PlayerMacView *)view).owner = handle;
    player_mac_main_window = handle;
    player_mac_install_delegate();
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

void player_mac_window_resize(void *handle, int width, int height) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    CGFloat scale = win->window.backingScaleFactor;
    if (scale <= 0) {
        scale = 1;
    }
    [win->window setContentSize:NSMakeSize(width / scale, height / scale)];
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

int player_mac_window_next_input(void *handle, int *kind, int *key_code, int *flags,
                                 int *pressed, int *repeat, int *append, double *x, double *y,
                                 char *path, int path_cap) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    if (win->input_tail == win->input_head) {
        return 0;
    }
    struct PlayerMacInputEvent event = win->input[win->input_tail];
    win->input_tail = (win->input_tail + 1) % PLAYER_MAC_INPUT_QUEUE;
    if (kind) {
        *kind = event.kind;
    }
    if (key_code) {
        *key_code = event.key_code;
    }
    if (flags) {
        *flags = event.flags;
    }
    if (pressed) {
        *pressed = event.pressed;
    }
    if (repeat) {
        *repeat = event.repeat;
    }
    if (append) {
        *append = event.append;
    }
    if (x) {
        *x = event.x;
    }
    if (y) {
        *y = event.y;
    }
    if (path && path_cap > 0) {
        if (event.kind == PLAYER_MAC_EVENT_OPEN_FILES) {
            strlcpy(path, event.path, (size_t)path_cap);
        } else {
            path[0] = '\0';
        }
    }
    return 1;
}

void player_mac_window_close(void *handle) {
    struct PlayerMacWindow *win = (struct PlayerMacWindow *)handle;
    win->window.delegate = nil;
    [win->window close];
    free(win);
}
