# BUG-014 — chat_stream returns a successful partial result if the server dies after the first token

**Severity:** P2
**Area:** inference / server.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `server.rs:391-435`

---

## Description

If `llama-server` crashes after emitting a few SSE frames, `read()` returns
0 (EOF), the loop breaks, and `chat_stream` (`server.rs:391-435`) returns
`Ok(full)` with whatever partial text arrived.

The engine's validator may accept a truncated-but-plausible sentence, and
the user gets a **silently cut-off rewrite with no error**. (The
non-streaming `chat()` path has the same shape but is less likely to be
truncated mid-sentence since it parses a complete JSON body.)

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-inference/src/server.rs` | 391-435 | `chat_stream` read loop — EOF after partial frames returns `Ok` |

## Reproduction

1. Start a dictation with a local model and streaming on.
2. Kill the `llama-server` process mid-generation (while the pill is
   showing words).
3. The pill stops, and the final injected text is a truncated sentence with
   no error surfaced.

## Unit test cases (must pass after fix)

```rust
// 1. A stream that EOFs before [DONE] must be an error, not Ok(partial).
#[test]
fn stream_eof_without_done_is_error() {
    let mut client = FakeStreamServer::emitting(vec![
        sse("Hello"),
        sse(" world"),
        // then EOF — no [DONE]
    ]);
    let res = client.chat_stream("prompt");
    assert!(res.is_err(),
        "EOF without [DONE] must be Err, got Ok({:?})", res.ok());
}

// 2. A normal stream that completes with [DONE] is still Ok.
#[test]
fn stream_with_done_is_ok() {
    let mut client = FakeStreamServer::emitting(vec![
        sse("Hello"),
        sse(" world"),
        sse_done(),
    ]);
    let out = client.chat_stream("prompt").unwrap();
    assert_eq!(out, "Hello world");
}

// 3. A mid-stream child death surfaces a failure the engine can fall back on.
#[test]
fn mid_stream_death_surfaces_fallback_error() {
    let mut client = FakeStreamServer::dies_after_frames(2);
    let res = client.chat_stream("prompt");
    assert!(res.is_err());
    // Engine contract: Err => fall back to raw input.
}
```

## Acceptance criteria

- [ ] A stream that ends without `[DONE]` (server died) returns `Err`,
      triggering the engine's fallback-to-input.
- [ ] A normal stream (with `[DONE]`) still completes with `Ok`.
- [ ] The user never gets a silently truncated rewrite.

## How to test (manual / smoke)

1. **Smoke (2 min):** start a long polish, kill `llama-server` mid-stream.
   - **Pass:** the dictation falls back to the raw input (not a truncated
     rewrite) and an error is logged.
   - **Fail:** the injected text is a cut-off sentence and the logs show a
     successful completion.

2. **Regression (1 min):** a normal polish still streams and completes.
   - **Pass:** full rewrite injected, no errors.
   - **Fail:** normal streams now error or stop early.

## Fix direction

In `chat_stream`, track whether `[DONE]` was received. If the read loop hits
EOF (`read() == 0`) **without** `[DONE]`, return
`Err("stream ended before [DONE]")` instead of `Ok(full)`. The engine
already treats `Err` as fallback-to-input, so this is a small change:

```rust
let mut done = false;
loop {
    // ... parse SSE frame ...
    if data == "[DONE]" { done = true; break; }
    if n == 0 { break; } // EOF
}
if !done {
    return Err(Error::StreamEndedBeforeDone);
}
Ok(full)
```

## Related

- BUG-013 (server death is more likely when orphan/lifecycle handling is broken)
- G015 (silent server)
