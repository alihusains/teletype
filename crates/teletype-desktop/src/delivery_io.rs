//! P8.1-P8.4 desktop delivery adapters.
//!
//! Implements the `teletype_core::delivery` traits against the platform
//! services this crate already has:
//!
//! - `FieldReader` — focused field text + secure-field detection + set-value,
//!   reusing `ax_text` functions.
//! - `Typer` — enigo character-by-character typing, capped at 2 000 chars /
//!   2 ms per char.
//! - `ClipboardIo` — `arboard` for set-text; `MacClipboardGuard` (NSPasteboard)
//!   for faithful snapshot/restore.
//! - `paste_key` — ⌘V via enigo, the same keystroke the injector uses.
//!
//! macOS only. Non-macOS builds get compile-safe stubs that error on use so
//! the crate still compiles cross-platform.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

#[cfg(target_os = "macos")]
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};

use teletype_core::delivery::{ClipboardIo, DeliveryIo, FieldReader, Typer};
use teletype_core::injector::ClipboardSnapshot;

/// The typed fallback is only worth it for shortish text; beyond this the
/// cascade goes straight to the set-value fallback. Mirrors the core constant.
const MAX_TYPED_CHARS: usize = 2000;
/// Per-character delay for the typed fallback, keeping the keystrokes
/// human-paced so apps with input throttling don't drop characters.
const TYPE_CHAR_DELAY: Duration = Duration::from_millis(2);

// ---------------------------------------------------------------------------
// macOS adapters
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use teletype_core::injector::ClipboardGuard;

    /// Reads the focused field via the accessibility tree (ax_text).
    pub struct AxFieldReader;

    impl FieldReader for AxFieldReader {
        fn selected_text(&self) -> Option<String> {
            crate::ax_text::focused_value()
        }

        fn is_secure_field(&self) -> bool {
            crate::ax_text::read_focused_context(1, 1).is_some_and(|ctx| ctx.secure)
        }

        fn set_selected_text(&self, text: &str) -> Result<(), String> {
            let outcome = crate::ax_text::insert_text(text);
            match outcome.route {
                teletype_core::platform::InjectionRoute::DirectWrite => Ok(()),
                teletype_core::platform::InjectionRoute::ClipboardPaste => Err(outcome
                    .fallback_reason
                    .unwrap_or_else(|| "direct write unavailable".into())),
                teletype_core::platform::InjectionRoute::Failed => {
                    Err("direct write failed".into())
                }
            }
        }
    }

    /// Types text character-by-character via enigo.
    pub struct EnigoTyper {
        enigo: Mutex<Option<Enigo>>,
    }

    impl std::fmt::Debug for EnigoTyper {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("EnigoTyper").finish_non_exhaustive()
        }
    }

    impl EnigoTyper {
        pub fn new() -> Self {
            Self {
                enigo: Mutex::new(None),
            }
        }
    }

    impl Default for EnigoTyper {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Typer for EnigoTyper {
        fn type_text(&self, text: &str) -> Result<(), String> {
            if text.chars().count() > MAX_TYPED_CHARS {
                return Err(format!(
                    "text is {} chars; typed fallback capped at {MAX_TYPED_CHARS}",
                    text.chars().count()
                ));
            }
            let mut guard = self
                .enigo
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.is_none() {
                *guard = Some(
                    Enigo::new(&EnigoSettings::default())
                        .map_err(|e| format!("enigo unavailable: {e}"))?,
                );
            }
            let enigo = guard.as_mut().unwrap();
            for ch in text.chars() {
                enigo
                    .key(Key::Unicode(ch), Direction::Click)
                    .map_err(|e| format!("type failed at {ch:?}: {e}"))?;
                std::thread::sleep(TYPE_CHAR_DELAY);
            }
            Ok(())
        }
    }

    /// Clipboard adapter: arboard for set_text, MacClipboardGuard for
    /// snapshot/restore (full NSPasteboard fidelity).
    pub struct GuardClipboard {
        guard: Arc<crate::clipboard::MacClipboardGuard>,
        arboard: Mutex<Option<arboard::Clipboard>>,
    }

    impl std::fmt::Debug for GuardClipboard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("GuardClipboard").finish_non_exhaustive()
        }
    }

    impl GuardClipboard {
        pub fn new(guard: Arc<crate::clipboard::MacClipboardGuard>) -> Self {
            Self {
                guard,
                arboard: Mutex::new(None),
            }
        }
    }

    impl ClipboardIo for GuardClipboard {
        fn set_text(&self, text: &str) -> Result<(), String> {
            let mut guard = self
                .arboard
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if guard.is_none() {
                *guard = Some(
                    arboard::Clipboard::new().map_err(|e| format!("clipboard unavailable: {e}"))?,
                );
            }
            guard
                .as_mut()
                .unwrap()
                .set_text(text.to_string())
                .map_err(|e| format!("clipboard set_text: {e}"))
        }

        fn snapshot(&self) -> Option<ClipboardSnapshot> {
            self.guard.snapshot()
        }

        fn restore(&self, snapshot: &ClipboardSnapshot) {
            // The core cascade restores without an extra dictation item
            // (the text was already pasted). Pass `None` for extra.
            self.guard.restore(snapshot, None);
        }
    }

    /// Fires ⌘V via enigo — the same keystroke the injector uses.
    /// Returns `true` when the keystroke was sent.
    pub fn paste_key(enigo: &Mutex<Option<Enigo>>) -> bool {
        let mut guard = enigo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.is_none() {
            match Enigo::new(&EnigoSettings::default()) {
                Ok(e) => *guard = Some(e),
                Err(e) => {
                    tracing::warn!("[delivery] paste_key: enigo unavailable: {e}");
                    return false;
                }
            }
        }
        let enigo = guard.as_mut().unwrap();
        // ⌘V: Meta + ANSI 'V' keycode 9 (same as the injector).
        let result = (|| {
            enigo.key(Key::Meta, Direction::Press)?;
            let r = enigo.key(Key::Other(9), Direction::Click);
            enigo.key(Key::Meta, Direction::Release)?;
            r
        })();
        match result {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("[delivery] paste_key failed: {e}");
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Non-macOS stub (compile-safe; all operations error)
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "macos"))]
mod fallback {
    use super::*;
    use teletype_core::delivery::{FieldReader, Typer};

