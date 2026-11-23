# Learned-word pill: EnviousWispr teardown and Teletype implementation plan

**Status:** proposed — no code changed.
**Date:** 2026-11-23.
**Sources read (read-only):**
- EnviousWispr `saurabhav88/EnviousWispr`, shallow clone at `f9ca2fcb6fe4068f87de9f7558db22e8fbf4eac6` (2026-11-23) in `/private/var/folders/sz/rrj0cwrs3h568n61l1nlfrjm0000gn/T/opencode/enviouswispr`.
- Teletype working tree at `49ad13b` (uncommitted work present; none touched).
- The earlier capability plan `docs/ew-dictionary-port-plan.md` (dictionary/learn-from-edits backend; most of it is now ported).

**Question answered here:** how does "learned this word → added to dictionary" work end to end upstream, what already exists in Teletype, what is actually missing, and how to build the missing part with the same UX/UI.

---

## 1. How EnviousWispr implements it

### 1.1 Detection and save (backend)

| Step | Upstream file | Behavior |
|---|---|---|
| Watch the pasted region after every dictation | `Sources/EnviousWisprAppKit/App/ObservedCorrectionWatcher.swift` | Uses Accessibility to re-read the field, aligns the user's edits to the paste (`EditAlignment.swift`), settles at most two judged bursts per paste, guards on generation/revision/cancel, and keeps weak evidence as "held" corrections. |
| Filter candidates | `Sources/EnviousWisprPostProcessing/CorrectionCandidateFilter.swift` | Rejects number formatting, scattered deletions, punctuation-spanning phrases, pairs the user blocked, etc. Keyed by `CorrectionPairKey` (`Sources/EnviousWisprCore/CorrectionProposal.swift`): `v1:` + casefolded `[original, corrected]`. |
| Judge | `Sources/EnviousWisprCore/CorrectionJudging.swift`, `RulesCorrectionJudge.swift`, `CoreMLCorrectionJudge.swift` | Rule judge or downloaded model decides whether the edit is a real mishearing. |
| Resolve the save target | `CustomWordSaveHelper.proposalTarget` in `Sources/EnviousWisprAppKit/App/CustomWordsCoordinator.swift` | Exactly one of: new word, existing user word, override of a pack word, or restore of a tombstoned built-in. |
| Write and prove the write | `LearnedCorrectionCoordinator.swift` | Appends `aliases` + `learnedAliases`, re-reads to prove the write, stores an exact `UndoRecord` (`operation: added | updated | restoredBuiltin`, `preSave`, `postSave`), then shows the pill. |
| Outcomes | `LearnedCorrectionCoordinator.swift` | `learned`, `alreadyCovered`, `refused`, `blocked`, `noLongerWanted`. A second "no" on the same pair blocks it permanently (`Sources/EnviousWisprPostProcessing/LearnMemory.swift`). |
| Undo | `LearnedCorrectionCoordinator.swift` | The click **claims** the record synchronously; the write may finish after the pill is gone. Outcomes: `undone`, `alreadyChanged`, `failed`, `stale`. Blocked-pair memory records the user's "no". |

The word shown is always the **canonical** (correct) word; the mishearing is never printed on the pill (founder copy decision in `CorrectionLearnedPillCopy`).

### 1.2 Floating pill (the UX being requested)

`Sources/EnviousWisprAppKit/App/Overlay/Views/CorrectionLearnedPillView.swift` (421 lines) is the whole picture:

