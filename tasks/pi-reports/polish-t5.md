# T5 — Polish learning loop v1

Status: verified in main tree.

Added `learning/profile_bandit.rs` and wired Learning data into `insights.rs` and `InsightsScreen.tsx`, with a profile-choice hook in `pipeline.rs`. Promotion rule: per `(app, profile)` tally of settled/kept/undone plus thumbs; challenger must be adjacent (Clean↔Natural, Natural↔Structured), have >=12 settled takes, <=1 negative, and score >= incumbent + 0.15; at most one automatic change per app per 7 days. Transform/Off are never auto-selected. Ambiguous spans, settled-none, NeverChange, and CopyRaw produce no signal.

Undo is exposed as `ProfileUndoAction` in the Insights payload; the UI patches `polishAppProfiles[scopeKey]` via existing settings commands and hides the row.

Verification: learning filter 211/211; final combined core 1368 lib, desktop 607, vitest 57 files / 587 tests, tsc clean. Lead decisions: desktop must load `BanditStore` in `commands/history.rs`, feed tallies from `edit_watch.rs` and `polish_feedback.rs`, and pass bandit state into the pipeline before runtime learning affects takes. No brain change needed.
