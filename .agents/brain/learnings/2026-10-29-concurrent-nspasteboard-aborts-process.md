---
id: L086
title: Concurrent NSPasteboard access aborts the process, and the per-guard mutex could not prevent it
status: verified
type: debugging
area: teletype-core / injector + autotext placeholders / teletype-desktop clipboard
agent: opencode
tool: opencode
date: 2026-10-29
---

# Concurrent NSPasteboard access aborts the process, and the per-guard mutex could not prevent it

## Problem

Two independent things pointed at the same code:

1. Ali reported seeing `teletype-placeholder-test-1234` sitting in his macOS
   clipboard, and sometimes pasted into his text during dictation.
2. `cargo test -p teletype-core --lib` died non-deterministically (about every
   other run) with `signal: 11, SIGSEGV` and occasionally `signal: 6, SIGABRT`.

## Symptoms

- Crash report: `EXC_BAD_ACCESS (SIGSEGV)`, `KERN_INVALID_ADDRESS`, faulting
  thread named `autotype::placeholders::tests::clipboard_token_never_fails`,
  stack `objc_msgSend` -> `-[NSPasteboard _updateTypeCacheIfNeeded]` ->
  `-[NSPasteboardItem stringForType:]` -> `arboard::Clipboard::get_text` ->
  `read_clipboard`.
- `--test-threads=1`: 934 passed, twice, no crash. Default parallel threads:
  crashed 4 of 6 runs.
- Skipping `clipboard_expands_to_current_text_when_available` (the only writer)
  and keeping every other test: 6 of 6 clean. So the crash needed a reader and
  a writer running at the same time.

## Initial hypotheses

1. The new `ClipboardRestore` drop guard added too much ObjC call surface.
   Rejected by the crash report: the faulting thread is the *unmodified*
   `clipboard_token_never_fails`, and the guard only added one read and one
   write. It raised the frequency, it did not create the race.
2. A pre-existing flake somewhere else in `teletype-core`. Rejected: the
   crash disappears entirely when the clipboard write test is skipped.
3. Concurrent `NSPasteboard` use from two test threads. Confirmed by the
   single-thread control run and the skip control run.

## Investigation steps

1. Three controlled runs to isolate the variable: full suite, `--skip
   clipboard`, and `--test-threads=1`.
2. Read the `.ips` crash report and parsed the faulting thread's stack, rather
   than guessing from the abort signal.
3. `git diff HEAD -- placeholders.rs` to separate what this session changed
   from what was already there.
4. Mapped every pasteboard touch in the process (`grep arboard`, `grep
   NSPasteboard`): `injector.rs` (read, set, restore thread), `placeholders.rs`
   (`read_clipboard`), `desktop/clipboard.rs` (the guard), `delivery_io.rs`,
   `tray.rs`, `commands/transforms.rs`, `command_executor.rs`.

## Evidence

- Crash report `teletype_core-...-2026-10-29-105317.ips` (faulting thread 2 =
  `clipboard_token_never_fails`).
- `--test-threads=1` -> `934 passed` x2; default threads -> 4 crashes in 6.
- `crates/teletype-desktop/src/clipboard.rs` already said "NSPasteboard is not
  documented as thread-safe, so every call is serialised", but the mutex was a
  field of `MacClipboardGuard`, and `command_executor.rs` and `transforms.rs`
  each build a *fresh* guard per operation, so those locks never met.

## Root cause

Two separate defects with one cause: every thread in the process shares one
`NSPasteboard` general pasteboard, and nothing serialised access to it.

- Test/clipboard-content defect: `clipboard_expands_to_current_text_when_available`
  wrote a probe string into the user's real pasteboard and never restored it.
  `is_headless()` only checks the `CI` env var, so any local `cargo test`
  clobbered Ali's clipboard. That text then reached his screen two ways:
  a quick-tap release pastes the clipboard (`tap_paste` -> Cmd+V) and a
  `{{clipboard}}` AutoText entry expands from it. Not fixed in HEAD, and it
  predated this session.
- Crash: the same test's `set_text` raced another thread's `get_text` inside
  AppKit's pasteboard type cache.

## Fix

1. `teletype-core/src/injector.rs`: added `pub fn pasteboard_lock()`, a
   process-wide `static Mutex<()>`, plus `with_fresh_clipboard()` for the
   detached restore thread. Taken at every leaf pasteboard operation
   (`Clipboard::new` in the inject loop, `set_text`, `unreadable`, the restore
   arms) and never nested, because the lock is not reentrant.
2. `autotext/placeholders.rs`: `read_clipboard()` takes the lock; expansion is
   now parameterised on the clipboard source (`expand_with(text, app,
   &dyn Fn() -> String)`), so the unit tests use a fake source. The probe
   `set_text` test is gone and replaced with tests for exact expansion, lazy
   probing (a template without `{{clipboard}}` must not read it), and the
   empty-on-failure contract.
3. `teletype-desktop`: `MacClipboardGuard` lost its per-instance mutex and uses
   the global lock (it is now a unit struct); `delivery_io::GuardClipboard::set_text`,
   `tray.rs`, `commands/transforms.rs` and `command_executor.rs` (snapshot
   fallback, `set_text`, `restore_previous_text`) each hold the lock around
   their pasteboard call.
4. Regression test `pasteboard_lock_has_one_holder_at_a_time`: 8 threads x 200
   acquisitions, asserting two holders are never inside at once.

## Verification

- `cargo test -p teletype-core --lib` 10 consecutive runs: 10 clean (was
  crashing in about half).
- `cargo clippy -p teletype-core -p teletype-desktop --all-targets`: no
  warnings.
- `cargo test -p teletype-core -p teletype-desktop`: all binaries green
  (core lib 937, desktop lib 425).
- Sentinel proof that no test touches the pasteboard any more: `pbcopy` a known
  string, run `cargo test --workspace`, read it back. `TELETYPE-CLEAN-VERIFY-OK`
  survived the run unchanged (exit 0, zero failed binaries). Before the fix the
  same run replaced whatever was there with `teletype-placeholder-test-1234`.

## Reusable lesson

- A mutex that guards an *OS shared resource* cannot live on a per-call object.
  If callers construct the wrapper freely (`MacClipboardGuard::new()` per
  operation), the lock is decoration. Put it on the process, next to the
  resource.
- `SIGSEGV` inside `objc_msgSend` on a `-[NSPasteboard ...]` frame, that
  vanishes under `--test-threads=1`, is a thread race, not a bad pointer in
  Rust code. Run the single-threaded control before reading any Rust source.
- A unit test that mutates shared user state (the real pasteboard, the real
  home directory, the real clock) is a defect even when it passes: it is
  invisible in CI and destructive on a dev machine. Inject the source instead.
- Two symptoms can share one cause. Ali's stray clipboard text and the
  segfault looked unrelated; both were the same test touching AppKit.

## Promotion

Candidate. Reproduced (crash control runs) and fixed (10 clean runs), but the
in-app race between the restore thread and a `{{clipboard}}` expansion has not
been observed live on a desktop, so the production half of the fix is reasoned
from the call map, not from an app-level repro.