- Appears in the normal dictation-pill slot, content-sized capsule.
- 36 pt round icon disc (`character.book.closed`, 16 pt glyph, accent wash); to its right a 15 pt semibold word over a 12 pt subtitle: **"Added to dictionary"** or **"Updated in dictionary"**.
- Filled **Undo** button (13 pt semibold, 14×6 padding, accent wash) and a quiet **×** (28 pt round target, 11 pt glyph).
- Geometry: 12 px icon→text, 16 px text→controls, 6 px Undo→×, 12 px leading / 10 px trailing / 10 px vertical padding. Word capped at 400 pt and shrinks to 0.5× before tail truncation.
- **12 s dwell** with a 3 px rainbow countdown rail at the bottom inset. `elapsedFraction` drives a trim from 0→1, so the rail *fills as time runs out*; a 1.5 % spark is always drawn. Hover **pauses** it; leaving resumes with at least **2 s** left.
- Phases, all the same pill identity:
  - `learned` → live Undo, ×, countdown, VoiceOver "`<word>`, `<subtitle>`. Undo available."
  - `undoPending` → controls hidden but their space reserved (no size jump), hover ignored, 3 s expiry.
  - `undone` → "**Undone**" (added words show "**“<word>” removed**"), 1.5 s, no button.
  - `undoError` → "**Couldn’t undo**" in error red, 3 s.
  - save failure → separate one-line notice "**Couldn’t save “<word>”**", 3 s, no button.
- × has VoiceOver label "Close and keep “<word>”".
- Copy is centralised in `CorrectionLearnedPillCopy`; whole sentences are localized, including the curly quotes.

`Sources/EnviousWisprAppKit/App/Overlay/LearnedCorrectionOverlayPresenter.swift` adapts the coordinator to the overlay: `present` → `isStillWanted()` read immediately before commit → `onUndo` (claim + write) → `onEnded`. `OverlayReducer.swift` arbitrates: the word is always saved, but the pill is **refused** while the pipeline is busy or another feature occupies the slot; a new learned pill replaces an old unanswered one; only a `learned` (unanswered) pill owes an "ended" report.

### 1.3 Dictionary and Learn-from settings UI

- `Views/Settings/YourWordsView.swift` — fixed heading banner with **Enable Dictionary**, a 216 pt left rail with four tabs and taglines: **Your Words — "Words you added"**, **Vocabulary Packs — "Ready-made lists"**, **Learn from — "Learn as you go"**, **Quick Add — "Add from any app"**.
- `Views/Settings/CustomTermsSection.swift` — list card with word count eyebrow, search, category filter pills including an **Auto-learned** filter, pagination, bulk "Mass edit", import/export, and rows: canonical + sparkles (auto-learned) + category/usage subtitle, edit/delete. Per-row learned provenance lives in the edit sheet (`CustomWordEditSheet.swift`).
- `Views/Settings/LearningSection.swift` — Learn-from tab: **Self-Learning Dictionary** card with toggle, "Checked by: …" status + "Try again", learn-more link, and a **learning-memory row ("N blocked" → Forget → confirmation)**. The "Learn from my edits" toggle lives here, not in a different settings page.
- Learned terms also feed a lexicon and a spell-check of learned words (`LearnedWordCheckerEligibility`, `WordCheckRuntime`).

---

## 2. What Teletype already has

Most of the backend is already ported and wired; this is **not** a greenfield feature.

- `crates/teletype-core/src/learning/` — `observer.rs`, `diff.rs`, `judge.rs`, `settle.rs`, `promote.rs`, `learn_memory.rs` (2 refusals → permanent block, 90-day TTL), `store.rs` (SQLite), `boost.rs`, `feedback.rs`, `replay.rs`, `profile_bandit.rs`, `contacts.rs`.
- `crates/teletype-core/src/dictionary.rs` — `LearnedChange::{Created, Backfilled, AliasAdded, Unchanged}`, `insert_learned`, `revert_learned`, `learned_from` / `learned_at`.
- `crates/teletype-desktop/src/dictation.rs:7319…7710` — `observe_pending_edit` / edit-watch learns, applies judge + blocked-pair guards, writes the dictionary, builds `LearnedOffer`, arms a **30 s** `OfferUndoExpired`, updates the tray with Undo, emits the Tauri event `learned-preference` to the main window with `{ message, ids }`. Word-only offers carry an `offer:<id>` sentinel.
- `crates/teletype-desktop/src/commands/personalization.rs` — `LearnedOffer`, `undo_learned(ids)`, `undo_offer_from_state`, `undo_offer_in` (reverts preference + dictionary + lexicon atomically), `undo_learned_term`, `forget_learned`, `set_profile_settings`.
- `ui/src/App.tsx:286…320, 497…520` — main-window toast `Learned: <word>` with **Undo / Keep** buttons; stale Undo shows a dialog.
- `ui/src/screens/DictionaryScreen.tsx` — same four-tab rail and taglines; Your Words has add/edit form, pronunciation, aliases, teach-by-voice, match strictness, learned sparkles badge, "exact only" and "used N×" pills, pack and Quick Add tabs.
- `ui/src/screens/PersonalizationScreen.tsx` — "What I learned" rows with source labels and `undo_learned_term`.
- `ui/src/settings/DictationPolishSections.tsx:880…980` — "Learn from my edits" toggle (reads the profile gate, `set_personalization_enabled`) and learning-data deletion.
- Overlay plumbing already exists and is the key enabler: `crates/teletype-desktop/src/overlay.rs::show_result / dismiss_result / resize_to / set_interactive`, `PendingShow::Card(id)`, and `set_pill_size` with an optional `card` id. `ui/src/pill.tsx` renders `PolishDiffCard` (460 pt, buttons, measured size report) and `ui/src/pillMachine.ts` has a `result` phase with an 8 s TTL.

