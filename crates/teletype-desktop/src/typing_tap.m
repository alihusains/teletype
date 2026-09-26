// CGEventTap for key-down events (AutoText-while-typing).
//
// Passive (listen-only) session-level tap that watches kCGEventKeyDown and
// reports the hardware key code to the Rust side via a C function pointer.
// The Rust side maps key codes to characters, buffers them, and expands
// `/trigger` sequences when a delimiter (space/enter) is typed.
//
// Follows the exact pattern of fn_tap.m: same structure, same error
// handling, same thread safety (the tap fires on the main run loop).

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>

// Called with the virtual key code of every key-down event.
typedef void (*TeletypeTypingTapCallback)(int64_t key_code);

static void typingTapLog(const char *msg) {
    FILE *f = fopen("/tmp/teletype-typing-tap.log", "a");
    if (f) {
        fprintf(f, "%s\n", msg);
        fclose(f);
    }
}

static CFMachPortRef g_tap = NULL;
static CFRunLoopSourceRef g_tapSource = NULL;
static TeletypeTypingTapCallback g_callback = NULL;

void teletype_typing_tap_stop(void);

static CGEventRef typingTapCallback(CGEventTapProxy proxy,
                                    CGEventType type,
                                    CGEventRef event,
                                    void *refcon) {
    (void)proxy;
    (void)refcon;
    if (type == kCGEventKeyDown && g_callback) {
        int64_t kc = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
        g_callback(kc);
    }
    return event; // passive: never consume
}

// Returns 1 on success, 0 on failure.
int teletype_typing_tap_start(TeletypeTypingTapCallback callback) {
    // Stop any existing tap first.
    teletype_typing_tap_stop();

    g_callback = callback;

    // A session-level tap sees events when our app isn't focused.
    // Listen-only (kCGEventTapOptionListenOnly): we observe, never modify.
    CFMachPortRef port = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionListenOnly,
        CGEventMaskBit(kCGEventKeyDown),
        typingTapCallback,
        NULL);
    if (!port) {
        typingTapLog("CGEventTapCreate FAILED (no Accessibility permission?)");
        return 0;
    }
    g_tap = port;
    g_tapSource = CFMachPortCreateRunLoopSource(NULL, port, 0);
    CFRunLoopAddSource(CFRunLoopGetMain(), g_tapSource, kCFRunLoopCommonModes);
    CGEventTapEnable(port, TRUE);
    typingTapLog("tap started on main runloop");
    return 1;
}

void teletype_typing_tap_stop(void) {
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
}
