#![no_main]
//! Fuzz the deterministic ITN pass (`teletype_core::itn::normalize`).
//!
//! Properties checked per input:
//! - crash-freedom on arbitrary bytes (valid and invalid UTF-8);
//! - determinism: the same input always yields the same output (ITN is
//!   documented as a pure value transform; a mismatch means hidden global
//!   state crept in).

use libfuzzer_sys::fuzz_target;
use teletype_core::itn;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let a = itn::normalize(s);
        let b = itn::normalize(s);
        assert_eq!(a, b, "ITN must be deterministic");
    }
});
