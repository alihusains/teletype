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
#include <errno.h>
#include <pthread.h>
#include <time.h>

// 1 = modifier pressed (as sole modifier), 0 = released.
typedef void (*TeletypeModTapCallback)(unsigned char down);

// CGEventFlags bits for each modifier we may be asked to watch. These are
// the generic CoreGraphics masks: each one covers BOTH sides of the key
// (e.g. kCGEventFlagMaskShift is set for left AND right Shift). The older
// IOKit device bits (0x1 left Ctrl, 0x2 left Shift, 0x4 right Shift,
// 0x8 left Cmd) must NOT be used here: they match only one physical key
// and even the wrong one (0x2 means "left Shift held", which made a bare
// "Alt" hotkey fire on Shift and never on Option).
enum {
    TT_MOD_FN    = 0x00800000, // NX_SECONDARYFNMASK
    TT_MOD_CTRL  = 0x00040000, // kCGEventFlagMaskControl
    TT_MOD_ALT   = 0x00080000, // kCGEventFlagMaskAlternate
    TT_MOD_SHIFT = 0x00020000, // kCGEventFlagMaskShift
    TT_MOD_CMD   = 0x00100000, // kCGEventFlagMaskCommand (NX_COMMANDMASK)
};
// All the modifier bits we consider "other modifiers" for the sole-modifier
// test. The Fn bit is tracked separately because it is not part of the
// standard modifier-flag mask on every event.
static const CGEventFlags kAllOtherMods =
    (CGEventFlags)(TT_MOD_CTRL | TT_MOD_ALT | TT_MOD_SHIFT | TT_MOD_CMD);

// Pure decision function, unit-tested from the Rust side. `target` is one
// of the TT_MOD_* bits; `flags` is the raw flagsChanged value. Returns 1
// while the target modifier is held and no other modifier (including Fn)
// is, 0 otherwise.
int teletype_mod_tap_sole_down(unsigned int target, unsigned int flags) {
    unsigned int targetDown = flags & target;
    unsigned int otherModsHeld =
        (target == TT_MOD_FN)
            ? (flags & kAllOtherMods)
            : ((flags & kAllOtherMods & ~target) | (flags & TT_MOD_FN));
    return targetDown && !otherModsHeld;
}

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

// Lifecycle handshake. `g_thread_alive` is 1 from the moment a tap thread is
// reserved until that thread reports completion, so stop() never blocks on a
// thread that missed its wakeup (see wait_for_tap_exit).
static pthread_mutex_t g_thread_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_thread_done = PTHREAD_COND_INITIALIZER;
static int g_thread_alive = 0;

/// Longest a stop may block waiting for the tap thread to clean up.
static const int kStopTimeoutMs = 1000;

void teletype_mod_tap_stop(void);

// Marks the tap thread as finished so stop() can join it without blocking and
// a later start() may run again. Every exit path of the thread must call this.
static void mark_tap_thread_finished(void) {
    g_started = 0;
    pthread_mutex_lock(&g_thread_lock);
    g_thread_alive = 0;
    pthread_cond_broadcast(&g_thread_done);
    pthread_mutex_unlock(&g_thread_lock);
}

// Waits up to `timeout_ms` for the tap thread to report that it exited.
// Returns 1 when it is gone, 0 when it is still shutting down.
//
// CFRunLoopWakeUp is dropped when the target run loop is not running yet, and
// a tap thread that missed its wakeup would sleep for its whole run-loop
// timeout. stop() runs on the main thread (register_hotkey), so joining that
// thread unconditionally used to freeze the app.
static int wait_for_tap_exit(int timeout_ms) {
    struct timespec deadline;
    clock_gettime(CLOCK_REALTIME, &deadline);
    deadline.tv_sec += timeout_ms / 1000;
    deadline.tv_nsec += (long)(timeout_ms % 1000) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec += 1;
        deadline.tv_nsec -= 1000000000L;
    }

    pthread_mutex_lock(&g_thread_lock);
    while (g_thread_alive) {
        if (pthread_cond_timedwait(&g_thread_done, &g_thread_lock, &deadline) == ETIMEDOUT) {
            break;
        }
    }
    int gone = !g_thread_alive;
    pthread_mutex_unlock(&g_thread_lock);
    return gone;
}

