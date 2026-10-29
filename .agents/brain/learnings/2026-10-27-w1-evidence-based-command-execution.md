# W1 evidence-based command execution: form-encoding, private paste helper, and enigo API gotchas

**ID:** L067
**Date:** 2026-10-27
**Agent:** pi (cmd-w1-effects-evidence-tools task)
**Area:** teletype-desktop / command_executor (Effects trait, evidence-based send_message, type_text)
**Status:** verified

## Problem

While implementing W1 (Effects trait, evidence-based `send_message`, four new
tools `compose_email` / `create_calendar_event` / `open_file` / `type_text`),
three non-obvious platform/library facts surfaced that would trip up the next
person touching this code or a future live-action task.

## Initial hypotheses

1. "Use `urlencoding::form_url_encode` or hand-encode the compose/gcal URLs" —
   rejected, a `url` dependency already exists.
2. "Just call `crate::delivery_io::paste_key(&enigo)` for the ⌘V" — assumed it
   was crate-root reachable.
3. "Use `enigo::Enigo::new()` and `enigo.keydown/keyup`" — assumed the 0.5-era
   trait-based API.

## Investigation steps + evidence

1. **Form encoding, not percent-encoding.** `url::form_urlencoded::byte_serialize`
   encodes a space as `+`, an `@` as `%40`, and a `/` as `%2F` (application/
   x-www-form-urlencoded). My first URL tests asserted `su=Hi%20%26%20there`
   and failed — the real output was `su=Hi+%26+there`. The URLs are *valid*;
   the assertion was wrong. `form_urlencoded::byte_serialize` is the correct
   choice here because it also encodes `&` (→ `%26`), which is what stops a
   dictated body containing `&su=evil` from injecting a `su` parameter into the
   Gmail URL. (If you ever switch to plain `percent_encoding`, a space stays
   `%20` and you must still ensure `&` is encoded — but the form encoder does
   all of it and is the idiomatic query-string builder.)

2. **`delivery_io::paste_key` is private.** It is a `pub fn` inside
   `mod macos { ... }` in `crates/teletype-desktop/src/delivery_io.rs`, but that
   module is `mod macos` (private) and not re-exported at the crate root. From
   another module it is *inaccessible* (`E0425: cannot find function
   paste_key in module crate::delivery_io`). The fix is to **inline** the
   identical ⌘V keystroke in `command_executor.rs` (mirroring the body of
   `paste_key`), not to reach across into the private submodule. This keeps
   `command_executor` self-contained and avoids touching `delivery_io.rs` (not
   in the W1 file-ownership list).

3. **enigo 0.6 API.** `enigo::Enigo::new()` takes **one** argument:
   `&enigo::Settings`. And the `.key(Key, Direction)` method is provided by the
   `enigo::Keyboard` trait — you must `use enigo::Keyboard as _;` in scope or
   you get `E0599: no method named key`. The paste is:
   ```rust
   use enigo::Keyboard as _;
   enigo.key(enigo::Key::Meta, enigo::Direction::Press)?;
   enigo.key(enigo::Key::Other(9), enigo::Direction::Click)?; // ANSI 'V' = 9
   enigo.key(enigo::Key::Meta, enigo::Direction::Release)?;
   ```
   (This matches `delivery_io::macos::paste_key` exactly.)

## Root cause

- (1) is a genuine library-behavior gotcha (form vs. percent encoding).
- (2) is a module-visibility mismatch between the "paste helper exists" mental
  model and the actual `mod macos` (private) scoping.
- (3) is a version-specific API surface (0.6 changed from the 0.5 trait-based
  `keydown(key, code)` to `key(Key, Direction)` behind the `Keyboard` trait).

## Fix

- Use `url::form_urlencoded::byte_serialize(...).collect::<String>()` for all
  query values in `compose_email` / `create_calendar_event`; assert the
  *form-encoded* output (`+` for spaces) in tests, and add a guard test that a
  body containing `&su=evil` does not produce a `su` parameter.
- Inline the ⌘V keystroke in `command_executor.rs` (see evidence #3) rather
  than importing the private `delivery_io::paste_key`.
- Gate the `type_text` live actions (clipboard + keystroke) behind
  `#[cfg(target_os = "macos")]` so the unit tests never touch the clipboard or
  fire a keystroke.

## Verification

- `cargo test -p teletype-desktop --lib` → 388 passed, 0 failed (incl. 62
  `command_executor::tests`), after the reviewer fixed four defects (ASCII
  guard in `parse_local_datetime`, required `compose_email` subject/body, live
  frontmost-app identity in `capture_window_identity`, and a real second
  evidence read in `execute_send_message`).
- `cargo clippy -p teletype-desktop --lib` → clean; `cargo clippy -p
  teletype-inference --lib` → clean after adding `// SAFETY:` comments to the
  three libc unsafe blocks in `command_llm.rs`.
- `cargo check -p teletype-desktop --lib` → clean.
- Live `type_text` / live `send_message` evidence round-trip are **UNVERIFIED**
  — they require a real clipboard + keystroke / real Messages send, which the
  W1 task forbids (no GUI, no real iMessage). Deferred to a separate
  live-action task.

## Reusable lesson

When adding a cross-module caller for a helper, check its *actual* module
visibility first (`mod x` private vs `pub mod x` / re-exported) — a `pub fn` in
a private module is not callable from elsewhere. For query-string building in
Rust, prefer `url::form_urlencoded::byte_serialize` and remember it emits form
encoding (space → `+`), not percent encoding. For enigo 0.6, always import the
`Keyboard` trait and pass `&Settings` to `Enigo::new`.

## Linked

- Decision D036 (W1 evidence-based command execution contract)
- ADR (none — this is a learning, not a durable architecture decision; the
  evidence-based-send decision is recorded as D036)
