# BUG-019 — No CI contract test asserts every setting has a caller and every advertised feature has an engine path

**Severity:** P1 (structural — prevents a whole bug class)
**Area:** CI / ipc_contract.rs
**Status:** open
**Found:** 2026-09-28 QA audit (meta-finding across all 5 agents)

---

## Description

A dozen findings from this audit are the same shape: a feature or setting that
exists in the project brain and/or the UI but has no production caller or no
engine path, so it is a silent no-op. Prior QA audit already named the pattern:
"a guard that cannot be observed failing is not a guard."

Concrete instances this audit found that the pattern explains:

- **BUG-002:** spoken-emoji + spoken-punctuation toggles advertised in the
  brain, zero code (grep 0 hits).
- **BUG-003:** unload-model-after timer advertised, `unload()` implemented but
  zero callers; ModelManager is dead code.
- **BUG-009:** show_tray_icon toggle persists but has no live-apply setter
  (dead `let _show_tray` fossil at lib.rs:296).
- **BUG-010:** typing_autotext_enabled read once at startup, save_settings
  never calls typing::set_enabled.
- **BUG-012:** Personalization screen rendered but no NAV entry (unreachable).
- **BUG-017:** fillerCounts wire-shape mismatch (object vs array) that tsc
  cannot catch.

The existing `crates/teletype-core/tests/ipc_contract.rs` (5 tests) checks
NAMES and CASING of the IPC surface by parsing `invoke()` call sites out of
ui/src, `#[tauri::command]` out of the backend, `generate_handler!` out of
lib.rs, and Rust struct fields — then diffs them. But it does NOT check:

- (a) whether a persisted setting has a production CALLER (a setter that
  applies it, not just stores it),
- (b) whether an advertised feature has an engine path,
- (c) the SHAPES of nested objects/arrays/Options (only names/casing).

That is why BUG-002/003/009/010/012/017 all merged green.

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-core/tests/ipc_contract.rs` | — | existing contract test (names/casing only) |
| `crates/teletype-core/tests/ipc_contract.rs` | 589 | ratchets restore_emoji as "no UI" (the SETTINGS_WITHOUT_UI list) |
| `crates/teletype-desktop/src/lib.rs` | 296 | dead `let _show_tray` fossil (example of a half-finished live-apply) |

## Reproduction

Not a single repro; it is a missing guard. The reproduction is the audit
itself: 6 of the 19 findings are instances of this pattern, and none were
caught by CI.

## Unit test cases (must pass after fix)

```rust
// 1. Every persisted setting must be read AND applied somewhere in crates/.
//    A setting that is only written (save_settings) and never read-applied
//    is a finding. Known-UI-only fields are allow-listed.
#[test]
fn every_persisted_setting_has_a_production_caller() {
    let fields = parse_settings_struct_fields(); // existing parser
    for field in fields.iter().filter(|f| !SETTINGS_UI_ONLY.contains(f)) {
        assert!(has_read_apply_call_site(field),
            "setting `{field}` is persisted but never applied — silent no-op");
    }
}

// 2. Every non-developer Screen variant in ui/src/App.tsx must have a NAV entry.
#[test]
fn every_nav_screen_is_reachable() {
    let screens = parse_screen_union("ui/src/App.tsx");
    let nav = parse_nav_base("ui/src/App.tsx");
    for s in screens.iter().filter(|s| s != "developer") {
        assert!(nav.contains(s), "Screen `{s}` is rendered but has no NAV entry");
    }
}

// 3. Curated list (const array, derived from brain decision log + roadmap) of
//    user-facing advertised features; each must map to a setting or command.
#[test]
fn advertised_features_have_an_engine_path() {
    for feature in ADVERTISED_FEATURES {
        let (setting, command) = feature.setting_or_command();
        assert!(setting_exists(setting) || command_exists(command),
            "advertised feature `{feature}` has no setting or engine command");
    }
}

// 4. Extend the existing name/casing diff to nested shapes: a Rust
//    BTreeMap/Vec/Option field must be consumed in the UI as the matching
//    JS shape (object/array/nullable), not just by name.
#[test]
fn nested_ipc_shapes_match() {
    for field in parse_struct_fields_with_types() {
        assert!(ui_consumes_matching_shape(&field),
            "`{}`: Rust {:?} does not match the JS shape the UI passes",
            field.name, field.rust_type);
    }
}
```

## Acceptance criteria

- [ ] ipc_contract.rs gains the four test groups above.
- [ ] The test FAILS (red) when BUG-002/003/009/010/012/017 are present, and
      PASSES once they are fixed. (Prove it by temporarily reintroducing one.)
- [ ] CI runs ipc_contract.rs as a GATING job (not informational).
- [ ] A new setting/feature added without a caller or engine path fails CI.

## How to test (manual / smoke)

1. **Smoke:** run `cargo test -p teletype-core --test ipc_contract` and confirm
   the new tests run and currently FAIL on the known instances (BUG-002 etc.).
2. **Regression:** fix one instance (e.g. add the personalization NAV entry
   for BUG-012) and confirm the corresponding test flips green.
3. **Negative:** temporarily remove a setter (e.g. comment out the
   `typing::set_enabled` call) and confirm the test goes red.

## Fix direction

Extend `ipc_contract.rs` — it already has the parsing machinery for
`invoke()` sites, `#[tauri::command]`, `generate_handler!`, and struct fields.
Add the caller-check, nav-reachability, advertised-feature, and nested-shape
checks. The advertised-feature list is curated (derived from the brain's
decision log + roadmap) and lives in the test as a const array. Make the job
gating in ci.yml. (The prior audit noted ci.yml's clippy step cannot fail
because it has no `-D warnings` and 74 pre-existing warnings — that is a
separate task; this ticket is about the contract test specifically.)

## Related

- BUG-002, BUG-003, BUG-009, BUG-010, BUG-012, BUG-017 (the instances)
- The prior audit's "pattern worth naming" note in the project brain
- Gotcha G011 (the original IPC casing bug this test was built to catch)
