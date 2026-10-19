//! Fuzz target for the SSE frame parser (`SseFrameParser`).
//!
//! Goals:
//! 1. No panic on any byte sequence — including truncated frames, invalid
//!    JSON payloads, arbitrary UTF-8 / non-UTF-8 sequences, and embedded
//!    null bytes.
//! 2. Chunking invariant: splitting the same input at every possible byte
//!    offset produces the same output as a single push.
//!    A violation means the parser's pending-buffer logic has a bug.
//!
//! Run with:
//!   cargo fuzz run fuzz_sse_parser   (from crates/teletype-inference/)
//!
//! The fuzz target uses `#[cfg(fuzzing)]` to access the private parser via
//! `teletype_inference::server::fuzz_surface`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use teletype_inference::server::fuzz_surface;

fuzz_target!(|data: &[u8]| {
    // 1. Single-push must not panic.
    let reference = fuzz_surface::parse_sse_once(data);

    // 2. Every two-way split must give the same result as a single push.
    //    Check a small set of split positions to keep each corpus entry fast.
    //    cargo-fuzz drives many corpus entries per second, so O(n) splits per
    //    entry is fine for short inputs (the typical SSE frame is < 200 bytes).
    let len = data.len();
    for split_at in [0, len / 4, len / 2, 3 * len / 4, len] {
        let split_result = fuzz_surface::parse_sse_split(data, split_at);
        // Both must match the single-push reference.
        assert_eq!(
            reference, split_result,
            "chunking invariant violated at split {split_at}/{len}"
        );
    }
});
