---
id: 2026-10-11-tauri-state-cannot-be-constructed-in-tests
title: tauri::State cannot be constructed in unit tests; drive state-free paths headlessly
date: 2026-10-11
agent: lead (qoder)
area: crates/teletype-desktop/src/dictation.rs (command confirmation tests)
status: verified
---

## Problem
Adding unit tests for the command confirmation hold (S3) required driving
`Session::command_ready` / `command_confirm` headlessly. A `confirm: Some`
payload is held in `pending_commands` and the hold path calls `self.audit(...)`,
which resolves `AppState` via `self.state()` → `self.app.state::<AppState>()`.
In a test session `app` is `None`, so `state()` panics.

The tempting fix is to give the test session an owned `AppState` (via a
`test_state: Option<AppState>` field) and add a `state_inner() -> &AppState`
that returns it. That hits a hard wall.

## Root cause
`tauri::State<'r, T>` is a tuple struct `(&'r T)` with a **private field**, so it
cannot be constructed outside the `tauri` crate. A `Session` in a unit test has
no `AppHandle`, so there is no `tauri::State` to return. Returning
`&AppState` from the production path is also impossible: the
`&*app.state::<AppState>()` deref borrows a **temporary** `tauri::State`, and
E0515 ("cannot return value referencing temporary value") fires whether the
temporary is a bare local, a named local, or wrapped in a trailing block — the
borrow checker drops it at end-of-statement/scope, before the `return`.

## Fix
Do NOT try to make `execute_command` / `audit` runnable headlessly. Instead:
1. Make the behavior under test **state-free**. The hold test only needs to
   assert `pending_commands` contains the op — it does not need the audit ring.
2. Guard the side effects that DO need state so they no-op headlessly, mirroring
   the existing `flash` `headless_flash` pattern:
   - `audit`: `let Some(app) = self.app.as_ref() else { return; };` before
     `app.state::<AppState>()`.
   - the `command-confirm-requested` emit in `command_ready`: wrap in
     `if let Some(app) = self.app.as_ref() { ... }`.
3. Delete the test that tried to run `execute_command` headlessly
   (`command_confirm_with_state_executes_text_insert`) and its helpers
   (`session_with_state`, `session_state_for_tests`, the `test_state` field).
   The `confirm: None` non-hold path is proven by contrast: the hold branch is
   the only place that inserts into `pending_commands`, so a confirm-less
   payload never lands there.

## Verification
`cargo test -p teletype-desktop --lib command_` → 28 passed, 0 failed. Full
`--lib` → 125 passed, 0 failed.

## Reusable lesson
When a Tauri controller method needs `AppState` but the unit test has no
`AppHandle`, do not construct `tauri::State` (impossible) or return a
deref'd `&AppState` from a temporary (E0515). Make the asserted behavior
state-free and no-op the state-dependent side effects (audit, `emit`) in the
headless path — the same pattern `flash` already uses. A test that needs to
drive a real side effect (text injection, app launch) belongs in an integration
test with a real app handle, not a headless unit test.