**Stale comment to remove:** `dictation.rs` says the pill is click-through and "its fixed sizes leave no room for a button without resizing surgery". That was true before the polish result card; `show_result` now makes the pill interactive and the frontend reports measured size via `set_pill_size`. The learned pill reuses exactly that mechanism.

---

## 3. Gap analysis

| EnviousWispr | Teletype today | Gap |
|---|---|---|
| Learned pill in the pill slot: word + "Added/Updated in dictionary" + Undo + × | Main-window toast + tray Undo; pill shows nothing | **Core missing piece** |
| 12 s dwell, hover pause (2 s floor), countdown rail | No timer; toast persists until dismissed; 30 s backend expiry | Missing |
| Undo morphs (`Undone`, `“word” removed`, `Couldn’t undo`, `Couldn’t save “…”`) | Toast closes; stale Undo opens a dialog | Missing |
| Exact undo record, claim-on-click | `LearnedOffer` records `LearnedChange`s; undo reverts preference + dictionary + lexicon | Parity; make the claim atomic under concurrent Undo |
| Save failure surfaces on the pill | Save failure is only logged; a success toast/offer is still emitted | **Bug to fix** |
| Dictionary page has search, auto-learned filter, count, import/export | 4-tab parity, learned badge, but no search/filter | Minor UI parity |
| Learn-from tab owns toggle + status + learning-memory Forget | Tab is informational; controls live in Settings/Personalization | UI parity |
| Pill refuses to displace other overlay content | Result card mechanism exists but no arbitration for learned | Design rule needed |

---

## 4. Target UX specification (port, adapted to Teletype tokens)

Everything below is the intended behavior; copy and metrics mirror upstream, colors use Teletype's pill palette (`DARK_SURFACE`, `DARK_BORDER`, `#8bbaff` accent, `rgba(79,142,247,0.18)` wash, `SECONDARY_TEXT`, `ERROR_TEXT`, `RAINBOW`).

### 4.1 Wire payload (new event `learned-word`, emitted only to the `pill` window)

```ts
interface LearnedWordPayload {
  card: number;      // overlay card id, allocated by show_learned()
  offerId: number;   // backend undo offer id
  word: string;      // canonical word only
  kind: "added" | "updated";  // Created => added; Backfilled | AliasAdded => updated
}
interface LearnedWordErrorPayload { card: number; word: string }
```

`learned-word-error` is emitted when the dictionary write fails. No offer is created in that case.

### 4.2 States and timings

| Phase | Title | Subtitle | Controls | Countdown | TTL |
|---|---|---|---|---|---|
| `learned` | `<word>` | "Added to dictionary" / "Updated in dictionary" | Undo (filled), × (quiet) | rail fills with elapsed time; hover pauses | 12 s, pause, 2 s resume floor |
| `undoPending` | `<word>` | same | reserved invisible (no resize) | none | 3 s max, then close silently |
| `undone` | "Undone" for updated; "“<word>” removed" for added | — | none | none | 1.5 s |
| `undoError` | "Couldn’t undo" (error red) | — | none | none | 3 s |
| `learnedError` | "Couldn’t save “<word>”" (error red) | — | none | none | 3 s |

