#![no_main]
//! Fuzz `teletype_core::learning::diff::diff_words` on two arbitrary strings.
//!
//! Properties checked per (pasted, observed) pair:
//! - crash-freedom on arbitrary UTF-8 (invalid bytes are filtered through
//!   `from_utf8_lossy` first, since the API takes `&str`);
//! - determinism: the same pair always yields the same ops;
//! - identical inputs produce only `Equal` ops;
//! - empty pasted produces no `Delete` ops.

use libfuzzer_sys::fuzz_target;
use teletype_core::learning::diff::{diff_words, EditOp};

fuzz_target!(|data: &[u8]| {
    // Split the input into two strings at the midpoint; use lossy
    // conversion so invalid UTF-8 does not abort the target.
    let mid = data.len() / 2;
    let pasted = String::from_utf8_lossy(&data[..mid]);
    let observed = String::from_utf8_lossy(&data[mid..]);
    let (pasted, observed): (&str, &str) = (&pasted, &observed);

    let a = diff_words(pasted, observed);
    let b = diff_words(pasted, observed);
    assert_eq!(a, b, "diff_words must be deterministic");

    // Identical inputs: only Equal ops.
    let same = diff_words(pasted, pasted);
    assert!(
        same.iter().all(|op| matches!(op, EditOp::Equal(_))),
        "identical inputs must produce only Equal ops: {same:?}"
    );

    // Empty pasted: nothing can be deleted.
    let from_empty = diff_words("", observed);
    assert!(
        from_empty.iter().all(|op| !matches!(op, EditOp::Delete(_))),
        "empty pasted must not produce Delete ops: {from_empty:?}"
    );
});
