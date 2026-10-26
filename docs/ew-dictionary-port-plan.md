# EW Dictionary & Learn-from-Edits — Porting Plan

## Scope and Method

**Upstream repos inspected (read-only):**
- `ew/EnviousWispr` (macOS, Swift) at `eb031ba08490` (2026-10-25)
- `ew/enviouswispr-windows` (Windows, C#/.NET) at `fcaa4250e256` (2026-10-15)

**Local repo:** teletype at the same working tree; no files modified.

**Method.** I walked the upstream git history for dictionary/learning commits, then read the
relevant source files at HEAD:

- `Sources/EnviousWisprPostProcessing/LearnMemory.swift` — durable learn-memory store (#3975,
  commit `63c940d6`)
- `Sources/EnviousWisprPostProcessing/CorrectionCandidateFilter.swift` — learn-eligibility filter
  (number formatting `faad17d1`, scattered deletions `2c1888f9`, blocked pairs from `63c940d6`)
- `Sources/EnviousWisprPostProcessing/WordCorrector.swift` — punctuation/stopword join guards
  (`7c9cdd80`)
- `Sources/EnviousWisprAppKit/Views/Settings/YourWordsView.swift`, `LearningSection.swift`,
  `LearnFromEditsSettingsPresentation.swift`, `QuickAddTeachingSection.swift` — the four-tab
  Dictionary page UI
- `Sources/EnviousWisprPipeline/SelectionAcquisition.swift` — Quick Add's AX + clipboard ladder
  (commit `f52094df`, PR #4007)
- `Sources/EnviousWisprAppKit/App/CustomWordsCoordinator.swift` — coordinator wiring for the
  Forget memory row
- `scripts/settings-map/receipts/additions/selfLearningDictionary.forgetMemory.json` — i18n receipt
  proving the Forget row ships in all locales
- `Sources/EnviousWisprLivePreview/Engines/ApplePreviewRecognizer.swift` — learned-word hints to
  Apple ASR (`927a8c99`)
- Windows: `git log` + targeted file scan only (the Windows repo has no self-learning pipeline at
  all — see Do-Not-Port)

Every claim below is cited to `file:line` (relative to the repo named on the line). Line numbers
are for the HEADs above.

**Local code inspected (for the "Our current state" column):**
- `crates/teletype-core/src/learning/` (judge.rs, promote.rs, diff.rs, observer.rs, store.rs)
- `crates/teletype-core/src/dictionary.rs`, `crates/teletype-core/src/pipeline.rs`
  (`correct_with_dictionary`, lines 741–870)
- `crates/teletype-desktop/src/dictation.rs` (quick-add shortcut, lines 1218–1265),
  `crates/teletype-desktop/src/text_selection.rs` (AX-only, no clipboard fallback)
- `ui/src/screens/DictionaryScreen.tsx`, `ui/src/screens/PersonalizationScreen.tsx`

---

## Capability Table

| Upstream feature (EW) | Upstream evidence | Our current state (teletype) | Gap | Effort | Notes |
|---|---|---|---|---|---|
| **Durable learn-memory: refuses/nos/blocks stored per pair in `learn-memory.json`** | `LearnMemory.swift:1-90` (structure, caps: 2 000 unblocked, 10 000 blocked, 90-day TTL); commit `63c940d6` | Nothing. Refused/undone learn attempts are not remembered at all. `LearningStore` has tombstones for learned *words* (`store.tombstone`) but no record of "user said no to pair X". | Full feature | M (2–4 days) | Self-contained: one JSON file, one lock, a filter hook. No model dependency. |
| **"No" × 2 → pair blocked forever** | `CorrectionCandidateFilter.swift:142` (`blockedByUser` check), `LearnMemory.swift:52-56` (`LearnUserNoResult.blocked`); commit `63c940d6` chunk 2 | We tombstone the *word* on undo (`promote.rs:324` `undo()`), but a fresh identical mishearing can re-promote it because the *pair* is not remembered. | Partial (word-level, not pair-level) | S (1 day) if memory exists | Depends on row 1. |
| **Forget learning memory (settings row + confirm)** | `LearnFromEditsSettingsPresentation.swift:85-115` (row + alert strings), `LearningSection.swift` (sheet), i18n receipt `selfLearningDictionary.forgetMemory.json` | No such control. "Clear learned preferences" in PersonalizationScreen.tsx:241 clears *preferences*, not learn memory; nothing clears blocked pairs. | Full feature | S (1 day) | Pure UI + one delete-file call. Depends on row 1. |
| **Number-word → same-digits never learned ("two" → "2" = style, not mishearing)** | `CorrectionCandidateFilter.swift:232-236,306` (`isNumberFormatting`, `numberWordReadings`), commit `faad17d1` | `judge.rs:202` skips spans that *look like* numbers (`number_or_date_like`), which blocks digit→word and mixed cases, but a word→same-digit span ("two"→"2") is not specially rejected — it can reach promotion if phonetic sim is high enough (both are short, but sim("two","2") is low in practice; still, "fourteen"→"14" passes). | Partial (different shape) | S (1 day) | Add `number_word_reading(from) == digits(to)` check in `skip_reason` or `changed_pairs`. Port the number tables (EN units/teens/tens/ordinals; DE/NL optional). |
| **Scattered single-letter deletions are learnable ("Whisper" → "Wispr")** | commit `2c1888f9`: deletion-only is blocked only when *every* alignment removes ≥2 letters from one place | `diff.rs` emits `Delete` ops; `judge.rs:620` classifies insert/delete-only edits as `Rewrite` — i.e. we never learn *any* deletion-only fix, scattered or not. | Missing (stricter than EW) | M (2–3 days) | Requires reworking `changed_pairs` to emit single-letter deletion pairs to the judge. EW accepted a residual: two 1-letter trims and two 1-letter half-edits now reach the judge un-scored. Decide if we accept the same residual. |
| **Word correction never joins across punctuation ("still open. AI" ≠ "still OpenAI")** | commit `7c9cdd80`: `sliceCrossesPunctuation` + `joinsThroughStopword` predicates in `WordCorrector.swift`; Pass 0/1/2 eligibility | `pipeline.rs:741-870` (`correct_with_dictionary`): pass 1 goes through `rescoring::rescore_words` over tokens with punctuation *stripped* (`t.chars().filter(|c| c.is_alphanumeric())`, line ~810), and a claimed span keeps only first-token leading + last-token trailing punctuation. Nothing stops a multi-word canonical from spanning a period/comma, and no stopword guard exists ("bank of america" style is fine, but "a PI"→"API" and "open, AI"→"openAI" would fire). | Missing (real user-visible bug class) | M (2 days) | Add the two predicates at the `rescore` call site in `correct_with_dictionary`; mirror in the learned-word claim path (`ownerIntercepts` in EW). |
| **Quick Add: synthetic-Copy fallback ladder for apps with no AX selection (Electron, cold apps, menu-bar reads)** | `SelectionAcquisition.swift:318-700` (`acquire`, `attemptCopyFallback`, `ClipboardRestore` states), commit `f52094df` (PR #4007) | `dictation.rs:1218-1265` + `text_selection.rs` (27 lines): pure AX read via `ax_text::selected_text()`, no clipboard fallback, no restore, no focus recover. Silent failure ("quick add: no selection"). | Missing fallback (big feature) | L (1–2 weeks) | EW's ladder is large and battle-tested: clipboard snapshot/give-back with ownership check, app bring-back, Electron `AXManualAccessibility` wake, 1.5 s key-wait. Port incrementally: (1) snapshot+restore Copy fallback, (2) focus recover, (3) Electron wake. |
| **Menu-bar "Add to Dictionary" route** | `MenuBarController.swift:221,275` (item `Add "…"` / `Add to Dictionary`), rename commit `7d6f2355` | No menu item. Only the global shortcut path exists. | Missing | S (0.5 day) | Small tray menu entry calling the same `quick_add_selected_word`. |
| **Learned-word hints fed to the ASR preview** | `ApplePreviewRecognizer.swift` (commit `927a8c99`: "hint saved and learned words to Apple recognition" — brief canonical hints built from saved + learned user words) | We have no Apple-ASR preview path (local Parakeet only). No equivalent seam. | N/A | — | Cannot port; the hook point doesn't exist in our stack. See Do-Not-Port. |
| **Dictionary page = 4-tab rail (Your Words / Vocabulary Packs / Learn from / Quick Add) with taglines** | `YourWordsView.swift:33-125` (`DictionaryTab` enum, taglines, fixed banner + rail layout) | `DictionaryScreen.tsx:60-64`: identical 4-tab structure already (`words`/`packs`/`learn`/`quick` with the same taglines). | Parity achieved | — | We evidently already mirror this. |
| **"Self-Learning Dictionary" row with availability states (downloading, learn-only, disabled)** | `LearnFromEditsSettingsPresentation.swift:1-289` (availability enum + per-state copy), `LearningSection.swift:60-130` | `PersonalizationScreen.tsx:341-360`: master switch + 3 sub-toggles ("Learn from my corrections", "Learn app-specific preferences", "Remember preferred terminology"), no availability states, no "Checked by…" status line, no download model concept (our judge is a local heuristic — `judge.rs` has no model). | Partial (simpler, by design) | S–M | We don't download a judge model, so most availability states don't apply. Port only: the blocked-count row + Forget (rows 1–3), and optionally a "how it works" explainer. |
| **Contacts import ("Keep in sync on launch")** | `LearningSection.swift` (`contactsCard`, `ContactsImportConfirm`) | We have `contacts_cmd.rs` / `contacts_import.m` — contacts import exists separately. | Out of scope for this plan | — | Not a dictionary feature; skip. |
| **Teaching words by voice (speak the pronunciation)** | Not in the six named upstream commits; upstream uses "Add Selected Word" + manual entry. | We already have a voice-teach flow in `DictionaryScreen.tsx` (`TeachMode`, `startTeach`, line ~190). | We go further | — | Keep; no EW counterpart to port. |
| **Windows self-learning parity** | `ew/enviouswispr-windows` at `fcaa4250e256`: no learn-from-edits pipeline, no `LearnMemory`, no `CorrectionCandidateFilter` in the log (0 commits matching `learn`); only Quick Add (`1306b13`) and "Your Words" import (`c7503c9`) exist. | We are macOS-primary like the upstream macOS repo. | N/A | — | Windows upstream has nothing to port from. |

---

## UI Parity Detail

### EW's Dictionary page (macOS)

Structure (`YourWordsView.swift:126-175`): fixed banner (`DictionarySettingsHeading`: heading +
Enable Dictionary row) above a fixed left rail; only the selected tab's pane scrolls. Four
rail rows, each name + tagline:

1. **Your Words** — "Words you added"
2. **Vocabulary Packs** — "Ready-made lists"
3. **Learn from** — "Learn as you go"
4. **Quick Add** — "Add from any app"

**Learn from tab** (`LearningSection.swift:95-130`): one card per feature, *not* one blended
column (founder feedback 2026-08-29, quoted in-file). The Self-Learning Dictionary card
(LearningSection.swift:137-210):
- Row: title "Self-Learning Dictionary" + "beta" pill + trailing toggle (disabled per
  availability, stored value never snaps off).
- Copy paragraph ("What the feature does… Your Words is a page name.").
- Status line: "Checked by: {judge}. Learned words are checked before they're used." (or a
  learn-only/downloading variant) + optional "Try again" action.
- "Learn more" link → `https://enviouswispr.com/help/self-learning-dictionary/`.
- **#3975 additions** (LearnFromEditsSettingsPresentation.swift:85-115): while the memory is
  non-empty, a row "Learning memory: {N} blocked" + "Forget" button → alert
  "Forget what EnviousWispr remembers about your fixes?" / "Blocked words may be learned again."
  with Forget (destructive) / Cancel.

**Quick Add tab** (`QuickAddTeachingSection.swift:79-150`): three numbered step cards
(1. Highlight a word / 2. Press {live chord} or open the EnviousWispr menu and choose the Add item
/ 3. It's in your dictionary) plus two callouts (shortcut + menu-bar). The chord shown is the
*live* binding, gated by `MenuBarController.shortcutLabel` (the same arbitration check the menu
bar uses) — if the binding would lose arbitration to Record/Cancel, the card says nothing about a
shortcut. Explicit design decision (in-file): no demo video frame, no standalone intro line.

### Our page (`DictionaryScreen.tsx`)

Already matches the 4-tab rail + taglines (DictionaryScreen.tsx:60-64) and the Quick Add tab
shows the live hotkey with a "not set" fallback (lines 950-995), but:
- Step cards are prose, not numbered cards; no menu-bar callout (we have no menu route yet).
- The Learn tab (lines 904-948) is an explainer with learned/total counts and a badge note — it
  has no toggle (the gates live on PersonalizationScreen), no status line, no blocked-memory row,
  no Forget control.

**Parity work:** add the blocked-count row + Forget flow to the Learn tab (or move the
self-learning toggle there from Personalization, matching EW's single home). Estimated 1 day.

---

## Port Order

1. **Learn memory (rows 1–3): `LearnMemory` + `blockedByUser` filter + Forget UI.** Self-contained,
   no model dependency, and it's the user-facing "stop learning this word permanently" control our
   users will ask for once learning is on by default. ~1 week.
   - Port shape (not Swift): a `learn_memory.rs` in `teletype-core` writing
     `learn-memory.json` next to the learning store, with the same caps (2 000 / 10 000 / 90-day)
     and drop-on-contention semantics (a busy lock means "not remembered", never queued).
   - Hook points: `promote::consider_mishearing` (skip blocked pairs before the judge; EW checks
     *after* shape reasons, before word-list checks — CorrectionCandidateFilter.swift:142), and
     the undo path (`personalization.rs:292 undo_learned`) records a user "no".
2. **Punctuation/stopword join guards in word correction (row 6).** This is a correctness fix for
   an existing feature, independent of learning. ~2 days.
3. **Number-word → digits eligibility (row 4).** One filter function + number tables. ~1 day.
4. **Scattered single-letter deletions (row 5).** Reworks the diff→judge handoff; needs a design
   decision on the accepted residual. 2–3 days.
5. **Menu-bar "Add to Dictionary" item (row 8).** ~0.5 day.
6. **Quick Add clipboard-fallback ladder (row 7).** Largest item; only worth it once the AX-only
   path's failure rate is measured (EW motivated it with measured WhatsApp/Electron/cold-app
   failures). Do (1) snapshot+restore Copy fallback first; (2) focus recover; (3) Electron wake.
   1–2 weeks.
7. **UI parity pass (blocked row / Forget / Quick Add step cards).** ~1 day, overlaps items 1 and 6.

---

## Do Not Port

- **Apple-ASR learned-word hints** (`ApplePreviewRecognizer.swift`, `927a8c99`): our stack has no
  Apple recognizer and no hint seam; a Parakeet equivalent is a research project, not a port.
- **Judge-model availability machinery** (`LearnFromEditsSettingsPresentation` download/learn-only
  states): EW's judge is a downloaded model with arm/adapter eligibility; our judge is a local
  heuristic with no download. Porting the state machine would be theatre.
- **Settings Map / settings-search registrations** (`.settingsMapRegistration(...)` calls):
  EW-internal tooling for their settings index; we have no equivalent.
- **i18n receipts** (`scripts/settings-map/receipts/additions/*.json`): EW's localization pipeline
  artifact; we translate differently.
- **Anything from `ew/enviouswispr-windows`**: it has no self-learning pipeline, no learn memory,
  no correction filter. Only Quick Add exists there, and the macOS upstream is strictly ahead on
  it (ownership-checked clipboard give-back vs the Windows `1306b13` snapshot/restore). No
  Windows-specific behavior is missing from the macOS source.
- **Contacts import card** inside the Learn-from tab: we have contacts import as a separate flow;
  do not re-shape it for parity.
- **The Quick Add "demo frame"**: EW explicitly cut the "PREVIEW COMING SOON" panel after a
  founder decision (QuickAddTeachingSection.swift header comment) — don't add a placeholder.
- **Telemetry sites** (`quick_add.opened`, `learn_refused_by_user`, `learn_blocked_seen`,
  Sentry breadcrumb wiring): EW-specific; add our own observability when we land each feature.

---

## Open Questions

1. **Where does the self-learning toggle live?** EW puts it on the Dictionary page (Learn from
   tab); we split gates across PersonalizationScreen (master + 3 sub-toggles). For parity, should
   the Dictionary Learn tab become the single home (toggle + status + memory row), making
   Personalization's learning section a link? Or keep both?
2. **Scattered-deletion residual (item 4):** EW accepts that two 1-letter trims and two 1-letter
   half-edits reach the judge un-scored (`2c1888f9`). Do we accept the same, or require a judge
   threshold for that shape before it can promote?
3. **Quick Add fallback scope:** do we need (3) Electron `AXManualAccessibility` wake for our user
   base, or is (1)+(2) enough? EW needed all three; our failure data may differ.
4. **`learn-memory.json` location:** EW keeps it *outside* `custom-words.json` so dictionary
   imports/exports/transactions are untouched. Our learning store is SQLite
   (`learning/store.rs`). Same idea, different shape: a new table in the existing store (my
   recommendation — no new file, no new lock) or a JSON file next to it (closer to upstream)?
5. **90-day TTL and caps:** EW's numbers (2 000 / 10 000 / 90 days, 23 ms measured reload at
   12 000 entries) are tuned for their reload budget. Re-measure on SQLite before copying.
6. **Number tables:** EW reads "ONE word with the app's own number tables" (EN units/teens/tens/
   ordinals + hyphenated tens; DE/NL via `LanguageNumberParser`). We're English-first — is the EN
   table alone enough for now?
