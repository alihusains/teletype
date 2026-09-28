// Native macOS hotkey capture panel.
//
// The web-based KeyboardEvent API cannot detect the Fn key or several other
// special keys. This C/Obj-C module creates a native NSPanel with an
// NSEvent local monitor that captures ALL key events including Fn, and
// converts them to Tauri global-shortcut strings (e.g. "Cmd+Shift+Space").
//
// The panel is a small non-activating window centered on screen. The user
// presses a key combo, the display updates live, Enter confirms, Esc cancels.

#import <Cocoa/Cocoa.h>
#import <Carbon/Carbon.h>  // for kVK_Function and other key codes

// Global state for the capture session.
static NSPanel *g_panel = nil;
static NSTextField *g_display = nil;
static id g_monitor = nil;
static NSString *g_currentHotkey = nil;
static BOOL g_waitingForKey = NO;

// Forward declaration.
void teletype_stop_hotkey_capture(void);

// Map a virtual key code + modifier flags to a Tauri shortcut string.
//
// IMPORTANT: the Fn key is a *modifier* on macOS (NSEventModifierFlagFunction),
// and the tauri-plugin-global-shortcut parser (global-hotkey) has no "Fn"
// modifier token — parsing "Fn+..." fails with UnsupportedKey. So Fn is
// reported as the main key (keyboard-types Code::Fn), and any other modifiers
// are dropped: a bare Fn press → "Fn" (works as a push-to-talk hotkey), and
// Fn+other combos are not captured (they'd be unparseable anyway).
static NSString *tauriHotkeyFromEvent(NSEvent *event) {
    NSEventModifierFlags mods = event.modifierFlags;
    BOOL fn = (mods & NSEventModifierFlagFunction) != 0;
    BOOL cmd = (mods & NSEventModifierFlagCommand) != 0;
    BOOL ctrl = (mods & NSEventModifierFlagControl) != 0;
    BOOL alt = (mods & NSEventModifierFlagOption) != 0;
    BOOL shift = (mods & NSEventModifierFlagShift) != 0;

    // Bare modifier presses (no Fn): show what the user has so far.
    if (event.type == NSEventTypeFlagsChanged) {
        if (fn) return @"Fn";
        NSMutableArray *parts = [NSMutableArray array];
        if (cmd) [parts addObject:@"Cmd"];
        if (ctrl) [parts addObject:@"Ctrl"];
        if (alt) [parts addObject:@"Alt"];
        if (shift) [parts addObject:@"Shift"];
        if (parts.count == 0) return nil;
        return [parts componentsJoinedByString:@"+"];
    }

    // A regular key press while Fn is held: Fn can't be expressed as a
    // modifier in Tauri shortcut strings, so report the bare key only.
    // (The Fn-modified meaning of the key is not representable.)
    if (fn) {
        switch (event.keyCode) {
            case kVK_Function: return @"Fn";
            default: break;
        }
        // Fall through to report the plain key name below.
    }

    // For a regular key press, get the key name.
    unichar ch = [event.charactersIgnoringModifiers characterAtIndex:0];
    NSString *keyName = nil;

    switch (event.keyCode) {
        case kVK_Space: keyName = @"Space"; break;
        case kVK_Function: keyName = @"Fn"; break;
        case kVK_LeftArrow: keyName = @"Left"; break;
        case kVK_RightArrow: keyName = @"Right"; break;
        case kVK_UpArrow: keyName = @"Up"; break;
        case kVK_DownArrow: keyName = @"Down"; break;
        case kVK_Home: keyName = @"Home"; break;
        case kVK_End: keyName = @"End"; break;
        case kVK_PageUp: keyName = @"PageUp"; break;
        case kVK_PageDown: keyName = @"PageDown"; break;
        case kVK_Delete: keyName = @"Delete"; break;
        case kVK_ForwardDelete: keyName = @"Delete"; break;
        case kVK_Escape: keyName = @"Escape"; break;
        case kVK_Return: keyName = @"Enter"; break;
        case kVK_Tab: keyName = @"Tab"; break;
        case kVK_ANSI_KeypadEnter: keyName = @"Enter"; break;
        case kVK_F1: keyName = @"F1"; break;
        case kVK_F2: keyName = @"F2"; break;
        case kVK_F3: keyName = @"F3"; break;
        case kVK_F4: keyName = @"F4"; break;
        case kVK_F5: keyName = @"F5"; break;
        case kVK_F6: keyName = @"F6"; break;
        case kVK_F7: keyName = @"F7"; break;
        case kVK_F8: keyName = @"F8"; break;
        case kVK_F9: keyName = @"F9"; break;
        case kVK_F10: keyName = @"F10"; break;
        case kVK_F11: keyName = @"F11"; break;
        case kVK_F12: keyName = @"F12"; break;
        default:
            if (ch >= 0x20 && ch < 0x7F) {
                keyName = [[NSString stringWithCharacters:&ch length:1] uppercaseString];
            } else {
                return nil;
            }
            break;
    }

    NSMutableArray *parts = [NSMutableArray array];
    if (cmd) [parts addObject:@"Cmd"];
    if (ctrl) [parts addObject:@"Ctrl"];
    if (alt) [parts addObject:@"Alt"];
    if (shift) [parts addObject:@"Shift"];
    [parts addObject:keyName];

    return [parts componentsJoinedByString:@"+"];
}

