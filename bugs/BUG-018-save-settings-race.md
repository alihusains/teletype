# BUG-018 — save_settings rewrites the entire Settings document; read-modify-write race can revert fields

**Severity:** P2 (low probability, confusing)
**Area:** desktop / commands.rs + ui
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `save_settings` and the three multi-caller sites

---

## Description

`save_settings` deserializes the **full** `Settings` struct from the UI's
copy and calls `replace_settings`. Three UI sites do
`get_settings` → mutate one field → `save_settings` with the full spread:
`pill.tsx:812-815`, `OnboardingScreen.tsx:96-98`,
`ModelsScreen.tsx:303-307`.

If a background path writes a setting between the read and the write (e.g.
`set_active_style_profile` from the Styles screen, a model selection, or a
second window), the stale full-struct write **clobbers that field with its
old value**. Classic read-modify-write race on a whole-document store;
`lib.rs` even keeps a separate `replace_settings` seam for exactly this.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/commands.rs` | (save_settings) | full-struct deserialize + `replace_settings` |
| `ui/src/pill.tsx` | 812-815 | get → mutate → save full spread |
| `ui/src/screens/OnboardingScreen.tsx` | 96-98 | same pattern |
| `ui/src/screens/ModelsScreen.tsx` | 303-307 | same pattern |

## Reproduction

1. Open the Models screen (it does get → mutate → save).
2. While it's open, change the active style profile from the Styles screen.
3. The Models screen's save clobbers the style profile back to its stale
   value.

(Hard to hit deterministically; it's a race.)

## Unit test cases (must pass after fix)

```rust
// 1. Saving one field must not revert a concurrently-changed field.
#[test]
fn patch_does_not_revert_concurrent_field() {
    let state = test_state();
    let base = get_settings(&state);
    // Concurrent writer changes the style profile.
    set_active_style_profile(&state, "casual");
    // A stale screen saves its copy with only the hotkey changed.
    let mut stale = base.clone();
    stale.hotkey = "cmd+shift+space".into();
    patch_settings(&state, stale);
    let now = get_settings(&state);
    assert_eq!(now.hotkey, "cmd+shift+space");
    assert_eq!(now.active_style_profile, "casual",
        "patch clobbered a field changed after the read");
}

// 2. replace_settings merges (or callers use patch semantics) so a partial
//    update cannot overwrite unrelated fields.
#[test]
fn partial_update_merges_not_replaces() {
    let state = test_state();
    set_active_style_profile(&state, "formal");
    patch_settings(&state, settings_with_only_hotkey("cmd+shift+space"));
    assert_eq!(get_settings(&state).active_style_profile, "formal");
}
```

## Acceptance criteria

- [ ] Saving one setting does not revert another setting that changed in
      between.
- [ ] The three multi-caller sites send only the changed field (patch
      semantics) rather than the full struct.

## How to test (manual / smoke)

1. **Smoke (2 min):** change two different settings from two different
   screens in quick succession (e.g. hotkey in the pill + style profile on
   the Styles screen).
   - **Pass:** both changes persist.
   - **Fail:** one of the two is reverted to its old value.

2. **Regression (1 min):** a single-screen save still works.
   - **Pass:** editing and saving one setting on the Settings screen
     persists.
   - **Fail:** single-screen saves are lost or partially applied.

## Fix direction

Add a `patch_settings` command that takes only the changed field(s) and
merges under the lock, rather than replacing the whole document. Migrate
`pill.tsx`, `OnboardingScreen`, and `ModelsScreen` to it. Keep
`save_settings` for the Settings screen (which legitimately edits the whole
form) but have it **merge, not replace**.

## Related

- BUG-009 / BUG-010 (the live-apply gap is the other half of the settings
  story)
- G011 (IPC wire-shape mismatches)
