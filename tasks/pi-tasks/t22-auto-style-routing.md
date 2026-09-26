# Task: T2.2 — Per-app auto-style routing (context-aware default transform)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.

## Files you may edit
- `crates/teletype-core/src/style.rs` (per-app overrides on the style profile)
- `crates/teletype-core/src/context/mod.rs` or wherever `ApplicationContext` lives
  (match/bundle helpers ONLY if needed)
- `crates/teletype-desktop/src/commands.rs` (UI-facing get/set for per-app overrides)
- `crates/teletype-desktop/src/lib.rs` (register new commands)
- `crates/teletype-core/src/pipeline.rs` (resolution order ONLY if the transform is
  chosen there)

## Why
Roadmap T2.2 (Tier 2, "It just knows"): Gmail → email polish, Slack → casual,
automatically. Context (`ApplicationContext` with app name/bundle id) and style
profiles already exist; what is missing is the auto-routing: a per-app default
that applies when the user has NOT explicitly picked a transform/style for this
dictation.

## Read first
1. `crates/teletype-core/src/style.rs` (whole file, 191 lines): the
   `StyleProfile` shape, how the active style id is stored, and what a style
   contains.
2. `crates/teletype-core/src/context/` (or `context.rs`): `ApplicationContext`
   fields (app name, bundle id, etc.).
3. `crates/teletype-desktop/src/commands.rs` lines 140-150 (the `active_style_id`
   setting) and lines 1726+ (`update_style_profile`): the existing style command
   patterns to copy.
4. `crates/teletype-core/src/pipeline.rs`: where the transform/style is selected
   before the engine runs (search for `style` and `transform`).

## Build
1. Add a per-app override map to the style/profile layer, e.g.
   `app_style_overrides: BTreeMap<String, String>` mapping a normalized app key
   (lowercased app name) to a style profile id (or transform id — follow whatever
   the pipeline actually consumes; if both exist, style id is the right level).
   Keep it in the same store as the styles so it persists with them.
2. Resolution order (document it in a doc comment at the call site):
   explicit user selection (this dictation) > per-app override for the current
   frontmost app > user's global active style > default.
   The per-app override must NOT override an explicit per-dictation selection.
3. Commands: `set_app_style_override(app_key, style_id)` (empty string removes)
   and `get_app_style_overrides()`. Register both in `lib.rs`.
4. In the pipeline/dictation path that picks the style, consult the override
   using `state.platform.active_application()` (see commands.rs:676 for the
   pattern).

## Out of scope
- No UI (the per-app picker is a follow-up; the data layer + commands are this task).
- No LLM-based register detection (that is P5.2, a later task).
- No changes to the transform engine or prompts.

## Verification (paste real output)
1. `cargo test -p teletype-core --lib 2>&1 | tail -3`
2. `cargo test -p teletype-desktop --lib 2>&1 | tail -3`
3. `cargo build -p teletype-desktop 2>&1 | tail -3`
4. Unit tests (in the module where the resolution lives):
   - override for app A applies when A is frontmost
   - explicit selection beats the override
   - unknown app falls through to global default
   - overrides survive a store save/load round-trip
