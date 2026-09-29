# BUG-010 — typing_autotext_enabled toggle is dead (no live apply)

**Severity:** P1 (feature keeps working after the user turned it off)
**Area:** desktop / lib.rs + commands.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `lib.rs:373-378`

---

## Description

`typing_autotext_enabled` is read **once** at startup (`lib.rs:373-378` →
`typing::set_enabled`), but `save_settings` never calls `typing::set_enabled`
again. So toggling "Enable AutoText while typing" off in Settings persists the
value and updates the checkbox, but `/trigger` expansion **keeps firing while
typing until a restart**.

This is the exact "silent no-op" class: the feature keeps working after the user
turned it off.

### User sees

They disable AutoText typing, the checkbox flips to off, and the trigger still
expands. They assume it's still on (or that the toggle is broken) and either
re-enable it (no-op) or walk away thinking the setting is ignored.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/lib.rs` | 373-378 | read once at startup → `typing::set_enabled` |
| `crates/teletype-desktop/src/commands.rs` | (save_settings) | never calls `typing::set_enabled` |

## Reproduction

1. Settings → AutoText → toggle "Enable while typing" **OFF**.
2. Type a known AutoText trigger followed by space in any app.
3. It **still expands**.
4. Restart — now it's off.

## Unit test cases (must pass after fix)

```rust
// 1. Saving typing_autotext_enabled=false must call typing::set_enabled(false) at runtime.
#[test]
fn saving_typing_autotext_off_applies_live() {
    let mut typing = MockTyping::new(); // records set_enabled calls
    typing.set_enabled(true); // startup state

    save_settings(&mut typing, Settings { typing_autotext_enabled: false, ..default() });

    assert_eq!(typing.last_enabled(), Some(false),
        "saving typing_autotext_enabled=false must call typing::set_enabled(false) at runtime");
}

// 2. Saving typing_autotext_enabled=true must re-enable it at runtime.
#[test]
fn saving_typing_autotext_on_applies_live() {
    let mut typing = MockTyping::new();
    typing.set_enabled(false);

    save_settings(&mut typing, Settings { typing_autotext_enabled: true, ..default() });

    assert_eq!(typing.last_enabled(), Some(true));
}

// 3. After toggling off, the typing tap must not expand a trigger.
#[test]
fn trigger_does_not_expand_after_toggle_off() {
    let mut typing = MockTyping::new();
    typing.set_enabled(true);
    assert_eq!(typing.expand("/todo "), "TODO: ");

    typing.set_enabled(false); // what save_settings should do

    assert_eq!(typing.expand("/todo "), "/todo ",
        "typing tap must stop expanding after the toggle is turned off");
}
```

## Acceptance criteria

- [ ] Toggling `typing_autotext_enabled` off **stops** `/trigger` expansion
      immediately (no restart).
- [ ] Toggling it on **re-enables** expansion immediately.
- [ ] `save_settings` calls `typing::set_enabled(new_value)` when the value
      changes.
- [ ] `cargo test -p teletype-desktop` passes.

## How to test (manual / smoke)

1. **Smoke (1 min):**
   - Toggle "Enable while typing" off.
   - Type a known trigger + space.
   - **Pass:** no expansion.
   - **Fail:** it expands.
   - Toggle back on, type again. **Pass:** it expands.

2. **Regression (1 min):**
   - Leave it off, restart.
   - **Pass:** still off after restart.

## Fix direction

In `save_settings`, when `typing_autotext_enabled` changes, call
`typing::set_enabled(new_value)`. The function already exists (used at startup,
`lib.rs:373-378`); it just needs to be invoked from the save path. Extract the
startup read into a shared `apply_typing_autotext(bool)` used by both startup
and live-apply so the two can't drift.

## Related

- BUG-009 — same silent-noop class (`show_tray_icon`)
- L007 — typing autotext implementation
