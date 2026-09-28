//! macOS platform implementation.
//!
//! Uses NSWorkspace for frontmost-app detection and TCC checks for
//! permissions. The objc2 calls are minimal and wrapped in `unsafe` blocks
//! with documentation.

use teletype_core::{
    context::{self, ApplicationContext},
    platform::{InjectionOutcome, PasteShortcut, Permission, PermissionKind, Platform},
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
        // accessibility_trusted does a CGEvent tap round-trip; on a busy
        // first launch it can stall, which used to hang get_permissions and
        // leave the onboarding screen blank. Run it off the main thread.
        let accessibility = std::thread::spawn(accessibility_trusted)
            .join()
            .unwrap_or(false);
        vec![
            Permission {
                kind: PermissionKind::Microphone,
                granted: mic,
            },
            Permission {
                kind: PermissionKind::Accessibility,
                granted: accessibility,
            },
        ]
    }

    fn request_permission(&self, kind: PermissionKind) {
        match kind {
            PermissionKind::Microphone => {
                // Trigger the system TCC prompt via a C trampoline that calls
                // `[AVAudioApplication requestRecordPermissionWithCompletionHandler:]`.
                // This is what registers Teletype in System Settings > Privacy >
                // Microphone. The C side owns the Obj-C block, which is awkward
                // to construct from pure Rust/objc2 0.6.
                tracing::info!("request_permission: microphone — calling TCC prompt");
                extern "C" {
                    fn teletype_request_mic_permission();
                }
                // SAFETY: pure C function that triggers the OS TCC prompt; no
                // shared mutable state, no lifetime invariants.
                unsafe { teletype_request_mic_permission() }
                tracing::info!("request_permission: microphone — TCC prompt call returned");
            }
            PermissionKind::Accessibility => {
                if !accessibility_trusted() {
                    extern "C" {
                        fn teletype_request_accessibility_permission();
                    }
                    // SAFETY: pure C function that shows the OS accessibility
                    // prompt; no shared mutable state, no lifetime invariants.
                    unsafe { teletype_request_accessibility_permission() }
                }
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

    fn insert_text(&self, text: &str) -> InjectionOutcome {
        // Tier 1: write straight into the focused field through the
        // Accessibility API, verified by read-back. No clipboard round trip and
        // no settle sleeps. Falls through to the clipboard when AX refuses or
        // the write cannot be confirmed.
        crate::ax_text::insert_text(text)
    }

    fn focused_text(&self) -> Option<String> {
        crate::ax_text::focused_value()
    }
}

/// Returns the frontmost application's (bundle_id, name) via NSWorkspace.
unsafe fn frontmost_app() -> Option<ApplicationContext> {
    use objc2::{msg_send, rc::Retained, runtime::AnyObject};
    use objc2_foundation::NSString;

    // SAFETY: all msg_send! calls below target documented AppKit selectors on
    // objects we own; results are converted to owned Rust types.
    let workspace: Retained<AnyObject> =
        unsafe { msg_send![objc2::class!(NSWorkspace), sharedWorkspace] };
    // SAFETY: NSWorkspace.frontmostApplication returns a retained NSRunningApplication.
    let frontmost: Option<Retained<AnyObject>> =
        unsafe { msg_send![&*workspace, frontmostApplication] };
    let app = frontmost?;

    // SAFETY: NSRunningApplication.bundleIdentifier returns a retained NSString or nil.
    let bundle_id: Option<Retained<NSString>> = unsafe { msg_send![&*app, bundleIdentifier] };
    // SAFETY: NSRunningApplication.localizedName returns a retained NSString or nil.
    let name: Option<Retained<NSString>> = unsafe { msg_send![&*app, localizedName] };

    let id = bundle_id.map(|s| s.to_string()).unwrap_or_default();
    let name = name.map(|s| s.to_string()).unwrap_or_default();
    Some(context::normalize(&id, &name))
}

/// Checks the microphone authorization status via the C helper, which reads
/// `AVAudioApplication.sharedInstance.recordPermission` (macOS 14+).
/// Returns `true` only when the status is `authorized`.
fn microphone_authorized() -> bool {
    extern "C" {
        fn teletype_mic_authorization_status() -> i32;
    }
    // SAFETY: teletype_mic_authorization_status is a pure C function that reads
    // an Obj-C property and returns an int; no shared mutable state.
    (unsafe { teletype_mic_authorization_status() }) == 1
}

/// Checks whether the app has accessibility access.
///
/// Two-layer check:
/// 1. `teletype_tcc_accessibility_granted()` — functional check via
///    AXUIElementCreateSystemWide. Works when the app is properly
///    authorized.
/// 2. `teletype_tcc_db_accessibility_granted()` — reads the system TCC
///    database by bundle ID. Catches the case where the app IS in the TCC
///    DB as granted but `AXIsProcessTrusted()` returns false due to an
///    ad-hoc code signature hash change (common during development).
fn accessibility_trusted() -> bool {
    extern "C" {
        fn teletype_tcc_accessibility_granted() -> i32;
        fn teletype_tcc_db_accessibility_granted() -> i32;
    }
    // SAFETY: both are pure C functions with no shared mutable state.
    let functional = unsafe { teletype_tcc_accessibility_granted() } == 1;
    if functional {
        return true;
    }
    // Fall back to TCC DB check for ad-hoc signed dev builds.
    // SAFETY: pure C function that reads the TCC database; no shared mutable state.
    (unsafe { teletype_tcc_db_accessibility_granted() } == 1)
}