// The event monitor callback.
static void teletypeKeyMonitor(NSEvent *event) {
    if (event.type == NSEventTypeKeyDown || event.type == NSEventTypeFlagsChanged) {
        if (event.type == NSEventTypeKeyDown) {
            if (event.keyCode == kVK_Return || event.keyCode == kVK_ANSI_KeypadEnter) {
                if (g_currentHotkey.length > 0) {
                    [g_currentHotkey writeToFile:@"/tmp/teletype_hotkey_result.txt"
                                     atomically:YES
                                     encoding:NSUTF8StringEncoding
                                        error:nil];
                }
                teletype_stop_hotkey_capture();
                return;
            }
            if (event.keyCode == kVK_Escape) {
                teletype_stop_hotkey_capture();
                return;
            }
        } else {
            // A bare modifier (Ctrl, Cmd, Alt, Shift, or Fn pressed alone) is
            // a legitimate hotkey, but it never fires a keyDown, so the Return
            // path above can't commit it. Commit on release: a flagsChanged
            // event that drops the modifier the user was holding means they
            // let go, so save what they pressed.
            NSEventModifierFlags mods = event.modifierFlags;
            BOOL fn = (mods & NSEventModifierFlagFunction) != 0;
            BOOL anyOther = (mods & (NSEventModifierFlagCommand | NSEventModifierFlagControl |
                                     NSEventModifierFlagOption | NSEventModifierFlagShift)) != 0;
            if (!fn && !anyOther && g_currentHotkey.length > 0) {
                [g_currentHotkey writeToFile:@"/tmp/teletype_hotkey_result.txt"
                                 atomically:YES
                                 encoding:NSUTF8StringEncoding
                                    error:nil];
                teletype_stop_hotkey_capture();
                return;
            }
        }

        NSString *hotkey = tauriHotkeyFromEvent(event);
        if (hotkey) {
            g_currentHotkey = hotkey;
            g_waitingForKey = (event.type == NSEventTypeFlagsChanged);
            [g_display setStringValue:[NSString stringWithFormat:@"%@%@",
                hotkey, g_waitingForKey ? @" …" : @""]];
        }
    }
}

// Start the capture panel. Must be called from the main thread.
void teletype_start_hotkey_capture(void) {
    teletype_stop_hotkey_capture();

    g_currentHotkey = @"";
    g_waitingForKey = NO;

    NSRect frame = NSMakeRect(0, 0, 420, 130);
    g_panel = [[NSPanel alloc] initWithContentRect:frame
                                         styleMask:NSWindowStyleMaskNonactivatingPanel |
                                                   NSWindowStyleMaskTitled
                                           backing:NSBackingStoreBuffered
                                             defer:NO];
    [g_panel setTitle:@"Set Hotkey"];
    [g_panel setLevel:NSStatusWindowLevel];
    [g_panel setHidesOnDeactivate:NO];
    [g_panel setBackgroundColor:[NSColor windowBackgroundColor]];

    NSRect labelFrame = NSMakeRect(16, 34, 388, 62);
    g_display = [[NSTextField alloc] initWithFrame:labelFrame];
    [g_display setStringValue:@"Press a key combination…"];
    [g_display setFont:[NSFont systemFontOfSize:22 weight:NSFontWeightMedium]];
    [g_display setAlignment:NSTextAlignmentCenter];
    [g_display setEditable:NO];
    [g_display setSelectable:NO];
    [g_display setBezeled:NO];
    [g_display setDrawsBackground:NO];
    [g_panel.contentView addSubview:g_display];

    NSScreen *screen = [NSScreen mainScreen];
    if (screen) {
        NSRect screenFrame = [screen visibleFrame];
        NSRect panelFrame = NSMakeRect(
            NSMidX(screenFrame) - 210,
            NSMidY(screenFrame) - 65,
            420, 130);
        [g_panel setFrame:panelFrame display:YES];
    }

    [g_panel makeKeyAndOrderFront:nil];

    NSEventMask mask = NSEventMaskKeyDown | NSEventMaskFlagsChanged;
    g_monitor = [NSEvent addLocalMonitorForEventsMatchingMask:mask
                                                      handler:^NSEvent * _Nullable (NSEvent * _Nonnull event) {
        teletypeKeyMonitor(event);
        return nil;
    }];
}

// Stop the capture panel and clean up.
void teletype_stop_hotkey_capture(void) {
    if (g_monitor) {
        [NSEvent removeMonitor:g_monitor];
        g_monitor = nil;
    }
    if (g_panel) {
        [g_panel orderOut:nil];
        g_panel = nil;
        g_display = nil;
    }
    g_currentHotkey = nil;
    g_waitingForKey = NO;
}
