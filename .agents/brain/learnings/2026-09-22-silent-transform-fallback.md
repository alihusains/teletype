---
id: L001
title: Text transforms silently did nothing (empty provider at startup + 300-token cap treated as fallback)
status: candidate
type: debugging
area: transforms / inference
agent: opencode (mimo-v2.6-flash-free)
tool: code inspection + cargo tests
date: 2026-09-22
---

# Text transforms silently did nothing

## Problem

After a restart, AI text transforms (Polish, Professional, etc.) produced no
visible change: dictated text came back deterministic-cleaned (fillers,
AutoText) but never rewritten, with no error anywhere.

## Symptoms

- `AppState.inference` was `None` until the user manually re-picked a model
  via Settings (`select_model` / `select_openai_provider`).
- With a provider loaded, short inputs transformed but medium/long dictations
  still fell through unchanged.
- No user-visible signal distinguished "no transform requested" from
  "transform requested but skipped".

## Initial hypotheses

1. Prompt quality: the generic prompt lacked an explicit list/formatting rule
   (EnviousWispr's cloud prompt has one), so the model output was rejected or
   unhelpful.
2. Provider never selected: the settings choice might not map to a live
   provider instance.
3. Output truncation: `GenerationParams::default()` (300 tokens) might be
   cutting generations.

## Investigation

1. Traced `Pipeline::run` (`pipeline.rs`): the `Some(transform)` +
   `None(inference)` arm returns `(expanded, None)` with no log or flag.
2. Traced provider installation: only `select_model` (commands.rs:725) and
   `select_openai_provider` (commands.rs:843) call `inference.replace(...)`;
   `lib.rs::run()` never reads `settings.selected_llm_provider`.
3. Checked both providers' stop handling: `openai_compat.rs:201` and
   `server.rs:227` both `return Err(...)` when generation stops at
   `max_tokens`, and `engine.rs` maps any `Err` to fallback-to-input.
4. Compared prompts against EnviousWispr's `CloudFixedPromptBuilder.swift`
   (enumeration-to-list rule, segmentation guard) and `CustomWordsManager.swift`
   `builtinDefaults` (brand/acronym seeds).

## Evidence

- `grep` for `inference.replace` found exactly two sites, both inside
  in-session commands; no startup call site exists.
- `GenerationParams::default()` = `max_tokens: 300`; validator has no
  truncation check because the provider already returned `Err`.
- EW's cloud prompt line ~119: announced sets become `- ` lists with spoken
  ordinals dropped; line ~121: short in-sentence runs must stay prose.

## Root cause

Two independent silent fallbacks stacked:

1. Selection persisted, instance did not: the provider lived only in memory
   and nothing rebuilt it at startup, so every post-restart transform
   short-circuited to AutoText-only expansion.
2. The generic path's 300-token cap: hitting it is an `Err`, and `Err` means
   fallback-to-input, so anything longer than ~300 output tokens silently
   never transformed even with a healthy provider.

## Fix

Approved three-part fix (user: "yes please", 2026-09-22):

1. `commands::rehydrate_provider(app)` runs on a background thread from
   `lib.rs::run()`: local-server → catalog check + `warm_up`; openai-compat →
   rebuild from keyring key (no probe, per D004). Outcome logged via
   `log_line`.
2. Fallback surfaced: `PipelineResult.transform_skipped_no_model` flag,
   logged by `dictation.rs`; `get_model_status` distinguishes "configured but
   not loaded" from "no model selected".
3. EW-derived rules added to generic `CORE_RULES` only (enumeration → `- `
   list with worked example, segmentation restraint, spoken-format
   normalization) and generic `max_tokens` scaled to input length
   (`clamp(300, 2048)`). `EG1_SYSTEM_PROMPT` untouched (training contract).
   Dictionary seeded once with generic EW builtins via
   `Dictionary::seed_builtins()` + `builtin_version` flag.

## Verification

- `cargo check --workspace --all-targets`: exit 0.
- `cargo test --workspace`: 149 core (+1 new seed test) + 12 speech +
  16 inference + 6 desktop, 0 failed.
- `cargo clippy --workspace --all-targets`: 0 errors, warnings pre-existing.
- `npx tsc --noEmit`: clean.
- Not yet done: live dictation run in the packaged/dev app (promotion gate).

## Reusable lesson

- A persisted *selection* of an in-memory object is not persistence: every
  provider kind needs an explicit rehydration path at startup, or behavior
  degrades silently after every restart.
- Never treat "hit the token cap" as a soft failure without checking how the
  caller maps `Err`: here `Err` = give up and return the input, so an
  undersized cap looks identical to "the model did nothing".
- When a feature silently no-ops, look for stacked fallbacks first; fixing
  only one of them leaves the symptom intact.

## Evidence / sources

- PR: (none yet, uncommitted)
- Commit: (none yet)
- Issue: brain open question Q001 (uncommitted P0 batch review)
- Documentation: `.agents/brain/PROJECT_BRAIN.md` G006/G007
- User confirmation: approval of the three-part fix, 2026-09-22

## Promotion

Promote to `verified` when a live dictation run in the app confirms: provider
restored after restart (Developer tab shows "[teletype] LLM restored: ..."),
and a spoken enumeration renders as a bulleted list.

---
