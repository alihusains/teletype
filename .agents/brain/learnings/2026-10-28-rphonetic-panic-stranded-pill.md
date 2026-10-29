---
id: L082
title: A third-party char-boundary panic in the controller handler stranded the pill; rphonetic is unsafe on non-ASCII
status: verified
type: debugging
area: teletype-core/rescoring + learning/judge + teletype-desktop/dictation controller
agent: main session (Qoder)
tool: cargo test, osascript (System Events Ctrl), CGWindowList probes, live app log
date: 2026-10-28
---

# A third-party char-boundary panic in the controller handler stranded the pill; rphonetic is unsafe on non-ASCII

## Problem

After one or two dictations the pill broke: it stayed visible showing a clipped
"Tr..." and every later hotkey press was ignored, so dictation was dead until
the app restarted. Reported live by Ali on 2026-10-28 as "the pill is broken
now, after 1-2 dictations nothing is working properly".

## Symptoms

- Pill window visible at its idle size (58x24) showing clipped text, forever.
- `/tmp/teletype-app.log` had `Error event handler panicked` with
  `byte index 4 is not a char boundary; it is inside 'É' (bytes 3..5)` of
  `UZKÉ`, at `rphonetic-4.0.0/src/double_metaphone.rs:234`.
- After the panic, a fresh Ctrl hold produced zero new log lines: the
  controller still believed a take was in flight.
- Tray still worked; the process was alive. Not the G026 spool deadlock
  (`sample` showed the dictation thread parked in `recv`, i.e. healthy).

## Initial hypotheses

1. The React pill machine was stuck in a busy phase (frontend state bug).
2. The overlay window placement/show-hide logic was wrong.
3. G026 recurrence: controller spool join deadlock.
4. A panic in a pipeline worker stranding the pill (G029 class).

## Investigation

1. Read `ui/src/pillMachine.ts` and `overlay.rs`: window show/hide is
   backend-owned by `overlay::update`, which hides on `PillState::Idle`; a
   stuck pill means the controller never reached `go_idle()`.
2. Read the controller loop: `Controller::spawn` wraps `session.handle(event)`
   in `catch_unwind` and only logs. So a panic aborts that event with no
   terminal state: the phase stays busy, later hotkeys are swallowed by the
   state machine, and the overlay is never told to go idle.
3. Pulled the panic payload from the app log and traced it to the phonetic
   rescoring pass: `rescore_with_confidence` runs on the controller thread
   inside `transcribed`, and `rescoring.rs:125` / `:303` call
   `DoubleMetaphone::double_metaphone` on lowercased transcript words.
4. Read rphonetic 4.0.0: `char_at` does `value[index as usize..]` with a byte
   index, and `contains` slices `&value[start..start+length]`. Any non-ASCII
   input can panic. Confirmed the same use in `learning/judge.rs::phon_sim`.
5. Reproduced the panic with a unit test on `uzké` (exact production string)
   before changing any code.

## Evidence

- `cargo test` on the new tests failed with the identical panic text and the
  identical library line as the production log.
- Live reproduction: System Events Ctrl hold produced the panic take; a second
  Ctrl hold produced no log lines and the pill stayed at 58x24 onscreen
  (window id captured via CGWindowList).

## Root cause

Two defects compounding:

1. **Third-party char-boundary panic.** rphonetic 4.0.0 Double Metaphone is
   byte-indexed and panics on any string with a multi-byte char. The phonetic
   rescue path (`rescoring` pass 2, and `phon_sim` in the learning judge) fed
   it raw lowercased transcript text, so any accented word (Polish, French,
   etc.) crashed the handler.
2. **No controller-level recovery.** The `catch_unwind` around
   `session.handle(event)` logged and continued, but did not end the take.
   G029's `WorkerExitGuard` only covered worker threads, not a panic inside
   the controller's own event handler. So the phase stayed busy and the pill
   stayed visible.

## Fix

- `teletype-core/src/rescoring.rs`: new `metaphone_code()` returns `Some(code)`
  only for ASCII input; non-ASCII spans/terms score on Jaro-Winkler alone
  (character-based, safe).
- `teletype-core/src/learning/judge.rs::phon_sim`: returns Jaro-Winkler
  directly when either side is non-ASCII.
- `teletype-desktop/src/dictation.rs`: the panic arm now logs the payload and
  calls `session.recover_after_panic()`, which runs `go_idle()` for a busy
  session (or pushes an idle pill for an already-idle one). The recovery call
  is itself inside `catch_unwind`, so a second panic cannot kill the thread.
- Tests: `non_ascii_low_conf_word_does_not_panic`,
  `phon_sim_handles_non_ascii`, `recover_after_panic_returns_a_busy_session_to_idle`.

## Verification

- Core lib 928/928 and desktop lib 410/410 green.
- Live: two consecutive Ctrl-hold takes on the restarted app; the second take
  pasted ("inject: pasted verified (26 chars)") and the pill window reported
  `onscreen=false` after each. Before the fix, the second take was inert.

## Reusable lesson

- Treat every third-party string algorithm as ASCII-only until proven
  otherwise: rphonetic is byte-indexed and panics inside multi-byte chars.
  Guard before the call, do not `catch_unwind` around it as the design.
- A panic guard on a long-lived event loop is only half a fix: the handler
  path must also carry an explicit recovery that returns UI state to idle.
  RAII guards cover workers; the loop that dispatches events needs its own
  recovery arm. Same class as G029, one layer up.

## Evidence / sources

- Panic: `rphonetic-4.0.0/src/double_metaphone.rs:234` via
  `crates/teletype-core/src/rescoring.rs` `double_metaphone` calls.
- Live log: `/tmp/teletype-app.log` (`event handler panicked`, `inject: pasted verified`).
- Tests: `crates/teletype-core/src/rescoring.rs`,
  `crates/teletype-core/src/learning/judge.rs`,
  `crates/teletype-desktop/src/dictation.rs`.
- User confirmation: 2026-10-28 bug report (pill broken after 1-2 dictations).

## Promotion

Verified by the exact panic string in unit tests, live reproduction before the
fix, and two consecutive live takes after it.
