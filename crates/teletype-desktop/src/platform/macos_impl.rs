//! macOS platform implementation.
//!
//! Uses NSWorkspace for frontmost-app detection and TCC checks for
//! permissions. The objc2 calls are minimal and wrapped in `unsafe` blocks
//! with documentation.

use teletype_core::{
    context::{self, ApplicationContext},
    platform::{PasteShortcut, Permission, PermissionKind, Platform},
};

pub struct MacosPlatform;

impl Platform for MacosPlatform {
    fn active_application(&self) -> Option<ApplicationContext> {
        // SAFETY: frontmost_app only reads Obj-C objects via msg_send!
        // and returns owned Rust types.
        unsafe { frontmost_app() }
    }

    fn permissions(&self) -> Vec<Permission> {
        // SAFETY: microphone_authorized is wrapped in catch_unwind so an
        // unrecognized-selector NSException becomes a benign false, not an abort.
        let mic = std::panic::catch_unwind(microphone_authorized).unwrap_or(false);
        vec![
            Permission {
                kind: PermissionKind::Microphone,
                granted: mic,
            },
            Permission {
                kind: PermissionKind::Accessibility,
                granted: accessibility_trusted(),
            },
        ]
    }

    fn request_permission(&self, kind: PermissionKind) {
        match kind {
            PermissionKind::Microphone => {
                // Call AVAudioApplication.requestAuthorization to trigger the
                // system TCC prompt. This is what makes Teletype appear in
                // System Settings > Privacy & Security > Microphone.
                use objc2::msg_send;
                use objc2::runtime::{AnyClass, AnyObject};
                // SAFETY: msg_send! on AVAudioApplication.shared is a
                // documented Obj-C API; catch_unwind guards against a missing class.
                let _ = std::panic::catch_unwind(|| unsafe {
                    if let Some(cls) = AnyClass::get(c"AVAudioApplication") {
                        let shared: *mut AnyObject = msg_send![cls, shared];
                        if !shared.is_null() {
                            let _: *mut AnyObject = msg_send![shared, requestAuthorization];
                        }
                    }
                });
            }
            PermissionKind::Accessibility => {
                // No programmatic prompt; user must enable in System Settings.
            }
        }
    }

    fn permission_settings_url(&self, kind: PermissionKind) -> Option<String> {
        Some(match kind {
            PermissionKind::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone".into()
            }
            PermissionKind::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
                    .into()
            }
        })
    }

    fn default_hotkey(&self) -> String {
        "Cmd+Shift+Space".into()
    }

    fn paste_shortcut(&self) -> Option<PasteShortcut> {
        Some(PasteShortcut::CommandV)
    }

    fn setup_notes(&self) -> Vec<String> {
        vec![
            "Grant Microphone access in System Settings → Privacy & Security → Microphone.".into(),
            "Grant Accessibility access in System Settings → Privacy & Security → Accessibility."
                .into(),
        ]
    }
}

/// Returns the frontmost application's (bundle_id, name) via NSWorkspace.
unsafe fn frontmost_app() -> Option<ApplicationContext> {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::NSString;

    let workspace: Retained<AnyObject> = msg_send![objc2::class!(NSWorkspace), sharedWorkspace];
    let frontmost: Option<Retained<AnyObject>> = msg_send![&*workspace, frontmostApplication];
    let app = frontmost?;

    let bundle_id: Option<Retained<NSString>> = msg_send![&*app, bundleIdentifier];
    let name: Option<Retained<NSString>> = msg_send![&*app, localizedName];

    let id = bundle_id.map(|s| s.to_string()).unwrap_or_default();
    let name = name.map(|s| s.to_string()).unwrap_or_default();
    Some(context::normalize(&id, &name))
}

/// Checks the microphone authorization status.
///
/// Uses `AVAudioApplication.shared.authorizationStatus` when the class is
/// available (macOS 14+); falls back to `granted = false` otherwise so the
/// UI can prompt the user. The Obj-C call is wrapped in `catch_unwind` because
/// an unrecognized selector raises an NSException that would otherwise abort
/// the process.
fn microphone_authorized() -> bool {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    // SAFETY: msg_send! on AVAudioApplication/AVCaptureDevice is a documented
    // Obj-C API; catch_unwind guards against a missing class or selector.
    std::panic::catch_unwind(|| unsafe {
        // Try AVAudioApplication.shared.authorizationStatus (macOS 14+).
        if let Some(cls) = AnyClass::get(c"AVAudioApplication") {
            let shared: *mut AnyObject = msg_send![cls, shared];
            if !shared.is_null() {
                let status: i64 = msg_send![shared, authorizationStatus];
                return status == 1; // Authorized
            }
        }
        // Fallback: AVCaptureDevice.authorizationStatus (older macOS).
        if let Some(cls) = AnyClass::get(c"AVCaptureDevice") {
            let status: i64 = msg_send![cls, authorizationStatus];
            return status == 2; // Authorized
        }
        false
    })
    .unwrap_or(false)
}

/// Checks `AXIsProcessTrusted()`.
fn accessibility_trusted() -> bool {
    // AXIsProcessTrusted is a C function in ApplicationServices.
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    // SAFETY: AXIsProcessTrusted is a thread-safe C function that requires
    // no invariants beyond a valid process context.
    unsafe { AXIsProcessTrusted() }
}
