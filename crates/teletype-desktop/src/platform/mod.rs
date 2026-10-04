//! Platform-specific implementations of the `Platform` trait.

use teletype_core::{
    context::ApplicationContext,
    platform::{PasteShortcut, Permission, PermissionKind, PermissionState, Platform},
};

/// Creates the platform implementation for the current OS.
pub fn create() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacosPlatform)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsPlatform)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Box::new(GenericPlatform)
    }
}

/// Generic fallback (Linux dev, tests).
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct GenericPlatform;

impl Platform for GenericPlatform {
    fn active_application(&self) -> Option<ApplicationContext> {
        None
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
        Some(PasteShortcut::ControlV)
    }
    fn setup_notes(&self) -> Vec<String> {
        vec!["Linux: X11 required for text injection.".into()]
    }
}

#[cfg(target_os = "macos")]
mod macos_impl;
#[cfg(target_os = "macos")]
pub use macos_impl::MacosPlatform;

#[cfg(target_os = "windows")]
mod windows_impl;
#[cfg(target_os = "windows")]
pub use windows_impl::WindowsPlatform;
