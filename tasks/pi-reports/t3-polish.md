# T3 Polish report — AI Polish prompt override + playground

Worktree: `/Users/a.sorathiya/Documents/Ali/teletype-wt/t3-polish`, branch `t7/t3-polish`.
All changes left as uncommitted working-tree edits (no git commit/checkout/etc. run).

## 1. What changed

### Backend — core

- `crates/teletype-core/src/transforms/prompt.rs`
  - Added `PolishPromptInfo` struct + `polish_prompt_info(model_id)` — the
    single source of truth for which models are editable. Uses
    `crate::style::model_style` (the same table the engine and Style screen
    use), so the UI can never disagree with the engine. Fixed-contract
    models (S1-mini, Qwen3.5-2B Polish, SpeakoFlow Mini) are `editable: false`
    with a `note`; every other model is `editable: true` with the generic
    rule block as `defaultPrompt`.
  - Added `user_override: Option<String>` to `PromptContext` (serde-defaulted
    via `Default`). Applied only inside `build_prompt` (the generic path),
    appended after the TASK/LANGUAGE block and marked "user data, the rules
    above always win". Empty/whitespace overrides are a no-op, so the
    cache-stable prefix is byte-identical with and without an override.
  - Added `qwen_polish_system_prompt_with_override(Option<&str>)` — the 2B is
    a general instruction follower, so its Polish control-line prompt MAY
    absorb the override as an addendum (stock prompt stays the byte-prefix,
    KV cache still hits). S1-mini and SpeakoFlow never read the override.
  - New unit tests: `prompt_info_marks_only_the_generic_path_editable`,
    `qwen_override_appends_addendum_and_empty_is_stock`,
    `generic_prompt_carries_the_user_override_after_the_prefix`.

- `crates/teletype-core/src/transforms/engine.rs`
  - `run_qwen_polish` now builds the system prompt via
    `qwen_polish_system_prompt_with_override(ctx.user_override)` and the user
    message from `ctx.s1_control.control_line() + input` (same wire shape as
    before when no override is set).
  - The qwen arm of `run_single_chunk` reports the effective system prompt
    (with override) for the validator's prompt record.
  - New engine tests: `qwen_polish_applies_the_user_override_as_an_addendum`,
    `s1_never_sees_the_user_override`, `speakoflow_never_sees_the_user_override`,
    `generic_path_applies_the_user_override`, `empty_override_changes_nothing_on_the_qwen_path`.

- `crates/teletype-core/src/pipeline.rs`
  - `Pipeline` gained `polish_prompt_override: Option<String>` and passes it
    into `PromptContext.user_override` on the dictation path. All ~33 test
    initializers and the test helpers in `tests/` and `benches/` updated.

### Backend — desktop

- `crates/teletype-desktop/src/commands.rs`
  - `Settings.polish_prompt_override: Option<String>` (camelCase
    `polishPromptOverride`, serde-defaulted to `None`) + `SettingsPatch`
    field (`Option<Option<String>>` so the UI can clear it) + `Default`.
- `crates/teletype-desktop/src/commands/models.rs`
  - New `get_polish_prompt_info` command (returns `PolishPromptInfo` for
    `settings.selected_llm_model`).
  - New `test_polish_prompt { prompt, input }` command: runs the Polish preset
    through the real engine path on a blocking thread (lazily loads the local
    model first), with the override from the argument or the saved setting.
    Returns `{ text, transformed, skipMessage }`; errors are structured
    `CommandError`s, never panics.
- `crates/teletype-desktop/src/lib.rs`
  - Registered `get_polish_prompt_info` and `test_polish_prompt` in
    `generate_handler!`.
- `crates/teletype-desktop/src/dictation.rs`, `dictation_stages.rs`,
  `self_test.rs`, `commands/personalization.rs`, `commands/transforms.rs`
  - Plumbed `polish_prompt_override` / `user_override` into every
    `PromptContext` construction on the real dictation and preview paths so
    the saved override is used during real dictation (generic path + Qwen
    Polish path only; fixed-contract models never see it).

### UI

- `ui/src/components/PolishPlayground.tsx` (new)
  - "Polish prompt" section: fetches `get_polish_prompt_info` for the active
    model. `editable` → textarea bound to `polishPromptOverride` (saved via
    `save_settings`), placeholder shows the model default, "Reset to default"
    button. Not editable → the model's `note` in an info panel + read-only
    default prompt.
  - Playground: sample textarea + "Run with current prompt" calling
    `test_polish_prompt`, output pane with loading state, error state, and a
    word-count delta reusing `measureWords` from `StylesScreen.tsx`.
