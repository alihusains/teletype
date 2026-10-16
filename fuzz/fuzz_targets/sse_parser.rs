#![no_main]
//! Fuzz the incremental SSE frame parser
//! (`teletype_inference::server::SseFrameParser`).
//!
//! The parser buffers raw bytes across arbitrary chunk boundaries, so the
//! fuzzer feeds each input in random-sized slices and asserts:
//! - crash-freedom on arbitrary bytes (valid and invalid UTF-8);
//! - chunking-independence: any partition of the same byte stream yields
//!   the same sequence of `data:` payloads. A mismatch means the parser's
//!   buffering logic is order-of-delivery-dependent, i.e. it would corrupt
//!   a real streaming response.

use libfuzzer_sys::fuzz_target;
use teletype_inference::server::SseFrameParser;

/// Feed `data` to the parser in slices of `chunk` bytes (1..=len) and
/// collect every payload it yields. `chunk == 0` means "one push".
fn parse_all(data: &[u8], chunk: usize) -> Vec<String> {
    let mut parser = SseFrameParser::new();
    let mut out: Vec<String> = Vec::new();
    let step = if chunk == 0 { data.len().max(1) } else { chunk };
    let mut start = 0;
    while start < data.len() {
        let end = (start + step).min(data.len());
        out.extend(parser.push(&data[start..end]));
        start = end;
    }
    if data.is_empty() {
        out.extend(parser.push(b""));
    }
    out
}

fuzz_target!(|data: &[u8]| {
    // A deterministic pseudo-random chunk size derived from the input:
    // different inputs exercise different chunkings, and the same input
    // always exercises the same chunking (reproducible crashes).
    let chunk = (data.iter().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(*b as u32)) % 7) as usize;
    let a = parse_all(data, chunk);
    let b = parse_all(data, 1); // byte-at-a-time: the finest chunking
    assert_eq!(a, b, "SSE parsing must not depend on chunk boundaries");
});
