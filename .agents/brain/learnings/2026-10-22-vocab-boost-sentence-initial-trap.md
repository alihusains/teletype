# Vocabulary boost: sentence-initial capitals and function words are not vocabulary

**ID:** L047
**Date:** 2026-10-22
**Agent:** Qoder (lead session, earnings22 re-run)
**Area:** evals / `scripts/eval/subset.py` / `earnings22-vocab`
**Status:** verified

## Problem

`extract_vocab_terms` builds the per-file boost list that the bake-off feeds to
engines (`ttf_set_vocab` on Ultra, `initial_prompt` on Whisper, the CTC
rescorer on sherpa/TDT). The first version treated every capitalized word as
an entity. It only dropped the first word of the file's first sentence
(`m.start() == 0`), so every later sentence contributed its opening capital,
and the lone-word pass added any capitalized token anywhere. A 13k-character
earnings call contributed `The`, `Today`, `Please`, `Thank` and similar
function words as boost terms.

## Symptoms

- `vocab.json` for all six earnings22 files was full of function words and
  sentence starters, not just company/person names.
- Boost results were confounded: Ultra's boost-on cell looked catastrophic
  (6.88% -> 14.42% verbatim), Whisper appeared to benefit
  (69.62% -> 65.98%), and the pre-fix numbers could not be compared with any
  future run because the term list itself was wrong.
- Recall was high on paper while entity accuracy fell, which is what a
  function-word-heavy list produces: the engine places common words correctly
  and entity terms in the wrong places.

## Investigation

1. Dumped `~/.cache/teletype-eval/earnings22-vocab/vocab.json`; the first
   file carried `Conference Call`, `Vice President` (legitimate) beside
   `The`, `Today`, `Please` (grammar).
2. Traced to the phrase regex and its `m.start() == 0` special case: it encoded
   "the first phrase is sentence-initial" but not "any phrase is".
3. Confirmed the single-word pass had no sentence-position or stopword check.

## Root cause

Capitalization carries no entity signal at a sentence start, and function
words are capitalized for grammar. The extractor had no sentence-position
test and no stopword list, so the boost budget was spent on grammar.

## Fix

- `_at_sentence_start(text, start)` scans back over whitespace and opening
  quotes to the previous `.`, `!`, `?`, `:`, `;` or newline.
- Phrase pass: drop the first word when the phrase starts a sentence, then
  trim leading/trailing `STOPWORDS`; require at least two remaining words.
- Single-word pass: skip sentence starts and stopwords.
- Module docstring states the caveat; a doctest pins the behavior
  (`Good morning. Today we thank Loma Negra and Sergio Faifman.`).

## Verification

- `python3 -m doctest scripts/eval/subset.py` -> exit 0.
- Regenerated the six-file `vocab.json` (2026-10-22 23:33); first file now 73
  terms led by `Loma Negra`, `Sergio Faifman`, `Morgan Stanley`, `EBITDA`.
- Re-ran all six `earnings22-vocab` cells at `981900e`; pre-fix reports kept
  under `*-pre-vocab-fix/`:
  - Ultra: verbatim 6.88% -> 10.61% (was 6.88% -> 14.42%), entity 84.5% ->
    75.4%; still a regression, p50 2.6 s -> 19.7 s.
  - TDT v3: 12.18% -> 12.18% (was 12.18% -> 100.00%), entity 84.5% both
    ways; the 100% cell was the empty-word-list harness bug, now confirmed
    to be inert.
  - Whisper: 69.62% -> 68.94% (was 69.62% -> 65.98%), entity 28.8% ->
    19.8%, p50 31.2 s -> 34.6 s; the apparent pre-fix gain was flattered by
    common-word overlap, not a dependable win.
- Full rollup and evidence gaps: `.agents/brain/asrs.md`.

## Reusable lesson

A heuristic term list is a label. If the extractor cannot distinguish grammar
from entities, every boost metric built on it measures the extractor as much
as the model. Exclude sentence starts and function words, pin the behavior
with a doctest, and re-run the boosted cells whenever the extractor changes.
Never compare recall across two different term lists: the denominators differ.
