---
id: L100
title: Closing the main window only hides it, and without a RunEvent::Reopen handler a Dock-icon click did nothing
status: verified
type: debugging
area: "teletype-desktop/lib.rs (windows + run events), tap teardown, idle unload"
agent: Buffy
tool: Freebuff
date: 2026-11-25
---

# A hidden main window plus no `Reopen` handler is an app that "never comes back"

## Problem

Reported against the previous pre-release (v0.2.21): after the app had been
running a while with models loaded, and after the user minimized / closed the
window, (a) the hotkey stopped working, (b) clicking the app icon never brought
the app to the foreground, and (c) Force Quit produced a report from macOS.

## Symptoms

- Hotkey presses stop producing dictation.
- Clicking the app icon does nothing; no window appears.
- Only Force Quit recovers, and macOS shows a report.

## Initial hypotheses

1. The main thread is wedged (L095's tap-teardown join) — the exact same report
   shape as v0.2.21, which L095 diagnosed.
2. The idle model unload leaves the engine unusable, so the next take dies.
3. Tauri's window/`Reopen` plumbing was never wired, so the window *cannot*
   come back independently of any hang.

## Investigation

1. Read the window lifecycle: `lib.rs` `on_window_event` intercepts
   `CloseRequested` for the `main` window, calls `api.prevent_close()` and
   `window.hide()`. The app deliberately keeps running in the tray.
2. Grepped the run hook: only `RunEvent::Exit` was handled. There is **no**
   `RunEvent::Reopen` branch, so the Dock-icon click (`applicationShouldHandleReopen`
   → Tauri `Reopen`) fell through to a no-op. The only path that raised the
   window was the `single_instance` plugin, which fires on a *second launch*, not
   on clicking the icon of a running app — that is why "opening it again" sometimes
   worked and clicking did not.
3. Re-read the tap teardown to check hypothesis 1: `fn_tap.m` and `mod_tap.m`
   already have the L095 fix (0.25 s run-loop wait, 1 s condvar deadline,
   `mark_tap_thread_finished()` on every exit, `start()` refuses a second
   thread, unguarded `pthread_join` only after a successful wait). No unbounded
   main-thread join remains in these helpers. `input_engine_tap.m` has no thread
   and no join.
4. The L095 fix (commit `2978641`) is contained in tag `v0.2.22`, not in
   `v0.2.21`, which is the pre-release the user installed.
5. `scripts/check-artifact-pins.sh` aside, the timers that fire while hidden are
   the idle-unload tick and the permission/liveness watcher; neither touches the
   window. The `SpeechModelManager::tick` race (plans/009) is the separate
   "model freed under a live take" defect, fixed in this batch.

## Root cause

Two independent defects shipped in the same window of time:

1. **The window could never come back.** `CloseRequested` hides the main window
   and nothing handles `RunEvent::Reopen`, so the one OS event that means "the
   user clicked the app icon and wants to see it" was dropped. The app looked
   wedged because it refused to show its window, not because it was hung.
2. **The hotkey could die for good.** Liveness recovery is capped at one
   re-registration per live→dead episode, so a tap macOS disabled while the
   window was hidden (or a re-registration refused because the previous tap
   thread was still winding down) stayed dead until the process restarted. With
   the window unreachable, the user had no way to trigger a repair either.

## Fix

`crates/teletype-desktop/src/lib.rs` run hook:

- handle `tauri::RunEvent::Reopen` by un-minimizing, showing and focusing the
  `main` window (same sequence the single-instance handler uses);
- at that moment, if `hotkey_liveness::hotkey_live(&configured_hotkey)` reports
  `Some(false)`, call `permission_watch::reregister_hotkey_now()` so coming back
  to the app also repairs a dead registration.

## Verification

- `cargo check -p teletype-desktop` exit 0.
- `cargo test -p teletype-desktop --lib` 649 passed, 0 failed.
- `cargo fmt --all -- --check` exit 0.
- **Not verified at runtime**: the Dock click needs a real window session; the
  handler is event-loop plumbing with no unit-test seam in this repo.

## Reusable lesson

A tray/accessory app that hides its window on close must also handle `Reopen`.
`prevent_close()` + `hide()` is a promise to the user that the window is
retrievable, and the only thing that keeps that promise is the `Reopen` branch
(plus `unminimize`). And any "recover the hotkey" path that the user can reach
by hand is worth having: when the automatic watcher is rate-limited to one
attempt per episode, the user's click is the natural second attempt.

## Evidence / sources

- `crates/teletype-desktop/src/lib.rs` (`on_window_event`, run hook,
  single-instance handler).
- `crates/teletype-desktop/src/fn_tap.m`, `mod_tap.m` (L095 fix present).
- `git tag --contains 2978641` → `v0.2.22`.
- User report, 2026-11-25 (pre-release install; minimize → hotkey dead → icon
  click does nothing → Force Quit).
