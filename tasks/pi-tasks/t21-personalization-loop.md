# Task: T2.1 — Wire the personalization feedback loop

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  any sibling reference tree.
- Files you may edit:
  - `crates/teletype-core/src/transforms/engine.rs` (add `input_text` to `TransformResult`)
  - `crates/teletype-core/src/transforms/validator.rs` (if `TransformResult` is constructed here)
  - `crates/teletype-desktop/src/commands.rs` (new command + wire the loop)
  - `crates/teletype-desktop/src/lib.rs` (register the command)
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
`extract_signals` / `apply_signals` in `crates/teletype-core/src/personalization/learn.rs`
ALREADY EXIST and are fully tested, but have ZERO production callers. This is the "it just
knows how I talk" differentiator: when a user edits the polished text the app inserted, the
app should learn the user's preferred greeting / sign-off / terminology and feed it into
future polish prompts. This is roadmap T2.1 (Tier 2, "It just knows").

## Read first
1. `crates/teletype-core/src/personalization/learn.rs` (whole file):
   - `Signal` struct (lines 19-34): fields `key`, `description`, `phrase`, `scope`.
   - `extract_signals(ai_output, final_text, app) -> Vec<Signal>` (line 36): compares the AI's
     polished output with the user's final edited text; returns greeting / sign-off /
     terminology swap signals.
   - `apply_signals(profile, signals) -> Vec<String>` (line 108): creates/bumps preferences on
     a `UserProfile`, returns changed preference ids.
2. `crates/teletype-core/src/personalization/mod.rs`:
   - `UserProfile` (line 108) and how `Preference` is stored. `apply_signals` takes
     `&mut UserProfile`.
3. `crates/teletype-desktop/src/commands.rs`:
   - Lines 700-810: the existing personalization commands (`get_profile`, `add_preference`,
     `set_profile_settings`, etc.). They use `state.profile` (a `Mutex<UserProfile>`) and
     `state.profile_store.save(&*profile)`. Mirror this.
   - Find how a `Tauri` command is declared (`pub fn ... -> CommandResult<...>`) and how
     `State<'_, AppState>` is used.
4. `crates/teletype-desktop/src/dictation.rs` around lines 1016-1035: where `result.final_text`
   is saved to history. The pipeline result `result` is a `PipelineResult` with
   `raw_input`, `final_text`, `transform: Option<TransformResult>`.
5. `crates/teletype-core/src/transforms/engine.rs` lines 64-70: `TransformResult` currently has
   `text` (post-transform), `transformed`, `metrics`. It does NOT keep the pre-transform
   input. You need the AI's polished output to diff against the user's edit.

## What to build
1. **Capture the AI's polished output.** Add a field `pub input_text: String` to
   `TransformResult` (the text as it entered the transform, i.e. what the model was asked to
   polish). Populate it at every `TransformResult { .. }` construction site in `engine.rs` and
   `validator.rs`. This is the `ai_output` for `extract_signals` (the model's cleaned version
   before the user edited it).
2. **New command `record_dictation_edit(ai_output: String, final_text: String) -> CommandResult<usize>`**
   in `commands.rs`:
   - Take the focused app context (use the same context the pipeline used; if only a default
     `ApplicationContext` is available, use that).
   - `let signals = extract_signals(&ai_output, &final_text, &ctx);`
   - If `signals.is_empty()`, return `Ok(0)`.
   - Lock `state.profile`, `let changed = apply_signals(&mut profile, &signals);`
   - `state.profile_store.save(&*profile)?;`
   - Return `Ok(changed.len())`.
3. **Register `record_dictation_edit`** in `lib.rs` `generate_handler!`.
4. **Unit test** in `learn.rs` (or a new `#[cfg(test)]` in `commands.rs` if you prefer, but
   `learn.rs` already has tests) proving: given an `ai_output` that says "Dear Alice" and a
   `final_text` that says "Hi Alice", `extract_signals` yields a greeting signal, and
   `apply_signals` adds a preference. (A test for the full command isn't required; the pure
   functions are the unit under test. The command is thin wiring.)

## Out of scope
- Do NOT build the "user edits the inserted text" OS-level text-watch (reading back the
  document the text was pasted into). That is a separate, larger task. For now the loop is
  exposed as a command `record_dictation_edit` that the app (or a future edit-watcher) calls
  with the AI output and the user's final text. The pure learning is what T2.1 is about.
- Do NOT change `extract_signals` / `apply_signals` heuristics; they are tested and correct.

## Verification (paste REAL output)
1. `cargo test -p teletype-core --lib personalization 2>&1 | tail -15` — all pass, incl. your new test.
2. `cargo build -p teletype-desktop 2>&1 | tail -5` — must compile.
3. `cargo clippy -p teletype-core -p teletype-desktop 2>&1 | tail -15` — no new warnings.
4. `git diff --stat` — show exactly which files changed.
