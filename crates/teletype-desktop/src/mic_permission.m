// macOS microphone-permission helpers.
//
// The AVAudioApplication API (macOS 14+) is the correct way to query and
// request mic access, but its shared instance + recordPermission property are
// awkward to reach from pure Rust/objc2 0.6 (no easy block construction, and
// the selectors differ from the older AVCaptureDevice API). So we expose two
// tiny C functions that own the Obj-C calls.

#import <Foundation/Foundation.h>
#import <AVFoundation/AVFoundation.h>

// Returns the current mic authorization status as an int:
//   0 = notDetermined, 1 = authorized, 2 = denied
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
        return (int)app.recordPermission;
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
