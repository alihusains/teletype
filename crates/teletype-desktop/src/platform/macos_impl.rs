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
        unsafe { frontmost_app() }
    }

    fn permissions(&self) -> Vec<Permission> {
        vec![
            Permission {
                kind: PermissionKind::Microphone,
                granted: unsafe { microphone_authorized() },
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
                // Trigger the system prompt by probing the default input device.
                use cpal::traits::HostTrait;
                let _ = cpal::default_host().default_input_device();
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

/// Checks `AVCaptureDevice.authorizationStatus() == AVAuthorizationStatusAuthorized (2)`.
unsafe fn microphone_authorized() -> bool {
    use objc2::msg_send;
    let status: i64 = msg_send![objc2::class!(AVCaptureDevice), authorizationStatus];
    status == 2
}

/// Checks `AXIsProcessTrusted()`.
fn accessibility_trusted() -> bool {
    // AXIsProcessTrusted is a C function in ApplicationServices.
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}
