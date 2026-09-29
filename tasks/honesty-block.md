# Task: the honesty block (BUG-002 + 003 build-or-remove, BUG-008, BUG-011)

**Status:** planned (next sprint)
**Filed:** 2026-09-28 by productivity monitor
**Tickets:** `bugs/BUG-002-spoken-emoji-punctuation-toggles-absent.md`, `bugs/BUG-003-unload-model-timer-noop.md`, `bugs/BUG-008-language-picker-8-vs-99.md`, `bugs/BUG-011-live-preview-hold-mode.md`

## Why "honesty block"

BUG-002/003 are the headline competitive risk: features advertised in the
brain/README that don't exist. Whisperflow comparisons will expose them.
Each needs a **product decision**: build the engine path, or remove the
advertise (setting, brain line, README claim). Decide per feature, then
execute. BUG-008 and BUG-011 are the other two user-facing overstatements /
dead-end UX from the audit.

## Read first

- All four tickets in `bugs/`.
- `.agents/brain/PROJECT_BRAIN.md` §13 (the 2026-09-25 EW-port line + the
  2026-09-28 correction).
- `crates/teletype-core/src/transforms/` (where spoken-emoji/punctuation
  would hook in), `crates/teletype-inference/src/server.rs` (unload path),
  `crates/teletype-desktop/src/dictation.rs` + `ui/src/pill.tsx` (BUG-011).

## Acceptance criteria

- **BUG-002:** either (a) spoken-emoji + spoken-punctuation toggles exist
  end-to-end (setting → transform → test) or (b) the claims are removed from
  brain/README and the settings UI. No half-state.
- **BUG-003:** either (a) unload-model-after timer works (setting → timer →
  `unload()` called) or (b) removed from brain/README.
- **BUG-008:** language picker count matches options shown (the full
  whisper.cpp table is already exposed in `speech_languages_static` as of
  2026-09-28 and the UI label derives from the list length — verify and
  close, or finish the Windows "not available" labeling per the ticket).
- **BUG-011:** live preview shows interim text in the default Hold mode for
  all 4 ported pills (or the 3 that lack it are explicitly documented as
  chrome-only with a reason).

## Verification

- `cargo test --workspace` green.
- `cd ui && npx tsc --noEmit` clean.
- Brain/README/usp.md re-grepped: no claim without an engine path.
- Manual smoke per ticket.

## Out of scope

- BUG-019 (the structural guard — separate task).
- New ASR engines or streaming ASR (P2).
