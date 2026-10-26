---
id: L066
title: learning.accuracy (personal_accuracy_history) is always empty in production
status: candidate
type: debugging
area: "crates/teletype-core/src/learning/, Insights UI"
agent: "qoder"
tool: "code inspection + grep"
date: 2026-10-26
---

# learning.accuracy (personal_accuracy_history) is always empty in production

## Problem

While doing a "bolder" pass on the Insights tab, I wanted to build an
"accuracy improving over time" trend section. It looked like the ideal proof
of *becoming* more productive, and the data type (`learning.accuracy`) already
flows through `insights::compute_learning`. But the section would be dead for
every real user.

## Symptoms

- `PersonalizationInsights.accuracy` is a real, populated-in-shape field, and the
  desktop layer reads it: `store.personal_accuracy_history(10)` in
  `crates/teletype-desktop/src/commands/history.rs:206`.
- The only place that ever WRITES rows is `run_replay`
  (`crates/teletype-core/src/learning/replay.rs:198`).
- `grep -r run_replay` outside `replay.rs` returns **no matches**: every caller
  lives in `replay.rs`'s `#[cfg(test)]` module. Nothing in the app or the desktop
  backend invokes it at runtime.

## Initial hypotheses

1. `run_replay` runs on an idle timer somewhere I had not yet grepped.
2. Accuracy rows are written inline during a normal dictation turn.
3. The store seeds/backs the field from another path.

## Investigation

1. Read `replay.rs`: `run_replay` re-transcribes recent takes and records
   accuracy, but only when called.
2. Grepped `run_replay` across `crates/`, excluding `replay.rs` itself: zero
   production callers (tests only).
3. Grepped `personal_accuracy_history`: the reader is wired in
   `history.rs:206`, but the writer (`run_replay`) has no runtime entry point.
4. Confirmed `run_replay` is only referenced from its own `#[cfg(test)]` block.

## Evidence

- `grep -rn run_replay crates/ --include=*.rs` -> all hits are in `replay.rs`;
  every one inside the `mod tests` region.
- `grep -rn run_replay crates/` excluding `replay.rs` -> no matches.
- `store.rs:922` test: `personal_accuracy_history(10).unwrap().is_empty()` on a
  fresh store, i.e. the table starts empty and is only filled by replay.

## Root cause

`run_replay` is a real, implemented feature, but it is never scheduled or
invoked by the running app. So `personal_accuracy_history` stays empty for real
users, and `learning.accuracy` in the Insights payload is always `[]`.

## Fix

No code fix. Decision: do NOT build an accuracy-trend UI. It would be a
permanently dead section, violating the repo's fix-or-remove dead-controls rule.
The bolder Insights pass instead leans on data that actually exists in
production: streak, heatmap, words, per-active-day, best streak, pace multiplier.

## Verification

- Grep proves the writer has no runtime caller.
- A fresh-store test asserts the history is empty by default.
- Demo UI works only because the dev `browserPreview.ts` fixture injects fake
  `accuracy` points; that masking is exactly why this is easy to miss.

## Reusable lesson

Before building any Insights (or other) UI section on a data field, verify the
field has a **production writer**, not just a populated shape and a reader. A
field that reads fine in the demo fixture can be permanently empty in the app.
`learning.accuracy` is the known example: `run_replay` has no runtime caller, so
accuracy is always empty for real users. Don't ship an accuracy UI until
`run_replay` is actually scheduled.

## Evidence / sources

- `crates/teletype-desktop/src/commands/history.rs:206` (reader)
- `crates/teletype-core/src/learning/replay.rs:198` (only writer, tests-only callers)
- `crates/teletype-core/src/learning/store.rs:922` (empty-by-default test)
- `ui/src/dev/browserPreview.ts` (fixture masks the empty field)

## Promotion

Promote to `verified` once a human or agent review confirms `run_replay` still
has no production caller. Re-check this before any accuracy-feature work, since
the field could gain a runtime caller later and become real.
