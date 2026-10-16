#![no_main]
//! A.4 `fuzz_decoder_output`: the decoder's JSON parse must never panic
//! on arbitrary token strings.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = teletype_decider::parse_tool_calls(s);
    }
});
