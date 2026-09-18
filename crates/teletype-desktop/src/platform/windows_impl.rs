//! Windows platform implementation.

use teletype_core::{
    context::{self, ApplicationContext},
    platform::{PasteShortcut, Permission, PermissionKind, Platform},
};

pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    fn active_application(&self) -> Option<ApplicationContext> {
        // Use GetForegroundWindow + GetWindowThreadProcessId + process name.
        // Simplified: return None for now; full impl needs windows-rs crate.
        None
    }

    fn permissions(&self) -> Vec<Permission> {
        // Windows grants mic access per-app; no reliable pre-check.
        vec![Permission {
            kind: PermissionKind::Microphone,
            granted: true,
        }]
    }

    fn request_permission(&self, _kind: PermissionKind) {}

    fn permission_settings_url(&self, kind: PermissionKind) -> Option<String> {
        Some(match kind {
            PermissionKind::Microphone => "ms-settings:privacy-microphone".into(),
            PermissionKind::Accessibility => "ms-settings:privacy-inkkeyboardhandwriting".into(),
        })
    }

    fn default_hotkey(&self) -> String {
        "Ctrl+Shift+Space".into()
    }

    fn paste_shortcut(&self) -> Option<PasteShortcut> {
        Some(PasteShortcut::ControlV)
    }

    fn setup_notes(&self) -> Vec<String> {
        vec!["Windows blocks simulated keys from reaching apps that run as administrator.".into()]
    }
}
