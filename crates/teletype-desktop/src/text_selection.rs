//! Cross-platform text selection and insertion.
//!
//! On macOS this delegates to the Accessibility API (`ax_text`). On other
//! platforms the functions return `None` / no-op so the rest of the code
//! compiles and degrades gracefully (the UI surfaces "not supported").

/// Returns the text currently selected in the focused application, if any.
#[cfg(target_os = "macos")]
pub fn selected_text() -> Option<String> {
    crate::ax_text::selected_text()
}

#[cfg(not(target_os = "macos"))]
pub fn selected_text() -> Option<String> {
    None
}

/// Inserts text at the caret, replacing any selection. Returns true on success.
#[cfg(target_os = "macos")]
pub fn insert(text: &str) -> bool {
    crate::ax_text::insert(text).landed()
}

#[cfg(not(target_os = "macos"))]
pub fn insert(_text: &str) -> bool {
    false
}
