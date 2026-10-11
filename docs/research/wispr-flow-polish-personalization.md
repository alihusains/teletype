# Wispr Flow polish, personalization, dictionary learning, and insights

Research report for the requested Teletype release: three style presets (Formal /
Casual / Very casual), user-defined shortcut UI, post-polish word/correction
suggestions, explicit accept/reject, `Learned by Teletype` provenance, and Polish
Insights.

Method: first-party Wispr Flow docs (fetched 2026-07-17), the local EnviousWispr
Windows reference implementation at `ew/enviouswispr-windows`, and the current
Teletype source. README and project-brain claims were not used as evidence.
Verified behavior is separated from recommendations; every external claim links
to its doc.

## 1. Summary

- Wispr Flow's "Styles" are per-app-category formatting presets (Formal, Casual,
  very casual, Excited!) chosen in a guided onboarding; they drive
  capitalization, punctuation, and trailing-period rules, and are a distinct
  feature from Transforms (post-hoc rewrite of selected text).
- Wispr's dictionary has two axes: a word (spelling to preserve) and an optional
  correction (wrong spelling → replacement). It auto-learns from the user's
  edits with a documented include/exclude policy.
- EnviousWispr's Quick Add is the closest local reference: it captures the
  foreground selection (published selection, else a synthetic Copy that
  snapshots and restores the clipboard with owner/sequence discrimination),
  refuses while a dictation or delivery is in flight, and confirms the proposed
  word in a dialog where the spoken form is pre-filled and the written form is
  selected for editing. Its vocabulary guard (`PolishVocabulary`) limits what
  reaches the polish prompt and keeps user-typed words out of system prompts.
- Teletype already implements most of the substrate: style profiles with
  per-app overrides, a dictionary with learned provenance, edit-observation
  learning (`edit_watch` + `personalization::learn`), transform definitions with
  shortcuts, and a local Insights screen. The gaps are the three preset names,
  a user-facing shortcut editor for Quick Add, post-polish *suggestions* with
  explicit accept/reject, and polish-specific insight counters.

## 2. Evidence table

