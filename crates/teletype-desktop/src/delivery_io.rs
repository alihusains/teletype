//! P8.1-P8.4 desktop delivery adapters.
//!
//! Implements the `teletype_core::delivery` traits against the platform
//! services this crate already has:
//!
//! - `FieldReader` — focused field text + secure-field detection + set-value,
//!   reusing `ax_text` functions.
//! - `Typer` — enigo `fast_text` (CGEvent unicode-string posts: safe from
//!   worker threads), capped at 2 000 chars.
//! - `ClipboardIo` — `arboard` for set-text; `MacClipboardGuard` (NSPasteboard)
//!   for faithful snapshot/restore.
//! - `paste_key` — ⌘V via enigo, the same keystroke the injector uses.
//!
//! macOS only. Non-macOS builds get compile-safe stubs that error on use so
//! the crate still compiles cross-platform.

use std::sync::Arc;
use std::sync::Mutex;

#[cfg(target_os = "macos")]
use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};

use teletype_core::delivery::{ClipboardIo, DeliveryIo, FieldReader, Typer};
use teletype_core::injector::ClipboardSnapshot;

/// The typed fallback is only worth it for shortish text; beyond this the
/// cascade goes straight to the set-value fallback. Mirrors the core constant.
const MAX_TYPED_CHARS: usize = 2000;

