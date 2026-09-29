# BUG-006 — AX injection allowlist includes search fields and URL bars

**Severity:** P1 (dictation lands in the wrong place)
**Area:** desktop / ax_text.rs
**Status:** open
**Found:** 2026-09-28 QA audit (injection/clipboard agent), verified by reading `ax_text.rs:78-83`

---

## Description

The direct AX injection tier (`ax_text.rs`, landed 2026-09-27) writes text at
the caret through the Accessibility API with read-back verification. The
writable-role allowlist at `ax_text.rs:78-83` includes:

```rust
const ROLE_SEARCH_FIELD: Name = "AXSearchField";
const ROLE_TEXT_URL_FIELD: Name = "AXURLTextField";

fn is_writable_role(role: &str) -> bool {
    role == ROLE_TEXT_FIELD
        || role == ROLE_TEXT_AREA
        || role == ROLE_COMBO_BOX
        || role == ROLE_SEARCH_FIELD      // ← browser search boxes
        || role == ROLE_TEXT_URL_FIELD    // ← browser URL bars
}
```

`AXSearchField` and `AXURLTextField` are exactly what browsers expose for the
omnibox/URL bar and in-app search boxes. Any real browser (Safari/Chrome/
Firefox) URL bar is an `AXURLTextField`.

### Scenario

User presses the hotkey while a browser tab is focused with the URL bar
focused (or the browser gives focus to the search field on hotkey press). Tier
1 AX write succeeds silently, verified "ok" by read-back (browsers report the
value honestly), so no clipboard fallback fires.

### User sees

Their entire dictation becomes the URL/search query — a navigated-to web page,
a junk search result, or a corrupted document field. The dictation is
"delivered" to the wrong place with full confidence logging
(`"inject: direct write ok"`).

### Why it is new

The prior audit fixed *double*-write and *whole-value* duplication (G018), not
*wrong-element* placement. The allowlist was built around "is this a text
role?" not "is this the element the user is typing in?"

## Files

| File | Lines | Role |
|------|-------|------|
| `crates/teletype-desktop/src/ax_text.rs` | 64-65 | the two role constants |
| `crates/teletype-desktop/src/ax_text.rs` | 78-83 | `is_writable_role` allowlist |
| `crates/teletype-desktop/src/ax_text.rs` | 512-518 | test that asserts both roles ARE writable (must be updated) |

## Reproduction

1. Open Safari or Chrome.
2. Click into the URL bar (or press Cmd+L).
3. Press the Teletype hotkey and dictate a sentence.
4. **The dictation lands in the URL bar**, not in the page's content area.
   The browser may navigate to the dictated text as a search query.

## Unit test cases (must pass after fix)

```rust
// 1. Search fields and URL fields must NOT be in the writable allowlist.
#[test]
fn search_and_url_fields_are_not_writable() {
    assert!(!is_writable_role("AXSearchField"),
        "AXSearchField must not be writable — dictation would land in a search box");
    assert!(!is_writable_role("AXURLTextField"),
        "AXURLTextField must not be writable — dictation would land in a URL bar");
}

// 2. The legitimate text roles must still be writable.
#[test]
fn text_roles_still_writable() {
    assert!(is_writable_role("AXTextField"));
    assert!(is_writable_role("AXTextArea"));
    assert!(is_writable_role("AXComboBox"));
}

// 3. Non-text roles must still be rejected.
#[test]
fn non_text_roles_still_rejected() {
    assert!(!is_writable_role("AXButton"));
    assert!(!is_writable_role("AXStaticText"));
    assert!(!is_writable_role("AXUnknownRole"));
}
```

## Acceptance criteria

- [ ] `AXSearchField` and `AXURLTextField` are removed from
      `is_writable_role`.
- [ ] When the focused element is a search field or URL field, the AX tier
      falls back to the clipboard injection path (which pastes at the caret via
      Cmd+V, landing in whatever the user is actually typing in).
- [ ] The existing test at `ax_text.rs:512-518` is updated to assert the two
      roles are **not** writable.
- [ ] `cargo test -p teletype-desktop` passes.

## How to test (manual / smoke)

1. **Smoke (2 min):**
   - Open Chrome, click into the URL bar.
   - Press the hotkey, dictate "hello world".
   - **Pass:** the URL bar is unchanged; the text is pasted into the page's
     content area (or, if the page has no text field, the clipboard fallback
     fires and the text goes to the clipboard).
   - **Fail:** "hello world" appears in the URL bar.

2. **Search box (1 min):**
   - Open an app with a search box (e.g., Finder's search, or a Notes search).
   - Focus the search box, dictate.
   - **Pass:** text does not land in the search box.

3. **Regression (1 min):**
   - Open a text editor (TextEdit), focus a text field, dictate.
   - **Pass:** text lands correctly in the text field (AX tier still works for
     legitimate text roles).

## Fix direction

Remove `ROLE_SEARCH_FIELD` and `ROLE_TEXT_URL_FIELD` from `is_writable_role`
(`ax_text.rs:82-83`). The AX tier will then fall back to the clipboard path
for those roles, which pastes at the caret via Cmd+V — the same behavior as
before the AX tier was added.

If search fields are genuinely needed for some use case (e.g., dictating into
a Notes search), add a **separate, explicit** setting
(`allowSearchFieldInjection: bool`, default `false`) rather than putting them
in the default allowlist.

## Related

- G018 (whole-value read-modify-write in terminals) — fixed, different bug
- The AX tier is new (2026-09-27); this is a placement bug, not a
  duplication bug.
