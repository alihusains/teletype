---
id: L038
title: The ⌘V clipboard fallback pastes into the app's default target, not the user's caret
status: candidate
type: debugging
area: injection
agent: lead
tool: qoder
date: 2026-10-11
---

# The ⌘V clipboard fallback pastes into the app's default target, not the user's caret

## Problem

User-reported critical bug: dictation "pastes on top of the text area instead of
at the user's cursor." Acceptance criterion: wherever the user left the caret,
that is where the dictation must land.

## Symptoms

In some apps the dictated text appeared in the wrong place (top of the field,
the search/omnibox bar, or a different element) rather than at the caret the user
had placed.

## Initial hypotheses

1. Tier 1 (direct AX write) was writing to the wrong offset.
2. Tier 2 (clipboard ⌘V) was pasting into the wrong place.
3. The delivery gate was not catching the case.

## Investigation

1. Read `ax_text.rs` end to end. Tier 1 (`insert_detailed` →
   `insert_via_selection_detailed`) reads `AXSelectedTextRange` for the caret,
   writes via `AXSelectedText`, then `move_caret` — it is caret-correct. Ruled
   out hypothesis 1 for the normal case.
2. Read `injector.rs`. Tier 2 sets the clipboard, sleeps 250 ms, presses ⌘V,
   sleeps 350 ms, restores. ⌘V pastes at the *app's current caret at paste
   time* — for an app whose focused element is not a writable AX text field
   (a browser's `AXSearchField`/`AXWebArea`, a non-text control, or no focused
   element), that "caret" is the app's default paste target, not the user's.
   This matched the symptom.
3. Read `dictation.rs::pipeline_ready` + `delivery_refusal` + `delivery.rs`.
   The gate only verifies *same app + same window*; it never checks that a
   writable text field is focused. So a Refused Tier 1 fell straight through to
   the ⌘V fallback with no guard.
4. Confirmed `insert_detailed` returns `(Insert::Refused, field_readable=false)`
   exactly when `focused_text_field()` is `None` (role not writable) — that
   `field_readable=false` is the signal the fix needed.

## Evidence

- `ax_text.rs:329` `insert_detailed`: `focused_text_field()` → `None` ⇒
  `(Refused, false)`.
- `ax_text.rs:186` `is_writable_role` allows only AXTextField/AXTextArea/
  AXComboBox (search/URL fields deliberately excluded — BUG-006), so browsers
  and web areas are `None` ⇒ Refused ⇒ fallback.
- `delivery.rs:50` `resolve_delivery` compares bundle id + window frame only;
  no field-focus check.
- `injector.rs` ⌘V is a keystroke into whatever the OS routes a paste to.

## Root cause

When the fast (AX) path is refused because the focused element is not a writable
text field, the code fell back to a ⌘V keystroke with no check that the focus
was a text field with a readable caret. ⌘V therefore landed in the app's
default paste target — not the user's caret. A secondary latent defect:
`selected_range` fell back to **end-of-text** when `AXSelectedTextRange` was
unreadable, so even a direct write could drop at the bottom of a large field.

## Fix

1. `dictation.rs::pipeline_ready`: probe the field once
   (`platform.insert_text(&payload.final_text)`), pass the outcome into
   `delivery_refusal`. The gate now refuses when `!outcome.field_readable`
   (no writable field focused) instead of ⌘V-ing into the wrong place. Text is
   kept in History and the user is told.
2. `ax_text.rs::selected_range`: when `AXSelectedTextRange` is unreadable, fall
   back to the **start** of the field `(0, 0)`, not the end.

## Verification

- `cargo build -p teletype-desktop` clean.
- `cargo test -p teletype-desktop -p teletype-core` all green.
- New tests: `a_refused_fast_path_with_no_writable_field_is_not_pasted`
  (refuses) and `a_refused_fast_path_with_a_readable_field_may_still_paste`
  (proceeds) in `dictation.rs`.
- **Live dictation repro NOT performed**: the currently selected speech model
  is Whistle, which fails to load in this build ("needs the Cactus Needle
  runtime"), so a real end-to-end dictation could not be driven. The fix is
  verified by unit tests and the code trace, not a live mic run.

## Reusable lesson

The delivery gate proves *which app/window* the text goes to, but not *which
field within it*. A keystroke paste (⌘V) is only caret-safe when the focus is a
writable text field with a readable selection range. `InjectionOutcome
.field_readable` is already true/false for exactly that, so the guard is a
single boolean check — no new AX query needed. When a fast path is refused,
decide whether the fallback is *positionally safe* before doing it; "it still
works" (text lands somewhere) is not "it landed where the user meant."

## Evidence / sources

- `crates/teletype-desktop/src/ax_text.rs` (`insert_detailed`, `selected_range`)
- `crates/teletype-desktop/src/dictation.rs` (`pipeline_ready`, `delivery_refusal`)
- `crates/teletype-core/src/delivery.rs` (`resolve_delivery`)
- `crates/teletype-core/src/injector.rs` (⌘V fallback)

## 2026-10-23 update: the pre-paste guard is missing from the live path

Re-opened. The P8.1 core cascade (`delivery.rs::deliver`) replaced the direct
path this fix guarded, and the guard did not come with it:

- `dictation.rs::delivery_refusal` (line ~5900) checks only same app/window
  (`gate_passed`) and `refuse_newline_in_terminal`. It no longer takes or
  checks a `field_readable` outcome.
- `field_readable` is produced by `ax_text.rs::insert_detailed` but is not read
  anywhere in the desktop delivery path (`grep field_readable` hits only
  `ax_text.rs` and `platform/mod.rs`).
- The cascade's unreadable-field check runs *after* the keystroke:
  `paste_and_verify` sets the clipboard and fires ⌘V first, and only then, if
  `selected_text()` is `None`, returns `PastedUnverified` (delivery.rs:570-578).
- `PastedUnverified` is logged at `Info` and stamped
  `clipboard(pasted,unverified)` (dictation.rs:5399-5407, 5453). A paste into
  the app's default target therefore still looks like a normal success.

So L038's failure mode is reachable again. The fix is the same shape as before,
but it must run **before** the clipboard write and the ⌘V: probe the focused
element for a writable text field (or reuse `insert_detailed`'s
`field_readable`), and if there is none, return `Refused` with the
"kept in History" message instead of pasting. Add a browser web-area repro and
a non-writable-control repro; unit tests alone did not catch the regression.

## 2026-10-23 fix: guard restored before the clipboard write

- `FieldReader::has_writable_focus` (default true, so adapters that cannot
  know do not block delivery; the macOS `AxFieldReader` overrides it).
- `deliver` refuses `PasteVerify` after mode selection and before the
  snapshot, clipboard write or ⌘V when no writable text field has focus.
- `ax_text::has_writable_focus` reuses `focused_text_field`, the same AX role
  probe the direct-write path already uses, and releases the +1 reference.
- Test `cascade_refuses_paste_without_a_writable_text_field` asserts the
  refusal, zero clipboard writes, zero snapshots, zero ⌘V, zero set-value and
  zero typing. It failed before the guard (`expected Refused, got Typed`) and
  passes after.
- Full `cargo test -p teletype-core -p teletype-desktop`: 17 test binaries,
  exit 0, no failures.

Still not live-verified: no real dictation into a browser web area has been
performed since the fix.

## Promotion

Candidate, re-opened and fixed on 2026-10-23 (guard unit-tested, live
dictation into a browser still not performed).
