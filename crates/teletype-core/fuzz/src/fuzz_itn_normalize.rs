//! Fuzz target for `itn::normalize` on arbitrary string inputs.
//!
//! Goals:
//! 1. No panic on any valid UTF-8 string.
//! 2. Idempotence: `normalize(normalize(s)) == normalize(s)`.
//!    A violation means the normalizer is modifying already-normalized text,
//!    which would cause visible changes on a second dictation pass.
//!
//! Run with: `cargo fuzz run fuzz_itn_normalize`

#![no_main]

use libfuzzer_sys::fuzz_target;
use teletype_core::itn::normalize;

fuzz_target!(|data: &[u8]| {
    // `normalize` takes a `&str`, so only process valid UTF-8.
    let Ok(s) = std::str::from_utf8(data) else { return };

    let once = normalize(s);
    let twice = normalize(&once);

    // Idempotence: normalizing an already-normalized string must be a no-op.
    // Use `assert_eq!` rather than a conditional: a violation must produce a
    // libfuzzer failure (non-zero exit) so the corpus entry is saved for replay.
    assert_eq!(once, twice, "itn::normalize is not idempotent on {s:?}");
});
