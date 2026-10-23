# Advertised EW-parity features had no engine path

**ID:** L011
**Date:** 2026-09-28
**Agent:** teletype-productivity-monitor (lead session)
**Area:** settings / transforms / inference lifecycle / docs
**Status:** verified

## Problem

The 2026-09-28 QA sweep found three features advertised in the project brain
and README that had zero or dead code paths:

- **Spoken-emoji + spoken-punctuation toggles** (BUG-002): brain item (3) of
  the 2026-09-25 EW settings port. `grep -rn "spoken_emoji\|spoken_punct"
  crates/ ui/` → 0 hits. The settings don't exist.
- **Unload-model-after timer** (BUG-003): brain item (5) of the same port.
  `unload()` exists in the model manager but has zero callers; no setting,
  no UI, no timer.
- **Live autotext toggle** (BUG-010): `typing_autotext_enabled` persisted to
  settings but was read once at startup; `save_settings` never applied it, so
  the toggle was a silent no-op until restart.

## Why it happened

The brain recorded the 2026-09-25 user decision ("port EW's Transcription-tab
settings in this order …") as the plan of record. Items (1) and (2) landed;
items (3) and (5) never did, but nothing marked them as not-done. The README
and brain kept reading as "the app does this" because the decision line read
as a completed port.

## Evidence

- `bugs/BUG-002-spoken-emoji-punctuation-toggles-absent.md` (grep 0 hits,
  verified 2026-09-28 on branch `qa-audit-fixes`)
- `bugs/BUG-003-unload-model-timer-noop.md` (`unload()` zero callers)
- `bugs/BUG-010-typing-autotext-toggle-dead.md` (read-once-at-startup)
- Fix for BUG-010 landed 2026-09-28: `save_settings` now calls
  `typing::set_enabled` on change (commands.rs), verified by
  `cargo test -p teletype-desktop` (61 pass).

## Reusable lesson

Before advertising a feature in the brain, README, or usp.md, verify two
things: (1) the setting has a **production caller** (not just a struct field),
and (2) the feature has an **engine path** (the code that actually does the
thing exists and is reachable). A user decision to build a feature is a plan,
not a shipped claim. This is exactly the gap BUG-019's CI contract
(`ipc_contract.rs` extended to assert setting→caller and feature→engine-path)
would close mechanically.

## How to apply

- When writing a brain entry for a new feature, record its status as
  *planned* until the engine path exists and is tested.
- When a decision lists multiple port items, track each item's status
  separately; one landing does not complete the list.
- The brain's 2026-09-25 EW-port line now carries a 2026-09-28 correction
  noting items (3)/(5) are not done (see PROJECT_BRAIN.md §13).