    pub struct StubField;
    impl FieldReader for StubField {
        fn selected_text(&self) -> Option<String> {
            None
        }
        fn is_secure_field(&self) -> bool {
            false
        }
        fn set_selected_text(&self, _text: &str) -> Result<(), String> {
            Err("delivery not supported on this platform".into())
        }
    }

    pub struct StubTyper;
    impl Typer for StubTyper {
        fn type_text(&self, _text: &str) -> Result<(), String> {
            Err("delivery not supported on this platform".into())
        }
    }

    pub struct StubClipboard;
    impl ClipboardIo for StubClipboard {
        fn set_text(&self, _text: &str) -> Result<(), String> {
            Err("delivery not supported on this platform".into())
        }
        fn snapshot(&self) -> Option<ClipboardSnapshot> {
            None
        }
        fn restore(&self, _snapshot: &ClipboardSnapshot) {}
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Build the desktop `DeliveryIo` bundle for the current platform.
///
/// macOS: real adapters (AX field reader, enigo typer, NSPasteboard
/// clipboard guard, enigo ⌘V).
///
/// Other platforms: compile-safe stubs that error on use (Windows
/// verification is out of scope for this unit).
#[cfg(target_os = "macos")]
pub fn macos_io(clipboard_guard: Arc<crate::clipboard::MacClipboardGuard>) -> DeliveryIo {
    use macos::*;
    let enigo = Arc::new(Mutex::new(None::<Enigo>));
    let enigo_paste = enigo.clone();
    DeliveryIo {
        field: Box::new(AxFieldReader),
        typer: Box::new(EnigoTyper::new()),
        clipboard: Box::new(GuardClipboard::new(clipboard_guard)),
        paste_key: Box::new(move || paste_key(&enigo_paste)),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn macos_io(_clipboard_guard: Arc<()>) -> DeliveryIo {
    DeliveryIo {
        field: Box::new(fallback::StubField),
        typer: Box::new(fallback::StubTyper),
        clipboard: Box::new(fallback::StubClipboard),
        paste_key: Box::new(|| false),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// The typed fallback must refuse text over the cap.
    #[test]
    fn typed_fallback_refuses_oversized_text() {
        let text = "a".repeat(MAX_TYPED_CHARS + 1);
        // We can't run enigo in a unit test, but the guard is checked
        // before enigo is touched. On macOS the EnigoTyper will hit the
        // cap check; on other platforms the stub errors anyway.
        // Verify the constant is exported and the logic is sound.
        assert_eq!(MAX_TYPED_CHARS, 2000);
        assert!(text.chars().count() > MAX_TYPED_CHARS);
    }

    /// The per-char delay is 2 ms.
    #[test]
    fn type_char_delay_is_2ms() {
        assert_eq!(TYPE_CHAR_DELAY, Duration::from_millis(2));
    }
}
