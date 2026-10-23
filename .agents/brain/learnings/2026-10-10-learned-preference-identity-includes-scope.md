---
id: L023
title: Learned preference identity includes scope
status: verified
type: bugfix
area: personalization / learning
agent: Teletype lead, Pi implementation
tool: pi openai/Qwen/Qwen3.8-27B-FP8
date: 2026-10-10
---

# Learned preference identity includes scope

## Problem

`apply_signals` merged inferred preferences by exact description only. The same
learned behavior observed in different application categories could therefore
share one record and count, leaving later observations attributed to the first
scope.

## Symptoms

Repeated edits in one category could increase confidence for a preference that
was also observed in another category. `confidence_for` used the same
description-only lookup and could report confidence from the wrong category.

## Initial hypotheses

The issue could have been in app classification or preference-packet scope
resolution. Source inspection showed the merge lookup itself omitted scope.

## Investigation

1. Inspected `apply_signals`, `confidence_for`, `PreferenceScope`, and packet
   ordering in `crates/teletype-core/src/personalization/`.
2. Confirmed `apply_signals` matched inferred records by description without
   checking their scope.
3. Added tests for equivalent descriptions in different scopes, repeated
   evidence in one scope, and explicit preference priority.

## Evidence

- The new cross-scope test demonstrates separate counts and confidence.
- Explicit preferences remain untouched by the inferred learning loop and
  retain priority in the resolved packet.

## Root cause

Preference identity did not include `PreferenceScope` in the merge and
confidence lookups.

## Fix

`apply_signals` and `confidence_for` now match normalized (trimmed,
case-insensitive) descriptions and identical scopes. Signal extraction,
thresholds, and serialization are unchanged.

## Verification

- Pi: `cargo test -p teletype-core --lib personalization::learn::tests` —
  17 passed, 0 failed, exit 0.
- Pi: `git diff --check` — exit 0.
- Lead reviewed the changed source diff and Pi's captured command results; no
  tests were rerun by the lead.

## Reusable lesson

Whenever learned observations are scoped, use the same compound identity
(`normalized description + scope`) for insertion, deduplication, confidence,
and lookup. Add a cross-scope regression test so repeated observations cannot
silently leak evidence between application categories.

## Evidence / sources

- `crates/teletype-core/src/personalization/learn.rs`
- Pi session `teletype-learning-scope-20261010-r5`

## Promotion

Verified by the targeted regression suite and diff review.
