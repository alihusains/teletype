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
        const void *keys[] = {kAXTrustedCheckOptionPrompt};
        const void *vals[] = {kCFBooleanTrue};
        CFDictionaryRef opts = CFDictionaryCreate(NULL, keys, vals, 2, &kCFTypeDictionaryKeyCallBacks,
                                                  &kCFTypeDictionaryValueCallBacks);
        AXIsProcessTrustedWithOptions(opts);
        CFRelease(opts);
    }
}
