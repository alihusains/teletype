// CGEventTap for bare-Fn key detection.
//
// Carbon RegisterEventHotKey never fires for Fn-only presses, so we watch the
// hardware Fn key (kVK_Function = 0x3f) with a session CGEventTap on a
// dedicated thread. The tap fires on flagsChanged events; we report edge
// transitions (down/up) to the Rust side via a C function pointer.
//
// The tap runs on its own thread with a dedicated run loop (the same pattern
// EnviousWispr uses for its keyboard listener). A watchdog timer checks every
// 2 seconds whether the tap has been disabled and re-enables it.
//
// Requires Accessibility permission (a session-level tap). The app already
// requires Accessibility for typing.

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>
#include <pthread.h>

// 1 = Fn pressed, 0 = Fn released.
typedef void (*TeletypeFnTapCallback)(unsigned char down);

static void fnTapLog(const char *msg) {
    FILE *f = fopen("/tmp/teletype-fntap.log", "a");
    if (f) {
        fprintf(f, "%s\n", msg);
        fclose(f);
    }
}

static TeletypeFnTapCallback g_callback = NULL;
static BOOL g_fnDown = NO;

// Tap thread state (worker-thread only, except g_tapRunLoop).
static CFMachPortRef g_tap = NULL;
static CFRunLoopSourceRef g_tapSource = NULL;
static CFRunLoopTimerRef g_watchdog = NULL;
static volatile int g_stopRequested = 0;
static pthread_t g_tapThread = 0;
static volatile int g_started = 0;
static CFRunLoopRef g_tapRunLoop = NULL;

void teletype_fn_tap_stop(void);

static void fnWatchdogCallback(CFRunLoopTimerRef timer, void *info) {
    (void)timer;
    (void)info;
    if (g_tap && !CGEventTapIsEnabled(g_tap)) {
        CGEventTapEnable(g_tap, TRUE);
        fnTapLog("watchdog: tap re-enabled");
    }
}

static CGEventRef fnTapCallback(CGEventTapProxy proxy,
                                CGEventType type,
                                CGEventRef event,
                                void *refcon) {
    (void)proxy;
    (void)refcon;
    if (type == kCGEventTapDisabledByTimeout ||
        type == kCGEventTapDisabledByUserInput) {
        if (g_tap) {
            CGEventTapEnable(g_tap, TRUE);
            fnTapLog("tap re-enabled after disable");
        }
        return event;
    }
    if (type == kCGEventFlagsChanged) {
        unsigned int kc = (unsigned int)CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
        // Fn arrives as flagsChanged with keycode 63 (kVK_Function).
        if (kc == 63) {
            CGEventFlags flags = CGEventGetFlags(event);
            const CGEventFlags kFnMask = (CGEventFlags)0x00800000;
            BOOL down = (flags & kFnMask) != 0;
            if (down != g_fnDown && g_callback) {
                g_fnDown = down;
                fnTapLog(down ? "FN DOWN" : "FN UP");
                g_callback(down ? 1 : 0);
            }
        }
    }
    return event; // passive: never consume
}

static void *fnTapThreadMain(void *arg) {
    (void)arg;
    fnTapLog("fn tap thread: starting");

    // Use kCGEventTapOptionDefault (active) for reliable background delivery.
    CFMachPortRef port = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionDefault,
        CGEventMaskBit(kCGEventFlagsChanged) | CGEventMaskBit(kCGEventKeyDown) | CGEventMaskBit(kCGEventKeyUp),
        fnTapCallback,
        NULL);
    if (!port) {
        fnTapLog("CGEventTapCreate FAILED (no Accessibility permission?)");
        g_started = 0;
        return NULL;
    }
    g_tap = port;
    g_tapSource = CFMachPortCreateRunLoopSource(NULL, port, 0);

    CFRunLoopRef rl = CFRunLoopGetCurrent();
    g_tapRunLoop = rl;
    CFRunLoopAddSource(rl, g_tapSource, kCFRunLoopCommonModes);

    g_watchdog = CFRunLoopTimerCreate(
        kCFAllocatorDefault,
        CFAbsoluteTimeGetCurrent() + 2.0,
        2.0,
        0,
        0,
        fnWatchdogCallback,
        NULL);
    CFRunLoopAddTimer(rl, g_watchdog, kCFRunLoopCommonModes);

    CGEventTapEnable(port, TRUE);
    g_started = 1;
    fnTapLog("fn tap thread: tap enabled, run loop running");

    while (!g_stopRequested) {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 3600, false);
    }

    fnTapLog("fn tap thread: cleaning up");
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

// Returns 1 on success, 0 on failure.
int teletype_fn_tap_start(TeletypeFnTapCallback callback) {
    teletype_fn_tap_stop();

    g_callback = callback;
    g_fnDown = NO;
    g_stopRequested = 0;
    g_started = 0;
    g_tapRunLoop = NULL;

    int rc = pthread_create(&g_tapThread, NULL, fnTapThreadMain, NULL);
    if (rc != 0) {
        fnTapLog("pthread_create FAILED for fn tap thread");
        g_tapThread = 0;
        return 0;
    }

    int waited = 0;
    while (!g_started && waited < 100) {
        usleep(10000);
        waited++;
    }
    if (!g_started) {
        fnTapLog("fn tap thread: start timed out");
        g_stopRequested = 1;
        pthread_join(g_tapThread, NULL);
        g_tapThread = 0;
        return 0;
    }

    fnTapLog("fn tap started on dedicated thread");
    return 1;
}

void teletype_fn_tap_stop(void) {
    if (!g_tapThread) {
        return;
    }
    g_stopRequested = 1;
    if (g_tapRunLoop) {
        CFRunLoopWakeUp(g_tapRunLoop);
    }
    pthread_join(g_tapThread, NULL);
    g_tapThread = 0;
    g_callback = NULL;
    g_fnDown = NO;
}
