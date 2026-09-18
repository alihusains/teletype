//! The platform boundary.
//!
//! Core logic depends only on this trait. `teletype-desktop` implements it per
//! OS; tests and the core use [`MockPlatform`].

use crate::context::ApplicationContext;
use serde::{Deserialize, Serialize};

/// A permission the app needs, and whether it is granted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    pub kind: PermissionKind,
    pub granted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Microphone,
    Accessibility,
}

/// An abstract paste shortcut, mapped to platform keys by the injector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteShortcut {
    /// ⌘V on macOS, Ctrl+V on Windows.
    CommandV,
    /// ⌃V — some apps only accept the control variant.
    ControlV,
}

/// OS-specific capabilities. Implementations must be cheap to call and must
/// never block the UI thread for more than a few milliseconds.
pub trait Platform: Send + Sync {
    /// The currently focused application, or `None` when detection is
    /// unavailable. Must not return fabricated data.
    fn active_application(&self) -> Option<ApplicationContext>;

    /// Permissions dictation needs, with current grant state.
    fn permissions(&self) -> Vec<Permission>;

    /// Trigger the OS permission prompt for `kind` (no-op if unsupported).
    fn request_permission(&self, kind: PermissionKind);

    /// URL that opens the OS settings screen for `kind`, if one exists.
    fn permission_settings_url(&self, kind: PermissionKind) -> Option<String>;

    /// The default hotkey string (platform accelerator syntax) for dictation.
    fn default_hotkey(&self) -> String;

    /// The paste shortcut the injector should use, if any.
    fn paste_shortcut(&self) -> Option<PasteShortcut>;

    /// OS-specific setup hints shown in Settings.
    fn setup_notes(&self) -> Vec<String>;
}

/// A platform that reports nothing — for unit tests and headless runs.
#[derive(Debug, Clone, Default)]
pub struct MockPlatform {
    pub app: Option<ApplicationContext>,
    pub paste: Option<PasteShortcut>,
}

impl MockPlatform {
    pub fn with_app(app: ApplicationContext) -> Self {
        Self {
            app: Some(app),
            paste: Some(PasteShortcut::CommandV),
        }
    }
}

impl Platform for MockPlatform {
    fn active_application(&self) -> Option<ApplicationContext> {
        self.app.clone()
    }

    fn permissions(&self) -> Vec<Permission> {
        vec![
            Permission {
                kind: PermissionKind::Microphone,
                granted: true,
            },
            Permission {
                kind: PermissionKind::Accessibility,
                granted: true,
            },
        ]
    }

    fn request_permission(&self, _kind: PermissionKind) {}

    fn permission_settings_url(&self, _kind: PermissionKind) -> Option<String> {
        None
    }

    fn default_hotkey(&self) -> String {
        "Ctrl+Shift+Space".into()
    }

    fn paste_shortcut(&self) -> Option<PasteShortcut> {
        self.paste
    }

    fn setup_notes(&self) -> Vec<String> {
        Vec::new()
    }
}