| Wispr behavior (doc) | EW implementation (local source) | Teletype current source | Implication |
|---|---|---|---|
| Styles: 4 presets (Formal, Casual, very casual, Excited!) per category after guided setup; English-only notice ("How to setup Flow Styles") | No style presets; one fixed polish prompt per provider (`src/Production/EnviousWispr.LLM/CloudPolishPrompt.cs`, `OllamaLocalPrompt.cs`) | `crates/teletype-core/src/style.rs` — 3 built-ins (Concise, Professional, Casual), `default_style_for_app()` maps Email/Document→professional, Chat/Social→casual, Coding→concise | Teletype's built-ins are instruction phrases, not formatting rules. Rename/replace built-ins with Formal/Casual/Very casual; keep the per-app default mapping. |
| Trailing-period rule depends on style: Formal never, Casual ≤10 sentences, Very Casual no limit; `!`/`?` never stripped ("How do I use Smart Formatting & Backtrack") | None (prompt-level only) | `crates/teletype-core/src/itn.rs`, `transforms/engine.rs` — no style-conditional trailing-period rule | Add a deterministic post-pass (not prompt-dependent) so the rule is testable and offline. |
| Context Awareness: reads app + text around cursor; categories Email/Work/Personal/Other; on-device casing/spacing matching; privacy exclusions (password fields, banking, URL bars) ("Context Awareness") | `src/Production/EnviousWispr.Services/Input/WindowsForegroundTargetProvider.cs`, `WindowsTextTargetAdapter.cs` (target capture + refusal reasons) | `crates/teletype-core/src/context/mod.rs` (`AppType` enum), `crates/teletype-desktop/src/platform/` | Teletype already classifies apps; keep the local-only context model (no cloud context) — a differentiator, keep documented. |
| Transforms: base Polish with 5 toggleable cleanup rules + up to 5 custom instructions (≤50 words each) + up to 5 writing samples per prompt (50–500 words) ("Rewrite text with Transforms and custom prompts") | Fixed prompts, no user-editable rules | `crates/teletype-core/src/transforms/mod.rs` (`TransformDefinition`, 8 custom slots pattern already exists — see `built_ins()`), `transforms/prompt.rs` | Teletype's transform system already covers "custom prompts". What's missing is the *base polish customization* (rule toggles + custom instructions) and writing samples. |
| Polish review flow: View Diff, Retry, Undo, Accept edits, thumbs up/down ("Rewrite text with Transforms and custom prompts") | None (paste is final) | `transforms/validator.rs` (fallback on failure), `delivery.rs` — no diff/accept UI | New: post-polish review affordance. Wispr's diff viewer is the model; Teletype can start lighter (suggestion chips + accept/reject) since dictation is auto-pasted. |
| Dictionary: word + optional "correct a misspelling" pair; whole-word match, case-insensitive, longer phrase wins, personal over team ("Teach Flow your words with the dictionary") | `CustomWordEntry(SpokenForm, Replacement, Strictness)` in `src/Production/EnviousWispr.Core/Settings/ReusableUserData.cs`; `VocabularyPresenter.AddWordAsync` (`EnviousWispr.Presentation/VocabularyPresenter.cs`) | `crates/teletype-core/src/dictionary.rs` — `DictionaryWord { word, pronunciation, fuzzy, learned_from, aliases, usage_count }`; `insert_learned(from, to)` | Teletype's dictionary already has learned provenance and fuzzy control. Missing: explicit spoken→written correction pairs as a first-class field (aliases are sound-alikes, one-way). |
| Auto-learned words: proper nouns, trade terms, acronyms, variant names, email addresses; skips filenames, URLs, numbers, dates, common names, everyday words | Suggested-alias chips in `MainWindow.xaml.cs` (`SuggestedAliasesPanel`, `AddSuggestedAliasAsync` ~line 1902) — user taps a chip to accept; chip stays on save failure | `crates/teletype-desktop/src/commands.rs` `learn_terminology_to_dictionary` (~line 1482) + `personalization/learn.rs` `extract_signals` | Teletype already auto-learns terminology from edits with confidence levels. Gap: no *suggestion UI* — learning is silent. Add a visible "Teletype learned X" surface with accept/reject. |
| Quick Add shortcut: select a word in another app, press shortcut, word is proposed | `App.xaml.cs` `HandleQuickAddAsync`/`QuickAddUnderLeaseAsync` (~line 2669): capture target → published selection or `WindowsClipboardPaste.TryReadSelectionAsync` (snapshot, clearing write, synthetic Copy, owner+sequence discrimination, restore only own write) → `SelectionAcquisitionPolicy.Decide` (Refuse if dictation running or delivery in flight) → dialog pre-fills spoken form, written form selected (`MainWindow.xaml.cs` `OpenQuickAdd` ~line 970) | `crates/teletype-desktop/src/dictation.rs` `register_quick_add_shortcut` (~line 421) + `commands.rs` `quick_add_selected_word` (~line 4072): AX `selected_text()`, first token, insert + save, no dialog | Teletype's Quick Add is a one-shot: no proposal confirmation, no busy/unsupported handling (fails silently or with a toast), no clipboard-borrow path (AX only). EW's policy class is directly portable as a design. |
| Polish vocabulary guard: only words present in the transcript, ≤24 words / ≤512 chars, in user message (never system prompt), local-only for cloud | `src/Production/EnviousWispr.Core/Dictation/PolishVocabulary.cs` — `Eligible()`/`Block()` exactly as described; `CloudPolishPrompt.cs` carries no dictionary | `crates/teletype-core/src/transforms/engine.rs` + `personalization/packet.rs` (preference packet) | Teletype's packet approach is equivalent in spirit; adopt the transcript-intersection + size caps as the guard spec. |
| Insights: WPM (last 100 transcripts <400s), fixes made (words corrected + dictionary fixes), total words, desktop categories, heatmap + streaks ("Your Usage tab") | None in EW | `crates/teletype-core/src/insights.rs` — `compute()` already has top_phrases, top_apps, WPM vs 40 wpm baseline, streaks, heatmap, `dictionary_words`, `learned_words`, `personalization_corrections` | Teletype's Insights is local-only (stronger privacy story). Gap: no "fixes made by polish" split (corrections vs dictionary substitutions) and no polish accept/reject rate. |
| Shortcut editor: per-action bindings, up to 4 each, ≤3 keys, modifier required, duplicate/reserved rejection, save-on-release, reset to default ("Customize keyboard and mouse shortcuts") | `EnviousWispr.Core/Input/HotkeyContracts.cs` + `HotkeyEdgeTracker.cs`, `HotkeyConflictDetectorTests.cs` | `crates/teletype-core/src/shortcuts.rs` — `parse`, `render`, `conflicts()`; `ui/src/settings/KeybindsClipboardSections.tsx` (record/cancel hotkeys only); `quick_add_hotkey` exists in settings but has no UI editor (commands.rs line 383, sync at 1148) | Teletype has the parsing/conflict primitives; the missing piece is a general keybind editor UI covering Quick Add + transforms. |