/// Item 3 (`settings.keep_text_on_clipboard`): what the restore should leave
/// on the pasteboard as an extra item, or `None` for a clean restore.
///
/// The core cascade calls [`ClipboardIo::restore`] with no arguments, so this
/// is the whole decision, kept pure (no pasteboard, no app) so the rule is
/// unit-testable: retention needs the setting on *and* non-empty text. Empty
/// text is excluded because `MacClipboardGuard` would write a blank item at
/// the back of the stack, and macOS reads the *last* item on a plain ⌘V, so a
/// blank "kept" entry is a paste that silently inserts nothing.
///
/// Deliberately NOT a second gate on whether to restore: `restore_clipboard`
/// is ignored here on purpose. See the note on the macOS `GuardClipboard`
/// adapter for why that setting has nothing to gate.
pub fn clipboard_retention(keep_text: bool, text: &str) -> Option<&str> {
    if keep_text && !text.is_empty() {
        Some(text)
    } else {
        None
    }
}

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
            if text.is_empty() {
                return Ok(());
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
            // enigo 0.6.1 `fast_text` posts plain text as CGEvent
            // unicode-string events (safe off-main), BUT when a chunk starts
            // with `\n`, `\r`, or `\t` it routes that character through
            // `key()` -> `keycode_to_string` -> TSMGetInputSourceProperty,
            // which is a main-thread-only TextServices API. Called from this
            // worker thread it asserts (`_dispatch_assert_queue_fail`,
            // SIGTRAP, whole-process crash) — crash report 2026-10-19
            // (teletype-2026-10-19-091041.ips) is exactly that path, reached
            // via the typed fallback on newline-containing dictation.
            //
            // A typed fallback cannot meaningfully reproduce a paragraph
            // break anyway, so normalize line breaks to a single space
            // before posting. That removes every character that would send
            // `fast_text` down the crashing `key()` path, and it is the
            // least-surprising mapping for text that had to fall back to
            // typing in the first place.
            let safe: String = text
                .chars()
                .map(|c| match c {
                    '\n' | '\r' | '\u{2028}' | '\u{2029}' => ' ',
                    other => other,
                })
                .collect();
            enigo
                .fast_text(safe.as_str())
                .map(|_| ())
                .map_err(|e| format!("type failed: {e}"))
        }
    }

    /// Clipboard adapter: arboard for set_text, MacClipboardGuard for
    /// snapshot/restore (full NSPasteboard fidelity).
    ///
    /// Module note on the two clipboard settings, since both are persisted and
    /// only one is honored here:
    ///  * `restore_clipboard` has no effect to honor. It is copied into
    ///    `teletype_core::pipeline::Pipeline` and never read there (the field
    ///    is write-only in the whole crate); the cascade in
    ///    `teletype_core::delivery::deliver()` snapshots and restores
    ///    unconditionally, and `MacClipboardGuard` refuses to clear a
    ///    pasteboard it cannot read back. So "put the user's copy back" is not
    ///    a user-selectable behaviour, it is the safety invariant this module
    ///    exists to enforce, and it stays unconditional.
    ///  * `keep_text_on_clipboard` is therefore the only real knob, and it is
    ///    implemented as the already-built extra-item mechanism
    ///    (`MacClipboardGuard::restore` with `extra`, i.e.
    ///    `ClipboardSnapshot::restored_with`): the user's items go back in
    ///    their original order and the dictation is appended last, so it
    ///    reaches the clipboard history instead of being destroyed by the
    ///    restore. That is exactly what the field's own doc comment promises.
    ///    Gating the restore itself would drop the user's previous copy, and
    ///    gating it *and* retention would give one behaviour two switches.
    pub struct GuardClipboard {
        guard: Arc<crate::clipboard::MacClipboardGuard>,
        arboard: Mutex<Option<arboard::Clipboard>>,
        /// Item 3: `settings.keep_text_on_clipboard`, read per delivery (the
        /// caller builds a fresh `DeliveryIo` for each take).
        keep_text: bool,
        /// The text most recently handed to `set_text`, which is what the
        /// restore republishes as the extra item. `deliver()` gives the
        /// adapter no way to pass it through `restore`, so it travels on the
        /// adapter: `set_text` and `restore` are the same cascade's pair, in
        /// that order, and `restore` clears it so a later delivery can't
        /// retain a stale take's text.
        last_text: Mutex<String>,
    }

    impl std::fmt::Debug for GuardClipboard {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("GuardClipboard").finish_non_exhaustive()
        }
    }

    impl GuardClipboard {
        pub fn new(guard: Arc<crate::clipboard::MacClipboardGuard>, keep_text: bool) -> Self {
            Self {
                guard,
                arboard: Mutex::new(None),
                keep_text,
                last_text: Mutex::new(String::new()),
            }
        }
    }

    impl ClipboardIo for GuardClipboard {
        fn set_text(&self, text: &str) -> Result<(), String> {
            *self
                .last_text
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = text.to_string();
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
            let text = std::mem::take(
                &mut *self
                    .last_text
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
            // Item 3: the user's own items always go back; `keep_text` only
            // decides whether the dictation is appended as one more.
            self.guard
                .restore(snapshot, clipboard_retention(self.keep_text, &text));
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
///
/// `keep_text` is `settings.keep_text_on_clipboard` for this delivery: the
/// caller builds a fresh bundle per take, so the setting is read live rather
/// than cached. The rule itself is [`clipboard_retention`]; on macOS the
/// `GuardClipboard` adapter carries the extra item through the restore.
#[cfg(target_os = "macos")]
pub fn macos_io(
    clipboard_guard: Arc<crate::clipboard::MacClipboardGuard>,
    keep_text: bool,
) -> DeliveryIo {
    use macos::*;
    let enigo = Arc::new(Mutex::new(None::<Enigo>));
    let enigo_paste = enigo.clone();
    DeliveryIo {
        field: Box::new(AxFieldReader),
        typer: Box::new(EnigoTyper::new()),
        clipboard: Box::new(GuardClipboard::new(clipboard_guard, keep_text)),
        paste_key: Box::new(move || paste_key(&enigo_paste)),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn macos_io(_clipboard_guard: Arc<()>, _keep_text: bool) -> DeliveryIo {
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

    /// The typed fallback must refuse text over the cap before constructing
    /// an Enigo or touching the platform input layer.
    #[cfg(target_os = "macos")]
    #[test]
    fn typed_fallback_refuses_oversized_text() {
        let typer = super::macos::EnigoTyper::new();
        let err = typer
            .type_text(&"a".repeat(MAX_TYPED_CHARS + 1))
            .unwrap_err();
        assert!(
            err.contains(&MAX_TYPED_CHARS.to_string()),
            "the cap error should name the cap: {err}"
        );
    }

    /// Item 3: the setting alone decides retention ...
    #[test]
    fn retention_follows_the_setting() {
        assert_eq!(
            clipboard_retention(true, "the dictation"),
            Some("the dictation")
        );
        assert_eq!(clipboard_retention(false, "the dictation"), None);
    }

    /// ... except for empty text, which would become a blank newest item and
    /// turn the user's next plain ⌘V into a paste that inserts nothing.
    #[test]
    fn retention_never_keeps_empty_text() {
        assert_eq!(clipboard_retention(true, ""), None);
    }
}
