# Task: settings/UI miscellany (BUG-017 + 018)

**Status:** deferred
**Filed:** 2026-09-28 by productivity monitor
**Tickets:** `bugs/BUG-017-fillercounts-object-vs-array.md`, `bugs/BUG-018-save-settings-race.md`

## Problem

- **BUG-017:** `fillerCounts` is a JSON object on the wire but
  `ui/src/screens/InsightsScreen.tsx` calls `.slice()` on it → TypeError
  masked by `.catch`, filler panel permanently empty.
- **BUG-018:** `save_settings` rewrites the entire Settings document; two
  concurrent saves (e.g. two settings tabs) can revert each other's fields
  (read-modify-write race).

## Read first

- Both tickets in `bugs/`.
- `crates/teletype-desktop/src/commands.rs` (`get_insights` / the command
  that returns fillerCounts; `save_settings`).
- `ui/src/screens/InsightsScreen.tsx` (the `.slice()` call site).
- `crates/teletype-core/src/insights.rs` (the shape produced).

## Acceptance criteria

- The wire shape and the UI agree (pick one: make the backend emit an array,
  or make the UI handle an object — follow the ticket's recommendation).
  Filler panel shows data.
- `save_settings` either merges only changed fields or serializes the whole
  save under the existing settings mutex so concurrent saves cannot revert
  fields. Add a test for the concurrent-save case.

## Verification

- `cargo test -p teletype-desktop`
- `cd ui && npx tsc --noEmit`
- Manual: open Insights with a populated history; filler panel renders.

## Out of scope

- BUG-009/010 live-apply hooks (already in `save_settings` — do not remove).
- Insights computation performance (that's a different ticket).