## 3. Detailed findings

### 3.1 Wispr Flow (first-party docs)

- **Styles** ("How to setup Flow Styles"): Formal (caps + punctuation), Casual
  (caps + less punctuation), very casual (no caps + less punctuation, Personal
  category only), Excited! (more exclamations). Chosen per category (Personal /
  Work / Email / Other) in a guided "Make Flow sound like you" setup; applies
  from the next dictation; English-only notice.
- **Trailing periods** ("How do I use Smart Formatting & Backtrack"): Formal
  never strips; Casual strips up to 10 sentences; Very Casual strips with no
  practical limit; `!` and `?` are never stripped; requires no selection and no
  existing terminal punctuation.
- **Context Awareness** ("Context Awareness"): on by default; 4 categories;
  browser recognized by website; on-device casing/spacing matching even when
  the feature is off; password fields, banking apps, URL bars excluded; app
  name + domain still sent when off.
- **Transforms** ("Rewrite text with Transforms and custom prompts"): 1–1000
  words; base Polish = 5 toggleable rules (concise, clarity, reorder, structure,
  maintain tone) + up to 5 custom instructions (≤50 words) + up to 5 writing
  samples (50–500 words) per prompt; View Diff with Accept/Retry/Undo;
  Auto-Apply After Dictation with 5-second back-to-back skip; 8 custom prompt
  slots.
- **Dictionary** ("Teach Flow your words with the dictionary"): word + optional
  misspelling correction; ≤60 chars per field; auto-learn include list (proper
  nouns, trade terms, acronyms, variant given names, email addresses) and skip
  list (filenames, URLs, numbers, dates, markdown, emoji, everyday words, very
  common names, everyday acronyms); corrections are whole-word,
  case-insensitive, longer phrase wins, literal replacement.
- **Insights** ("Your Usage tab"): desktop-only; WPM from up to 100 most recent
  transcripts <400s, silence excluded when available; "Fixes made by Flow" =
  words corrected + dictionary fixes; heatmap with streaks; local stats cleared
  on sign-out.
- **Shortcuts** ("Customize keyboard and mouse shortcuts"): ≤4 bindings per
  action, ≤3 keys, modifier or eligible mouse button required, duplicate and
  OS-reserved combinations rejected, save on key release, reset-to-default with
  no undo.

### 3.2 EnviousWispr Windows (`ew/enviouswispr-windows`)

