---
id: L095
title: Event-tap teardown joined the tap thread unbounded, freezing the app on the main thread
status: verified
type: debugging
area: "teletype-desktop/fn_tap.m, mod_tap.m, dictation::register_hotkey"
agent: Buffy
tool: Freebuff
date: 2026-11-22
---

# Event-tap teardown joined the tap thread unbounded

## Problem

Teletype 0.2.21, first launch then background: the app stops responding. The
Dock icon does nothing, and Force Quit produces a hang report instead of a
crash.

## Symptoms

`spindump` (Event: hang, unresponsive for 11 s before sampling):

```
Thread 0 (main):
  ... tauri_runtime_wry::handle_user_message
  teletype_desktop_lib::dictation::Controller::register_hotkey + 72
  teletype_desktop_lib::dictation::Controller::unregister_hotkey + 336
  teletype_fn_tap_stop + 68
  __ulock_wait            <- pthread_join, waiting forever
Thread 0xb6740d:
  fnTapThreadMain + 400
  CFRunLoopRunInMode -> __CFRunLoopServiceMachPort -> mach_msg2_trap  (asleep)
```

The tap thread is idle in its run loop and never notices the stop request.

## Initial hypotheses

1. The CGEventTap callback deadlocked on a CoreGraphics lock.
2. The tap thread was blocked in `CGEventTapCreate` (no Accessibility).
3. The stop request never reached the run loop.

## Investigation

1. Read the stack: the main thread is in `pthread_join`, so the hang is the
   join, not the tap callback (hypothesis 1 rejected).
2. Read `fn_tap.m`: the worker loops on
   `CFRunLoopRunInMode(kCFRunLoopDefaultMode, 3600, false)` and `stop()` sets a
   flag, calls `CFRunLoopWakeUp`, then `pthread_join` (hypothesis 3 supported).
3. Checked `/tmp/teletype-fntap.log`: many start/stop cycles, no watchdog
   re-enable lines, and `CGEventTapCreate FAILED` bursts when Accessibility was
   not granted. The tap was being torn down and rebuilt repeatedly.

## Root cause

`CFRunLoopWakeUp` is **dropped when the target run loop is not currently
running**. `teletype_fn_tap_start` returns as soon as the worker sets
`g_started = 1`, which happens just before the worker enters its run loop. A
stop in that window sets the flag and wakes a loop that is not running yet, so
the wakeup is lost. The worker then enters `CFRunLoopRunInMode` with a 3600 s
timeout and sleeps. `stop()` calls `pthread_join` with no timeout, and
`register_hotkey` reaches that stop on the **main thread**, so the app freezes
for up to an hour.

The permission/liveness watcher triggers exactly this path: it calls
`register_hotkey` (which first calls `unregister_hotkey`) whenever the tap
looks dead, and settings saves do the same for every change.

## Fix

`fn_tap.m` and `mod_tap.m`:

* The run loop wait is bounded (`0.25 s`), so a lost wakeup costs one interval,
  never the whole timeout.
* `stop()` waits on a condition variable with a 1 s deadline instead of calling
  `pthread_join` unconditionally; if the thread is still winding down it returns
  without blocking (and `start()` refuses to build a second tap).
* Every exit path of the tap thread (including `CGEventTapCreate` failure) marks
  the thread finished and signals, so a later start is never refused forever.
* `g_started` is cleared for the whole teardown so a liveness check cannot see a
  half-torn-down tap as healthy.

`dictation.rs`: `register_hotkey` returns early when the Fn binding is requested
and the tap is already live, so the watcher/settings churn no longer rebuilds a
healthy tap.

## Verification

* `clang -fsyntax-only -Wall -Wextra fn_tap.m mod_tap.m` clean.
* `cargo check -p teletype-desktop` passes (the build script compiles both .m).
* Not reproduced at runtime: the hang needs the app under Accessibility + a
  background re-registration, which this checkout cannot drive.

## Reusable lesson

Any native helper that a Rust command calls on the main thread must have a
**bounded** teardown. `pthread_join` after a `CFRunLoopWakeUp` is not bounded:
run-loop wakeups are level-triggered and dropped if the loop is not running, so
pair every cross-thread wakeup with a short run-loop timeout **and** a deadline
on the join. This is the same class of bug as the "release latches hotkey off"
learning (L091): liveness/restart paths that run on the main thread are where a
rare race becomes a permanent freeze.

## Evidence / sources

- User-provided `spindump` (`crashreportapple.txt`, 2026-11-22 15:34, v0.2.21).
- `/tmp/teletype-fntap.log` start/stop history.
- `crates/teletype-desktop/src/fn_tap.m`, `mod_tap.m`, `dictation.rs`.

## Promotion

Verified at the code and compile level from the recorded hang trace; runtime
confirmation on a signed build is still pending.