- Undo click: morph to `undoPending` **before** invoking, then call `undo_learned({ ids: ["offer:<offerId>"] })`. `"applied"` → `undone`; `"stale"` or invocation error → `undoError`.
- ×: "keep the word" — close the pill immediately, leave the learning and tray offer alone.
- A new take or any real pill state from Rust replaces the learned pill. A polish card never displaces a live learned pill; the learned pill takes precedence (the edit check fires ~15 s after the take, so the 8 s polish card is normally already gone).
- The backend 30 s offer lifetime stays for the tray. The main-window toast is emitted only when no pill was admitted (preference-only or lexicon-only learn, or pill busy). This avoids two live Undo buttons for one offer.

### 4.3 Visual and accessibility details

- Content-sized capsule: 36 px disc, 15 px/600 word, 12 px/400 subtitle, 1 px title gap, 13 px/600 Undo, 28 px × target, paddings 12/10/10, gaps 12/16/6, radius 999.
- Long words: `maxWidth: 400`, `fontSize` shrinks to 7.5 px before tail truncation. Results use the shorter "Undone" when the added-word title cannot fit.
- Countdown rail: 3 px, inset ~2 px from the bottom edge, rainbow gradient, drawn width `max(1.5%, elapsed/12s)`; stop frame updates while hovered and under reduced motion keep the rail but drop the bloom (no animation beyond the rail).
- VoiceOver: text element label "`<word>`, `<subtitle>`. Undo available."; Undo labeled "Undo"; × labeled "Close and keep “<word>”". The window never takes focus, so also post a macOS accessibility announcement on admission via the existing `ax_announce` helper.
- Buttons must be real `<button>` elements with `aria-label`; no click handler on the text.

---

## 5. Implementation plan

### Phase 0 — Decisions to confirm before coding

1. Suppress the main-window toast when the pill is admitted? **Recommended: yes** (see 4.2). Otherwise two Undo affordances race and the copy conflicts.
2. Multi-word edit: show the pill only when exactly one dictionary change occurred; keep the existing toast for "N words". **Recommended** so the copy stays honest.
3. Pill position: anchored in the existing configured pill slot, not the cursor. **Recommended** (reuses `resize_to`, consistent with all other pill surfaces).
4. Learn-from tab: mirror the toggle there and keep Personalization's copy as a link? **Recommended** for EW parity; do not duplicate two independent switches.

### Phase 1 — Backend notice contract (`crates/teletype-desktop/src/dictation.rs`, `commands/personalization.rs`)

1. Define serde structs `LearnedWordNotice { card, offer_id, word, kind }` and `LearnedWordSaveError { card, word }` next to `LearnedOffer` (or in `commands/personalization.rs`).
2. In the learn block of `observe_pending_edit` (~`dictation.rs:7520–7710`):
   - Keep `(change, canonical, kind)` together when calling `insert_learned`, so the pill has the word without a second dictionary lookup.
   - On `state.dictionary.update(...)` failure: **do not** create the offer/toast/tray Undo. Emit `learned-word-error` (if the pill is admitted) and log as today.
   - On success: after `last_learned` and tray update, if `self.phase == Idle` and there is exactly one dictionary change, call `overlay::show_learned(&app)` and `emit_to(PILL_LABEL, "learned-word", &notice)`. Otherwise follow the existing toast path.
   - Delete the stale "pill stays out of it" comment.
3. Make Undo claim atomic in `commands/personalization.rs` (`undo_learned`): take `last_learned` out under the lock before applying; if apply fails, restore it; stale ids return `Stale`. Tray `undo_offered` uses the same helper. This gives the pill's claim-on-click semantics and prevents a concurrent tray click from double-applying.
4. Keep the 30 s expiry and the `offer:<id>` sentinel unchanged.

