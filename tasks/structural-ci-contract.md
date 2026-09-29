# Task: structural CI contract (BUG-019)

**Status:** planned (structural — do once, prevents recurrence)
**Filed:** 2026-09-28 by productivity monitor
**Ticket:** `bugs/BUG-019-no-ci-contract-for-features-and-settings.md`

## Problem

The pattern behind a dozen of the 19 audit bugs: a setting exists in the
`Settings` struct and the UI, but has no production caller (BUG-003, 009,
010), or a feature is advertised in brain/README with no engine path
(BUG-002, 003). `ipc_contract.rs` catches wire-shape drift (casing, missing
commands) but not "setting with no caller" or "advertised feature with no
engine path."

## Read first

- The ticket in `bugs/` (it lists the known instances).
- `crates/teletype-core/tests/ipc_contract.rs` (existing contract test —
  extend it, don't fork it).
- `crates/teletype-desktop/src/commands.rs` (`Settings` struct +
  `save_settings`).

## Acceptance criteria

- The contract test asserts: every `Settings` field is read by at least one
  production code path (grep-able caller), and every field the UI writes has
  a `save_settings` hook or a documented reason it's apply-at-restart.
- Known instances (BUG-002/003) make the test FAIL until resolved — that's
  the point; land the test red for 002/003 and flip green when the honesty
  block closes.
- CI runs it (it runs `ipc_contract.rs` already via the whole-crate test).

## Verification

- `cargo test -p teletype-core ipc_contract`
- Inject a dead setting in a scratch branch and confirm the test catches it.

## Out of scope

- Fixing BUG-002/003 themselves (that's `tasks/honesty-block.md`).
- Linting beyond the contract test.
