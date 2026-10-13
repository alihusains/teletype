// CGEventTap for bare-modifier key detection (Fn, Ctrl, Cmd, Alt, Shift).
//
// Carbon RegisterEventHotKey never fires for a bare modifier press (no other
// key), so a single-modifier push-to-talk trigger is watched with a session
// CGEventTap on a dedicated thread. The tap fires on flagsChanged events;
// we report edge transitions (down/up) to the Rust side via a C function
// pointer, but ONLY while the target modifier is the sole modifier held, so
// normal typing (e.g. holding Ctrl while pressing a letter) does not trigger
// it.
//
// The tap runs on its own thread with a dedicated run loop (the same pattern
// EnviousWispr uses for its keyboard listener). This ensures the tap is
// always serviced regardless of whether the app is in the foreground or
// background, and regardless of what the main thread is doing.
//
// A watchdog timer on the same run loop checks every 2 seconds whether the
// tap has been disabled by the system and re-enables it.
//
// Requires Accessibility permission (a session-level tap). The app already
// requires Accessibility for typing.

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>
#include <pthread.h>

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

// Shared state, accessed only from the tap thread (except g_callback and
// g_target which are set before the thread starts and read-only after).
static TeletypeModTapCallback g_callback = NULL;
static CGEventFlags g_target = 0;
static BOOL g_down = NO;

// Tap thread state (worker-thread only, except g_tapRunLoop which is set
// by the worker and read by stop()).
static CFMachPortRef g_tap = NULL;
static CFRunLoopSourceRef g_tapSource = NULL;
static CFRunLoopTimerRef g_watchdog = NULL;
static volatile int g_stopRequested = 0;
static pthread_t g_tapThread = 0;
static volatile int g_started = 0;
static CFRunLoopRef g_tapRunLoop = NULL; // set by worker, read by stop()

void teletype_mod_tap_stop(void);

// Watchdog callback: re-enable the tap if the system disabled it.
static void watchdogCallback(CFRunLoopTimerRef timer, void *info) {
    (void)timer;
    (void)info;
    if (g_tap && !CGEventTapIsEnabled(g_tap)) {
        CGEventTapEnable(g_tap, TRUE);
        modTapLog("watchdog: tap re-enabled");
    }
}

static CGEventRef modTapCallback(CGEventTapProxy proxy,
                                 CGEventType type,
                                 CGEventRef event,
                                 void *refcon) {
    (void)proxy;
    (void)refcon;
    // macOS disables event taps that take too long or after certain user
    // input patterns. Re-enable immediately so the hotkey keeps working.
    if (type == kCGEventTapDisabledByTimeout ||
        type == kCGEventTapDisabledByUserInput) {
        if (g_tap) {
            CGEventTapEnable(g_tap, TRUE);
        }
        return event;
    }
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
            modTapLog(active ? "MOD DOWN (sole)" : "MOD UP");
            g_callback(active ? 1 : 0);
        }
    }
    return event; // passive: never consume
}

// Worker thread: creates the tap, adds it to this thread's run loop, and
// runs the run loop until stop is requested. All tap lifecycle operations
// happen here, matching the EnviousWispr LiveKeyboardListener pattern.
static void *tapThreadMain(void *arg) {
    (void)arg;
    modTapLog("tap thread: starting");

    // Create the tap on THIS thread. Use kCGEventTapOptionDefault (active)
    // instead of ListenOnly: active taps are more reliably serviced by macOS
    // even when the app is in the background. The callback still returns the
    // event unchanged, so behavior is passive.
    CFMachPortRef port = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionDefault,
        CGEventMaskBit(kCGEventFlagsChanged),
        modTapCallback,
        NULL);
    if (!port) {
        modTapLog("CGEventTapCreate FAILED (no Accessibility permission?)");
        g_started = 0;
        return NULL;
    }
    g_tap = port;
    g_tapSource = CFMachPortCreateRunLoopSource(NULL, port, 0);

    // Add the source to THIS thread's run loop.
    CFRunLoopRef rl = CFRunLoopGetCurrent();
    g_tapRunLoop = rl;
    CFRunLoopAddSource(rl, g_tapSource, kCFRunLoopCommonModes);

    // Watchdog: check every 2 seconds if the tap is disabled.
    g_watchdog = CFRunLoopTimerCreate(
        kCFAllocatorDefault,
        CFAbsoluteTimeGetCurrent() + 2.0,
        2.0,
        0,
        0,
        watchdogCallback,
        NULL);
    CFRunLoopAddTimer(rl, g_watchdog, kCFRunLoopCommonModes);

    // Enable the tap.
    CGEventTapEnable(port, TRUE);
    g_started = 1;
    modTapLog("tap thread: tap enabled, run loop running");

    // Run the run loop until stop is requested.
    while (!g_stopRequested) {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 3600, false);
    }

    // Cleanup.
    modTapLog("tap thread: cleaning up");
    if (g_tap) {
        CGEventTapEnable(g_tap, FALSE);
    }
    if (g_watchdog) {
        CFRunLoopTimerInvalidate(g_watchdog);
        CFRelease(g_watchdog);
        g_watchdog = NULL;
    }
    if (g_tapSource) {
        CFRunLoopRemoveSource(rl, g_tapSource, kCFRunLoopCommonModes);
        CFRelease(g_tapSource);
        g_tapSource = NULL;
    }
    if (g_tap) {
        CFMachPortInvalidate(g_tap);
        CFRelease(g_tap);
        g_tap = NULL;
    }
    g_tapRunLoop = NULL;
    return NULL;
}

// Returns 1 on success, 0 on failure. `target` is one of the TT_MOD_* bits.
int teletype_mod_tap_start(TeletypeModTapCallback callback, unsigned int target) {
    teletype_mod_tap_stop();

    g_callback = callback;
    g_target = (CGEventFlags)target;
    g_down = NO;
    g_stopRequested = 0;
    g_started = 0;
    g_tapRunLoop = NULL;

    int rc = pthread_create(&g_tapThread, NULL, tapThreadMain, NULL);
    if (rc != 0) {
        modTapLog("pthread_create FAILED for tap thread");
        g_tapThread = 0;
        return 0;
    }

    // Wait for the thread to create and enable the tap (up to 1 second).
    int waited = 0;
    while (!g_started && waited < 100) {
        usleep(10000); // 10 ms
        waited++;
    }
    if (!g_started) {
        modTapLog("tap thread: start timed out");
        g_stopRequested = 1;
        pthread_join(g_tapThread, NULL);
        g_tapThread = 0;
        return 0;
    }

    modTapLog("tap started on dedicated thread");
    return 1;
}

void teletype_mod_tap_stop(void) {
    if (!g_tapThread) {
        return;
    }
    g_stopRequested = 1;
    // Wake the tap thread's run loop so it can check the stop flag.
    if (g_tapRunLoop) {
        CFRunLoopWakeUp(g_tapRunLoop);
    }
    pthread_join(g_tapThread, NULL);
    g_tapThread = 0;
    g_callback = NULL;
    g_target = 0;
    g_down = NO;
}
