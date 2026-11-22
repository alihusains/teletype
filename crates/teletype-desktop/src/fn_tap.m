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
#include <errno.h>
#include <pthread.h>
#include <time.h>

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

// Lifecycle handshake. `g_thread_alive` is 1 from the moment a tap thread is
// reserved until that thread reports completion, so stop() never has to block
// on a thread that missed its wakeup (see wait_for_tap_exit).
static pthread_mutex_t g_thread_lock = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t g_thread_done = PTHREAD_COND_INITIALIZER;
static int g_thread_alive = 0;

/// Longest a stop may block waiting for the tap thread to finish cleaning up.
/// The thread polls the stop flag every 0.25 s, so a healthy teardown is far
/// quicker; the bound exists so a wedged thread can never freeze the caller.
static const int kStopTimeoutMs = 1000;

void teletype_fn_tap_stop(void);

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
// `pthread_join` alone is not safe here: CFRunLoopWakeUp is dropped when the
// target run loop is not running yet, and a tap thread that missed its wakeup
// would sleep for its whole (3600 s) run-loop timeout. stop() is reached from
// register_hotkey on the main thread, so that used to freeze the app for good.
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

static void fnWatchdogCallback(CFRunLoopTimerRef timer, void *info) {
    (void)timer;
    (void)info;
    if (g_tap && !CGEventTapIsEnabled(g_tap)) {
        CGEventTapEnable(g_tap, TRUE);
        // The release edge may have been lost while the tap was disabled.
        // Forget any held Fn state so the next physical press reports DOWN.
        g_fnDown = NO;
        fnTapLog("watchdog: tap re-enabled, Fn latch cleared");
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
            g_fnDown = NO;
            fnTapLog("tap re-enabled after disable, Fn latch cleared");
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
        mark_tap_thread_finished();
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

    // Bounded wait: a wakeup sent before this loop is running is dropped, so a
    // long timeout would let one missed wakeup strand the thread (and any
    // stop() joining it) for the whole timeout.
    while (!g_stopRequested) {
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, false);
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
    // Report completion so stop() can join immediately.
    mark_tap_thread_finished();
    return NULL;
}

// Returns 1 on success, 0 on failure.
int teletype_fn_tap_start(TeletypeFnTapCallback callback) {
    teletype_fn_tap_stop();

    // Never run two tap threads: they would share the globals below and race
    // each other's cleanup. If the previous thread is still shutting down the
    // caller retries later (the permission watcher polls liveness).
    pthread_mutex_lock(&g_thread_lock);
    int alive = g_thread_alive;
    if (!alive) {
        g_thread_alive = 1; // reserve before the thread exists
        g_stopRequested = 0;
    }
    pthread_mutex_unlock(&g_thread_lock);
    if (alive) {
        fnTapLog("fn tap start REFUSED: previous tap thread still shutting down");
        return 0;
    }

    g_callback = callback;
    g_fnDown = NO;
    g_started = 0;
    g_tapRunLoop = NULL;

    int rc = pthread_create(&g_tapThread, NULL, fnTapThreadMain, NULL);
    if (rc != 0) {
        fnTapLog("pthread_create FAILED for fn tap thread");
        pthread_mutex_lock(&g_thread_lock);
        g_thread_alive = 0;
        pthread_mutex_unlock(&g_thread_lock);
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
        if (g_tapRunLoop) {
            CFRunLoopWakeUp(g_tapRunLoop);
        }
        if (wait_for_tap_exit(kStopTimeoutMs)) {
            pthread_join(g_tapThread, NULL);
            g_tapThread = 0;
        }
        return 0;
    }

    fnTapLog("fn tap started on dedicated thread");
    return 1;
}

void teletype_fn_tap_stop(void) {
    pthread_mutex_lock(&g_thread_lock);
    int alive = g_thread_alive;
    pthread_mutex_unlock(&g_thread_lock);

    if (alive) {
        g_stopRequested = 1;
        // Report the tap as not live for the whole teardown, so a liveness
        // check cannot see a half-torn-down tap as healthy.
        g_started = 0;
        if (g_tapRunLoop) {
            CFRunLoopWakeUp(g_tapRunLoop);
        }
        if (!wait_for_tap_exit(kStopTimeoutMs)) {
            // Leave the thread to finish on its own: freezing the calling
            // thread (usually the main thread) is never acceptable, and start()
            // refuses to build a second tap while this one is winding down.
            fnTapLog("fn tap stop: tap thread still shutting down; not blocking");
            return;
        }
    }
    if (g_tapThread) {
        // The thread has already reported completion, so this returns at once.
        pthread_join(g_tapThread, NULL);
        g_tapThread = 0;
    }
    g_callback = NULL;
    g_fnDown = NO;
}

// 1 when the Fn tap has been started, still holds a live mach port, and has
// not been disabled by the system. Used for hotkey liveness checks.
int teletype_fn_tap_is_live(void) {
    return g_started && g_tap != NULL && CGEventTapIsEnabled(g_tap) ? 1 : 0;
}
