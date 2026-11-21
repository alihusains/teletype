# Personalization v2 — visible, screen-confirmed learning

Status: implemented in the main working tree, uncommitted.

## What changed
- **Screen-confirmed names:** `DictationRow` now persists `screen_terms` captured at hotkey-down. The observation moment feeds those terms into `JudgeContext`, lexicon promotion, and dictionary signal extraction. A correction matching an on-screen term is promoted immediately, waives the common-first-name skip, and uses a 0.45 phonetic floor. All other edits keep the old strict rules.
- **Polished words are not auto-added:** a word the model produced is learned only when it is screen-confirmed, already known, or explicitly protected. This avoids teaching hallucinations.
- **Never change → guard:** `never_change_terms(data_dir)` reads `polish_feedback.jsonl`; those terms are merged into the per-take `screen_terms`, so they reach the polish guard, prompt, and rescoring.
- **Visible learning:** word-only learning now emits a “Learned: …” toast/tray notice with a working Undo even when no preference changed. A stale toast cannot undo newer learning because word-only events carry an `offer:<id>` sentinel.
- **Insights:** learning summary is computed whenever `learning.sqlite` exists, not only when the language coach is on. “Ready to apply” candidates are displayed (capped at 3) but never applied automatically.

## Verification
Core 1390 passed, desktop 627 passed, eval self-test PASS, tsc clean, vitest 58 files / 601 tests, fmt clean, brain freshness PASS. Brain updated with D047.

## Remaining gap
ASR boost capacity is still 0, so learned lexicon terms do not yet influence recognition. Re-enabling it requires a measured bakeoff and was deliberately not bundled here.
