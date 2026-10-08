// Unified InputEngine tap (macOS).
//
// Single CGEventTap behind the InputEngine:
//
//   CGEventTap -> InputNormalizer -> ShortcutManager OR AutoTextManager
//             -> TextContext -> Text replacement/action.
//
// Shortcut behavior: KeyDown/KeyUp events whose (keycode, modifiers) match a
// runtime-updatable shortcut map are consumed (return NULL) and dispatched to
// Rust asynchronously. The target application never receives them. This is
// what makes two-key combos like Alt+1 work: Carbon RegisterEventHotKey does
// NOT swallow, so it cannot be used for Option combos.
//
// AutoText behavior: ordinary keyboard input is NEVER consumed. Non-matching
// KeyDown events are observed only: their resulting Unicode text (via
// CGEventKeyboardGetUnicodeString, i.e. layout-aware characters rather than
// physical keycodes) is forwarded to Rust, which maintains a recent-text
// buffer and expands triggers. Replacement happens afterwards via
// TextContext/TextInserter (AX direct write, clipboard+paste fallback with
// preserve/restore), so the trigger may appear briefly before replacement.
//
// Requires Accessibility permission (session-level tap).

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>
#import <CoreGraphics/CoreGraphics.h>

// Fired with the matched shortcut slot index (KeyDown only).
typedef void (*TeletypeInputActionCallback)(unsigned int slot);
// Observed text for AutoText: UTF-16 units of the resulting characters,
// plus the originating keycode/flags. kind: 0 = text, 1 = backspace,
// 2 = delimiter (space/enter), 3 = break (clear buffer).
typedef void (*TeletypeInputObserveCallback)(unsigned int kind,
                                             int64_t keycode,
                                             uint64_t flags,
                                             const uint16_t *chars,
                                             unsigned int char_len);

#define IE_MAX_BINDINGS 32

typedef struct {
    uint64_t flags;
    int64_t keycode;
} TeletypeInputBinding;

static void inputEngineLog(const char *msg) {
    FILE *f = fopen("/tmp/teletype-input-engine.log", "a");
    if (f) {
        fprintf(f, "%s\n", msg);
        fclose(f);
    }
}

static CFMachPortRef g_tap = NULL;
static CFRunLoopSourceRef g_tapSource = NULL;
static TeletypeInputActionCallback g_actionCb = NULL;
static TeletypeInputObserveCallback g_observeCb = NULL;
static TeletypeInputBinding g_bindings[IE_MAX_BINDINGS];
static uint32_t g_count = 0;
// Master liveness, shortcut firing, AutoText observation. While parked (hotkey
// capture panel open) neither fires nor observes, so the panel sees the raw
// second key of a combo.
static int g_enabled = 1;
static int g_shortcuts_on = 1;
static int g_observe_on = 1;
static int g_parked = 0;

void teletype_input_engine_stop(void);

// True when Teletype itself is frontmost. Shortcuts pass through (hotkey
// capture and settings typing keep working) and AutoText never triggers in
// our own windows.
static BOOL inputEngineIsFrontmostSelf(void) {
    NSRunningApplication *front = [[NSWorkspace sharedWorkspace] frontmostApplication];
    NSString *frontId = [front bundleIdentifier];
    NSString *ownId = [[NSBundle mainBundle] bundleIdentifier];
    return frontId != nil && ownId != nil && [frontId isEqualToString:ownId];
}

