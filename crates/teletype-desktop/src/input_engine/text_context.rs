//! TextContext / TextInserter: the shared abstraction over the focused field.
//!
//! Both engines end here: `ShortcutManager` (selection transforms) and
//! `AutoTextManager` (trigger replacement) read through [`TextContext`] and
//! write through [`TextInserter`]. Future features reuse the same two types
//! instead of growing their own AX/clipboard code.
//!
//! Write order is always direct-first:
//! 1. AX replace/insert (no clipboard involved, nothing to preserve).
//! 2. Clipboard + paste fallback with preserve/restore (only here is the
//!    clipboard touched at all).

use teletype_core::platform::PasteShortcut;

/// True when `id`/`name` identify Teletype itself. AutoText never expands in
/// our own windows (the native tap already skips them; this is the Rust-side
/// half so a forwarded event can never expand while settings has focus).
pub fn is_self_app(id: &str, name: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    id == "com.teletype.app" || name == "teletype" || id.contains("teletype")
}

/// The focused field's selected text, if any.
pub fn selected_text() -> Option<String> {
    crate::text_selection::selected_text()
}

/// Insert `text` over the current selection. Returns true when it landed.
pub fn insert_over_selection(text: &str) -> bool {
    crate::text_selection::insert(text)
}

/// Replace the just-typed trigger (`trigger_units` UTF-16 units before the
/// caret) with `replacement` via one AX replace primitive. Returns true when
/// the write landed (exact or app-normalised).
pub fn replace_trigger_ax(trigger_units: usize, replacement: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::ax_text::replace_before_caret(trigger_units, replacement).landed()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (trigger_units, replacement);
        false
    }
}

/// Fallback used only when the AX write refuses: backspace the trigger (plus
/// the delimiter when one completed it), then paste the replacement through
/// the shared injector, which snapshots and restores the clipboard around the
/// paste. Returns true when the paste was queued.
pub fn backspace_then_paste(
    backspaces: usize,
    replacement: &str,
    injector: &teletype_core::injector::TextInjector,
) -> bool {
    use enigo::{Direction, Key, Keyboard};
    let Ok(mut enigo) = enigo::Enigo::new(&enigo::Settings::default()) else {
        tracing::warn!("text-inserter: enigo init failed");
        return false;
    };
    for _ in 0..backspaces {
        if enigo.key(Key::Backspace, Direction::Click).is_err() {
            tracing::warn!("text-inserter: backspace failed");
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    std::thread::sleep(std::time::Duration::from_millis(30));
    injector.inject(
        replacement.to_string(),
        true,
        false,
        true,
        Some(PasteShortcut::CommandV),
    );
    true
}

/// UTF-16 length of `s`, the unit AX ranges use.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_self_app() {
        assert!(is_self_app("com.teletype.app", "Teletype"));
        assert!(is_self_app("x", "Teletype"));
        assert!(!is_self_app("com.apple.Notes", "Notes"));
        assert!(!is_self_app("", ""));
    }

    #[test]
    fn utf16_counts_surrogate_pairs_as_two() {
        assert_eq!(utf16_len("a"), 1);
        assert_eq!(utf16_len("😀"), 2);
        assert_eq!(utf16_len("/home-address"), 13);
    }
}
