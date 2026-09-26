# Task: T2.1 — Wire the personalization feedback loop (re-dispatch)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Paste the REAL output of every verification command in your final report, not prose.
- Note: a previous dispatch of this task may have left partial work behind. Run
  `git status` and read the files below FIRST; if any of the listed work already
  exists in the working tree, verify it and finish the gaps instead of redoing it.

## Files you may edit
- `crates/teletype-desktop/src/commands.rs` (new `record_dictation_edit` command)
- `crates/teletype-desktop/src/lib.rs` (register the command)
- `crates/teletype-core/src/transforms/engine.rs` (add `input_text: Option<String>` to
  `TransformResult` ONLY if not already present)
- `crates/teletype-core/src/pipeline.rs` (expose the raw ASR input on the pipeline
  result ONLY if not already present)

## Why
`extract_signals` / `apply_signals` in `crates/teletype-core/src/personalization/learn.rs`
ALREADY EXIST and are fully tested, but have ZERO production callers. This is the
"it just knows how I talk" differentiator (roadmap T2.1): when a user edits the
polished text the app inserted, the app learns the user's preferred greeting /
sign-off / terminology and feeds it into future polish prompts.

## Read first
1. `crates/teletype-core/src/personalization/learn.rs` (whole file):
   - `Signal` struct (lines 19-34): fields `key`, `description`, `phrase`, `scope`.
   - `extract_signals(ai_output, final_text, app) -> Vec<Signal>` (line 36).
   - `apply_signals(profile, signals) -> Vec<String>` (line 108): takes
     `&mut UserProfile`, returns changed preference ids.
2. `crates/teletype-core/src/personalization/mod.rs`: `UserProfile` (line 108),
   how `Preference` is stored.
3. `crates/teletype-desktop/src/commands.rs`:
   - `set_profile_settings` (~line 748) and the `state.profile` /
     `state.profile_store.save(...)` pattern used at lines 714-807 — copy it.
   - `save_settings` (~line 277) for the command signature style
     (`app: AppHandle, state: State<'_, AppState>, ...`).
   - Line 676: `let app_ctx = state.platform.active_application().unwrap_or_default();`
     — the `ApplicationContext` that `extract_signals` needs.
4. `crates/teletype-desktop/src/lib.rs`: the `tauri::generate_handler![...]` list
   (~line 460) where commands are registered.

## Build
1. New command in `commands.rs`:

       #[tauri::command]
       pub async fn record_dictation_edit(
           state: State<'_, AppState>,
           ai_output: String,
           final_text: String,
       ) -> CommandResult<Vec<String>>

   - If `ai_output == final_text` (or either is empty), return `Ok(vec![])`
     without touching the profile.
   - Respect the profile gates: only learn if `profile.learn_from_edits` is true
     (and for app-scoped signals, `profile.learn_app_specific`). Read the
     `UserProfile` fields to confirm the exact gate names; if `learn_terminology`
     exists, gate terminology signals on it too.
   - `let app_ctx = state.platform.active_application().unwrap_or_default();`
   - `let signals = extract_signals(&ai_output, &final_text, &app_ctx);`
   - Lock `state.profile`, `apply_signals(&mut profile, &signals)`,
     `state.profile_store.save(&profile)?`, return the changed ids.
2. Register the command in `lib.rs` `generate_handler`.
3. (Only if missing) Thread the raw ASR text as `input_text` through
   `TransformResult` so the UI can send it back. Check first: if
   `TransformResult` already carries the input, skip this step.

## Out of scope
- No UI changes (the UI call-site is a separate task).
- No changes to `learn.rs` (it is tested and correct).
- No changes to dictation.rs, injector, or the pipeline worker.

## Verification (paste real output)
1. `cargo test -p teletype-core --lib 2>&1 | tail -3`
2. `cargo test -p teletype-desktop --lib 2>&1 | tail -3`
3. `cargo build -p teletype-desktop 2>&1 | tail -3`
4. Add at least one unit test in `commands.rs` (or the closest testable seam)
   proving: identical texts return no signals; a sign-off swap produces a
   preference. If `commands.rs` has no test module yet, test the pure part by
   calling `extract_signals` + `apply_signals` directly in a test in
   `learn.rs`-style, and say so in the report.
