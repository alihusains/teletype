# T4 Style features: why they appear not to apply

Worktree: `/Users/a.sorathiya/Documents/Ali/teletype-wt/t4-style`, branch `t7/t4-style`.
Date: 2026-07-11. Author: pi (t7-batch).

## 1. The exact mechanism, per model channel

`crates/teletype-core/src/style.rs` owns one table, `model_style` (style.rs:272-296),
read by both the engine and the Style screen so they cannot disagree:

| model id | channel | polishes_from_card | cleanup_only |
|---|---|---|---|
| `s1-mini` | `ControlLine` | true | true |
| `qwen3.5-2b` | `ControlLine` | true | false |
| `speakoflow-mini` | `FixedBehaviour` | true | true |
| anything else (incl. `""`, `fast`, `quality`, `qwen2.5-3b`, …) | `StylePhrases` | false | false |

The shipped default model is `s1-mini` (`crates/teletype-inference/src/catalog.rs:316`,
"the recommended model leads the list", catalog.rs:625).

Per channel, what a style can actually do (all in
`crates/teletype-core/src/transforms/engine.rs`, `run_single_chunk`):

1. **`StylePhrases`** (any general instruct model): the resolved style's phrases are
   appended to `PromptContext.preferences` by the pipeline
   (`crates/teletype-core/src/pipeline.rs:559`) and `build_prompt` renders them into
   the model prompt as a `USER PREFERENCES (apply these):` block
   (`crates/teletype-core/src/transforms/prompt.rs:335-341`). Every style has a
   visible effect, for every transform.

2. **`ControlLine`** (`s1-mini`, `qwen3.5-2b`): for the Polish preset the engine
   bypasses `build_prompt` entirely and sends a fixed card message whose first user
   line is the control line `[Styling: …] [Structure: …] [Context: …]`
   (engine.rs:448 `is_card_cleanup`; engine.rs:530-548 dispatch;
   `prompt.rs:106-119` `build_control_line_messages`). The control line is built from
   `ctx.s1_control` only — **`ctx.preferences` is never read on this path**.
   `run_qwen_polish` (engine.rs:290-300) and `run_s1` (engine.rs:270-282) both take
   only `input` + `ctx.s1_control`. So the *style list* never reaches these models;
   what reaches them is the Tone / Structure / Context setting.
   `qwen3.5-2b` additionally keeps `StylePhrases` behaviour for every *non-Polish*
   transform (engine.rs:284-289 comment; `cleanup_only: false`), so styles apply
   there outside Polish.

3. **`FixedBehaviour`** (`speakoflow-mini`): the card protocol is a fixed system
   prompt + the bare transcript, no control line, no wrapper
   (`prompt.rs:199-201` `build_speakoflow_messages`). Nothing the user can set in
   Style reaches the prompt.

The pipeline itself is correct: `pipeline.rs:545-561` resolves the style id
(`default_style_for_app` → `resolve_style_id` with bundle id tried before name) and
pushes every phrase of the winning profile into `style` (the preferences vector)
with dedupe. The phrases reach `run_transform_blocking` for every model. It is the
*engine's per-model routing* that drops them for the two card Polish paths.

The Style screen consumes `style_resolution`
(`crates/teletype-desktop/src/commands/personalization.rs:948-980`), which reports the
cascade, `effectivePhrases`, and a `capability` struct computed from the same
`model_style` table (personalization.rs:686-708). The screen renders from
`capability.stylesApply` / `readsControlLine` / `compareMeaningful`
(`ui/src/screens/StylesScreen.tsx:252-259`).

## 2. Why it appears "not to apply"

This is **mostly correct behaviour, surfaced badly**, plus one real wiring bug in the
preview path:

1. **The shipped default model structurally ignores style phrases.** The default
   (`s1-mini`) is `ControlLine` + `cleanup_only`: Polish is its only transform
   (engine.rs:455-472 skips every non-Polish preset with `ModelNotCapable`), and the
   Polish card prompt never carries `ctx.preferences`. So a user who activates
   "Professional" and dictates into the default model sees no change, and the Style
   page's Band A shows the cascade resolving to that style with its phrases listed
   under "What the model is told" — a promise the model cannot keep. The page does
   append one line ("does not read this list…") and disables the Activate buttons,
   but the phrases are still shown as "told" to the model.

2. **The Compare panes are misleading on the default model.** For `s1-mini`
   `compareMeaningful` is false (card Polish ignores style), so the compare editor
   is hidden — correct. But `preview_style_profile`
   (personalization.rs:1002-1060) builds `PromptContext { preferences:
   merge_style_phrases(...), .. }` and calls `run_transform_blocking` with the
   *production auto-apply transform*. If the user has a non-Polish transform with
   `auto_apply` set, `is_polish` is false and the 2B would go down the generic
   path; for `s1-mini` the same call is skipped as `ModelNotCapable`. The preview
   therefore cannot demonstrate a style difference on the two card models, and on
   the 2B it *can* differ only via the generic path — while production Polish uses
   the card path. The preview and production disagree about which prompt the active
   style reaches for the 2B's Polish preset.