**Acceptance:** Rust unit tests for kind mapping (`Created` → added, backfilled/alias → updated), save-failure → no offer, and concurrent Undo returning Stale on the second call.

### Phase 2 — Overlay card plumbing (`crates/teletype-desktop/src/overlay.rs`, `commands/settings.rs`, `lib.rs`)

1. Generalise the single card slot: keep one monotonically increasing `CardIds` (currently `ResultCard`); `show_result` and the new `show_learned(app) -> u64` both allocate from it. One id space means a stale dismiss can never match a different card.
2. `show_learned` mirrors `show_result`: `request_show(PendingShow::Card(id))` when hidden, `set_interactive(app, true)`, returns the id. Add `show_learned_error(app) -> u64` (interactive **false** — no buttons).
3. `update()` already clears the card slot on every pill state push, so a new recording ends the learned pill; keep that.
4. Reuse `dismiss_pill_result(card)` for all card kinds. Update its doc comment (and `PendingShow::Card` / `set_pill_size` comments) to say "notice card (polish result or learned word)", not only polish result.
5. Extend `announcement()`/the macOS announce path with the learned notice text if we want VoiceOver to speak it from the app (the pill webview is not focusable). The frontend event handler can also call an `announce` command; pick one owner — **recommended: Rust**, same as other pill states.

**Acceptance:** `cargo test -p teletype-desktop overlay::`, plus the existing size-report tests still pass.

### Phase 3 — Pill UI (`ui/src/pill.tsx`, new `ui/src/pillLearned.tsx` or a local component)

1. Add a `LearnedWordPill` component with the exact copy, metrics, palette and a11y labels from §4. Extract copy into a `LEARNED_COPY` constant (whole sentences, curly quotes) so tests can assert them.
2. Render it in a dedicated branch before the normal pill branches:
   ```tsx
   if (machine.pill.phase === "learned" || machine.pill.phase === "learnedError") {
     return <div style={rootStyle}><div ref={pillRef}><LearnedWordPill … /></div></div>;
   }
   ```
   Like `PolishDiffCard`, it is independent of the selected pill style so Undo never moves between styles.
3. Size reporting: rename `resultCard` → `noticeCard`, include learned phases, and add those phases to the measured-size branch condition (`!usesPortedStyle && phase !== "result" && …`). Once the size report with the card id lands, `resize_to` shows the window.
4. Countdown rail and hover: driven by props from the machine (§4), not an internal clock, so tests can freeze `elapsedMs`.
5. On unmount/expiry, call `dismiss_pill_result(card)` through the existing "card left for idle" effect; generalise it from result-only to all notice cards.

### Phase 4 — State machine and timers (`ui/src/pillMachine.ts`, events from §4.1)

1. Add phases:
   ```ts
   | { phase: "learned"; word: string; kind: "added" | "updated"; offerId: number; card: number;
       status: "live" | "pending" | "undone" | "undoError"; elapsedMs: number }
   | { phase: "learnedError"; word: string; card: number }
   ```
2. Add actions: `learned-show`, `learned-hover`, `learned-undo`, `learned-undo-result`, `learned-expire`, `learned-error-show`, `learned-error-expire`, `learned-dismiss`. Every action carries the card id; stale ids are dropped. `polish-diff` is ignored while a `learned` pill is live. `pill-state` (a new take) replaces it as today.
3. `useTauriEvent("learned-word")` and `"learned-word-error"` handlers dispatch after the Rust `show_learned` call; drop a payload whose card is not admitted.
4. Timers (mirroring the result TTL effect):

   | Trigger | Effect |
   |---|---|
   | enter `learned` with `status: "live"` | deadline = now + 12 s; start a rAF/`setInterval` publishing `elapsedMs` (only while not hovered); schedule expiry |
   | hover in | store remaining, clear timer, freeze `elapsedMs` |
   | hover out | deadline = now + `max(remaining, 2000)`; resume |
   | Undo accepted | `status: "pending"`, clear hover/timer, 3 s silent-close timer |
   | `undo_learned` → applied | `status: "undone"`, 1.5 s timer |
   | stale / invoke error | `status: "undoError"`, 3 s timer |
   | expiry of any result | `learned-dismiss` → idle → effect calls `dismiss_pill_result(card)` |

   Use `performance.now()` for the deadline so a wall-clock change cannot move the rail. Under `useReducedMotion()`, keep the rail static per second and skip the bloom.

