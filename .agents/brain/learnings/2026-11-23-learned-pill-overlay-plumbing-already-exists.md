---
id: L098
title: The pill's "click-through, no room for a button" comment is stale; the result-card plumbing already supports an interactive learned-word pill
status: verified
type: architecture
area: "desktop/overlay, dictation edit-watch, ui/pill"
agent: OpenCode
tool: Hy4 flash Free
date: 2026-11-23
---

# The pill's "click-through, no room for a button" comment is stale; the result-card plumbing already supports an interactive learned-word pill

## Problem

A comment in `crates/teletype-desktop/src/dictation.rs` (in the edit-watch learn block) says the pill "is click-through outside recordings and its fixed sizes leave no room for a button without resizing surgery", so learning only shows as a tray Undo item and a main-window toast. Planning an EnviousWispr-style learned-word pill looked like it needed new overlay machinery.

## Symptoms

- The word is saved and a `LearnedOffer` is recorded, but nothing appears on the floating pill.
- The toast in `ui/src/App.tsx` says only `Learned: <word>` and cannot distinguish added vs. updated.
- The comment implies the pill is fixed-size and non-interactive outside a recording.

## Initial hypotheses

1. The pill window is inherently click-through and fixed-size; a button requires a larger overlay rewrite.
2. The learned offer has too little information (no canonical word / added-vs-updated kind) to render a pill.
3. A learned pill would need a new expiry/interaction protocol in Rust.

## Investigation

1. Read `crates/teletype-desktop/src/overlay.rs`: `show_result`/`dismiss_result` already call `set_interactive` (`set_ignore_cursor_events`), and `resize_to` sizes the OS window from a frontend measurement.
2. Read `ui/src/pill.tsx` and `ui/src/pillMachine.ts`: `PolishDiffCard` is a content-sized, interactive 460pt card with buttons; `set_pill_size` reports its measured size with a card id; `result` phase has an 8s TTL and a dismiss path.
3. Read `crates/teletype-desktop/src/commands/settings.rs::set_pill_size` / `dismiss_pill_result`: the card id gates the pending show and late dismiss, so a learned card can reuse the same slot.
4. Read `crates/teletype-core/src/dictionary.rs`: `LearnedChange::{Created, Backfilled, AliasAdded}` already provides the added-vs-updated distinction the pill copy needs.
5. Read `crates/teletype-desktop/src/commands/personalization.rs`: `LearnedOffer.matches` accepts the `offer:<id>` sentinel for any offer, so the pill can undo by offer id without preference ids.

## Evidence

- `overlay.rs:287-345` — `show_result` sets interactive before the card renders; `set_interactive` flips `set_ignore_cursor_events`.
- `overlay.rs:389+` — `resize_to` with a card id shows a hidden pill only for that card's size report.
- `ui/src/pill.tsx:2369-2388` — result branch renders independently of the selected pill style.
- `crates/teletype-desktop/src/dictation.rs:~7600` — the stale comment; the code immediately below emits only `learned-preference` with `{ message, ids }`.

## Root cause

The comment predates the polish result card. The overlay gained interactive, measured, card-id-gated surfaces for that card, but the learn path was never revisited, so the visible half of the feature stayed in the main window.

## Fix

No code fixed yet. `docs/learned-word-pill-plan.md` proposes: emit a typed `learned-word` notice from the edit-watch learn block, reuse the shared overlay card slot (`show_learned`/`dismiss_pill_result`), render a 12s hover-pausable pill with Undo/× and morph states in `pill.tsx`/`pillMachine.ts`, and surface dictionary save failures instead of logging them.

## Verification

Read the cited code at the current working tree (HEAD `49ad13b`); no runtime test of the proposed pill yet. The plan's verification section lists Rust, vitest, and manual macOS checks.

## Reusable lesson

Before treating an overlay/UI affordance as impossible, check whether a recent feature already built the needed mechanism (interactivity, measured resize, id-gated show/dismiss). Stale "can't do this" comments are cheap to repeat and expensive to believe.
