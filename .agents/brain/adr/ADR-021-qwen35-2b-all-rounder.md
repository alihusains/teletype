# ADR-021 - Qwen3.5-2B is the all-rounder: cleanup, Polish axes and Prompt Engineer in one model

- Status: accepted
- Date: 2026-10-24
- Deciders: Ali (request: "recommend these 2 S1-mini and Qwen 3.5 2B", "for
  Polish, Prompt Engineer add labels to 3.5 2B in the models section", "S1 mini
  also has this ... we will also need the same to work with 3.5 2B"), inline
  implementation
- Implements: D031; extends ADR-020 (per-host polish catalog) and ADR-019
  (first-run shortlist); reuses the S1 control-line protocol for a second model

## Context

Ali's standing constraint set for the polish model: under 3B parameters,
current generation, under 1200 MB on disk, low RAM, low latency, good accuracy.
The product goal is one model that does dictation cleanup, Polish with the
Wispr-style toggles, and Prompt Engineer, so the user does not pick a different
model per task.

The bake-off (`evals/runs/model-bakeoff-2026-10-24/report.md`) found exactly
one candidate that fits: Qwen3.5-2B IQ4_XS at 1,173 MB, 1,403 MB RSS, 218 ms
p50 total. S1-mini is better at pure English cleanup (97.8 vs 96.3 post-ITN)
and much smaller, but it cannot follow instructions: its fine-tune matches a
fixed card prompt, and every instruction-following probe returned the input or
punctuation-only output. The cleanup-only fine-tunes (SpeakoFlow Mini,
utter-mini) corrupt URLs, emails and names and cannot run Prompt Engineer.

Two shorter lists existed before this decision: the GPU shortlist led with
s1-mini, speakoflow-mini and qwen2.5-3b, which Ali explicitly called outdated
(Qwen2.5 is "way too old"). The AI Models polish rows also gave no signal about
what a row is for, so the all-rounder and a cleanup-only row looked
interchangeable.

The other half of the request was parity with S1-mini's settings. S1-mini
takes a control line (`[Styling: ...] [Structure: ...] [Context: ...]`); the
Styles screen showed the axis block only when `s1-mini` was active. Enabling
the 2B without teaching it that protocol would have produced a dead control,
and enabling the block without an honest Compare story would have promised a
difference the 2B does not produce.

## Decision

### Catalog (`crates/teletype-inference/src/catalog.rs`)

- Add `qwen3.5-2b` (unsloth/Qwen3.5-2B-GGUF, revision
  `f6d5376be1edb4d416d56da11e5397a961aca8ae`, file `Qwen3.5-2B-IQ4_XS.gguf`,
  1,172,996,352 bytes, sha256 `3639f34b...2ed11c1cd`, Apache-2.0) at
  `sort_order: 2`, directly after s1-mini.
- Add `CatalogEntry::purpose: &'static str`, the one-line job label the UI
  shows as a chip: `"Dictation cleanup"` for s1-mini and speakoflow-mini,
  `"Polish + Prompt Engineer"` for the 2B, empty where the description already
  makes the job obvious.
- Keep `recommended: true` on s1-mini only. Several recommended rows would
  make the badge meaningless (the ADR-020 lesson), so the 2B carries
  `verdict: Recommended` and a `purpose` chip instead of a second badge.
- GPU shortlist becomes `["s1-mini", "qwen3.5-2b"]`; the CPU shortlist stays
  `["s1-mini", "speakoflow-mini"]` because `runs_on_cpu: false` for the 2B
  would filter it out anyway and a CPU-only build should not lead with a
  2B at a minute per polish.
- `chat-template-kwargs {"enable_thinking": false}` is pinned for
  `qwen3.5-2b` as well as `s1-mini`; a thinking 2B burns the token budget
  before printing text.

### Polish path (`crates/teletype-core/src/transforms/`)

- `is_qwen_polish = provider.model_id() == "qwen3.5-2b" && is_polish` joins
  the lean fixed-message path (`is_card_cleanup`). A non-polish transform on
  the same model (Prompt Engineer) still goes through `build_prompt`, so the
  instruction is genuinely applied; a test pins both branches.
- `build_qwen_polish_messages` reuses the shared
  `build_control_line_messages`: the control line is the first line of the
  user message, the transcript follows bare. No wrapper tags, no preferences.
- The system prompt is an editor, not a normalizer: five numbered edits
  (concise, clear, reorder, structure, tone), all unconditional. The concise
  rule names the fact categories it may not delete ("a fact, name, date,
  time, source or recipient") because the first version dropped `readme`,
  `Tuesday` and `2 pm` while shortening. A separate rule requires terminal
  punctuation on sentences and bullets. Round 4 (cleanup) and Round 5 (editor,
  24/24) in the bake-off report carry the evidence.
- Two formatting promises are enforced in code because the 2B would not keep
  them in the casual register: `prose_from_bullets` folds returned lists into
  paragraphs on the prose path, and `capitalize_leads` capitalizes the first
  word of the text and each bullet without touching URLs, paths or
  identifiers.
- The 2B Polish path validates with `validate_editor`, a 0.15 OffTopic floor.
  Aggressive conciseness shares 0.21 to 0.25 of content words with the input;
  an unrelated answer still scores near zero and is rejected.
- `SkipReason::ModelNotCapable` still applies only to s1-mini and
  speakoflow-mini.

### UI

- AI Models polish rows render the `purpose` chip; the Polish section hint says
  a row marked "Polish + Prompt Engineer" also runs transform presets, and
  cleanup-only rows cannot.
- Styles shows the three axes when the active model is s1-mini or
  qwen3.5-2b (`supportsControlLine`), names the model in the copy, and the
  Compare notice says the 2B ignores learned phrases
  (`polishIgnoresPhrases`), so both panes are not advertised as different.

## Alternatives considered

- **Ship s1-mini only.** Cheapest and fastest, but no Prompt Engineer and no
  style controls; it does not meet the "one model does everything" goal.
- **Ship the 2B as the only recommended model.** It is 2.5x the disk, 1.7x the
  RSS and 2.6x the p50 of s1-mini, for English cleanup that is slightly worse.
  A first run should not pay that for users who only dictate.
- **Add a preferences line to the 2B polish prompt.** Probed and rejected:
  `Preferences: warm, concise, no jargon` made casual + prose + fidelity echo
  the transcript in 2 of 8 cells and lose `example.com`
  (`axis-matrix-prefs-probe.json`).
- **Teach the 2B the full `build_prompt` shape.** Experiment 1 showed the heavy
  prompt suppressed the style rules and produced Prompt Engineer meta-text; the
  rule-first lean prompt is what made both work.

## Consequences

- Two shortlist rows, two jobs: S1-mini is the fast English cleanup default;
  Qwen3.5 2B is the all-rounder for Polish axes and Prompt Engineer. The user
  no longer has to infer that from model sizes.
- The 2B's Polish prompt is model-specific by design. A future model that takes
  the control line needs its own id in the `is_*` dispatch and the same
  unconditional-cleanup rule.
- List outputs are one complete sentence per bullet, ending with a period;
  prose outputs never contain bullet markers, because the engine folds any
  list the model returns on that path.
- RAM ceiling: 1,403 MB RSS plus the speech engine. This is the top of what the
  cap allows and is why the 2B is not the CPU shortlist lead.

## Evidence

- `evals/runs/model-bakeoff-2026-10-24/report.md` Rounds 1-5; raw rows in
  `axis-matrix.json` and `editor-matrix.json`; harnesses `axis_matrix.py` and
  `editor_matrix.py`.
- Tests: `polish_with_qwen_uses_the_control_line`,
  `qwen_polish_capitalizes_a_lowercase_greeting`,
  `capitalize_leads_touches_words_but_not_urls_or_identifiers`,
  `prose_mode_folds_returned_bullets_into_a_paragraph`,
  `non_polish_with_qwen_uses_its_own_instruction` (core);
  `editor_floor_accepts_aggressive_compression_but_still_rejects_off_topic`
  (validator); `shortlisted_rows_name_the_job_they_are_for`,
  `qwen35_is_pinned_and_stays_under_the_size_ceiling` (inference);
  `model_status_ships_the_polish_role_under_the_key_the_ui_reads` (desktop);
  `labels each shortlisted row with the job it is for` (UI).
- `cargo test -p teletype-core -p teletype-inference` green (790 core tests);
  `tsc --noEmit` clean; UI suite 194 tests / 22 files; live screenshot
  read-back of the Styles and Models copy in the `?demo&ready&qwen35`
  preview.