5. Undo invocation lives in the hook (as `selectDevice` etc. do): `undoLearnedWord(offerId, card)` dispatches `learned-undo`, invokes `undo_learned`, maps `"applied"` / other to `learned-undo-result`.

**Acceptance:** vitest tests freeze time and assert: 12 s → dismiss; hover at 11.5 s → still live after 13 s; unhover → at least 2 s more; Undo → pending → undone with correct title; stale → "Couldn’t undo"; × → no undo call; a new take during pending does not resurrect the card.

### Phase 5 — Dictionary UI parity (`ui/src/screens/DictionaryScreen.tsx`, `DictationPolishSections.tsx`)

1. Your Words list: add a search input (word, aliases, learned-from), an **Auto-learned** filter pill next to the existing match-strictness controls, a count eyebrow, and a visible "Learned from “<misheard>”" line under the word (currently only a tooltip). Keep edit/delete and the "used N×" pill.
2. Learn tab: move/mirror the **Learn from my edits** toggle here (bound to the profile gate via `get_profile` + `set_personalization_enabled`, as `DictationPolishSections.tsx` already does), plus a status line ("Checks run locally · no audio or text leaves this Mac"), a link to the Personalization section for the other gates, and a **learning-memory** row with blocked count and a Forget button + confirmation calling the existing `forget_learned` command.
3. Keep Personalization's "What I learned" list; it is the Teletype analogue of upstream's learned-terms surface.
4. Do **not** port the model-download availability states (our judge is local) or contacts import card (we have a separate flow) — see `docs/ew-dictionary-port-plan.md` §Do Not Port.

### Phase 6 — Verification

- `cargo test -p teletype-core learning dictionary` — judge, learn memory, `insert_learned`/`revert_learned`.
- `cargo test -p teletype-desktop commands::personalization overlay::` — offer matching, undo atomicity, card ids.
- `cargo clippy --workspace --all-targets` and `cargo fmt --check` (or the repo's documented equivalents in `AGENTS.md`/`docs/architecture.md`).
- `cd ui && npm test` (focus: `pillStates`, `polishDiffCard`, `pillLayoutHigh/Medium/Low`, `a11y`, plus the new `learnedWordPill` suite) and `npm run build`.
- Manual on macOS: dictate a known mishearing (e.g. "margets" → "Markets"), check the pill appears at the configured slot with the right subtitle, hover pauses the rail, × keeps the word, Undo removes it and shows "“Markets” removed", a second Undo is stale, disable dictionary save (or inject a failure) to see "Couldn’t save “…”", and confirm the tray Undo still tracks the same offer. Also check a second take replaces the pill and the window hides at idle.

---

## 6. Risks and open decisions

- **Undo race with the tray.** The tray menu and the pill can both invoke undo. Phase 1 item 3 fixes this by claiming under the lock.
- **Two clocks.** Pill dwell 12 s vs. backend offer 30 s is intentional (tray fallback). If we later shorten the backend TTL, keep them documented together.
- **Timing of learning.** The edit check fires ~15 s after the take, so the pill can appear well after the user has moved on. That is existing behavior; changing it is a separate latency project, not part of this UX port.
- **Multi-word learns.** Deferred to the toast by decision 2 in Phase 0; revisit if users report it.
- **Window focus.** The pill never takes focus; VoiceOver announcements must come from the app-side `ax_announce` helper, not from an aria-live region in the webview.

## 7. Out of scope

- Reworking the judge/candidate filter (already ported; see `docs/ew-dictionary-port-plan.md`).
- Quick Add clipboard-fallback ladder, menu-bar "Add to Dictionary", contacts import card, model-download availability states.
- Cross-platform work: this is macOS overlay plumbing first, matching the upstream source; Windows can reuse the Tauri event and React component but has no `ax_announce` path.
