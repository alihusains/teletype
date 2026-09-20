// CGEventTap for bare-Fn key detection.
//
// Carbon RegisterEventHotKey never fires for Fn-only presses, so we watch the
// hardware Fn key (kVK_Function = 0x3f) with a passive CGEventTap. The tap
// fires on flagsChanged events; we report edge transitions (down/up) to the
// Rust side via a C function pointer.
//
// Requires Accessibility permission (kCGEventTapListenedEvents on a
// session-level tap). The app already requires Accessibility for typing.

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>

// 1 = Fn pressed, 0 = Fn released.
typedef void (*TeletypeFnTapCallback)(unsigned char down);

static void fnTapLog(const char *msg) {
    FILE *f = fopen("/tmp/teletype-fntap.log", "a");
    if (f) {
        fprintf(f, "%s\n", msg);
        fclose(f);
    }
}

static CGEventRef g_tap = NULL;
static CFRunLoopSourceRef g_tapSource = NULL;
static TeletypeFnTapCallback g_callback = NULL;
static BOOL g_fnDown = NO;

void teletype_fn_tap_stop(void);

static CGEventRef fnTapCallback(CGEventTapProxy proxy,
                                CGEventType type,
                                CGEventRef event,
                                void *refcon) {
    (void)proxy;
    (void)refcon;
    if (type == kCGEventFlagsChanged) {
        unsigned int kc = (unsigned int)CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
        // Fn arrives as flagsChanged with keycode 63 (kVK_Function).
        if (kc == 63) {
            CGEventFlags flags = CGEventGetFlags(event);
            // The Fn bit is NX_SECONDARYFNMASK (0x00800000), NOT 0x800000000.
            // From logs: flags=0x800100 when Fn is DOWN, 0x100 when UP.
            // 0x800100 = 0x800000 | 0x100, so the Fn bit is 0x800000.
            const CGEventFlags kFnMask = (CGEventFlags)0x00800000;
            BOOL down = (flags & kFnMask) != 0;
            if (down != g_fnDown && g_callback) {
                g_fnDown = down;
                g_callback(down ? 1 : 0);
            }
        }
    }
    return event; // passive: never consume
}

// Returns 1 on success, 0 on failure.
int teletype_fn_tap_start(TeletypeFnTapCallback callback) {
    // Stop any existing tap first.
    teletype_fn_tap_stop();

    g_callback = callback;
    g_fnDown = NO;

    // A session-level tap also sees events when our app isn't focused.
    // Listening to flagsChanged is non-intrusive (no typing interference).
    CFMachPortRef port = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionListenOnly,
        CGEventMaskBit(kCGEventFlagsChanged) | CGEventMaskBit(kCGEventKeyDown) | CGEventMaskBit(kCGEventKeyUp),
        fnTapCallback,
        NULL);
    if (!port) {
        fnTapLog("CGEventTapCreate FAILED (no Accessibility permission?)");
        return 0;
    }
    g_tapSource = CFMachPortCreateRunLoopSource(NULL, port, 0);
    CFRunLoopAddSource(CFRunLoopGetMain(), g_tapSource, kCFRunLoopCommonModes);
    CGEventTapEnable(port, TRUE);
    fnTapLog("tap started on main runloop");
    return 1;
}

void teletype_fn_tap_stop(void) {
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
    g_fnDown = NO;
}
