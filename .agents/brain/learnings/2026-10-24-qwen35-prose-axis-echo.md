---
id: L058
title: On the 2B, a "prose" style setting suppressed cleanup until the prompt made cleanup unconditional
status: verified
type: debugging
area: "Polish prompt / Qwen3.5-2B control line"
agent: "qoder"
tool: "llama-server matrix harness, cargo test, vitest"
date: 2026-10-24
---

# On the 2B, a "prose" style setting suppressed cleanup until the prompt made cleanup unconditional

## Problem

Qwen3.5-2B was given the S1 control line so the same Styling / Structure /
Context toggles work on the all-rounder model. The 16-cell axis matrix showed
that every cell with `[Structure: prose]` returned the transcript unchanged,
including punctuation and capitalization.

## Symptoms

- 8 of 48 matrix cells clean.
- Casual + prose + fidelity came back as `can you send the report to shivon at
  example dot com ...` with no capital, no terminal period and `example.com`
  reported lost.
- Lists cells were clean, and the same model applied all four explicit rules
  under a rule-first prompt, so this was not a capability limit.

## Investigation

1. Ran 4 stylings x 2 structures x 2 contexts x 3 cases through the app's
   llama-server flags at temperature 0. Every prose cell echoed.
2. Compared with Experiment 2, where the same model applied concise,
   structure, tone and reorder rules. The difference was prompt shape, not
   model.
3. Read the system prompt as the model would: the settings block said
   "Structure sets the shape ... prose keeps running sentences", and nothing
   said cleanup was still mandatory. The model treated "prose" as "leave the
   wording alone".

## Root cause

The axes were written as independent instructions. A model that must choose
between "keep the sentence shape" and the general cleanup rules resolves the
conflict by obeying the most specific, most recent instruction, which was
`prose`. So the style setting silently disabled the normalizer.

## Fix

Make cleanup unconditional and put that rule first:

> Always clean the transcript: fix punctuation, capitalization, filler words,
> false starts and self-corrections. The settings change register and shape,
> never whether the text is cleaned.

Also define list items as "a complete sentence" so bullets do not degrade into
fragments.

Result: prose 24/24 clean, lists correct in all 24 cells (12 flagged only by a
checker rule that expects a terminal period on the last bullet), no lost
fidelity tokens, no em dashes, styling still visible (`Can you ...` at casual
and semi-casual, `Please ...` at semi-formal and formal).

## Rejected probe

Adding `Preferences: warm, concise, no jargon` to the control line made
casual + prose + fidelity echo verbatim in 2 of 8 cells and lose
`example.com`. Preferences are not part of the 2B polish prompt; the Styles
Compare copy says the 2B ignores learned phrases instead.

## Verification

- `evals/runs/model-bakeoff-2026-10-24/report.md` Round 4, raw
  `axis-matrix.json`, harness `axis_matrix.py`.
- `polish_with_qwen_uses_the_control_line` asserts the system prompt, the
  `[Styling: casual] [Structure: prose] [Context: email]\n{input}` user
  message, the generic path remaining unused, and a transformed result.
- `cargo test -p teletype-core -p teletype-inference` green (785 core tests).

## Reusable lesson

When a user-facing style toggle is added to a normalizer, state explicitly
that normalization is unconditional. A model reads a "do not restructure"
setting as permission to skip cleanup entirely, and the failure looks like a
weak model rather than a prompt conflict. Score list outputs on bullet
structure, not on a terminal period.
