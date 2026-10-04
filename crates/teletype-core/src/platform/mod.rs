//! The platform boundary.
//!
//! Core logic depends only on this trait. `teletype-desktop` implements it per
//! OS; tests and the core use [`MockPlatform`].

use crate::context::ApplicationContext;
use serde::{Deserialize, Serialize};

/// How far along a permission is.
///
/// The distinction between [`Denied`] and [`NotDetermined`] is the whole reason
/// this is not a bool: macOS will still show its own prompt for a permission
/// that was never requested, and never will for one that was refused. The
/// settings UI offers a different action per state, so collapsing the two
/// forces it to either nag for a permission the OS will never grant, or tell
/// someone to open System Settings who only needed to click "Allow".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionState {
    Granted,
    Denied,
    /// Never asked. The OS will still present its own prompt.
    NotDetermined,
    /// This platform has no such permission to ask for.
    Unsupported,
}

/// A permission the app needs, and how far along it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permission {
    pub kind: PermissionKind,
    pub state: PermissionState,
}

impl Permission {
    pub fn granted(&self) -> bool {
        self.state == PermissionState::Granted
    }

    /// Whether asking the OS again can still succeed. False once the user has
    /// refused, which is the only state where the answer changes what the UI
    /// should offer.
    pub fn can_prompt(&self) -> bool {
        matches!(
            self.state,
            PermissionState::Granted | PermissionState::NotDetermined
        )
    }
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

/// How a piece of text was delivered into the focused app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InjectionRoute {
    /// Written straight into the focused field through the OS accessibility
    /// API. Fastest path: no clipboard round trip, no settle sleeps.
    DirectWrite,
    /// Put on the clipboard and pasted with the platform shortcut. The
    /// universal fallback, and the only route that works where a direct write
    /// is refused.
    ClipboardPaste,
    /// Delivery failed. The text is recoverable from the transcript archive.
    Failed,
}

/// The result of trying to insert text into the focused app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectionOutcome {
    pub route: InjectionRoute,
    /// Why the direct-write tier was skipped or failed. `None` on the first
    /// successful route. Surfaced in the log so a regression in the fast path
    /// is visible rather than silent.
    pub fallback_reason: Option<String>,
    /// Whether the focused field could be read. `false` means the target app
    /// is not exposing its accessibility tree (a sleeping Chromium host, no
    /// focused element at all). A clipboard paste still works there, but the
    /// landing is unverifiable, so the dictation must not be retained on the
    /// clipboard afterwards: give the clipboard back clean.
    pub field_readable: bool,
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

    /// Inserts `text` at the caret of the focused field using the fastest
    /// mechanism the OS offers, falling back when it has to.
    ///
    /// Implementations must be safe to call from a background thread, must
    /// never report success for text that did not land, and must not block
    /// longer than a few hundred milliseconds. The default implementation is
    /// the clipboard route, so a platform without a direct-write API keeps
    /// working unchanged.
    fn insert_text(&self, text: &str) -> InjectionOutcome {
        let _ = text;
        InjectionOutcome {
            route: InjectionRoute::ClipboardPaste,
            fallback_reason: Some("no direct-write route on this platform".into()),
            // The default route never reads the field, so nothing is known.
            // Callers treat unknown as readable (today's behaviour); only a
            // positive unreadable report suppresses retention.
            field_readable: true,
        }
    }

    /// Reads the focused field's current text, if the platform can. Used to
    /// observe what the user changed a dictation into, which is what feeds the
    /// personalization loop. `None` when unsupported or nothing is focused.
    fn focused_text(&self) -> Option<String> {
        None
    }

    /// OS process id of the frontmost application, when the platform exposes
    /// it. Used with [`Platform::active_application`] to detect an app switch
    /// between record start and delivery: the bundle id proves *which* app,
    /// the pid guards against a recycled one. `None` means unknown, which the
    /// delivery gate treats as no evidence, never as a mismatch.
    fn active_pid(&self) -> Option<u32> {
        None
    }

    /// Bring the app with `pid` to the front. Best-effort: `false` when the
    /// platform cannot, the pid is dead, or it belongs to another app now.
    /// The caller re-verifies afterwards; this never proves anything alone.
    fn activate_pid(&self, _pid: u32) -> bool {
        false
    }

    /// Frame (x, y, w, h) of the frontmost app's focused window, when the
    /// accessibility tree exposes it. Catches the same-app-different-window
    /// switch. `None` means unreadable, which passes the gate.
    fn focused_window_frame(&self) -> Option<[i64; 4]> {
        None
    }
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
                state: PermissionState::Granted,
            },
            Permission {
                kind: PermissionKind::Accessibility,
                state: PermissionState::Granted,
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

    fn insert_text(&self, text: &str) -> InjectionOutcome {
        let _ = text;
        InjectionOutcome {
            route: InjectionRoute::ClipboardPaste,
            fallback_reason: Some("mock platform".into()),
            field_readable: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_platform_defaults_to_the_clipboard_route() {
        let p = MockPlatform::default();
        let outcome = p.insert_text("hello");
        assert_eq!(outcome.route, InjectionRoute::ClipboardPaste);
        assert!(outcome.fallback_reason.is_some());
        assert_eq!(p.focused_text(), None);
    }

    #[test]
    fn injection_outcome_round_trips_through_the_ipc_casing() {
        // The UI reads these over IPC, so the camelCase rename is a contract.
        let json = serde_json::to_string(&InjectionOutcome {
            route: InjectionRoute::DirectWrite,
            fallback_reason: None,
            field_readable: true,
        })
        .unwrap();
        assert!(json.contains("\"directWrite\""), "{json}");
        assert!(json.contains("\"fallbackReason\""), "{json}");
        assert!(json.contains("\"fieldReadable\""), "{json}");
    }
}
