# Polish gate: a forbidden signal that intersects a case's own required rendering makes that case unsatisfiable

**ID:** L040
**Date:** 2026-10-22
**Agent:** pi (t2r polish-gate repair)
**Area:** evals / polish-adversarial / polish_behavior_gate.py
**Status:** verified

## Problem

`evals/polish-adversarial/behavior-cases.jsonl` was created (T2) with several
cases whose `forbidden` list contained a signal that is *inside the only
correct rendering the case also requires* via `must_contain`:

- `num-time` requires `9:15`/`nine fifteen am` but forbids `9 15` — and the
  judge's `contains_phrase` normalizes punctuation to spaces, so the correct
  `9:15` normalizes to `9 15` and trips the forbidden signal.
- `tech-identifiers` required `FEATURE_FLAGS` but its original `forbidden`
  list included `FEATURE_FLAGS_ENABLED`-style variants that the boundary-aware
  matcher still flagged.
- `ent-person` required keeping `Priya` but forbade the phrase `priya to`.

Net effect: no real model output could pass those cases end-to-end, so a
"correct-output passes the gate" self-test could never be written against the
real corpus.

## Why it happened

The forbidden lists were written to catch *over-polishing* (e.g. "the model
renders the time as 9 15 instead of 9:15"), but the author did not check that
the judge's own normalization (punctuation→space, casefold) would not make a
correct rendering collide with its own trap.

## Root cause

`contains_phrase` strips punctuation: `9:15` → `9 15`. A forbidden token that
is the *punctuation-stripped form of a required token* can never be satisfied.
Same for case/separator variants of a required identifier.

## Fix

1. Removed the self-intersecting signals from the corpus (each case's
   `forbidden` now only contains signals a *genuinely incorrect* output would
   produce). `num-time`'s `9 15` is now documented in `notes` as a misread
   signal rather than a live forbidden string; the case's real trap (wrong
   clock time / wrong day) still fires via `must_contain` + the other
   forbidden strings.
2. The `--self-test` end-to-end wiring check (test #13) uses a small
   self-consistent **synthetic** corpus written to a temp dir, not the real
   17-case corpus, so the pass/fail exit-code wiring is proven without
   depending on every real case being stub-satisfiable.

## Reusable lesson

When authoring an eval corpus with `forbidden` + `must_contain` fields that a
string-containment judge grades, run the correct reference output through the
judge before trusting the corpus. A forbidden string that normalizes to (or
contains) a required string is an unsatisfiable case, not a stricter check.
Add a corpus-validation step (reference-output dry run) to `--list-corpus`
if this is ever re-authored.

## Evidence

- `python3 scripts/eval/polish_behavior_gate.py --self-test` → PASS (exit 0),
  proving the correct→exit-0 / over-polished→exit-1 wiring.
- `--list-corpus` → 17 cases, 6 behaviors, no structural problems.
- 16 of 17 real cases confirmed to fire a `critical` finding on a
  deliberately over-polished variant (see the verification run in the
  t2r-polish-gate-repair report); `num-time` is the honest exception and is
  documented as such rather than silently "fixed" into a weaker trap.
