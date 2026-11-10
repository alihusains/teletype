# L090: Dictation silent-corruption from over-eager matching (possessive 's, bare emoji nouns)

**ID:** L090
**Date:** 2026-11-09
**Agent:** pi (lead)
**Project area:** teletype-core / pipeline / emoji
**Status:** verified
**Evidence:** failing-then-passing tests `possessive_keeps_its_apostrophe_s` (pipeline.rs) and `bare_common_nouns_require_the_suffix` (emoji.rs); live dictation transcripts in `~/Library/Application Support/com.teletype.app/transcripts/2026-11-09.md`

## Problem

Two independent silent-corruption bugs surfaced the same way — the user dictated
ordinary English and the output was mangled:

1. **Possessive dropped:** dictating "Zowie's" produced "Zowie" (the `'s` vanished).
2. **Common noun → emoji:** dictating "third party" produced "third 🎉".

## Symptoms

- `"analyze Zowie's log"` → `"analyze Zowie log"` (possessive `'s` removed).
- `"Composio is a third-party service"` → `"Composio is a third-🎉 service"`.
- Both were silent: no error, the text just looked wrong.

## Initial hypotheses

- For the emoji: assumed the ASR (Parakeet) was emitting the glyph. Ruled out —
  `🎉` is not in the Parakeet vocab (`parakeet_vocab.json`), so the model cannot
  emit it.
- For the possessive: assumed the alias path (`Zoe`→`Zowie`) was over-consuming.
  Ruled out — the reported case was the canonical word "Zowie's", not the alias.

## Investigation

Traced each dictation through the pipeline stages in order (ASR → dictionary
correction → `emoji::restore` → `format_spoken_emoji` → polish). The emoji glyph
only exists in two places, so the source had to be one of them.

- `format_spoken_emoji` (spoken_emoji.rs) requires an explicit trigger word
  ("emoji"/"emoticon") after the phrase, so bare "party" cannot fire it.
- `emoji::restore` (emoji.rs, step 5, behind `restore_emoji`) matches triggers from
  `emoji-dictionary.json`. That file had **bare** single-word triggers for ordinary
  vocabulary: `"party"`, `"muscle"`, `"eye"`, `"eyes"`. That is the emoji source.
- For the possessive: `correct_with_dictionary` builds a "bare" token by filtering
  to alphanumerics, so `"Zowie's"` → `"Zowies"`, which fuzzy-matched `"Zowie"`
  (edit distance 1, similarity 0.833 ≥ the 0.80 standard floor) and was replaced
  with the canonical, dropping the `'s`.

## Root cause

Both are the same class of bug: **a matching rule that fires on ordinary
vocabulary instead of only on the intended, unambiguous surface.**

- The emoji dictionary violated its own documented ambiguity rule ("a bare common
  noun is only a trigger when unambiguous on its own").
- The dictionary fuzzy matcher's bare-token extraction destroys apostrophes, so a
  possessive becomes a near-plural that fuzzy-matches the base word.

## Fix

- `emoji-dictionary.json`: removed the bare triggers `party`, `muscle`, `eye`,
  `eyes`; require the explicit `<word> emoji` form (added `party emoji`;
  `muscle/eye/eyes emoji` already existed). `sparkles`, `checkmark`, `tada` stay
  bare (not ordinary vocabulary).
- `pipeline.rs` (`correct_with_dictionary`, single-word tier): when a token has an
  apostrophe, match only the core before the first apostrophe and re-attach the
  apostrophe suffix in the output. `"Zowie's"` now matches core `"Zowie"` and keeps
  the `'s`.
- `pipeline.rs` (`correct_with_dictionary`, entry build): index single-word
  aliases as exact-only entries so the per-token tier also corrects an alias
  possessive — `"Zoe's"` (ASR hearing `"Zowie's"` as `"Zoe's"`) -> `"Zowie's"`.
  Pass 1 already rewrites non-possessive aliases; the apostrophe suffix stops that
  exact match, so the per-token tier fills the gap. Alias entries are `fuzzy: false`
  to avoid new near-miss false positives.

## Verification

- `cargo test -p teletype-core`: 1120 passed, 0 failed (incl. `possessive_keeps_its_apostrophe_s`,
  which now also asserts `"Let me analyze Zoe's performance."` ->
  `"Let me analyze Zowie's performance."`, and `bare_common_nouns_require_the_suffix`).
- `cargo test -p teletype-desktop`: 591 passed; `-p teletype-inference`: 130 passed.
- `npx tsc --noEmit`: clean. App rebuilt and serving HTTP 200.

## Reusable lesson

When dictated text comes back corrupted, do not assume the ASR. Walk the pipeline
stage-by-stage (ASR → dictionary → emoji → polish) and, at each text-rewriting
stage, ask: "can this rule fire on an ordinary word the user actually said?" The
two highest-risk stages are the ones that match on bare tokens (dictionary fuzzy,
emoji restore) — they are where possessives, contractions, and common nouns get
eaten. Guard them: possessives/contractions match on the core and preserve the
suffix; bare common-noun emoji triggers require the explicit "emoji" suffix.
