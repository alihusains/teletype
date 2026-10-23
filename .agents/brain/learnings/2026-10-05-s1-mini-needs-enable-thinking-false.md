# L013: S1-mini returns empty output unless the Qwen3 thinking mode is disabled

**ID:** L013
**Date:** 2026-10-05
**Agent:** Kilo (Qwen)
**Project area:** teletype-inference (llama-server spawn) / teletype-core (transform engine)
**Status:** verified
**Evidence:** reproduced live against the running S1 server (empty `content`, `finish_reason: stop`, text in `reasoning_content`); fixed server with `--chat-template-kwargs '{"enable_thinking":false}'` returns clean text for the same request; model card `superwhisper/s1-mini-GGUF` documents the requirement verbatim; regression test `s1_mini_gets_enable_thinking_false_other_models_get_none` in `crates/teletype-inference/src/server.rs`

## Problem

S1-mini (a Qwen3-0.6B fine-tune) returned an empty string for every polish request, so every dictation took fell back to raw text with the toast "Polish failed (S1 generation failed). Using raw text" while the status card said "S1-mini is loaded and will clean up your dictation."

## Symptoms

- Status showed S1-mini loaded and ready.
- Every take produced "Polish failed (S1 generation failed). Using raw text".
- The server was healthy (HTTP 200, `finish_reason: stop`), so it looked like a model quality issue rather than a configuration bug.

## Initial hypotheses

1. Model file corrupt (wrong; sha256 matches the catalog entry).
2. Timeout/stall (wrong; responses arrived in ~100 ms).
3. Prompt format off-distribution (wrong; system prompt + control line match the card exactly).
4. Template/thinking-mode mismatch (right).

## Investigation

- Queried the running server with the exact app request shape: `content` was empty; the cleaned text appeared in `reasoning_content` instead.
- Tried control-line variants (all four styling values, prose/lists, with/without system prompt): always empty or `content: "<think>\n"`.
- Fetched the S1-mini GGUF model card. It says: the embedded chat template is Qwen3's, which defaults to thinking mode; S1-mini was trained with thinking off, so the assistant turn must start with an empty think block, or "you will get no usable output at all: the model emits an empty think block and stops. This is the single most common way to get a blank result from this model." In llama.cpp that means `--jinja` together with `--chat-template-kwargs '{"enable_thinking":false}'`. The app spawned with `--jinja` but not the kwargs.

## Root cause

`llama-server` was spawned without `--chat-template-kwargs '{"enable_thinking":false}'`. The Qwen3 template then opens the assistant turn in thinking mode; S1-mini, trained with thinking off, emits an empty think block and stops. The OpenAI-compatible response has empty `content`, which the engine treats as a generation failure.

## Fix

- `crates/teletype-inference/src/server.rs`: `ServerProvider` now carries `chat_template_kwargs` derived from the model id (`s1-mini` → `{"enable_thinking":false}`, everything else → `{}`). Applied at spawn (`--chat-template-kwargs`) and injected into every `/v1/chat/completions` body (batch + streaming) via `apply_chat_template_kwargs`, so the choice survives server-version quirks.
- `crates/teletype-core/src/transforms/engine.rs`: `run_s1`/`run_eg1` now return `Result<String, String>` and propagate the provider's real error text; the toast now reads e.g. "S1 generation failed: llama-server returned empty output" instead of an opaque constant.
- `crates/teletype-desktop/src/commands.rs` + `crates/teletype-core/src/llm.rs`: new `InferenceProvider::is_alive()` (default `true`; `ServerProvider` overrides via `Child::try_wait`). The status chip reports "error: model server stopped" instead of "loaded" when the process is dead, so "loaded" is always truthful.

## Verification

- `cargo test -p teletype-core -p teletype-inference` (389 + 48 tests) and `-p teletype-desktop --lib` (73 tests) all pass; clippy clean on the changed crates.
- Live: restarted the dev app; the spawned server command line shows `--chat-template-kwargs {"enable_thinking":false}`; a streaming request through it returns the full cleaned transcript.

## Reusable lesson

For Qwen3-based GGUF models served by llama.cpp, "loaded" (health check passes) is not "usable": the chat template's thinking default must match how the model was trained. When a model returns empty `content` with `finish_reason: stop`, check `reasoning_content` first and verify the template kwargs against the model card before suspecting weights or prompt format.
