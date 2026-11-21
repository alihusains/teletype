---
id: L090
title: Polish learning stores exist but have no production caller, so profiles and feedback are inert
status: deprecated
type: debugging
area: "teletype-core/learning, polish pipeline"
agent: Buffy
tool: Freebuff
date: 2026-11-20
---

# Polish learning stores exist but have no production caller, so profiles and feedback are inert

## Problem

The polish learning loop (profile bandit + user feedback) is implemented and unit-tested, and the UI presents it as live ("Gmail: Structured learned from 12 takes", thumbs, "Never change this"). In the working tree it changes nothing: per-app profile choice is always the static default, feedback is never read, and a "Never change this" term never reaches the guard.

## Symptoms

- Profiles resolve from the static per-app table on every take; no take ever uses a learned profile.
- `polish_feedback.jsonl` grows, but nothing consumes it — the append-only store is write-only.
- Clicking "Never change this" persists a row and has no effect on later takes.
- The Insights "learned profile" UI can only render from a state that production never builds.
- The card's Why popover says `Profile: unknown` even though Rust resolved the profile for that take.

## Initial hypotheses

1. The promotion thresholds are too strict, so the bandit never changes anything.
2. The bandit state is not persisted, so it resets each launch.
3. The stores are persisted but the pipeline is handed a default state.
4. The UI is wired to a different state than the pipeline.

## Investigation

1. Read `crates/teletype-core/src/learning/profile_bandit.rs` — `BanditStore` with `load`/`save` exists (`:351-386`), `BanditState` is serde.
2. Searched for every constructor/caller: `grep -rn "BanditStore\|BanditState" --include=*.rs crates`.
3. Searched the desktop crate specifically: `grep -rn "bandit" crates/teletype-desktop/src` → zero hits.
4. Searched feedback consumers: `grep -rn "FeedbackStore" --include=*.rs crates` → only its own module and the Tauri write command.
5. Read the call site: `pipeline.rs:305` passes `&BanditState::default()`; `insights.rs:536` does the same in `compute_learning`.
6. Checked the guard's production inputs: `guard.rs:44 collect_protected(input, terms, boost_terms)` receives dictionary/boost terms only; never-change rows are not merged in.

## Evidence

- `pipeline.rs:305` — `choose_take_profile(..., &crate::learning::profile_bandit::BanditState::default(), ...)` on every take.
- `insights.rs:536` — same default in the non-bandit `compute_learning` path.
- No `BanditStore` or `FeedbackStore` construction anywhere in `crates/teletype-desktop/`.
- `profile_bandit.rs:394` — `feedback_outcome(NeverChange) => None`, so the never-change vote is deliberately not even a bandit signal.
- `pill.tsx:1620` — the UI copy reads "Saved locally — T5 will use it" while `pill.tsx:1888` prints `Profile: unknown`.

## Root cause

Two independent missing wires with the same shape:

1. **No loader.** The stores are correct and tested, but production never instantiates them, so `AppState` has no memory of learned state and the pipeline is handed an empty default. This is the same failure mode as L057 (a personalization loop that was implemented, tested, and had no production caller for weeks).
2. **No consumer for feedback.** Even with a persisted state, nothing reads `polish_feedback.jsonl`, and "never change" is not connected to the guard's existing `terms` input.

The tests passed because each unit tested its own component with a hand-built state; no test asserted the production call graph.

## Fix

Implemented 2026-11-20. `AppState` now owns a `BanditState` loaded from `polish_bandit.json`; `update_bandit` is the single locked writer. Settle verdicts are attributed via `polish_profile`/`app_id` on the dictation row, replayed into the bandit, and passed through `promote_profile` in `edit_watch::settle_sweep`. Thumbs feedback feeds the bandit only on a newly written row. `compute_learning_with_bandit` drives Insights, `undo_profile_learning` applies a real bandit undo, `choose()` honors a recorded active change, and the resolved profile is encoded into the existing instruction channel before `Pipeline::run`. The honest reason/profile reach the polish card. Never-change rows still store a term but do not reach the guard — that remains a separate gap.

## Verification

`cargo test -p teletype-core` (1376 lib + all integrations), `cargo test -p teletype-desktop` (624 lib + 26 integration, 0 failed), UI `tsc --noEmit`, and vitest 58 files / 599 tests all pass after the wiring. A live Mac acceptance run from a persisted `polish_bandit.json` is still pending.

## Reusable lesson

For any new "learning"/"personalization" store in this repo, a unit test over the store is not evidence that the feature works. Check the production call graph explicitly: every store needs (a) an `AppState` owner that loads at startup and saves on change, (b) a caller in the pipeline or settle path, and (c) one integration test that starts from a persisted file, not a default state. Treat `Something::default()` at a call site in `pipeline.rs` as a red flag: it is where a loop silently goes inert. This repo has now hit this pattern twice (L057, this one).

## Evidence / sources

- Commit: none yet — T0–T7 are uncommitted in the working tree at `e1fd01f`.
- Documentation: `docs/research/polish-retention-strategy.md` §3.2 G2, §9.4; `tasks/pi-reports/polish-t5.md`.
- Plan: `polish-feature-update.md` §2A, PF-01/PF-02/PF-03.

## Promotion

Deprecated 2026-11-20 after the production call graph was wired. Keep the reusable lesson; the specific "no caller" symptom no longer applies.
