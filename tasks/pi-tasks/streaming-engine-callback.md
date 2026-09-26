# Task: Wire streaming into the transform engine + Pipeline (forward tokens to a callback)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Why
T1.1. Task A (streaming-trait-provider) adds `generate_with_system_stream` to the
`InferenceProvider` trait (default falls back to batch). THIS task makes the transform
engine use it so tokens flow to a caller callback, and exposes that callback from the
`Pipeline` so the dictation worker can emit them to the pill. Do NOT touch dictation.rs,
the pill, or llm.rs (owned by Task A).

IMPORTANT: This task DEPENDS on Task A's trait method existing. Read `llm.rs` first; if
`generate_with_system_stream` is NOT on the trait, STOP and say so (do not invent it).
This task is dispatched after Task A completes.

## Read first
- `crates/teletype-core/src/llm.rs` — confirm `generate_with_system_stream` exists.
- `crates/teletype-core/src/transforms/engine.rs` — the three `generate_with_system`
  call sites: `run_eg1`, `run_s1`, and the generic path in `run_transform_blocking`.
- `crates/teletype-core/src/pipeline.rs` — `Pipeline` struct (fields) and the
  `engine::run_transform_blocking(...)` call site (~line 199), plus every `Pipeline {`
  struct-literal construction in the `#[cfg(test)]` module (there are ~10).

## What to build

### 1. `engine.rs` — thread an optional token hook through to the streaming call
- Add an `on_token: Option<&dyn FnMut(&str)>` parameter to `run_transform_blocking`
  (and to the internal `run_eg1` / `run_s1` helpers).
- At each of the three `generate_with_system(&system, &user, params)` call sites, switch
  to `generate_with_system_stream(&system, &user, params, &mut |tok| {
      if let Some(f) = on_token { f(tok); }
  })`.
- Keep the returned `raw` (full string) and ALL downstream validation/validator logic
  byte-for-byte identical. Streaming must not change the final text or the validator
  behavior.

### 2. `pipeline.rs` — expose the callback from Pipeline
- Add a field to the `Pipeline` struct:
  ```rust
  /// Optional sink for streaming transform tokens (T1.1). `None` (the default in
  /// tests) means no live preview; the transform still runs and returns the full text.
  pub token_sink: Option<&'a mut dyn FnMut(&str)>,
  ```
  NOTE: this is `&'a mut dyn FnMut(&str)` — a mutable reference. If the lifetime/borrow
  makes the struct awkward (it borrows mutably), an acceptable alternative is
  `pub token_sink: Option<&'a dyn Fn(&str)>` is NOT enough (we need per-token calls) —
  use the `mut` version. If the `&mut` in a struct is too fiddly to construct, use
  `pub token_sink: Option<crate::llm::TokenSink>` where you define a small
  `pub struct TokenSink<'a>(&'a mut dyn FnMut(&str))` newtype to make it ergonomic.
  Pick whichever compiles cleanly and is least invasive; document the choice.
- Update the single `engine::run_transform_blocking(provider, t, &protected.text, &ctx)`
  call site (~line 199) to pass `self.token_sink.as_deref_mut()`.
- Update EVERY `Pipeline { ... }` struct-literal in the `#[cfg(test)]` module to add
  `token_sink: None,` (search for `Pipeline {` — there are ~10). This is mechanical.

### 3. Unit test (engine.rs)
A mock provider whose `generate_with_system_stream` calls `on_token` twice ("Hel" then
"lo"). Assert: both tokens forwarded in order AND the returned full text is "Hello",
AND the validator still accepts it.

## Verification (run these, paste REAL output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype && cargo build -p teletype-core 2>&1 | tail -8`
2. `cargo test -p teletype-core transforms 2>&1 | tail -15`
3. `cargo test -p teletype-core pipeline 2>&1 | tail -10`
4. `cargo build -p teletype-desktop 2>&1 | tail -5`

## Out of scope (do NOT touch)
- `crates/teletype-core/src/llm.rs` (Task A owns the trait).
- `crates/teletype-inference/**` (Task A owns the provider).
- `crates/teletype-desktop/src/dictation.rs` (I wire the worker inline).
- Any `ui/**` file (I wired the pill inline).
