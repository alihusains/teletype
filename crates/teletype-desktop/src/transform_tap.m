// Swallowing CGEventTap for transform + Quick Add shortcuts.
//
// Carbon RegisterEventHotKey does NOT consume the keystroke on macOS: an
// Alt/Option combo like Alt+1 still reaches the focused app and types its
// option character, replacing the selection the transform was about to
// polish. This tap runs at kCGHeadInsertEventTap with
// kCGEventTapOptionDefault (active, filterable), matches KeyDown events
// against the Rust-provided (flags, keycode) list, calls back into Rust to
// run the transform, and returns NULL so the keystroke never reaches the app.
//
// Requires Accessibility permission (session-level tap). The app already
// requires it for AX selected_text / insert.

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>

// Called with the matched binding slot index.
typedef void (*TeletypeTransformTapCallback)(unsigned int slot);

#define TT_MAX_BINDINGS 32

typedef struct {
    uint64_t flags;
    int64_t keycode;
} TeletypeBinding;

static void transformTapLog(const char *msg) {
    FILE *f = fopen("/tmp/teletype-transform-tap.log", "a");
    if (f) {
        fprintf(f, "%s\n", msg);
        fclose(f);
    }
}

static CFMachPortRef g_tap = NULL;
static CFRunLoopSourceRef g_tapSource = NULL;
static TeletypeTransformTapCallback g_callback = NULL;
static TeletypeBinding g_bindings[TT_MAX_BINDINGS];
static uint32_t g_count = 0;
// 1 while transforms may fire, 0 while the hotkey-capture panel is open.
// The capture panel must see the second key of a combo, so the tap that
// would swallow it is parked for the duration of the capture.
static int g_enabled = 1;

void teletype_transform_tap_stop(void);

// True when Teletype itself is frontmost. Transforms act on other apps, so
// our own keypresses (hotkey capture, typing in settings) pass through.
static BOOL teletypeIsFrontmost(void) {
    NSRunningApplication *front = [[NSWorkspace sharedWorkspace] frontmostApplication];
    NSString *frontId = [front bundleIdentifier];
    NSString *ownId = [[NSBundle mainBundle] bundleIdentifier];
    return frontId != nil && ownId != nil && [frontId isEqualToString:ownId];
}

void teletype_transform_tap_set_enabled(int enabled) {
    g_enabled = enabled ? 1 : 0;
}

static CGEventRef transformTapCallback(CGEventTapProxy proxy,
                                        CGEventType type,
                                        CGEventRef event,
                                        void *refcon) {
    (void)proxy;
    (void)refcon;
    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (g_tap) {
            CGEventTapEnable(g_tap, TRUE);
            transformTapLog("tap re-enabled after disable");
        }
        return event;
    }
    if (type != kCGEventKeyDown) {
        return event;
    }
    if (!g_enabled || g_count == 0 || !g_callback) {
        return event;
    }
    int64_t kc = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
    CGEventFlags flags = CGEventGetFlags(event) &
        (kCGEventFlagMaskAlternate | kCGEventFlagMaskCommand |
         kCGEventFlagMaskControl | kCGEventFlagMaskShift);
    for (uint32_t i = 0; i < g_count; i++) {
        if (g_bindings[i].keycode == kc && g_bindings[i].flags == (uint64_t)flags) {
            // Own app focused (capture panel, settings): let the key through
            // so recording a shortcut and typing in our fields keep working.
            if (teletypeIsFrontmost()) {
                return event;
            }
            g_callback(i);
            return NULL; // swallow: never reaches the focused app
        }
    }
    return event;
}

// Returns 1 on success, 0 on failure (usually missing Accessibility permission).
int teletype_transform_tap_start(TeletypeTransformTapCallback callback) {
    if (g_tap) {
        g_callback = callback;
        return 1;
    }
    g_callback = callback;
    CFMachPortRef port = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionDefault,
        CGEventMaskBit(kCGEventKeyDown),
        transformTapCallback,
        NULL);
    if (!port) {
        transformTapLog("CGEventTapCreate FAILED (no Accessibility permission?)");
        return 0;
    }
    g_tap = port;
    g_tapSource = CFMachPortCreateRunLoopSource(NULL, port, 0);
    CFRunLoopAddSource(CFRunLoopGetMain(), g_tapSource, kCFRunLoopCommonModes);
    CGEventTapEnable(port, TRUE);
    transformTapLog("tap started on main runloop");
    return 1;
}

// Replace the binding list. Copies at most TT_MAX_BINDINGS entries.
void teletype_transform_tap_set_bindings(const uint64_t *flags,
                                          const int64_t *keycodes,
                                          unsigned int count) {
    uint32_t n = count > TT_MAX_BINDINGS ? TT_MAX_BINDINGS : count;
    for (uint32_t i = 0; i < n; i++) {
        g_bindings[i].flags = flags[i];
        g_bindings[i].keycode = keycodes[i];
    }
    g_count = n;
}

void teletype_transform_tap_stop(void) {
    if (g_tapSource) {
        CFRunLoopSourceInvalidate(g_tapSource);
        CFRelease(g_tapSource);
        g_tapSource = NULL;
    }
    if (g_tap) {
        CFRelease(g_tap);
        g_tap = NULL;
    }
    g_callback = NULL;
    g_count = 0;
}
