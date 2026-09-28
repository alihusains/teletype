//! Reproduction for the personalization-loop index bug (finding TT-01 / BUG-10).
//!
//! ## The contract
//!
//! `edit_watch::diff(value, inserted)` returns
//! `EditDiff::Edited { at, replacement }` only when the **entire** `inserted`
//! string is still present verbatim in the field, starting at byte offset `at`,
//! and `replacement` is everything the field holds after it (the user's edit).
//! `PendingEdit::inserted` is documented as "The exact text we inserted."
//!
//! So the user's edited version of the dictation is `inserted + replacement`,
//! equivalently `value[at..]`. The field offset `at` is *not* part of it.
//!
//! ## The bug
//!
//! `dictation.rs:1291` writes:
//!
//! ```ignore
//! let user_text = format!("{}{}", &pending.inserted[..at], replacement);
//! ```
//!
//! `at` indexes `value` (the whole field) and is then used to slice
//! `inserted` (Teletype's own text). `at` is routinely larger than
//! `inserted.len()`, and routinely lands inside a multi-byte character.
//! Two panics, both reachable in ordinary use:
//!
//! 1. `at > inserted.len()`        -> "byte index N is out of bounds"
//! 2. `at` splits a multi-byte char -> "byte index N is not a char boundary"
//!
//! When `at == 0` (the insertion is at the start of the field) there is no
//! panic, but the result is `replacement` alone: the learner is handed the
//! user's tail and never sees the words that were dictated.
//!
//! ## Why it is S1 rather than a crash-and-restart
//!
//! The call sits inside the per-event `catch_unwind` (`dictation.rs:95-100`),
//! which only logs "event handler panicked". `observe_pending_edit` runs
//! *before* `self.phase = Phase::Transforming` (`dictation.rs:871` vs `880`),
//! so `self.phase` is still the `Phase::Transcribing` set in `stop()`, and
//! `next_id` is already bumped. The pipeline worker is never spawned, so
//! `PipelineDone` can never arrive to advance the phase, and no input can
//! leave `Transcribing` (`state.rs:101` returns `Nothing` for `HotkeyDown`,
//! `state.rs:131` for `Toggle`). The user loses the dictation outright: no
//! injection, no history entry, no archive line, and a pill stuck on
//! "Transcribing..." until they force-quit.
//!
//! macOS Notes, Mail and TextEdit all expose `focused_text()` through
//! `AXTextArea`, so the success path of `focused_text()` is the common case,
//! not a corner.
//!
//! ## The fix
//!
//! One line, and `at` disappears:
//!
//! ```ignore
//! let user_text = format!("{}{}", pending.inserted, replacement);
//! ```

use teletype_desktop_lib::edit_watch::{diff, EditDiff};

/// The exact expression from `dictation.rs:1291`.
fn rebuild_as_shipped(inserted: &str, at: usize, replacement: &str) -> String {
    format!("{}{}", &inserted[..at], replacement)
}

/// The expression the module's own comment describes.
fn rebuild_correct(inserted: &str, replacement: &str) -> String {
    format!("{inserted}{replacement}")
}

// ---------------------------------------------------------------------------
// 1. no panic, but the dictated words are thrown away
// ---------------------------------------------------------------------------

#[test]
fn bug_10_at_offset_zero_the_learner_never_sees_the_dictated_words() {
    // Dictating into an empty field, then fixing a word. This is the single
    // most common shape of all: a fresh paragraph.
    let inserted = "the deploy went out on friday";
    let field = format!("{inserted} on Monday instead");

    let EditDiff::Edited { at, replacement } = diff(&field, inserted) else {
        panic!("fixture must produce an Edited diff");
    };
    assert_eq!(at, 0, "the insertion is at offset 0 of an empty field");

    let shipped = rebuild_as_shipped(inserted, at, &replacement);
    assert_eq!(
        shipped, " on Monday instead",
        "BUG-10: the learner is handed only the user's edit, so the whole \
         dictation is invisible to the personalization loop"
    );
    assert!(
        !shipped.contains("deploy"),
        "BUG-10: the dictated content was dropped from the learnable text"
    );

    // The correct expression keeps both halves.
    let correct = rebuild_correct(inserted, &replacement);
    assert!(
        correct.contains("deploy"),
        "correct rebuild keeps the dictation"
    );
    assert!(correct.contains("Monday"), "correct rebuild keeps the edit");
}

