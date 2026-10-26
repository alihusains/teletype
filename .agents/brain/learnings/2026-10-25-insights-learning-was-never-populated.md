---
id: L060
title: The Insights learning card was dead in production because the command never filled `insights.learning`
status: verified
type: debugging
area: "insights / learning store / desktop IPC"
agent: "qoder"
tool: "cargo check -p teletype-desktop, vitest, Chrome DevTools preview"
date: 2026-10-25
---

# The Insights learning card was dead in production because the command never filled `insights.learning`

## Problem

The Insights screen has a personalisation section driven by
`Insights.learning`: learned terms, edit rate, and the most-corrected phrase.
The core function that builds it (`insights::compute_learning`) was tested and
the UI rendered it, but in a real install the section always showed its empty
state.

## Symptoms

- The learning summary was populated in unit tests and in the `?demo` browser
  preview (which injects `DEMO_INSIGHTS`), so every visual check looked fine.
- The desktop command that serves `get_insights` constructed `Insights` and
  populated impact, records, milestones, heatmap and daily, then returned it
  without ever touching `learning`.

## Root cause

`crates/teletype-desktop/src/commands/history.rs` never opened the learning
store. `compute_learning` had exactly one caller: the tests. The preview
fixture supplied the field, which is why the dead path was invisible from the
UI.

## Fix

After the history aggregation, if `<app_data>/learning/learning.sqlite` exists,
open it and fill the section:

```rust
let dictations = store.recent_dictations(300).unwrap_or_default();
let lexicon = store.lexicon_all().unwrap_or_default();
let accuracy = store.personal_accuracy_history(10).unwrap_or_default();
insights.learning = insights::compute_learning(&dictations, &accuracy, &lexicon, now);
```

The existence check keeps a fresh install from creating an empty store just to
read it. A store that will not open logs a warning and leaves the section
empty rather than failing the whole Insights request. The 300-row cap bounds
the top-fix pass, which runs a phonetic similarity over every changed span and
re-runs whenever the window regains focus.

## Verification

- `cargo check -p teletype-desktop` clean.
- New `ui/src/__tests__/insights-proof.test.ts` and
  `ui/src/test/insightsProof.test.tsx`; UI suite 26 files / 223 tests green,
  `tsc --noEmit` clean.
- Live preview at `/?demo` shows learned terms, the edit rate and the
  most-corrected phrase; wide/narrow and light/dark screenshots read back.

## Reusable lesson

A demo/preview fixture can make a dead backend field look healthy. When a card
is permanently empty in production, grep for writers of the field it reads,
not just for its renderer; if the only callers of the compute function are
tests, the IPC layer is the bug. Optional reads from a sidecar store should
gate on the file existing so a read does not create state.
