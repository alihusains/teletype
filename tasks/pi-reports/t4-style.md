# T4 Style features: fix + simplify report

Worktree: `/Users/a.sorathiya/Documents/Ali/teletype-wt/t4-style`, branch `t7/t4-style`.
Diagnosis: `tasks/pi-reports/t4-style-diagnosis.md` (written first, per the task's
sequencing rule).

## 1. What changed, and why (file by file)

| File | Change | Reason |
|---|---|---|
| `crates/teletype-core/src/transforms/engine.rs` | +2 tests: `style_phrases_reach_the_prompt_for_a_style_capable_model`, `a_different_style_changes_the_prompt_for_a_style_capable_model` (new `StyleCapableRecorder` mock, model id `fast` = `StylePhrases` channel) | The task's core ask: prove a style phrase actually reaches the prompt for a style-capable model, and that switching style changes the prompt bytes. No production code changed - the injection path was already correct. |
| `crates/teletype-core/src/pipeline.rs` | +2 assertions in `unknown_app_type_keeps_global_active_style` | Proves the resolved style's phrases land in the model-visible `USER PREFERENCES (apply these):` block at pipeline level, not just engine level. |
| `crates/teletype-core/src/transforms/validator.rs` | `words()` made `pub(crate)` with a doc comment (no behaviour change) | The test's mock output is validated through the same accounting the production path uses; the helper is now shared instead of private. |
| `ui/src/screens/StylesScreen.tsx` | Rewrote the three "this model ignores styles" messages (Band A / Band B / Band C) so each fact is stated once; Band C notice collapsed from a full bordered section to one line; tooltip shortened; winning cascade rung no longer repeats its note (rank + In-use pill already say it) | The user's "too much duplicates": the same three facts appeared up to four times. All functionality kept: cascade ladder, effective-phrase chips, compare pane, styles list, create form, Tone/Structure/Context band. 867 -> 852 lines. |
| `ui/src/test/stylesModelCapability.test.tsx` | 2 expectations updated to the new wording | The tests pin the user-visible honesty strings; the strings changed, the behaviour they guard (fixed-behaviour models never shown as applying styles; control-line models pointed at the Tone/Structure/Context band) is unchanged. |

## 2. Key decision for the lead

**The "not applying" is correct behaviour for the shipped default, not a broken
injection path** (full evidence in the diagnosis report):

- `s1-mini` (the recommended/default model, catalog.rs:316) is `ControlLine` +
  `cleanup_only`. Its Polish card prompt never carries `ctx.preferences`
  (engine.rs:448, 530-548); non-Polish presets are skipped as `ModelNotCapable`
  (engine.rs:455-472). So no style can apply on the default model by design.
- `qwen3.5-2b` Polish also uses the card path (styles apply only outside Polish).
- `speakoflow-mini` is `FixedBehaviour`: nothing user-settable reaches its prompt.
- Every other model (`fast`, `quality`, `gwen2.5-3b`, `gemma2-2b`, any
  openai-compat id) is `StylePhrases`: phrases reach the prompt via
  `pipeline.rs:559` -> `build_prompt`'s `USER PREFERENCES` block
  (prompt.rs:335-341). That path is now covered by the two new engine tests.

So Part B was scoped to (a) the missing proof (tests) and (b) page honesty +
dedup, per the task's sequencing rule.

**Flag for the lead - file I needed but do NOT own:**
`crates/teletype-desktop/src/commands/personalization.rs`.
`preview_style_profile` (line ~1002) builds a `PromptContext` with the style's
phrases and runs the production *auto-apply* transform. For `qwen3.5-2b` with the
default auto-apply Polish, production uses the card path (phrases never reach the
prompt), but the preview's transform selection can diverge from production's
routing, so a Compare run can show a difference that dictation would not produce
(or vice versa when a non-Polish auto-apply transform is enabled). Fixing it
means making the preview use the same per-model routing the engine uses. Left
untouched - outside my ownership list.

