# Task: settings/UI miscellany (BUG-017 + 018)

**Status:** closed (both fixed; verified 2026-10-01)
**Filed:** 2026-09-28 by productivity monitor
**Tickets:** `bugs/BUG-017-fillercounts-object-vs-array.md`, `bugs/BUG-018-save-settings-race.md`

## Outcome

- **BUG-017 — fixed.** `ui/src/screens/InsightsScreen.tsx` no longer calls
  `.slice()` on the `fillerCounts` object: the total uses
  `Object.values(usage.fillerCounts).reduce((s, n) => s + n, 0)` and the list
  renders through `CountList`, which iterates `Object.entries`. The filler
  panel shows data. (The 2026-09-30 commit `d34f6d0` notes the same.)
- **BUG-018 — fixed (commit `d34f6d0`, 2026-09-30).** `save_settings`
  serializes the entire read-modify-write under `AppState::save_lock`
  (`commands.rs`), so two concurrent saves cannot revert each other's fields.

## Verification

- `cargo test --workspace` green.
- `cd ui && npx tsc --noEmit` clean.
- Manual: open Insights with a populated history; filler panel renders.
