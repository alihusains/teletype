---
id: L-20261022-insights-window-full-history
title: A windowed aggregator must receive full history, not a pre-filtered slice
status: verified
type: debugging
area: insights / history persistence
agent: Qoder (lead) + pi (implementation)
tool: cargo test, SQLite, live app logs
date: 2026-10-22
---

# A windowed aggregator must receive full history, not a pre-filtered slice

## Problem

Insights gave different answers for the same metrics when the user switched
between Today / Week / Month / Year / Lifetime / Custom. Lifetime was supposed
to cover everything since first use but did not. Learned-fix counters read as
zero.

## Symptoms

- Totals, "all-time" habits, records and milestones all moved with the range
  filter; the user read that as the filters being broken.
- Bounded ranges undercounted because they read the in-memory snapshot
  (`DictationHistory::MAX_ENTRIES = 1000`), not the database.
- `dictionary_learned` and `personalization_corrections` were missing entirely
  from the response after the refactor.

## Initial hypotheses

1. The SQLite writer trimmed old rows, so lifetime could never be complete.
2. The UI labels and the backend disagreed about which metrics are
   range-scoped.
3. The window filter was applied twice, or not at all.

## Investigation

1. Read `insights::compute_for_window(all, window, now)` and its tests.
2. Traced `get_insights` in `commands/history.rs`: it passed `window_hist`
   (already filtered to the selected range) as `all`.
3. Checked `persist::push_history` / `read_history`: the DB trimmed at 1000
   rows and bounded ranges never read the DB at all.
4. Diffed the pi implementation against the intended contract and found the
   two learned counters had been dropped.

## Evidence

- The all-time pass in `compute_for_window` reads `all.entries` directly, but
  `get_insights` handed it a range-filtered vector. So "all-time" views were
  functions of the selected range.
- `MAX_HISTORY_ENTRIES` was enforced on insert; nothing older than 1000 takes
  survived to be counted.
- `git diff` showed the two counter assignments removed, with their tests
  still green because no test asserted them.

## Root cause

The aggregator's contract is "give me everything; I filter the window myself".
The caller violated it three ways: pre-filtered input, a capped in-memory
source for bounded ranges, and missing counter wiring.

## Fix

- `get_insights` loads the full DB (`read_history(dir, None)`), merges the
  in-memory rows (memory wins by id), filters legacy blank rows, and passes
  the full history to `compute_for_window`, which applies `in_window` itself.
- `push_history` no longer trims; `MAX_HISTORY_ENTRIES` now caps only the
  `history_all` snapshot.
- `read_history` is read-only with a short busy timeout, safe alongside the
  writer thread.
- Restored `dictionary_learned` and `personalization_corrections`, scoped by
  `in_window`.
- UI labels now state the split: totals/apps/phrases/pace follow the range;
  habits, records, milestones and first activity are all-time.

## Verification

- `cargo test -p teletype-core --lib`: 775 passed, 0 failed.
- `cargo test -p teletype-core transforms`: 138 passed, 0 failed.
- `cargo build -p teletype-desktop`: exit 0.
- `npx tsc --noEmit`: exit 0.
- Browser render check: range labels and all-time headings present; range
  buttons switch without error.

## Reusable lesson

When a function takes `(all_data, window)`, never pass it data filtered by
the same window. Keep the unfiltered source authoritative, filter exactly
once inside the aggregator, and assert the contract with a test where a
window changes totals but not all-time views. Also: a refactor that drops an
output field passes green tests unless a test asserts the full response
shape, so test the fields the UI actually reads.

## Evidence / sources

- User report: "Even after selecting different filters ... lifetime should
  give the answer for a collective ... till the end of today."
- Decision D023 in `.agents/brain/PROJECT_BRAIN.md`.
- `crates/teletype-core/src/insights.rs`,
  `crates/teletype-desktop/src/commands/history.rs`,
  `crates/teletype-core/src/persist.rs`.
