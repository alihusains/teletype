// macOS permission helpers (microphone + accessibility).
//
// The AVAudioApplication API (macOS 14+) is the correct way to query and
// request mic access, but its shared instance + recordPermission property are
// awkward to reach from pure Rust/objc2 0.6 (no easy block construction, and
// the selectors differ from the older AVCaptureDevice API). So we expose
// tiny C functions that own the Obj-C calls.

#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>
#import <ApplicationServices/ApplicationServices.h>
#import <objc/runtime.h>

// Returns the current mic authorization status as an int:
//   0 = notDetermined, 1 = granted, 2 = denied
//
// AVAudioApplicationRecordPermission uses 4-char codes, NOT plain ints:
//   Undetermined = 'undt', Denied = 'deny', Granted = 'grnt'.
int teletype_mic_authorization_status(void) {
    @autoreleasepool {
        Class cls = NSClassFromString(@"AVAudioApplication");
        if (!cls) {
            return 0;
        }
        id shared = [cls performSelector:@selector(sharedInstance)];
        if (!shared) {
            return 0;
        }
        AVAudioApplication *app = (AVAudioApplication *)shared;
        AVAudioApplicationRecordPermission perm = app.recordPermission;
        if (perm == AVAudioApplicationRecordPermissionGranted) {
            return 1;
        }
        if (perm == AVAudioApplicationRecordPermissionDenied) {
            return 2;
        }
        return 0;
    }
}

// Triggers the system TCC prompt for microphone access. This is what makes
// the app appear in System Settings > Privacy & Security > Microphone.
// Non-blocking: the completion handler is a no-op.
void teletype_request_mic_permission(void) {
    @autoreleasepool {
        Class cls = NSClassFromString(@"AVAudioApplication");
        if (!cls) {
            return;
        }
        [cls requestRecordPermissionWithCompletionHandler:^(BOOL granted) {
            (void)granted;
        }];
    }
}

// Returns 1 if the app is trusted for accessibility, 0 otherwise.
int teletype_accessibility_trusted(void) {
    return AXIsProcessTrusted() ? 1 : 0;
}

// Shows the system accessibility prompt that directs the user to enable the
// app in System Settings > Privacy & Security > Accessibility. Unlike the mic
// prompt, this is the only programmatic way to nudge the user toward granting
// accessibility — there is no "grant" API.
void teletype_request_accessibility_permission(void) {
    @autoreleasepool {
        // Build the options dictionary with an explicitly allocated CFString
        // key instead of the raw CFSTR constant array. CFDictionaryCreate with
        // a C array of CFSTR constants crashed inside __NSDictionaryI_new
        // (EXC_BAD_ACCESS) when called from a tokio worker thread; the
        // CFStringCreateWithCString path does not.
        CFStringRef key =
            CFStringCreateWithCString(NULL, "AXTrustedCheckOptionPrompt", kCFStringEncodingUTF8);
        const void *ks[] = {(const void *)key};
        const void *vs[] = {kCFBooleanTrue};
        CFDictionaryRef opts =
            CFDictionaryCreate(NULL, ks, vs, 1, &kCFTypeDictionaryKeyCallBacks,
                                &kCFTypeDictionaryValueCallBacks);
        CFRelease(key);
        AXIsProcessTrustedWithOptions(opts);
        CFRelease(opts);
    }
}

// Checks the system TCC database for accessibility permission by bundle ID.
// Returns 1 if granted (auth_value=2), 0 otherwise.
// This is more reliable than AXIsProcessTrusted() for ad-hoc signed apps
// where the code signature hash changes between builds.
int teletype_tcc_accessibility_granted(void) {
    @autoreleasepool {
        // Try to read the system TCC database
        NSString *dbPath = @"/Library/Application Support/com.apple.TCC/TCC.db";
        // We can't use SQLite directly from Obj-C without linking libsqlite3,
        // so we'll use a different approach: check if we can create an 
        // AXUIElement and query another app.
        
        // Alternative: use the accessibility API to check if we can actually
        // read another app's UI elements
        AXUIElementRef sysWide = AXUIElementCreateSystemWide();
        if (!sysWide) return 0;
        
        // Try to get the focused application - if this works, we have accessibility
        AXUIElementRef focusedApp = NULL;
        AXError err = AXUIElementCopyAttributeValue(sysWide, 
            kAXFocusedApplicationAttribute, (CFTypeRef *)&focusedApp);
        
        int result = 0;
        if (err == kAXErrorSuccess && focusedApp) {
            result = 1;
            CFRelease(focusedApp);
        }
        CFRelease(sysWide);
        return result;
    }
}
