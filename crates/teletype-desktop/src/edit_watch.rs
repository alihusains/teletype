//! Watching what the user does to a dictation after it lands.
//!
//! # Why this exists
//!
//! The personalization loop can only learn from a signal if someone observes
//! the user's edits. `record_dictation_edit` has existed as a Tauri command
//! since the loop was designed, but nothing in the app ever called it, so the
//! loop was dormant: the profile carried explicit preferences the user typed
//! in, and no learned ones.
//!
//! # The observation problem
//!
//! There is no edit event from a text field. Text fields do not notify, and
//! key events are global, not per-field. So the only way to see an edit is to
//! read the field back and compare. That produces two hard requirements:
//!
//! 1. **We must know what we inserted.** A diff needs both sides, so the
//!    inserted text is recorded at injection time.
//! 2. **We must read the field when the user is likely done**, not immediately.
//!    Reading straight after insertion always shows our own text unchanged.
//!    Reading on the *next* dictation is the reliable moment: by then the user
//!    has either finished editing the previous dictation or moved on, and in
//!    both cases the field still holds the text they edited.
//!
//! # Why this is conservative
//!
//! A wrong signal is worse than no signal: it teaches the model a preference
//! the user never expressed, which then shows up in their prose. So every
//! guard below fails closed. If the app is not focused, the value does not
//! contain what we inserted, the field grew by more text than an edit could
//! explain, or the app is Teletype itself, nothing is learned.
//!
//! The one thing this does *not* try to do is track the caret or reconstruct
//! what the user typed versus deleted. It only compares the beginning of the
//! field, because that is the part we wrote.

use std::time::{Duration, Instant};

/// How long after an insertion we are still willing to consider the field
/// edited. Long enough for a human to fix a sign-off, short enough that a
/// field left open overnight is not treated as an edit made in context.
const WATCH_WINDOW: Duration = Duration::from_secs(15 * 60);

/// An insertion we are watching for changes.
#[derive(Debug, Clone)]
pub struct PendingEdit {
    /// The exact text we inserted.
    pub inserted: String,
    /// The app we inserted into, as a normalised application name.
    pub target: String,
    /// When the insertion happened.
    pub at: Instant,
}

/// The most recent insertion, if it is still worth checking.
#[derive(Debug, Clone, Default)]
pub struct EditWatch {
    pending: Option<PendingEdit>,
}

impl EditWatch {
    /// Records an insertion to watch.
    pub fn record(&mut self, inserted: String, target: String) {
        if inserted.trim().is_empty() {
            self.pending = None;
            return;
        }
        self.pending = Some(PendingEdit {
            inserted,
            target,
            at: Instant::now(),
        });
    }

    /// Whether there is something to check, and whether the watch has expired.
    pub fn status(&self) -> EditWatchStatus {
        match &self.pending {
            None => EditWatchStatus::Idle,
            Some(p) if p.at.elapsed() > WATCH_WINDOW => EditWatchStatus::Expired,
            Some(_) => EditWatchStatus::Watching,
        }
    }

    /// Takes the pending insertion, if it is still fresh.
    ///
    /// Returning it means the caller is responsible for calling
    /// [`Self::finish`] with the outcome, so a failed read does not silently
    /// discard the observation.
    pub fn take(&mut self) -> Option<PendingEdit> {
        match self.status() {
            EditWatchStatus::Watching => self.pending.take(),
            _ => {
                self.pending = None;
                None
            }
        }
    }

    /// Clears the watch without recording anything.
    pub fn finish(&mut self) {
        self.pending = None;
    }
}

/// What the watch is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditWatchStatus {
    /// Nothing pending.
    Idle,
    /// An insertion is being watched.
    Watching,
    /// The insertion is too old to attribute an edit to.
    Expired,
}

/// What comparing the field against our insertion tells us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditDiff {
    /// The field still holds exactly what we inserted. Not an edit.
    Unchanged,
    /// The field starts with what we inserted and continues, so the user
    /// appended to it. This is the common case and carries no preference
    /// signal, because we cannot tell an addition from a rewrite.
    Appended,
    /// The field starts with what we inserted up to `at`, then differs. The
    /// text after `at` is the user's edit.
    Edited { at: usize, replacement: String },
    /// The text we inserted is not in the field at all: the user replaced it,
    /// the field was cleared, or focus moved somewhere else. Not learnable.
    Unrecognisable,
}

