# Task: Add streaming generation to the InferenceProvider trait + ServerProvider (SSE)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Why
T1.1 (the #1 differentiator vs WisprFlow): we currently run a full-batch LLM transform
then paste the whole result (3-8s perceived). We need token streaming so the UI can
show text landing ~1s after the user stops talking. This task adds the streaming
capability to the inference layer. The pill/preview wiring is a SEPARATE task — do
NOT touch the UI or dictation.rs in this task.

## Read first (before writing any code)
- `crates/teletype-core/src/llm.rs` — the `InferenceProvider` trait, `GenerationParams`.
- `crates/teletype-inference/src/server.rs` — `ServerProvider`, its `chat()` method
  (currently sends `"stream": false`, parses a single JSON body). This is the main file.
- `crates/teletype-inference/src/openai_compat.rs` — the other provider; it must keep
  compiling via the trait's default impl (do not break it).
- `crates/teletype-core/src/transforms/engine.rs` — to see how `generate_with_system`
  is called (so the new method signature stays compatible).

## What to build

### 1. `crates/teletype-core/src/llm.rs` — add a streaming method to the trait
Add this method to `InferenceProvider` with a DEFAULT impl that falls back to the
existing batch `generate` (so every existing provider — ServerProvider's openai-compat
sibling, mocks, the LlamaProvider stub — keeps compiling unchanged):

```rust
/// Streams generated text, invoking `on_token` for each token as it arrives.
///
/// The default implementation runs the batch `generate` and delivers the whole
/// result in one callback, so providers that don't support streaming still work.
/// Providers that can stream (e.g. the local llama-server over SSE) override this
/// to call `on_token` per token. Returns the full concatenated text on success,
/// or an error (in which case `on_token` may have been called for partial output).
fn generate_stream(
    &self,
    prompt: &str,
    params: GenerationParams,
    on_token: &mut dyn FnMut(&str),
) -> Result<String, String> {
    let out = self.generate(prompt, params)?;
    on_token(&out);
    Ok(out)
}

/// Streaming variant of `generate_with_system` — same contract, chat-style.
fn generate_with_system_stream(
    &self,
    system: &str,
    user: &str,
    params: GenerationParams,
    on_token: &mut dyn FnMut(&str),
) -> Result<String, String> {
    let _ = system;
    self.generate_stream(user, params, on_token)
}
```

Add a unit test in `llm.rs` proving the default `generate_stream` calls the callback
exactly once with the full batch output (use a small mock provider).

### 2. `crates/teletype-inference/src/server.rs` — implement real SSE streaming
Add to `impl InferenceProvider for ServerProvider` (or a helper): override
`generate_with_system_stream` (and `generate_stream`) to do a real streaming request:

- Build the same JSON body as `chat()` but with `"stream": true`.
- POST to `{base_url}/v1/chat/completions` with the bearer key.
- Use a `reqwest::blocking::Client` (NO overall `.timeout()` on the client for the
  streaming call, or a generous one; instead enforce `params.timeout` as a deadline
  checked between tokens).
- Set header `Accept: text/event-stream`.
- Read the response body as a byte stream. Parse Server-Sent Events: lines starting
  with `data: `. Each `data:` payload is a JSON chunk of shape
  `{"choices":[{"delta":{"content":"..."}}]}` (llama.cpp / OpenAI-compatible). Extract
  `choices[0].delta.content`; when present and non-empty, call `on_token(delta)`.
- The stream ends with `data: [DONE]`.
- Accumulate all deltas into a `String`; return it.
- On HTTP error / network error: return Err (keep the existing one-retry-on-5xx spirit,
  but a retry mid-stream is not required; a single attempt is acceptable for V1).
- Respect `params.timeout` as a wall-clock deadline: if exceeded between tokens,
  return Err.

**Implementation note:** `reqwest::blocking::Response` exposes `.bytes()` (whole body)
but for streaming you want incremental reads. The cleanest approach with the blocking
client is to read the raw body in a loop. `reqwest::blocking::Response` does NOT expose
an incremental reader directly, so use this pattern: set the client to NOT buffer, and
read via the response's underlying stream is awkward in blocking mode.

**Recommended approach:** use `reqwest`'s blocking client but read the body incrementally
by calling `.chunk()` is not available in blocking. Instead, the simplest correct
approach is to use the **non-blocking** `reqwest` client + a tiny local async runtime
is overkill. So use this instead:

Open the request and read the body in a streaming manner using the `bytes` crate is
overkill. The pragmatic, dependency-free approach that works with the existing
`reqwest::blocking` dependency: read the whole response with `.bytes()` but parse it
as a sequence of SSE frames and call `on_token` per frame as you parse. This still
delivers tokens incrementally to the *callback* as parsing proceeds (the network is
already complete, but the callback fires per token in order, which is what the UI
needs for the progressive UX). Accept this for T1.1; true network-level streaming is
a follow-up. Document this tradeoff in a code comment.

If you find `reqwest` (the version in Cargo.lock) offers a blocking incremental body
reader, use it for true streaming and note it. Check `crates/teletype-inference/Cargo.toml`
for the reqwest features.

Add a unit test that, given a fake SSE body string, the SSE-parsing helper extracts the
right deltas in order and stops at `[DONE]`. Make the SSE parser a small pure function
(`fn parse_sse_deltas(body: &str) -> Vec<String>`) so it is unit-testable without a server.

## Verification (run these, paste REAL output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo build -p teletype-core -p teletype-inference 2>&1 | tail -8`
2. `cargo test -p teletype-core llm 2>&1 | tail -12`
3. `cargo test -p teletype-inference 2>&1 | tail -15`
4. `cargo build -p teletype-desktop 2>&1 | tail -5` (confirms the engine still compiles against the new trait)

## Out of scope (do NOT touch)
- `crates/teletype-core/src/transforms/engine.rs` (a separate task wires the callback).
- `crates/teletype-desktop/src/dictation.rs` (separate task emits the token event).
- Any UI file (`ui/**`).
- `crates/teletype-inference/src/openai_compat.rs` (it must compile via the default impl;
  do not edit it unless it fails to compile, and if it does, fix minimally).
