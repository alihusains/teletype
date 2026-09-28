// CGEventTap for bare-modifier key detection (Fn, Ctrl, Cmd, Alt, Shift).
//
// Carbon RegisterEventHotKey never fires for a bare modifier press (no other
// key), so a single-modifier push-to-talk trigger is watched with a passive
// CGEventTap. The tap fires on flagsChanged events; we report edge
// transitions (down/up) to the Rust side via a C function pointer, but ONLY
// while the target modifier is the sole modifier held, so normal typing
// (e.g. holding Ctrl while pressing a letter) does not trigger it.
//
// Requires Accessibility permission (a session-level tap). The app already
// requires Accessibility for typing.

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>

// 1 = modifier pressed (as sole modifier), 0 = released.
typedef void (*TeletypeModTapCallback)(unsigned char down);

// CGEventFlags bits for each modifier we may be asked to watch.
enum {
    TT_MOD_FN    = 0x00800000, // NX_SECONDARYFNMASK
    TT_MOD_CTRL  = 0x00000001, // kCGEventFlagMaskControl
    TT_MOD_ALT   = 0x00000002, // kCGEventFlagMaskAlternate
    TT_MOD_SHIFT = 0x00000004, // kCGEventFlagMaskShift
    TT_MOD_CMD   = 0x00000008, // kCGEventFlagMaskCommand
};
// All the modifier bits we consider "other modifiers" for the sole-modifier
// test. The Fn bit is tracked separately because it is not part of the
// standard modifier-flag mask on every event.
static const CGEventFlags kAllOtherMods =
    (CGEventFlags)(TT_MOD_CTRL | TT_MOD_ALT | TT_MOD_SHIFT | TT_MOD_CMD);

static void modTapLog(const char *msg) {
    FILE *f = fopen("/tmp/teletype-modtap.log", "a");
    if (f) {
        fprintf(f, "%s\n", msg);
        fclose(f);
    }
}

static CFMachPortRef g_tap = NULL;
static CFRunLoopSourceRef g_tapSource = NULL;
static TeletypeModTapCallback g_callback = NULL;
static CGEventFlags g_target = 0;
static BOOL g_down = NO;

void teletype_mod_tap_stop(void);

static CGEventRef modTapCallback(CGEventTapProxy proxy,
                                 CGEventType type,
                                 CGEventRef event,
                                 void *refcon) {
    (void)proxy;
    (void)refcon;
    if (type == kCGEventFlagsChanged) {
        CGEventFlags flags = CGEventGetFlags(event);
        BOOL targetDown = (flags & g_target) != 0;
        // Sole-modifier test: the target is held, and no other modifier is.
        // For the Fn target, the other-modifier mask is the standard four;
        // for a standard-modifier target, we also require Fn to be clear.
        BOOL otherModsHeld = (g_target == TT_MOD_FN)
            ? ((flags & kAllOtherMods) != 0)
            : ((flags & kAllOtherMods & ~g_target) != 0 || (flags & TT_MOD_FN) != 0);
        BOOL active = targetDown && !otherModsHeld;
        if (active != g_down && g_callback) {
            g_down = active;
            g_callback(active ? 1 : 0);
        }
    }
    return event; // passive: never consume
}

// Returns 1 on success, 0 on failure. `target` is one of the TT_MOD_* bits.
int teletype_mod_tap_start(TeletypeModTapCallback callback, unsigned int target) {
    teletype_mod_tap_stop();

    g_callback = callback;
    g_target = (CGEventFlags)target;
    g_down = NO;

    CFMachPortRef port = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionListenOnly,
        CGEventMaskBit(kCGEventFlagsChanged),
        modTapCallback,
        NULL);
    if (!port) {
        modTapLog("CGEventTapCreate FAILED (no Accessibility permission?)");
        return 0;
    }
    g_tap = port;
    g_tapSource = CFMachPortCreateRunLoopSource(NULL, port, 0);
    CFRunLoopAddSource(CFRunLoopGetMain(), g_tapSource, kCFRunLoopCommonModes);
    CGEventTapEnable(port, TRUE);
    modTapLog("tap started on main runloop");
    return 1;
}

void teletype_mod_tap_stop(void) {
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
    g_target = 0;
    g_down = NO;
}