## 3. Verification (real output)

### `cargo test -p teletype-core style`
```
test style::tests::a_bundle_id_override_resolves_against_a_bundle_id ... ok
test style::tests::app_override_applies_when_app_is_frontmost ... ok
test style::tests::built_ins_present_and_active_phrases_resolve ... ok
test style::tests::builtin_cannot_be_deleted_custom_can ... ok
test style::tests::category_default_precedence ... ok
test style::tests::control_line_shows_for_the_models_that_read_it ... ok
test style::tests::default_style_maps_app_categories ... ok
test style::tests::explicit_selection_beats_app_override ... ok
test style::tests::explicit_selection_outranks_every_override_key ... ok
test style::tests::model_style_serialises_camel_case ... ok
test style::tests::only_instruction_following_models_apply_styles ... ok
test style::tests::reset_restores_modified_builtin ... ok
test style::tests::set_override_requires_existing_profile_and_empty_removes ... ok
test style::tests::style_table_covers_every_card_model ... ok
test style::tests::unknown_app_falls_through_to_global_default ... ok
test style::tests::unknown_app_type_keeps_active_style ... ok
test transforms::prompt::tests::static_prefix_matches_default_list_style_build ... ok
test transforms::engine::tests::style_phrases_reach_the_prompt_for_a_style_capable_model ... ok
test transforms::engine::tests::a_different_style_changes_the_prompt_for_a_style_capable_model ... ok
test style::tests::overrides_survive_store_save_load_round_trip ... ok
test pipeline::tests::unknown_app_uses_global_active_style_in_pipeline ... ok
test pipeline::tests::app_style_override_applies_when_app_is_frontmost ... ok
test pipeline::tests::explicit_style_beats_app_override_in_pipeline ... ok
test pipeline::tests::unknown_app_type_keeps_global_active_style ... ok
test pipeline::tests::explicit_style_beats_override_and_category_default ... ok
test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 1068 filtered out; finished in 0.05s
(+ journey_14c_a_style_override_for_one_app_does_not_leak ... ok in integration targets)
EXIT: 0
```

### `cargo test -p teletype-core` (full)
```
test result: ok. 1106 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.34s
test result: ok. 62 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.25s
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.09s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.21s
test result: ok. 1 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Doc-tests teletype_core: ok. 0 passed
EXIT: 0
```
(0 failures across all targets.)

### `cd ui && npx tsc --noEmit`
```
(no output)
EXIT: 0
```

### Styles vitest
```
npx vitest run src/test/stylesModelCapability.test.tsx src/__tests__/style-measurement.test.ts
 Test Files  2 passed (2)
      Tests  12 passed (12)
```

### `ls -la /Users/a.sorathiya/Documents/Ali/teletype/tasks/pi-reports/t4-style-diagnosis.md`
```
-rw-r--r--  1 a.sorathiya  ICM\Domain Users  11090 Nov  8 22:38 /Users/a.sorathiya/Documents/Ali/teletype/tasks/pi-reports/t4-style-diagnosis.md
```

## 4. What I could NOT do

- Fix `preview_style_profile`'s prompt-path divergence (file not in my ownership
  list; flagged above for the lead).
- Change the AI Polish prompt-override feature (Task 3) or the model catalog /
  downloads (out of scope).

## 5. Left uncommitted

Working-tree edits only, as required. `git diff --stat`:
```
crates/teletype-core/src/pipeline.rs             |  6 ++
crates/teletype-core/src/transforms/engine.rs    | 95 ++++++++++++++++++++++++
crates/teletype-core/src/transforms/validator.rs |  6 +-
ui/src/screens/StylesScreen.tsx                  | 53 +++++--------
ui/src/test/stylesModelCapability.test.tsx       |  6 +-
5 files changed, 128 insertions(+), 38 deletions(-)
```
