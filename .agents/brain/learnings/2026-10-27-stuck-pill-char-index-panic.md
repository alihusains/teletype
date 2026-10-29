---
id: L069
title: Stuck dictation pill was a char-index-as-byte-slice panic that killed the pipeline worker
status: verified
type: debugging
area: teletype-core/autotext + teletype-desktop/dictation
agent: main session (Qoder)
tool: cargo, screenshots
date: 2026-10-27
---

# Stuck dictation pill was a char-index-as-byte-slice panic that killed the pipeline worker

## Problem

The dictation pill stuck on a stale message and never hid. Reported as happening
"even in the previous release". The product is unusable when the main indicator
never clears. The trigger was dictating text with non-ASCII characters (Polish,
Cyrillic) before or inside an AutoText spoken snippet.

## Symptoms

- The pill stayed visible on a stale line (live capture: "Too short to polish.
  Using ra...") and never returned to idle.
- No text was injected; the app stayed alive and responsive otherwise.
- Reproduced live: two screenshots a minute apart both showed the frozen pill.

## Initial hypotheses

1. The pill UI (React) never received an idle event — a frontend state bug.
2. The overlay window show/hide logic missing a "hide on idle" branch.
3. A cancelled/abandoned session left the controller in a busy `Phase`.

## Investigation

1. Read the pill path: the React pill only renders content and handles
   `pill-skip`; window show/hide is backend-owned by `overlay::update`, which
   hides the window on `PillState::Idle` unless `always_show_pill`. So a stuck
   pill means the controller never reached `go_idle()`.
2. Read the controller: `Event::PipelineWorkerDone` is a deliberate no-op for
   the newest session (it waits for `PipelineDone`); `PipelineDone` is only
   produced by the controller after `PipelineReady`. So a worker that dies
   between `PipelineWorkerDone` and `PipelineReady` strands the pill forever.
3. Located the worker (`.name("teletype-pipeline")`) and its usage block at
   `dictation.rs:4466-4477` (after `PipelineWorkerDone`, before `PipelineReady`).
4. Found the crash: `usage.rs` `count_phrase_occurrences` sliced `&rest[1..]`
   after `find(is_alphanumeric)` returned 0 on a multi-byte first char, and used
   `match_snippet_at`'s CHAR index as a BYTE offset in `&rest[end..]`. Live log:
   `byte index 1 is not a char boundary; inside 'З' of "Заборки не оно."`.

## Evidence

- The live panic message names `usage.rs` and the offending string.
- `autotext::match_snippet_at` (and the snippet scanners) are built on
  `Vec<char>` and return CHAR indices; `&str` slicing is by BYTE offset. The two
  coincide only for ASCII, so any non-ASCII transcript reaching the AutoText
  tiers panics.
- The same class existed in four functions on the live path:
  `usage::count_phrase_occurrences`, `protect::protect_snippets`,
  `expand::expand_snippets`, and `disambiguate::{word_after,is_symbol_request}`.

## Root cause

Char indices used as byte offsets. A multi-byte char before (or inside) a
matched snippet makes `&text[idx..]` land mid-character and panic. In the
pipeline worker the panic happened after the no-op `PipelineWorkerDone` and
before `PipelineReady`, so `PipelineDone` never fired and `overlay::update(Idle)`
never hid the pill.

## Fix

- Added `autotext::byte_offset(text, char_idx)` (clamps past-end to `text.len()`)
  and used it at every char→byte slicing site; the scan loop now advances one
  whole char (`char::len_utf8`) when it cannot skip to the next alphanumeric.
- Defense in depth: a `WorkerExitGuard` RAII guard in both dictation workers
  sends `Event::WorkerAbandoned { session }` if the worker ends (panic or a
  skipped terminal send) without a terminal hand-off; `exit_guard.terminal(ev)`
  disarms it. The controller's `WorkerAbandoned` handler calls `go_idle()` when
  the guard's session is the newest — the only place that can hide the window.

## Verification

- Both panics reproduced as failing tests first, then fixed.
- `cargo test -p teletype-core --lib` → 851 passed, 0 failed.
- `cargo test -p teletype-desktop --lib` → 401 passed, 0 failed.
- New tests: `usage::counts_spoken_snippets_in_non_ascii_text`,
  `usage::counts_non_ascii_snippet_phrases`,
  `protect::protects_snippet_phrases_in_non_ascii_text`,
  `expand::snippet_expands_in_non_ascii_text`, and an end-to-end
  `pipeline::non_ascii_voice_dictation_survives_snippet_paths` that runs the
  real `Pipeline` (protect+expand) AND the worker's `count_autotext` over the
  same non-ASCII text. Guard tests cover drop-without-terminal, disarm by
  terminal, survival through a real `catch_unwind` panic, and the newest-vs-
  superseded session rule.
- Live: rebuilt the binary, restarted the app; the pill is hidden at idle and
  the previously frozen pill is gone. A live non-ASCII **mic** dictation was
  **not** synthesized (no scriptable audio input), so the panic path is proven
  by tests that call the exact functions the worker calls with the exact
  failing inputs, not by a live utterance.

## Reusable lesson

Never mix char indices and byte slices in Rust. Any helper built on
`Vec<char>`/`char_indices` returns char indices; convert with a byte-offset
helper before slicing, and advance by whole chars, not bytes. Separately: a
background worker's death must reset any "busy" UI. Use an RAII guard that
fires on drop-without-completion, so a panic (or an early return) can never
strand a pill/spinner.

## Evidence / sources

- `crates/teletype-core/src/usage.rs`, `.../autotext/{mod,protect,expand,disambiguate}.rs`
- `crates/teletype-desktop/src/dictation.rs` (`WorkerExitGuard`, `Event::WorkerAbandoned`)
- Brain gotcha G029

## Promotion

Verified — reproduced live, fixed, and covered by regression tests at the panic
site and end to end.