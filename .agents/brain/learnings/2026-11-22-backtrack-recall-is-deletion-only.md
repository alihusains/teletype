---
id: L094
title: Backtrack recall comes from deletions only - drop the pronoun, keep the particle, skip the repeated connector
status: verified
type: debugging
area: "teletype-core/transforms/backtrack"
agent: Buffy
tool: Freebuff
date: 2026-11-22
---

# Backtrack recall comes from deletions only

## Problem

`resolve_backtracks` is a pre-model rewrite of every English voice take, and it
was silent on two very common spoken repairs:

```
"send the deck to Sarah, I mean, to Priya"  -> unchanged
"Can you send it over, I mean the PDF"      -> unchanged
"buy two of them, I mean three of them"     -> "buy two of three of them"  (corruption)
```

## Symptoms

The first two are harmless misses. The third is worse than doing nothing: the
resolver deleted the pronoun at the comma boundary and kept the quantity, so the
output contained both the retracted and the corrected number.

## Root cause

1. The clause walk stops at the comma, so the retracted clause was `them,` and
   the counted phrase `two of them` was split — the quantity survived.
   `us`, `them`, `it` are function words in `NON_FACTS`, so the walk never even
   reached the pronoun in `three of us,`.
2. `i mean the PDF` was rejected because the replacement was judged on its first
   token, `the` — a determiner is never a "contrast word".
3. `to Sarah, I mean, to Priya` was rejected for the same reason: the replacement
   opens with the repeated preposition `to`.

## Fix

Three rules, all of them **deletions** — the resolver never reorders words:

* A determiner-led replacement contrasts on its head (`the deck`, `the PDF`);
  the determiner stays in the output.
* A repeated connector is skipped for the contrast test and dropped from the
  output by the existing duplicate-connector rule. Skipping is gated on the
  connector repeating the word before the retracted clause, so `from Monday, I
  mean to Tuesday` cannot become `from to Tuesday`.
* A repaired object replaces a pronoun: the pronoun is removed (with the cue),
  and when a verb particle sits between them (`it over`) only the pronoun is
  removed, because `send over the deck` is as natural as `send the deck over`.
  The particle's comma is stripped so the result has no stranded comma.
* A counted object (`two of them`, `three of us`) is retracted whole: the walk
  no longer treats an object pronoun after `<quantity> of` as a boundary.

## Verification

`cargo test -p teletype-core --lib backtrack` -> 177 passed (6 new tests:
pronoun object, particle, repeated connector, quantified object, negative
cases, idempotency). Full crate suite green except `itn::tests::snapshot_eval_corpus`,
which fails because `evals/` is absent from that working tree (pre-existing,
unrelated). `cargo fmt --check` and clippy clean.

## Reusable lesson

Deterministic repair passes should be **deletion-only**: any rule that has to
move words introduces word-order bugs that are much harder to verify than a
deletion. Recall then grows by learning more deletion patterns (pronoun object,
repeated connector, counted phrase), each gated on a positive signal — a
contrast word, a determiner, or an exact repetition. When adding recall, first
check whether the current output for the same input is *worse than no rewrite*;
that is where the real bugs hide.

## Evidence / sources

- `crates/teletype-core/src/transforms/backtrack.rs`
- Related: L093 (quantity phrase split at the comma) — same class of bug.
- Commit: pending in the working tree on `main @ 6ce98b8`.

## Promotion

Verified by the new regression tests plus the full core suite.
