# BUG-009 — show_tray_icon toggle is a silent no-op until restart

**Severity:** P1 (setting appears to work but doesn't)
**Area:** desktop / commands.rs + lib.rs
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `lib.rs:296` and grepping `save_settings`

---

## Description

`save_settings` persists `show_tray_icon` and emits `settings-changed`, but
**nothing calls tray build/hide** — the only read site is `setup()` at startup
(`lib.rs:296-298`).

Worse, `lib.rs:296` has a dead line:

```rust
let _show_tray = settings.show_tray_icon;   // ← fossil, unused
let show_tray = settings.show_tray_icon;    // ← real read, startup only
```

The dead `let _show_tray` is a fossil of a half-finished live-apply. So
unchecking "Show tray icon" in Settings persists the value and updates the UI,
but the tray icon **stays in the menu bar until the next launch**.

### User sees

The toggle "works" (checkbox flips, setting saved) and the tray icon does not
disappear. It only goes away after a restart.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/commands.rs` | (save_settings) | persists `show_tray_icon`, no live apply |
| `crates/teletype-desktop/src/lib.rs` | 296-298 | tray built once at startup; dead `let _show_tray` fossil |

## Reproduction

1. Settings → uncheck "Show tray icon".
2. The tray icon remains in the menu bar.
3. Restart the app.
4. Now it's gone.

## Unit test cases (must pass after fix)

```rust
// 1. Toggling show_tray_icon off at runtime hides the tray (no restart).
#[test]
fn toggling_tray_off_hides_at_runtime() {
    let mut platform = MockPlatform::new(); // records show()/hide() calls
    platform.show_tray(); // startup state: visible

    save_settings(&mut platform, Settings { show_tray_icon: false, ..default() });

    assert!(platform.tray_hidden(),
        "tray must be hidden at runtime when show_tray_icon is toggled off");
}

// 2. Toggling show_tray_icon on at runtime rebuilds/shows it.
#[test]
fn toggling_tray_on_shows_at_runtime() {
    let mut platform = MockPlatform::new();
    platform.hide_tray(); // start hidden

    save_settings(&mut platform, Settings { show_tray_icon: true, ..default() });

    assert!(platform.tray_visible(),
        "tray must be shown at runtime when show_tray_icon is toggled on");
}

// 3. An unchanged value must not rebuild the tray (idempotency).
#[test]
fn unchanged_tray_value_is_noop() {
    let mut platform = MockPlatform::new();
    platform.show_tray();
    save_settings(&mut platform, Settings { show_tray_icon: true, ..default() });
    assert_eq!(platform.tray_operation_count(), 1, "no-op save should not touch the tray");
}
```

## Acceptance criteria

- [ ] Toggling `show_tray_icon` off **hides the tray immediately** (no restart).
- [ ] Toggling `show_tray_icon` on **shows it immediately**.
- [ ] The dead `let _show_tray` line at `lib.rs:296` is removed.
- [ ] Restarting with it off leaves it off.
- [ ] `cargo test -p teletype-desktop` passes.

## How to test (manual / smoke)

1. **Smoke (1 min):**
   - Uncheck "Show tray icon".
   - **Pass:** the tray icon disappears within a second.
   - **Fail:** it stays in the menu bar.
   - Re-check it. **Pass:** the tray icon reappears.

2. **Regression (1 min):**
   - Leave it off, restart the app.
   - **Pass:** tray icon still absent after restart.

## Fix direction

In `save_settings` (or a `settings-changed` handler), when `show_tray_icon`
changes, call the tray hide/show path:

- new value `false` → hide the tray item
- new value `true` → build/show the tray item

Extract the startup tray-building logic from `setup()` into a reusable
`apply_tray_visibility(&AppHandle, bool)` so both the startup path and the
live-apply path share it. Remove the dead `let _show_tray` line at `lib.rs:296`.

## Related

- BUG-010 — same silent-noop class (`typing_autotext_enabled`)
