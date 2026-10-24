---
id: L059
title: A 2B editor follows five numbered edits but shortens by deleting facts and ignores capitalization advice in the casual register
status: verified
type: debugging
area: "Polish prompt / Qwen3.5-2B editor path"
agent: "qoder"
tool: "llama-server editor matrix, cargo test, tsc/vitest, agent-browser screenshots"
date: 2026-10-24
---

# A 2B editor follows five numbered edits but shortens by deleting facts and ignores capitalization advice in the casual register

## Problem

The 2B Polish prompt had to do more than S1-mini: make the text more concise,
reword for clarity, reorder for readability, add structure, and keep the
selected tone. A stronger editor prompt was expected to trade cleanup safety
for editing. The question was how much of the five-edit promise the 2B could
keep, and which guarantees had to move into code.

## Symptoms

- First numbered editor prompt: 19 of 24 cells clean.
- Fidelity case under casual, semi-casual and formal dropped `readme`,
  `Tuesday` and `2 pm` while shortening. The prompt said "Keep every fact";
  the model treated sources and deadlines as optional.
- Clean casual input and casual bullets came back without a terminal period.
- Under casual and semi-casual the model wrote `hey, could you ...` even when
  the prompt asked for capitalization. The same lowercase `hey` survived a
  mid-prompt rule, a last-line rule, a clause inside the tone rule, and an
  example written with a capital.
- A 0.34 OffTopic floor rejected a legitimate aggressive rewrite (it shared
  0.21 to 0.25 of content words), which would have fallen back to raw text.

## Investigation

1. Ran 6 cases x 4 tones through the app's llama-server flags at temperature
   0, with the checker mirroring the app pipeline: prose folding, first-word
   capitalization, a 0.15 OffTopic floor, and number normalization.
2. Variant A named dates, times and file names in the fidelity rule. It
   recovered `Tuesday` and `2 pm` for casual and semi-casual but not `readme`,
   and formal still dropped the whole deadline clause.
3. Variant F put the detail categories in the concise rule. Casual still wrote
   `q three`; the other tones passed.
4. Variant H replaced the filler list with "tighten the wording but never
   delete a fact, name, date, time, source or recipient". All four tones kept
   every fact and every filler still disappeared. This was the winner.
5. A terminal-punctuation rule fixed prose and casual bullets. A rule placed
   only in the structure section did not.
6. Four capitalization phrasings all failed. The model treats a lowercase
   greeting as part of the casual register, not as a capitalization error.

## Root cause

"Keep every fact" is too abstract for a 2B to apply while compressing. The
model optimizes the shorter output and keeps the facts it considers central.
Naming sources, recipients, dates, times and file names turns an abstraction
into a checklist. Capitalization of a casual interjection is a register
decision the tone instruction overrides, so no wording of the capitalization
rule beats the tone instruction.

## Fix

Shipped prompt (short, numbered, rule-first):

- Five edits, all unconditional.
- Concise rule: "tighten the wording but never delete a fact, name, date,
  time, source or recipient."
- Fidelity rule: "Keep every name, date, time, number, URL, email and file
  name accurate. Write spoken forms in real form ...".
- "End every sentence, including each bullet, with a period, question mark or
  exclamation mark."
- Tone, language, no-em-dash and no-answer rules unchanged.

Code guarantees on the 2B Polish path:

- `prose_from_bullets` folds returned bullets into paragraphs when Structure
  is prose.
- `capitalize_leads` capitalizes the first word of the text and each bullet,
  leaving URLs, paths and identifiers alone.
- `validate_editor` uses a 0.15 OffTopic floor; an unrelated answer still
  scores near zero and is rejected.

Result: 24/24 cells clean with the final prompt. Rambling 38 words in, 24 to
28 out depending on tone, fillers gone and facts kept. Fidelity holds at all
four tones. Casual and semi-casual keep `hey` and contractions; semi-formal
and formal return "I am writing to request your review ...". Dictated
questions stay questions.

## Verification

- `evals/runs/model-bakeoff-2026-10-24/report.md` Round 5, raw
  `editor-matrix.json`, harness `editor_matrix.py`.
- `cargo test -p teletype-core -p teletype-inference` green (790 core tests).
- `cargo fmt --check` clean; `tsc --noEmit` clean; 194 UI tests green.
- Live screenshot read-back in the `?demo&ready&qwen35` preview: the Styles
  copy and the Models Polish hint both show the 2B wording.

## Reusable lesson

On a small model, phrase-level rules work when they name the concrete
categories to preserve. Do not ask a model to "keep the facts" while also
asking it to shorten; list the fact types. Formatting guarantees that repeat
across runs (bullet shape, capitalization, punctuation) belong in code, not in
another prompt clause.
