# T1 — Diff card v2: recovery and feedback

Status: verified in main tree.

Implemented the safe copy fallback (strict live undo was not available). The card offers "Copy what I said", per-span inspection, "Copy full text", "Never change this", thumbs up/down, and a "Why" popover with honest unknown model/profile/reason fields. No destructive focus-specific injection is attempted.

Feedback is stored locally in an append-only JSONL file at `polish_feedback.jsonl` in the app data dir via `teletype_core::learning::feedback::FeedbackStore` and the `record_polish_feedback` Tauri command. The command validates unknown take ids, empty protected terms, and empty spans as `CommandError` (`E_REQUEST`). Feedback is never applied to Polish behavior in this task.

Files: `ui/src/pill.tsx`, new `ui/src/lib/polishFeedback.ts`, new `crates/teletype-core/src/learning/feedback.rs`, `crates/teletype-core/src/learning/mod.rs`, new `crates/teletype-desktop/src/commands/polish_feedback.rs`, `commands.rs`, `lib.rs`, and `ui/src/test/polishDiffCard.test.tsx`.

Verification: core feedback 7/7; desktop filtered 5/5; final combined core 1368 lib, desktop 607, vitest 57 files / 587 tests, tsc clean, fmt clean. Limitation: live same-window undo remains a desktop capability gap; UI copy says "Copied what you said — paste to restore", never "Undo". No brain change needed.
