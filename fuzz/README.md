# Fuzzing (P0.5 tooling)

Requires nightly plus the `cargo-fuzz` runner (instrumentation only works
on nightly):

```sh
rustup toolchain install nightly
cargo install cargo-fuzz
cargo +nightly fuzz run itn_normalize -- -max_total_time=300
```

## Targets

| Target | Function under test | Status |
|---|---|---|
| `itn_normalize` | `teletype_core::itn::normalize` (public pure function) | wired; `cargo +nightly fuzz build` passes |
| `sse_reader` | SSE frame parser in `crates/teletype-inference/src/server.rs` (`SseFrameParser`) | **pending on P1.10**: the parser is currently private to `teletype-inference` and P1.10 rewrites its buffering (UTF-8 split fix). Wiring the target now would mean either testing a copy (proves nothing) or a visibility-only edit to `server.rs`, which the inference owner is editing concurrently for the EG-1 catalog removal. Drop-in harness once it is reachable: |

```rust
#![no_main]
use libfuzzer_sys::fuzz_target;
// Needs `SseFrameParser` (or a `pub(crate)`-plus-re-export test shim) to be public.
fuzz_target!(|data: &[u8]| {
    // Split `data` at two arbitrary points and require identical output:
    // random chunk boundaries (including mid-codepoint) must not change
    // the decoded deltas. This is the same property as the P1.10 proptest.
});
```

Until then the fallback is the proptest/insta coverage added as
`teletype-core` dev-dependencies by this task, plus the existing unit test
`sse_frame_parser_streams_across_chunk_boundaries` in `server.rs`.
