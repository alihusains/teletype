---
id: 2026-10-03-headless-objc-exception-aborts-test-binary
title: Headless CI clipboard calls raise a pending ObjC exception that aborts the whole test binary
status: verified
type: debugging
area: teletype-core / autotext placeholders / CI
agent: opencode
tool: openai/Qwen3.8
date: 2026-10-03
---

# Headless CI clipboard calls raise a pending ObjC exception that aborts the whole test binary

## Problem

The CI `Test (Rust + UI)` job (macos-latest) was failing `cargo test -p
teletype-core` with a process abort, not a normal test failure:

    fatal runtime error: Rust cannot catch foreign exceptions, aborting
    process didn't exit successfully: .../teletype_core-... (signal: 6, SIGABRT)

The failure point was random each run (right after an unrelated passing test
like `emoji::tests::bare_heart_is_ambiguous`), which made it look like a
flaky test in whatever happened to run last.

## Symptoms

- `cargo test -p teletype-core` aborts with SIGABRT on the GitHub `macos-latest`
  runner, but passes on a developer's desktop with a window server.
- The "failing" test is a victim, not the culprit: tests run in parallel
  threads, and the abort happens on whichever thread next enters the ObjC
  runtime after a foreign exception is left pending.
- No test assertion failure is printed; the binary dies before any
  `test result:` line for the lib target.

## Initial hypotheses

1. A real test regression in the emoji/autotext code (rejected: the emoji test
   is pure string logic and passes in isolation).
2. A flaky timing test (rejected: the abort is a SIGABRT from the runtime, not
   an assertion).
3. A native (ObjC) call raising a foreign exception that Rust cannot catch
   (confirmed).

## Investigation

1. Read the failed CI log: the only non-test line before the abort was
   `fatal runtime error: Rust cannot catch foreign exceptions, aborting`.
2. Searched `teletype-core` for native calls reachable from tests: found
   `arboard::Clipboard::new()` in `autotext/placeholders.rs` (`read_clipboard`
   and two `#[test]` fns).
3. Inspected `arboard 3.6.1/src/platform/osx.rs`: it calls
   `NSPasteboard::generalPasteboard()` and `pasteboardItems()` via raw ObjC
   `msg_send!`. On a headless box (no window server) these raise an ObjC
   exception. `arboard` maps it to a Rust `Err`, but the exception stays
   *pending* in the ObjC runtime.
4. Reproduced locally by setting `CI=true` (GitHub Actions sets this), which
   only aborted after the fix attempt below was made - see Root cause.

## Evidence

- CI run 37145653715, job "Test (Rust + UI)", step "Rust tests (core)":
  `fatal runtime error: Rust cannot catch foreign exceptions, aborting` then
  `signal: 6, SIGABRT`.
- `arboard` osx backend uses `msg_send![NSPasteboard::class(),
  generalPasteboard]` - a direct ObjC call with no exception trampoline.
- Locally, running the core lib tests with `CI=true` reproduced the abort once
  a code path still called the real clipboard; gating the call made it pass.

## Root cause

On a headless runner the macOS general pasteboard is not serviceable. Any
`arboard` clipboard call raises an Objective-C exception. `arboard` converts
that to a `Result::Err`, but the ObjC exception is never cleared - it remains
pending in the ObjC runtime. The next time *any* thread in the process enters
ObjC (or the runtime unwinds), Rust's "cannot catch foreign exceptions" guard
aborts the entire test binary. Because the lib tests run on many parallel
threads, the visible "failing" test is whichever thread happened to touch ObjC
next, so the crash site looks random.

The trap: `expand_placeholders` in `autotext/placeholders.rs` calls
`read_clipboard()` unconditionally whenever the input contains `{{`, even for
`{{date}}`/`{{time}}` tokens. So the real-clipboard read is not limited to the
two `{{clipboard}}` tests - it fires from any placeholder expansion, and
gating only the two named tests is not enough.

## Fix

`crates/teletype-core/src/autotext/placeholders.rs`:

- Added `is_headless()` (true when the `CI` env var is set and not "false").
- `read_clipboard()` now returns an empty string *before* constructing an
  `arboard::Clipboard` when headless, so no code path touches the real
  clipboard on CI.
- The `clipboard_expands_to_current_text_when_available` test returns early
  when headless, before its own `Clipboard::new()`.

The desktop `clipboard.rs` tests were checked and are pure (in-memory
`ClipboardSnapshot` structs), so they do not need gating.

## Verification

- `CI=true cargo test -p teletype-core` -> 449 passed, 0 failed, no SIGABRT.
- `cargo test -p teletype-core` (no CI, desktop) -> 449 passed (real clipboard
  path still exercised).
- `CI=true cargo test -p teletype-desktop` -> all passed.
- `cargo fmt --all --check` clean; clippy (via `scripts/check-clippy-clean.py`)
  clean.

## Reusable lesson

- "Rust cannot catch foreign exceptions, aborting" + SIGABRT in a macOS test
  binary = a native (ObjC/CF) call raised an exception that was left pending.
  Find the native call, not the test that printed last.
- `arboard` (and other crates that wrap ObjC via `msg_send!`) can leave a
  pending foreign exception even when they return `Err`. On headless CI, do
  not *attempt* the native call at all - gate it behind a `CI` check rather
  than relying on the `Err` being catchable.
- GitHub Actions sets `CI=true`; use it (not a display check) to detect
  headless runners.
- When a function calls a native side effect unconditionally (here
  `read_clipboard` inside `expand_placeholders`), gating the *callers* is not
  enough; gate the *source*.

## Evidence / sources

- CI run: https://github.com/alihusains/teletype/actions/runs/37145653715
- Commit: (the fix commit on main)
- arboard source: ~/.cargo/registry/src/.../arboard-3.6.1/src/platform/osx.rs

## Promotion

Verified: reproduced locally with `CI=true` and the abort disappears once the
real clipboard call is skipped on CI.

---