static CGEventRef inputEngineCallback(CGEventTapProxy proxy,
                                       CGEventType type,
                                       CGEventRef event,
                                       void *refcon) {
    (void)proxy;
    (void)refcon;
    if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
        if (g_tap) {
            CGEventTapEnable(g_tap, TRUE);
            inputEngineLog("tap re-enabled after disable");
        }
        return event;
    }
    if (!g_enabled || (type != kCGEventKeyDown && type != kCGEventKeyUp)) {
        return event;
    }
    int64_t kc = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode);
    CGEventFlags flags = CGEventGetFlags(event) &
        (kCGEventFlagMaskAlternate | kCGEventFlagMaskCommand |
         kCGEventFlagMaskControl | kCGEventFlagMaskShift);

    // 1. ShortcutManager: match keycode + modifiers against the live map.
    if (g_shortcuts_on && !g_parked && g_count > 0) {
        for (uint32_t i = 0; i < g_count; i++) {
            if (g_bindings[i].keycode == kc && g_bindings[i].flags == (uint64_t)flags) {
                if (inputEngineIsFrontmostSelf()) {
                    return event;
                }
                if (type == kCGEventKeyDown && g_actionCb) {
                    g_actionCb(i);
                }
                return NULL; // consume both down and up; app never sees them
            }
        }
    }

    // 2. AutoTextManager: observe only, never consume ordinary input.
    if (type == kCGEventKeyDown && g_observe_on && !g_parked && g_observeCb) {
        if (inputEngineIsFrontmostSelf()) {
            return event;
        }
        // Backspace (delete-backward) edits the recent-text buffer.
        if (kc == kVK_Delete) {
            g_observeCb(1, kc, (uint64_t)flags, NULL, 0);
            return event;
        }
        UniChar buf[8];
        UniCharCount got = 0;
        CGEventKeyboardGetUnicodeString(event, 8, &got, buf);
        if (got > 0) {
            unsigned int kind = 0;
            if (got == 1 && (buf[0] == 0x20 || buf[0] == 0x09)) {
                kind = 2; // space/tab delimiter
            } else if (got == 1 && (buf[0] == 0x0D || buf[0] == 0x03)) {
                kind = 2; // enter delimiter
            }
            g_observeCb(kind, kc, (uint64_t)flags, buf, got);
        } else {
            // No producible text (arrows, escape, F-keys): break the buffer so
            // a trigger cannot span across navigation.
            g_observeCb(3, kc, (uint64_t)flags, NULL, 0);
        }
    }
    return event;
}

// Returns 1 on success, 0 on failure (usually missing Accessibility).
int teletype_input_engine_start(TeletypeInputActionCallback actionCb,
                                 TeletypeInputObserveCallback observeCb) {
    g_actionCb = actionCb;
    g_observeCb = observeCb;
    if (g_tap) {
        return 1;
    }
    CFMachPortRef port = CGEventTapCreate(
        kCGSessionEventTap,
        kCGHeadInsertEventTap,
        kCGEventTapOptionDefault,
        CGEventMaskBit(kCGEventKeyDown) | CGEventMaskBit(kCGEventKeyUp),
        inputEngineCallback,
        NULL);
    if (!port) {
        inputEngineLog("CGEventTapCreate FAILED (no Accessibility permission?)");
        return 0;
    }
    g_tap = port;
    g_tapSource = CFMachPortCreateRunLoopSource(NULL, port, 0);
    CFRunLoopAddSource(CFRunLoopGetMain(), g_tapSource, kCFRunLoopCommonModes);
    CGEventTapEnable(port, TRUE);
    inputEngineLog("tap started on main runloop");
    return 1;
}

void teletype_input_engine_set_bindings(const uint64_t *flags,
                                         const int64_t *keycodes,
                                         unsigned int count) {
    uint32_t n = count > IE_MAX_BINDINGS ? IE_MAX_BINDINGS : count;
    for (uint32_t i = 0; i < n; i++) {
        g_bindings[i].flags = flags[i];
        g_bindings[i].keycode = keycodes[i];
    }
    g_count = n;
}

void teletype_input_engine_set_enabled(int enabled) {
    g_enabled = enabled ? 1 : 0;
}

void teletype_input_engine_set_shortcuts_on(int on) {
    g_shortcuts_on = on ? 1 : 0;
}

void teletype_input_engine_set_observe_on(int on) {
    g_observe_on = on ? 1 : 0;
}

void teletype_input_engine_set_parked(int parked) {
    g_parked = parked ? 1 : 0;
}

void teletype_input_engine_stop(void) {
    if (g_tapSource) {
        CFRunLoopSourceInvalidate(g_tapSource);
        CFRelease(g_tapSource);
        g_tapSource = NULL;
    }
    if (g_tap) {
        CFRelease(g_tap);
        g_tap = NULL;
    }
    g_actionCb = NULL;
    g_observeCb = NULL;
    g_count = 0;
}
