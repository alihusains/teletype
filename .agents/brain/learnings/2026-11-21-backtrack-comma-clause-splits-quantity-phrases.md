---
id: L093
title: Backtrack resolver split quantity phrases at the comma, leaving the retracted number in the output
status: verified
type: debugging
area: "teletype-core/transforms/backtrack, polish pipeline"
agent: Buffy
tool: Freebuff
date: 2026-11-21
---

# Backtrack resolver split quantity phrases at the comma, leaving the retracted number in the output

## Problem

`resolve_backtracks` (T4, deterministic self-correction resolver) turned a
spoken correction into ungrammatical text containing both the retracted and
the corrected quantity:

```
"It costs twenty dollars, correction, thirty dollars" -> "It costs twenty thirty dollars"
"we have three servers, correction, four servers"      -> "we have three four servers"
"ship on Monday, correction, on Tuesday"               -> "ship on on Tuesday"
```

## Symptoms

The resolver runs before the model on every English voice take, and polish is
Off by default for IDE/terminal apps, so the corrupted string could be delivered
verbatim. Two existing unit tests asserted the corrupted output and called it a
"known limitation of the token-based clause walk", which made the bug look
intentional.

## Initial hypotheses

1. The cue matcher consumed the replacement as part of the retracted span.
2. Number words were treated as protected facts, so the retraction bailed out
   and left both numbers in place.
3. The comma boundary in the clause walk ended the retracted clause after the
   unit noun ("dollars"), not after the quantity phrase.

## Investigation

1. Reproduced with a throwaway integration test calling
   `resolve_backtracks` on realistic corrections (no model, no app).
2. Read `retract_before_cue` / `retract_after_actuality` /
   `retract_after_rather`: all three walk left from the cue and stop at the
   first token carrying a comma.
3. Confirmed hypothesis 3 by observing that `twenty dollars,` retracted only
   `dollars`, leaving `twenty` adjacent to the replacement.
4. Checked the `keep_both` protected-fact logic: it only fires when the
   replacement is *not* a number, so it did not apply here.

## Evidence

- `resolve_backtracks("It costs 20 dollars, correction, 30 dollars")` returned
  `"It costs 20 30 dollars"`.
- `resolve_backtracks("the budget is five hundred, actually six hundred")`
  returned `"the budget is five six hundred"` and an existing test asserted
  exactly that string.

## Root cause

The clause walk treats the comma-carrying token as the whole retracted clause.
That holds for single-word clauses ("friday,") but not for quantity phrases,
where the comma falls after the unit noun and the number that the phrase is
about sits to its left. The unit was removed, the number survived, and the
replacement was appended: "twenty thirty dollars".

## Fix

- `extend_clause_over_quantity` walks the clause start left over quantity words
  (digit runs, `NUM_WORDS`) and over the connectors inside a quantity
  (`a hundred`, `one hundred and twenty`), but stops at names/verbs.
- Applied only when the boundary was a comma, so the stop-mark rule ("never
  cross a sentence stop") is unchanged.
- `drop_duplicated_leading_function_word` removes a replacement's leading
  function word when it repeats the word before the retracted span, so
  "on monday, correction, on tuesday" yields "on tuesday", not "on on tuesday".
- The two tests that encoded the corrupted output now assert the corrected
  output; new tests cover quantity phrases, counted nouns, time+unit,
  determined quantities, and repeated connectors.

## Verification

- `cargo test --lib backtrack` -> 171 passed, 0 failed.
- Full `cargo test` in `teletype-core` -> all suites green (1396 lib tests).
- `cargo fmt -p teletype-core -- --check` clean; `cargo clippy -p teletype-core
  --lib` clean.
- Probed apologies and emphasis ("Sorry I'm late...", "I'd rather...") to
  confirm they are still untouched.

## Reusable lesson

A deterministic rewrite pass has to be probed with realistic *phrase* shapes,
not only minimal cue+replacement pairs: the comma-bounded clause is a
phrase-shaped assumption, and the tests that assert "known limitations" are a
signal the limitation may be a bug. When a retraction removes one half of a
number+unit pair, the output is worse than doing nothing, so the safe rule is
"retract the whole quantity phrase or bail".

## Evidence / sources

- Crates: `crates/teletype-core/src/transforms/backtrack.rs`
- Commit: fix pending in the working tree on `main @ 6ce98b8`
- Related review: polish v0.2.20 code review (backtrack P0 finding)

## Promotion

Verified by the new regression tests plus the full core suite.