- **Quick Add flow** — `src/Production/EnviousWispr.App/App.xaml.cs`
  `HandleQuickAddAsync` / `QuickAddUnderLeaseAsync` (lines 2669–2790):
  1. Admitted under a presentation lease; refused if a session or exit is active.
  2. Captures the foreground target (`WindowsForegroundTargetProvider`);
     invalid target → dialog with "Select a word… try again".
  3. `SelectionAcquisitionPolicy.Decide`
     (`src/Production/EnviousWispr.Core/Input/SelectionAcquisitionPolicy.cs`):
     published selection wins in every state; else synthetic Copy unless
     dictation is running or a delivery is in flight → Refuse with a
     user-visible message ("…was busy with a dictation, so it left your
     clipboard alone").
  4. Synthetic Copy (`WindowsClipboardPaste.TryReadSelectionAsync`,
     `src/Production/EnviousWispr.Services/Input/WindowsClipboardPaste.cs`):
     snapshot first; clearing write (so a no-selection Copy can't return stale
     clipboard text); synthetic Ctrl+C; wait for the answer (400 ms deadline,
     20 ms settle, 5 ms poll), accepting it only while every change is owned by
     the same clipboard owner as the first change; restore only the write we
     made or the app's answer — never a newer third-party write; returns null
     on any failure and the caller always shows a message.
  5. Three distinct log events: `QuickAddRefused`, `QuickAddSelectionEmpty`,
     `QuickAddPrepared`.
  6. Dialog (`MainWindow.xaml.cs` `OpenQuickAdd`, line 970): pre-fills both the
     spoken and written form with the selection, focuses the written form with
     all text selected; save goes through the single door
     `SaveCustomWordFromPickerAsync` → `VocabularyPresenter.AddWordAsync`.
     Failed saves keep the input; strictness picker resets to default after
     each save.
- **Polish vocabulary guard** — `src/Production/EnviousWispr.Core/Dictation/PolishVocabulary.cs`:
  only dictionary words present in the transcript (whole-word, first-occurrence
  position order), max 24 words / 512 chars, carried in the user message
  inside a labelled `<SPELLINGS>` block with a fixed system-sentence describing
  the block; cloud prompts carry none of it (`CloudPolishPrompt.cs`).
- **Suggested aliases** — `MainWindow.xaml.cs` `AddSuggestedAliasAsync`
  (~line 1902): chips for proposed mishearing→correction pairs; the chip is
  removed only when the save succeeds, so a failed save keeps its remedy
  visible.

### 3.3 Teletype current state

- **Styles**: `crates/teletype-core/src/style.rs` — `StyleProfile` (name +
  `style_phrases`), built-ins `style-concise`, `style-professional`,
  `style-casual`; `resolve_style_id` (explicit > per-app override > active >
  none); `default_style_for_app` maps `AppType` (Email/Document → professional,
  Chat/Social → casual, Coding → concise). Per-app overrides keyed by bundle id
  or app name.
- **Dictionary**: `crates/teletype-core/src/dictionary.rs` — `DictionaryWord`
  with `fuzzy`, `learned_from`/`learned_at` provenance, `aliases`,
  `usage_count`; `insert_learned(from, to)` backfills provenance onto
  user-taught words; built-in seed v2 (exact-only).
- **Quick Add**: `crates/teletype-desktop/src/dictation.rs`
  `register_quick_add_shortcut` (line 421) + `commands.rs`
  `quick_add_selected_word` (line 4072): macOS AX `selected_text()`, first
  whitespace token, dedupe, insert + save. No proposal step, no busy guard,
  no non-macOS path, `quick_add_hotkey` setting exists but has no UI editor.
- **Edit observation / learning**: `crates/teletype-desktop/src/edit_watch.rs`
  (watch window, `diff`), `commands.rs` `record_dictation_edit` →
  `record_edit_core` + `learn_terminology_to_dictionary` (~line 1482);
  `crates/teletype-core/src/personalization/learn.rs` `extract_signals` /
  `apply_signals` with `Confidence` Weak/Medium/Strong (2/4/8 observations),
  scoped Global or per-`AppType`; gates `learn_from_edits`, `learn_terminology`,
  `learn_app_specific`.
- **Insights**: `crates/teletype-core/src/insights.rs` `compute()` — top
  phrases/apps, WPM vs 40 wpm baseline, streaks, heatmap, `dictionary_words`,
  `learned_words`, `personalization_corrections`; rendered in
  `ui/src/screens/InsightsScreen.tsx`.
- **Transforms**: `crates/teletype-core/src/transforms/` — `TransformDefinition`
  with shortcut + instruction, engine, gate, validator (markdown-fence/empty
  fallback to input), spoken-emoji, splitter. No base-polish rule toggles, no
  writing samples, no diff/accept UI.

## 4. Recommended release design (Teletype-native)

### 4.1 Three style presets: Formal / Casual / Very casual

Replace the three built-in profiles (keep the ids, change names/phrases — or
mint new ids and migrate) to match the requested presets:

- **Formal**: caps + full punctuation; keep trailing periods; professional
  tone. Default for Email and Document apps.
- **Casual**: caps + lighter punctuation; strip trailing periods in short
  dictations (≤10 sentences, mirroring Wispr's documented rule). Default for
  Chat/Social.
- **Very casual**: no forced caps at sentence starts (except proper nouns from
  the dictionary or learned words), minimal punctuation, trailing periods
  stripped. Default for Personal messaging.

Implementation notes:

- The trailing-period rule should be a deterministic post-pass in
  `transforms/engine.rs` (or a new `transforms/trailing.rs`) keyed on the
  resolved style — not a prompt phrase. Prompt phrases are advisory; the rule
  is measurable. Sentence counting must exclude URLs, decimals, version
  numbers, and abbreviations (same caveat Wispr documents).
- Keep `default_style_for_app` resolution; the preset mapping above is a
  one-line change per `AppType` arm.
- Per-dictation explicit selection and per-app overrides already exist and
  already win in the right order — no change.

### 4.2 User-defined shortcut UI

- Generalize `KeybindsClipboardSections.tsx` into a Keybinds section covering:
  record, cancel, Quick Add, and each transform. Reuse
  `shortcuts.rs::parse`/`conflicts` for validation, with the documented rules:
  ≤3 keys, requires a modifier, reject duplicates across all Teletype actions,
  reject OS-reserved combinations, save-on-release, Escape cancels recording
  without changing the binding.
- `quick_add_hotkey` already round-trips through settings and
  `sync_quick_add_shortcut` re-registers on change — the UI is the only missing
  piece.
- Wispr allows up to 4 bindings per action; Teletype should ship with one
  binding per action and reserve the data shape (`Vec<String>`) for later
  rather than building the multi-binding UI now.

### 4.3 Post-polish word/correction suggestions with explicit accept/reject

This is the largest new piece. Design:

1. **Detection**: after a polish run, diff `ai_output` vs `transcript` (the
   existing `edit_watch::diff` works). Classify changed tokens:
   - *Dictionary candidate*: a new capitalized/proper-noun-looking token that
     is not in the dictionary and not in the skip list (filenames, URLs,
     numbers, dates, emoji, everyday words — adopt Wispr's documented skip
     categories as the filter spec).
   - *Correction candidate*: a token the polish changed that looks like a
     mishearing fix (single-token substitution, both sides alphabetic).
2. **Presentation**: a lightweight "Teletype noticed" row in the tray/overlay
   after the dictation (not a modal): up to 3 chips, each
   `"heard 'margets' → 'Markets'?"` with Accept / Dismiss. Accept →
   `dictionary.insert_learned(from, to)` (already exists) or, for a correction
   pair, a new `DictionaryWord.correction_from` field. Dismiss → negative
   signal recorded so the same pair is not re-offered (mirror EW's "chip stays
   only while the offer is live" rule: a dismissed offer is tombstoned, not
   just hidden).
3. **Explicit accept/reject for learned preferences**: the personalization
   loop currently applies signals silently at confidence thresholds. Add a
   pending state: a learned preference or dictionary word surfaces once with
   its provenance ("Learned from your edit on {date} in {app}") and an
   Accept/Dismiss control; Accept promotes it, Dismiss decrements/blocks the
   signal key. Keep the silent path for already-accepted signal keys so the
   user is not re-asked.
4. **`Learned by Teletype` provenance**: `DictionaryWord.learned_from` /
   `learned_at` already exist and already render as a "learned" badge in the
   Dictionary UI (T5.1/D006). Extend the same provenance to learned
   *preferences* (the `Preference` struct has `learned_from`/`learned_to`) and
   to the suggestion chips themselves. The dictionary already backfills
   provenance onto user-taught twins — keep that.

### 4.4 Polish Insights

Extend `insights.rs` (all local, computed from existing history):

- **Polish fixes**: count of tokens changed by polish per range; split into
  *corrections* (mishearing fixes, dictionary substitutions) vs *cleanup*
  (filler, punctuation, casing) — mirrors Wispr's "Fixes made by Flow"
  headline = words corrected + dictionary fixes.
- **Suggestion outcomes**: accepted / dismissed / ignored counts for
  suggestions and learned preferences (the explicit accept/reject data from
  4.3 makes this measurable — Wispr has no equivalent because its learning is
  silent).
- Keep existing tiles (WPM, streaks, heatmap, learned_words).

### 4.5 Privacy/trust boundaries

- All learning, suggestions, and insights stay local (Teletype's existing
  model); state this in the Insights and Dictionary UI copy.
- Polish prompt guard: adopt EW's `PolishVocabulary` spec — only dictionary
  words present in the transcript, ≤24 words / ≤512 chars, in a labelled block
  in the user message, never in the system prompt; if a cloud polish provider
  is ever added, dictionary content does not leave the machine for
  local-only-dictionary promises.
- Quick Add selection capture: adopt EW's refusal policy — never borrow the
  clipboard while a dictation or delivery is in flight; any unclear state
  resolves to refusing with a visible message, silently doing nothing is the
  failure mode to avoid (EW logs three distinct events: refused / empty /
  prepared — replicate in tracing).
- Suggestion chips must never auto-apply: the user's explicit accept is the
  only mutation path for learned entries.

### 4.6 Quality / eval ideas

- Trailing-period rule: unit matrix (style × sentence count × trailing
  punctuation × URL/decimal presence) — deterministic, no model.
- Style presets: extend `evals/` with per-preset reference outputs (formality
  check: contraction rate, caps rate, punctuation density) and a regression
  gate that a preset change does not alter *other* presets' outputs.
- Suggestion filter: fixture corpus of polish diffs with labelled
  suggestions (should-offer / should-skip), run as an eval against the
  classifier; track false-offer rate as the headline metric (an offer that
  should not happen is worse than a missed one).
- Quick Add: integration test for the busy-guard (dictation in flight →
  refuse + message) and for first-token extraction (multi-word selection).
- Insights: property tests that `compute` counters are monotonic in the
  history they count and that suggestion outcomes sum correctly.

### 4.7 Safe rollout order

1. **Phase 1 (no behavior risk)**: rename/replace the three built-in style
   presets + deterministic trailing-period post-pass + eval fixtures.
   Acceptance: preset resolution tests, trailing-period matrix green, eval
   gate passes.
2. **Phase 2 (UI, additive)**: Keybinds editor (record/cancel/Quick Add/
   transforms) with conflict validation. Acceptance: rebinding Quick Add at
   runtime re-registers (existing `sync_quick_add_shortcut` path), duplicate
   and reserved combos rejected with messages.
3. **Phase 3 (learning loop, gated)**: Quick Add proposal dialog (spoken/written
   pre-filled, explicit save) + busy/unsupported refusals with messages;
   suggestion chips with accept/dismiss + tombstoning; provenance badges on
   learned preferences. Acceptance: no silent mutations (every learned entry
   has an accept event), refusal paths all user-visible, chip re-offer
   suppressed after dismiss.
4. **Phase 4 (insights)**: polish-fix split + suggestion-outcome tiles.
   Acceptance: counters verified against seeded histories; zero network.

### 4.8 Acceptance criteria (release)

- Dictating in an Email app defaults to Formal; in a chat app to Casual; the
  user can override per-app and per-dictation; trailing periods follow the
  style rule in all three presets, verified by the eval gate.
- Quick Add: in a supported app with a selection, a dialog proposes the word
  (spoken pre-filled, written selected); saving adds it with provenance;
  during an active dictation the shortcut refuses with a visible message and
  the clipboard is untouched.
- After a polish that fixes a misheard proper noun, a suggestion appears with
  the heard form; accepting writes it with `Learned by Teletype` provenance;
  dismissing suppresses re-offers; no learned entry exists without an accept
  event.
- Insights shows polish fixes split (corrections vs cleanup) and suggestion
  accept/dismiss counts, computed locally.
- All new UI copy states that learning is local.

## 5. Unresolved questions

- Should "Very casual" (no caps) be a *formatting* rule (deterministic
  lowercasing pass, which risks breaking proper nouns) or a *prompt* directive
  (softer, unmeasurable)? Wispr documents it as formatting; Teletype needs a
  proper-noun guard (dictionary + learned words) before a lowercasing pass is
  safe. Recommend: prompt directive for v1, deterministic pass only with the
  guard, flagged behind the preset.
- Quick Add on non-macOS: Teletype's `text_selection` is macOS-only (AX). EW's
  synthetic-Copy path is the reference, but porting the clipboard-borrow
  machinery is a real chunk of work — confirm whether the release targets
  macOS only.
- Suggestion delivery surface: tray notification vs in-app overlay vs
  Dictionary screen tab. Wispr has no equivalent (silent learning); EW uses
  in-app chips. The choice affects whether suggestions are seen at all on
  auto-paste dictations.
- Base-polish rule toggles (Wispr's 5 rules) and writing samples: in scope for
  this release or a follow-up? They are orthogonal to the six requested items
  but share the same settings surface.
- Whether learned *preferences* (not just dictionary words) should also get
  the pending-accept flow, or only terminology. The data model already
  supports it; the question is UX load.

## 6. Sources

First-party Wispr Flow docs (fetched 2026-07-17):

- "How to setup Flow Styles" — https://docs.wisprflow.ai/articles/2368263928-how-to-setup-flow-styles
- "Context Awareness" — https://docs.wisprflow.ai/articles/4678293671-feature-context-awareness
- "Rewrite text with Transforms and custom prompts" — https://docs.wisprflow.ai/articles/8068950331-how-to-use-transforms-beta
- "How do I use Smart Formatting & Backtrack" — https://docs.wisprflow.ai/articles/5373093536-how-do-i-use-smart-formatting-and-backtrack
- "Teach Flow your words with the dictionary" — https://docs.wisprflow.ai/articles/4052411709-teach-flow-your-words-with-the-dictionary
- "Your Usage tab: Track your dictation stats in Wispr Flow" — https://docs.wisprflow.ai/articles/8760230576-your-usage-tab-track-your-dictation-stats-in-wispr-flow
- "Customize keyboard and mouse shortcuts" — https://docs.wisprflow.ai/articles/2612050838-supported-unsupported-keyboard-hotkey-shortcuts

Note: the task's URL `2719941210-how-to-configure-polish-shortcuts-and-custom-prompts`
resolves to the same "Rewrite text with Transforms and custom prompts" article
(fetching it returned that article's content), so no separate evidence comes
from it.

Local sources (read-only): `ew/enviouswispr-windows/src/Production/...`
(`App.xaml.cs`, `MainWindow.xaml.cs`, `SelectionAcquisitionPolicy.cs`,
`WindowsClipboardPaste.cs`, `PolishVocabulary.cs`, `CloudPolishPrompt.cs`,
`OllamaLocalPrompt.cs`, `ReusableUserData.cs`, `VocabularyPresenter.cs`,
`HotkeyContracts.cs`) and Teletype `crates/teletype-core/src/{style,dictionary,
insights,shortcuts,personalization/*,transforms/*,context/mod.rs}`,
`crates/teletype-desktop/src/{dictation,commands,edit_watch,text_selection}.rs`,
`ui/src/settings/KeybindsClipboardSections.tsx`, `ui/src/screens/{
InsightsScreen,DictionaryScreen}.tsx`.