3. **`effectivePhrases` is reported as "what the model is told" regardless of
   channel.** `style_resolution` returns `store.active_phrases(&resolved)`
   (personalization.rs:973) even when the selected model's channel is
   `ControlLine`/`FixedBehaviour`, where those phrases reach nothing. The UI then
   has to add a corrective sentence afterwards. The report should say what the
   model is actually told.

4. **The UI duplicates the "this model ignores styles" message in three places**
   (see §4) and shows the style list at 60% opacity without hiding the per-row
   phrase lines, so the page reads as "styles are active, but also not active".

Conclusion: styles *do* apply for `StylePhrases` models (fast, quality, qwen2.5-3b,
gemma2-2b, qwen3-0.6b, neudecide-q4, any openai-compat id) and for `qwen3.5-2b`
outside Polish. For the shipped default they correctly cannot apply, and the page
does not say so plainly enough. There is **no bug in the pipeline→prompt path for
style-capable models** — but there is no test proving it (the existing engine tests
only cover the control line and card paths), and the preview command can
misrepresent it.

## 3. What "make it work for different styles" requires

Minimal correct changes:

1. **`teletype-core` (test, no behaviour change):** add an engine test proving the
   `StylePhrases` channel works end to end — a resolved style phrase in
   `PromptContext.preferences` appears in the prompt a style-capable model
   receives, and a different phrase produces a different prompt. This is the
   missing proof that styles apply at all.

2. **`preview_style_profile` (desktop command, owned by the lead's Task 3? no —
   it is in `crates/teletype-desktop/src/commands/personalization.rs`, which I do
   NOT own):** the preview must use the same prompt path production uses for the
   model's Polish preset. For card models it should refuse with a clear message
   instead of running a transform that cannot show the style. Flagged in the report
   to the lead; not changed by this task because the file is outside my ownership
   list (my owns: `style.rs`, `pipeline.rs` style path, `transforms/engine.rs` /
   `prompt.rs`, `StylesScreen.tsx`).

3. **`style.rs`:** give `ModelStyle` a plain-English "what this model is told"
   answer per channel so the screen and any future report cannot re-derive it.
   (Small addition, keeps the table the single source of truth.)

4. **`StylesScreen.tsx`:** for `ControlLine`/`FixedBehaviour` models, stop
   presenting the winning style's phrases as "what the model is told"; say what the
   model is actually told (the control line values, or nothing). Keep the
   corrective line, but only once.

5. **Simplify the page** (Part B.3, §4 for the concrete duplicates).

Which model channels are affected: `ControlLine` (s1-mini, qwen3.5-2b) and
`FixedBehaviour` (speakoflow-mini) for the UI honesty fix; `StylePhrases` only
gets the regression test. No change to the phrase-injection mechanism itself.

## 4. The duplicates on the Styles page

`ui/src/screens/StylesScreen.tsx` (867 lines). The same three facts are stated up
to four times:

| Fact | Where (line numbers at time of writing) |
|---|---|
| "This model does not read the style list / phrases" | Band A: `capability.channel === "controlLine" ? … "does not read this list…" : stylesApply ? "Read by …" : "…does not read these phrases, so they change nothing yet."` (lines ~472-484); Band B compare-disabled note: "…so both panes would come back the same. Choose a model that takes instructions (Qwen3.5 2B, Fast or Quality)…" (lines ~545-556); Band C notice: "…so the styles below are not applied. A model that takes instructions (Qwen3.5 2B, Fast or Quality) uses them." (lines ~648-655); and again in each row's Activate `title` tooltip (line ~745). |
| The model name + "writes in one fixed style that lives in the model itself" | Band B (~555) and Band C (~653) — nearly identical sentences. |
| "Its writing style comes from the Tone, Structure and Context settings/controls (at the bottom of this page)" | Band A (~478), Band B (~549), Band C (~651), and the control band's own intro (~775). Four times. |
| The style list is rendered twice-ish: each profile row shows its phrases joined by " · " (line ~727) AND Band A shows the effective phrases as chips (lines ~460-470) AND the compare pane header repeats the phrases (line ~618). | Three renderings of the same data. |

Structural duplication:

- **Band B's disabled branch and Band C's notice are two separate blocks that say
  the same thing** ("both panes would come back the same" ≈ "the styles below are
  not applied"). They can be one inline note.
- **The "New style" form** is a full bordered section (lines ~755-790) that is
  hidden entirely when `!stylesApply`; on `FixedBehaviour` models the user sees the
  list dimmed but cannot create styles, with no explanation of why creation is also
  unavailable (creation is fine — styles still exist for other models — but the
  current gating hides it, which is inconsistent with the list staying visible).
- **Band A's per-layer notes** restate the ranking rules in prose for every rung
  (five rows of "outranks" sentences); the rank number + winner pill already say it.
  The notes can shrink to one line per rung.

## 5. Part B scope decision

Per the task's sequencing rule: Part A reveals that "not applying" is **correct
behaviour for the shipped default** (a card model), not a broken injection path.
So Part B is scoped to:

1. The regression test proving style phrases reach the prompt for a
   style-capable model (engine + a style.rs-level test that the pipeline's
   injection produces the phrase in the prompt).
2. UI honesty: one plain "what this model is told" line per channel, the
   duplicates from §4 removed, compare pane preserved.
3. No change to the phrase-injection mechanism in `pipeline.rs` (it is correct) —
   only touched if a test shows otherwise.
