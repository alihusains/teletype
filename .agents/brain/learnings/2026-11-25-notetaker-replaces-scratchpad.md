# L: Notetaker replaces Scratchpad — notes, migration, and the dictation seam

## Problem
The old Scratchpad was append-only: dictation (and manual entry) could only
add timestamped lines to a single flat list. There were no titled notes, no
editing of an existing entry, and no way to route dictation into a specific
note. The product goal (Wispr Flow–style "Notetaker") required real notes:
create, title, edit body, delete, search, and an open note that dictation can
flow into.

## Symptoms / starting state
- `crates/teletype-core/src/scratchpad.rs` held a `Scratchpad` with a flat
  `Vec<ScratchEntry>` (transcript + timestamp only), plus `append`/`remove`/
  `clear`. No title, no updated_at, no notion of an open note.
- `ui/src/screens/ScratchpadScreen.tsx` rendered the append-only list.
- Dictation had no "where should this text go?" seam: `DictationController`
  only ever did clipboard/autotext insertion.

## Initial hypotheses
- The migration is the risky part: users with an existing `scratchpad.json`
  must not lose data when the shape changes.
- The dictation seam must be additive: turning routing on should never change
  what happens when routing is off.
- The storage key (`scratchpad.json`) and the settings key
  (`scratchpadEnabled`, camelCase) must stay stable so existing installs and
  the settings screen keep working.

## Investigation steps / design
- Model a `Note { id, title, body, created_at, updated_at }` and a `Notes`
  container holding `Vec<Note>` + `open_id`. Keep the module name
  `scratchpad.rs` (least churn) but rename the public types.
- `load()` reads `scratchpad.json`; if the top-level JSON is an array (the old
  shape), decode it as `Vec<ScratchEntry>` and convert each entry to a `Note`
  (body = transcript, title = first non-empty line, timestamps = file mtime),
  then persist the new shape. If it is an object, decode `Notes` directly.
- `save_note` updates the note and bumps `updated_at`; `delete_note` removes it
  and clears `open_id` if it was open; `append_note` targets the open note (or
  creates one); `set_open_note` rejects unknown ids.
- Wire the dictation seam in `teletype-desktop/src/dictation.rs`: when
  `scratchpad_enabled` is true and the window is focused, route the final text
  into the open note via `append_note`. All other behavior is untouched.
- Frontend: `NotetakerScreen.tsx` (list pane + editor pane, 600ms autosave
  debounce) with `confirmDialog` before delete. Rename the rail entry and all
  user-visible strings to "Notetaker".

## Evidence
- `cargo test -p teletype-core`: `scratchpad::tests::legacy_json_migrates_without_loss`,
  `legacy_migration_preserves_old_key_compatibility`, `save_moves_updated_at_and_renames`,
  `delete_closes_the_open_note`, `set_open_note_rejects_unknown_ids`,
  `append_with_no_open_note_creates_one`, `append_to_open_note_ordering`,
  `create_and_bounds`, `stamp_utc_formats_a_known_instant`, `real_clock_is_sane` — all pass.
- `cargo test -p teletype-desktop` green (18 migration tests + others).
- `cargo build -p teletype-core` and `-p teletype-desktop` exit 0.
- `cargo clippy -p teletype-core -p teletype-desktop --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` → FMT_OK.
- `npx tsc --noEmit` exit 0; `npx vitest run` 287/287 incl.
  `ui/src/test/notetakerScreen.test.tsx` (list/select/autosave/delete-confirm/search).
- Live app screenshots (window 1200x800, 2x): note list with search + delete,
  open note with title + body, delete-confirm native dialog.

## Root cause / key insight
The whole feature hinges on a single seam in the dictation pipeline: "does
this text become clipboard text, or does it go into the open note?" That seam
is gated by one setting (`scratchpadEnabled`) and one focus check, so it is
trivially safe to toggle and impossible to forget. Everything else (storage
shape, migration, IPC, UI) is downstream of that decision.

## Fix / verification
See Evidence above. Migration is covered by two dedicated tests that pin both
the data (no loss) and the key (old `scratchpad.json` path still read/written).

## Reusable lesson
When a feature is "replace X with Y but keep user data," the migration test is
the load-bearing artifact — write it first, against a fixture of the OLD
shape, and assert both (a) the data survives and (b) the storage key is
unchanged. The dictation-routed-into-a-note capability is a property of the
dictation pipeline, not the notetaker: keep the seam in `dictation.rs` and let
the notetaker module stay agnostic about how text arrives.

## Status
verified — 2026-11-25, `release/v0.2.7`, pi (backend+frontend engineer).