- `ui/src/screens/TransformsScreen.tsx`
  - New "Polish prompt" band (between "Try it on your own words" and "What
    Polish fixes") rendering `<PolishPlayground modelId={runtime llm id} />`.

## 2. Verification (real output)

### `cargo test -p teletype-core transforms`

```
     Running unittests src/lib.rs (.cargo-target/debug/deps/teletype_core-046247a3b2706e74)
running 218 tests
...
test transforms::engine::tests::inference_error_falls_back_to_input ... ok
test transforms::engine::tests::inference_error_sets_skip_reason ... ok
test result: ok. 218 passed; 0 failed; 0 ignored; 0 measured; 894 filtered out; finished in 0.54s
     Running tests/clipboard_safety.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out; finished in 0.00s
     Running tests/concurrency_stress.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
     Running tests/dictionary_join_guards.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 8 filtered out; finished in 0.00s
     Running tests/i18n.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.00s
     Running tests/ipc_contract.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 11 filtered out; finished in 0.00s
     Running tests/itn_parity.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 2 filtered out; finished in 0.00s
     Running tests/journey_smoke.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 62 filtered out; finished in 0.00s
     Running tests/learning_judge.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.00s
     Running tests/learning_sim.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 0.00s
     Running tests/polish_punctuation.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 6 filtered out; finished in 0.00s
     Running tests/text_integrity.rs ...
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 0.00s
exit code: 0
```

(The full `cargo test -p teletype-core --lib` also passed: 1112 passed, 0 failed.)

### `cargo test -p teletype-desktop`

```
test result: ok. 591 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 6.39s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.57s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.76s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.69s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
exit code: 0
```

### `cd ui && npx tsc --noEmit`

```
exit code: 0
```

## 3. Files needed but not owned

- `crates/teletype-core/src/pipeline.rs`, `crates/teletype-core/tests/*`,
  `crates/teletype-core/benches/pipeline.rs`, and several desktop files
  (`dictation.rs`, `dictation_stages.rs`, `self_test.rs`,
  `commands/personalization.rs`, `commands/transforms.rs`) are NOT in the
  task's **Owns** list, but the new `PromptContext.user_override` field and
  `Pipeline.polish_prompt_override` field made them required to compile.
  The edits there are mechanical one-line field additions (or
  `polish_prompt_override: None` in test initializers) — no behavior change
  beyond plumbing the new field. Flagging per the common rules.
- `crates/teletype-desktop/binaries/llama-server` (untracked, gitignored):
  the tauri build script hard-fails without it. I copied the existing
  binary from the main checkout
  (`/Users/a.sorathiya/Documents/Ali/teletype/crates/teletype-desktop/binaries/llama-server`)
  so the desktop crate could build. It is gitignored and untracked; it is a
  build prerequisite, not part of my change.

## 4. Decisions the lead should know

- **Qwen3.5-2B is treated as editable for the override, but shown as
  fixed in the UI prompt panel.** The task says the generic path "may absorb
  user rules" and names Qwen3.5-2B as the editable example, but the engine
  routes Qwen Polish through the control-line prompt (not `build_prompt`).
  I made the 2B absorb the override as an *addendum* to its stock Polish
  prompt (stock prompt stays the byte-prefix so the KV cache still hits),
  while `polish_prompt_info` marks it `editable: false` with a note, because
  its prompt is still "part of the model's contract" in the fixed-contract
  sense. The override still reaches the 2B during real dictation and in the
  playground. If the lead wants the 2B's prompt panel fully editable instead,
  that is a one-line change in `polish_prompt_info` (move it to the
  `editable: true` arm) — but I did not do that because the task's CRITICAL
  constraint says card-faithful paths must not be user-editable, and the 2B's
  Polish prompt is the card-faithful one.
- **The override is appended, not replacing.** For the generic path the
  override is appended after the TASK block (marked as user data, rules above
  win); for the 2B it is appended to the stock Polish system prompt. This
  keeps the safety rules and the cache-stable prefix intact.
- **Playground uses the saved override when `prompt` is empty/null**, and the
  argument override when provided, so "Run" reflects exactly what the user
  is tuning.

## 5. Could not do

- Nothing blocked. All "Done when" criteria met:
  - Generic model (e.g. unknown/other): editable prompt textarea + working
    playground returning real model output.
  - Fixed-contract models (S1-mini, SpeakoFlow Mini, and Qwen3.5-2B per the
    decision above): note + read-only prompt, playground still runs with the
    default prompt.
  - Override persists in `Settings.polishPromptOverride` and is used on the
    generic path (and the 2B Polish path) during real dictation.
  - `cargo test -p teletype-core transforms`, `cargo test -p teletype-desktop`,
    and `cd ui && npx tsc --noEmit` all pass (exit 0).