// Watchdog callback: re-enable the tap if the system disabled it.
static void watchdogCallback(CFRunLoopTimerRef timer, void *info) {
    (void)timer;
    (void)info;
    if (g_tap && !CGEventTapIsEnabled(g_tap)) {
        CGEventTapEnable(g_tap, TRUE);
        // A release edge may have been lost while the tap was disabled.
        g_down = NO;
        modTapLog("watchdog: tap re-enabled, modifier latch cleared");
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
            g_down = NO;
        }
        return event;
    }
    if (type == kCGEventFlagsChanged) {
        CGEventFlags flags = CGEventGetFlags(event);
        int active = teletype_mod_tap_sole_down((unsigned int)g_target,
                                                (unsigned int)flags);
        if ((BOOL)active != g_down && g_callback) {
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
        mark_tap_thread_finished();
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

    // Run the run loop until stop is requested. The wait is bounded so a
    // missed wakeup costs one interval instead of the whole timeout.
    while (!g_stopRequested) {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, false);
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
    // Report completion so stop() can join immediately.
    mark_tap_thread_finished();
    return NULL;
}

// Returns 1 on success, 0 on failure. `target` is one of the TT_MOD_* bits.
int teletype_mod_tap_start(TeletypeModTapCallback callback, unsigned int target) {
    teletype_mod_tap_stop();

    // Never run two tap threads: they would share the globals below and race
    // each other's cleanup.
    pthread_mutex_lock(&g_thread_lock);
    int alive = g_thread_alive;
    if (!alive) {
        g_thread_alive = 1; // reserve before the thread exists
        g_stopRequested = 0;
    }
    pthread_mutex_unlock(&g_thread_lock);
    if (alive) {
        modTapLog("tap start REFUSED: previous tap thread still shutting down");
        return 0;
    }

    g_callback = callback;
    g_target = (CGEventFlags)target;
    g_down = NO;
    g_started = 0;
    g_tapRunLoop = NULL;

    int rc = pthread_create(&g_tapThread, NULL, tapThreadMain, NULL);
    if (rc != 0) {
        modTapLog("pthread_create FAILED for tap thread");
        pthread_mutex_lock(&g_thread_lock);
        g_thread_alive = 0;
        pthread_mutex_unlock(&g_thread_lock);
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
        if (g_tapRunLoop) {
            CFRunLoopWakeUp(g_tapRunLoop);
        }
        if (wait_for_tap_exit(kStopTimeoutMs)) {
            pthread_join(g_tapThread, NULL);
            g_tapThread = 0;
        }
        return 0;
    }

    modTapLog("tap started on dedicated thread");
    return 1;
}

void teletype_mod_tap_stop(void) {
    pthread_mutex_lock(&g_thread_lock);
    int alive = g_thread_alive;
    pthread_mutex_unlock(&g_thread_lock);

    if (alive) {
        g_stopRequested = 1;
        // Not live during teardown, so a liveness check cannot see a
        // half-torn-down tap as healthy.
        g_started = 0;
        // Wake the tap thread's run loop so it can check the stop flag.
        if (g_tapRunLoop) {
            CFRunLoopWakeUp(g_tapRunLoop);
        }
        if (!wait_for_tap_exit(kStopTimeoutMs)) {
            // Never freeze the calling thread; start() refuses to build a
            // second tap while this one is winding down.
            modTapLog("tap stop: tap thread still shutting down; not blocking");
            return;
        }
    }
    if (g_tapThread) {
        // The thread has already reported completion, so this returns at once.
        pthread_join(g_tapThread, NULL);
        g_tapThread = 0;
    }
    g_callback = NULL;
    g_target = 0;
    g_down = NO;
}

// 1 when the modifier tap has been started, still holds a live mach port, and
// has not been disabled by the system. Used for hotkey liveness checks.
int teletype_mod_tap_is_live(void) {
    return g_started && g_tap != NULL && CGEventTapIsEnabled(g_tap) ? 1 : 0;
}
