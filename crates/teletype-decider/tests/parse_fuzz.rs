//! Proptest: `parse_tool_calls` never panics on arbitrary input.

use proptest::prelude::*;
use teletype_decider::parse_tool_calls;

proptest! {
    #[test]
    fn parse_never_panics_ascii(s in "[ -~]{0,200}") {
        // May return Err — that's fine. Must not panic.
        let _ = parse_tool_calls(&s);
    }

    #[test]
    fn parse_never_panics_unicode(s in "[^\0]{0,200}") {
        let _ = parse_tool_calls(&s);
    }

    #[test]
    fn parse_never_panics_raw(
        bytes in proptest::collection::vec(proptest::arbitrary::any::<u8>(), 0..500),
    ) {
        // Lossy conversion; the point is to feed weird bytes through.
        let s = String::from_utf8_lossy(&bytes);
        let _ = parse_tool_calls(&s);
    }
}