/// Compares the current field value against what we inserted.
///
/// `value` is the whole field, which may contain text the user had before we
/// inserted. The comparison is anchored on our own text, so a field with prior
/// content still works.
pub fn diff(value: &str, inserted: &str) -> EditDiff {
    if inserted.is_empty() {
        return EditDiff::Unrecognisable;
    }
    let Some(at) = value.find(inserted) else {
        return EditDiff::Unrecognisable;
    };
    let after = &value[at + inserted.len()..];
    if after.is_empty() {
        return EditDiff::Unchanged;
    }
    // Whitespace-only growth is a trailing newline or a space, not an edit.
    if after.trim().is_empty() {
        return EditDiff::Appended;
    }
    EditDiff::Edited {
        at,
        replacement: after.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_field_is_unchanged() {
        assert_eq!(
            diff("The deploy went out.", "The deploy went out."),
            EditDiff::Unchanged
        );
    }

    #[test]
    fn prior_content_before_our_insert_is_ignored() {
        // The field already had text; we inserted at the caret after it.
        let value = "Existing note. The deploy went out.";
        assert_eq!(diff(value, "The deploy went out."), EditDiff::Unchanged);
    }

    #[test]
    fn a_trailing_space_is_not_an_edit() {
        // Every dictation ends with a space when the user keeps typing. Treating
        // that as an edit would manufacture a signal on almost every dictation.
        assert_eq!(
            diff("The deploy went out. ", "The deploy went out."),
            EditDiff::Appended
        );
        assert_eq!(
            diff("The deploy went out.\n", "The deploy went out."),
            EditDiff::Appended
        );
    }

    #[test]
    fn a_rewritten_tail_is_an_edit_carrying_the_replacement() {
        let value = "The deploy went out. Best regards";
        match diff(value, "The deploy went out.") {
            EditDiff::Edited { at, replacement } => {
                // The insertion starts at 0 here, and what the user added after
                // it is the replacement.
                assert_eq!(at, 0);
                assert_eq!(replacement, " Best regards");
            }
            other => panic!("expected Edited, got {other:?}"),
        }
    }

    #[test]
    fn an_edit_after_prior_content_reports_the_anchored_offset() {
        // The field already held text, so the insertion does not start at 0. The
        // offset has to be the real one or the caller rebuilds the wrong text.
        let value = "Existing note. The deploy went out. Best regards";
        match diff(value, "The deploy went out.") {
            EditDiff::Edited { at, replacement } => {
                assert_eq!(at, 15);
                assert_eq!(replacement, " Best regards");
            }
            other => panic!("expected Edited, got {other:?}"),
        }
    }

    #[test]
    fn a_rewritten_sign_off_inside_the_insert_is_found_by_prefix() {
        // The user edited the middle of what we wrote, so the exact inserted
        // string is gone. This must be reported as unrecognisable rather than
        // guessed at, because guessing would teach a wrong preference.
        let value = "The deploy went out. Cheers";
        assert_eq!(
            diff(value, "The deploy went out. Best regards,"),
            EditDiff::Unrecognisable
        );
    }

    #[test]
    fn an_emptied_field_is_unrecognisable() {
        assert_eq!(diff("", "hello"), EditDiff::Unrecognisable);
        assert_eq!(diff("hello", ""), EditDiff::Unrecognisable);
    }

    #[test]
    fn recording_then_checking_reports_watching() {
        let mut w = EditWatch::default();
        assert_eq!(w.status(), EditWatchStatus::Idle);
        w.record("hello".into(), "com.apple.Notes".into());
        assert_eq!(w.status(), EditWatchStatus::Watching);
        let taken = w.take().expect("a fresh watch is takeable");
        assert_eq!(taken.inserted, "hello");
        assert_eq!(taken.target, "com.apple.Notes");
        assert_eq!(w.status(), EditWatchStatus::Idle);
    }

    #[test]
    fn an_empty_insertion_is_never_watched() {
        // Nothing to diff against, so watching would only produce noise.
        let mut w = EditWatch::default();
        w.record("   ".into(), "com.apple.Notes".into());
        assert_eq!(w.status(), EditWatchStatus::Idle);
    }

    #[test]
    fn an_expired_watch_is_dropped_rather_than_reported() {
        let mut w = EditWatch::default();
        w.record("hello".into(), "app".into());
        // Backdate past the window rather than sleeping.
        if let Some(p) = w.pending.as_mut() {
            p.at = Instant::now() - WATCH_WINDOW - Duration::from_secs(1);
        }
        assert_eq!(w.status(), EditWatchStatus::Expired);
        assert!(w.take().is_none());
    }
}
