---
id: L061
title: The polish rewrite ceiling exposed an unscrubbed NUL in the pipeline fallback
status: verified
type: debugging
area: "transforms / validator / pipeline"
agent: "qoder"
tool: "cargo test -p teletype-core (journey_smoke), rustfmt, clippy"
date: 2026-10-25
---

# The polish rewrite ceiling exposed an unscrubbed NUL in the pipeline fallback

## Problem

After adding the configurable polish ceiling, `journey_11c_control_characters_and_markup_do_not_leak_into_the_document`
failed with `NUL leaked from "hello\0world"`. The test had passed before the
guard existed and nothing in the guard touches control characters.

## Symptoms

- The new code changed only when a model rewrite is rejected: over the
  user-set ceiling, the pipeline now falls back to the user's input.
- The journey test uses a mock model that returns `"ok"`. `"ok"` previously
  passed validation and replaced the NUL-bearing input, so the assertion held
  by accident.
- With the ceiling active, `"ok"` is rejected as too much change, the raw input
  comes back, and the NUL becomes visible.

## Investigation

- Grepped core and desktop for sanitization: only `command_executor.rs` rejects
  control characters, and only for tool-call arguments.
- Read `Pipeline::run`: step 0 corrected dictionary terms, ITN and the transform
  operated on `input.text`, and no stage filtered control characters. The
  transcript went from `UnifiedInput` to `final_text` unchanged when the model
  output was rejected.

## Root cause

Delivery relied on the model output replacing the transcript. A fallback to the
input was always possible (empty output, fence, preamble, off-topic), so the
NUL hazard predated the guard; the stricter rejection path simply made it
reachable in a test. There was no defense in depth at the pipeline boundary.

## Fix

`Pipeline::run` now starts with step 0.0, `strip_control_characters`: it drops
C0/C1 controls except tab and newline, which are meaningful layout in a
transcript. The helper short-circuits when nothing needs stripping.

## Verification

- New unit test `control_characters_never_reach_the_final_text` covers NUL and
  BEL removal and tab/newline survival.
- `cargo test -p teletype-core --test journey_smoke`: 62 passed.
- Full `cargo test -p teletype-core`: 808 lib tests plus every integration
  suite green; `cargo clippy -p teletype-core --all-targets -- -D warnings`
  clean; rustfmt clean.

## Reusable lesson

A validator that can fall back to the input turns every pre-existing input
hazard into a delivery hazard. When adding a stricter rejection rule, ask what
the fallback now delivers and whether that value was ever sanitized. Sanitize
at the pipeline boundary, not at the model boundary.