// ---------------------------------------------------------------------------
// 2. panic: byte index out of bounds
// ---------------------------------------------------------------------------

#[test]
fn bug_10_panics_out_of_bounds_when_the_caret_is_mid_field() {
    // The user clicks into the middle of an existing paragraph -- the single
    // most ordinary thing to do -- and dictates there.
    let before = "The deploy is going out on Friday ";
    let inserted = "tomorrow please";
    let after = " as planned.";
    let field = format!("{before}{inserted}{after}");

    let EditDiff::Edited { at, replacement } = diff(&field, inserted) else {
        panic!("fixture must produce an Edited diff");
    };
    assert_eq!(at, before.len(), "`at` indexes the whole field");

    let res = std::panic::catch_unwind(|| rebuild_as_shipped(inserted, at, &replacement));
    assert!(
        res.is_err(),
        "expected the shipped expression to panic: at={at}, inserted.len()={}, \
         but it returned Ok",
        inserted.len()
    );

    // Same field, correct expression: no panic, right answer.
    let correct = rebuild_correct(inserted, &replacement);
    assert_eq!(correct, "tomorrow please as planned.");
}

// ---------------------------------------------------------------------------
// 3. panic: not a char boundary
// ---------------------------------------------------------------------------

#[test]
fn bug_10_panics_when_the_offset_splits_a_multi_byte_character() {
    // The dictation contains an emoji (4 bytes). Any offset in 9..12 lands
    // inside it, so the slice is in range but not on a boundary.
    let inserted = "ship it \u{1F680} now";
    let emoji_start = "ship it ".len();
    assert_eq!(emoji_start, 8, "the emoji starts at byte 8");

    // Prior content long enough that `at` lands inside the emoji: bytes 0..9.
    let before = "x".repeat(emoji_start + 1);
    let field = format!("{before}{inserted} please");
    assert!(
        !inserted.is_char_boundary(before.len()),
        "fixture must put the offset inside the emoji"
    );

    let EditDiff::Edited { at, replacement } = diff(&field, inserted) else {
        panic!("fixture must produce an Edited diff");
    };
    assert_eq!(at, before.len());

    let res = std::panic::catch_unwind(|| rebuild_as_shipped(inserted, at, &replacement));
    assert!(
        res.is_err(),
        "expected a char-boundary panic: at={at} is inside the 4-byte emoji"
    );
}

// ---------------------------------------------------------------------------
// 4. the whole matrix, so the fix cannot be partial
// ---------------------------------------------------------------------------

#[test]
fn bug_10_the_correct_expression_is_total_over_every_field_layout() {
    let cases = [
        ("", "hello world", " please"),
        ("prefix ", "hello world", "!"),
        (
            "a much longer prefix than the insertion itself ",
            "hi",
            " there",
        ),
        ("x", "\u{1F680}\u{1F680} launch", " go"),
        ("trailing", "\u{4F60}\u{597D}", " \u{4E16}\u{754C}"),
    ];
    for (before, inserted, after) in cases {
        let field = format!("{before}{inserted}{after}");
        let EditDiff::Edited { at, replacement } = diff(&field, inserted) else {
            panic!("{field:?} must produce an Edited diff");
        };
        assert_eq!(at, before.len());

        let correct = rebuild_correct(inserted, &replacement);
        assert_eq!(
            correct,
            format!("{inserted}{after}"),
            "correct rebuild for before={before:?}"
        );
        assert!(
            correct.contains(inserted.chars().next().expect("non-empty insertion")),
            "the learnable text must retain the dictated content"
        );
    }
}
