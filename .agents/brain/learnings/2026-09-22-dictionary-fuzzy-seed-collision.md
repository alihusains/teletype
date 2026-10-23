# L002: Builtin dictionary seeds must be exact-only, not fuzzy

**ID:** L002
**Date:** 2026-09-22
**Agent:** MiMo (OpenCode)
**Project area:** teletype-core / dictionary / pipeline
**Status:** verified
**Evidence:** failing-then-passing test `seeded_builtins_do_not_falsely_correct_common_words`; live dictation transcript corruption reported by the user

## Problem

Dictated text silently gained words that were never spoken: `iOS`, `API`, `CLI`
appeared in the middle of ordinary sentences ("but it is iOS doing this").

## Symptoms

- Transcript contains tech acronyms (API, CLI, iOS, EG-1, Claude, macOS, VS Code)
  where the speaker said ordinary words.
- Worst observed rewrite: `"I am apt to opt in"` -> `"I am API to API in"`.

## Initial hypotheses

1. ASR itself hallucinating acronyms (ruled out: user said transcription was fine).
2. EG-1 transform inserting them (ruled out: direct inference outputs were clean).
3. Dictionary post-correction false-matching (confirmed).

## Investigation

- `correct_with_dictionary` (pipeline.rs) rewrites any word within edit
  distance <= 2 of a dictionary entry. This feature exists for user-taught
  rare proper nouns.
- This session seeded `BUILTIN_DICTIONARY_WORDS` (API, CLI, ChatGPT, Claude,
  EG-1, GitHub, iOS, macOS, OpenAI, VS Code) into the same pool, i.e. common
  short acronyms went through the fuzzy sweeper.
- Cheap-hypothesis test: probe with common words near the seeds.

## Evidence

Prose probe pairs (edit distance <= 2): `apt|opt -> API`, `its|ice|ions -> iOS`,
`clip|call|club -> CLI`, `egg|ego -> EG-1`, `macro|macros -> macOS`,
`cloud|clause -> Claude`, `his code -> VS Code`.
Failing test output before the fix: left `"I am API to API in"`, right
`"I am apt to opt in"`.

## Root cause

Wrong match policy for seeded words: builtin seeds are common enough that
distance-2 is not "a near-miss of a proper noun", it is ordinary speech.

## Fix

- `DictionaryWord.fuzzy` field (serde default `true`, so existing user words
  keep the taught-mishearing behavior).
- Seeds inserted with `fuzzy: false` -> exact match only (punctuation-folded,
  case-insensitive) via `fold_lower`.
- `BUILTIN_DICTIONARY_VERSION` bumped 1 -> 2; the seed merge now also flips
  already-seeded words to exact-only, so existing installs migrate on next
  start (user's dictionary.json verified: `builtinVersion 2`, seeds
  `fuzzy: false`, user's own `AgentDesk` still `fuzzy: true`).

## Verification

`cargo test -p teletype-core` 169 pass incl.
`seeded_builtins_do_not_falsely_correct_common_words`; workspace tests, clippy,
`tsc --noEmit` green; dictionary.json observed at version 2 after restart.

## Reusable lesson

Fuzzy matching is only safe when the candidate set is rare. Never put common
short tokens into an edit-distance corrector without an exactness gate; EW's
own alias lists are explicit enumerations, not fuzzy sweeps.
