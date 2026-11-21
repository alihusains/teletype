---
id: L091
title: Lost modifier release edges can permanently latch the hotkey off
status: verified
type: debugging
area: teletype-desktop / macOS hotkey taps
agent: opencode
tool: cargo test + read-only call-path trace
date: 2026-11-20
---

# Lost modifier release edges can permanently latch the hotkey off

## Problem

In v0.2.16 the dictation hotkey can stop working permanently after a while. Clicking the app icon then appears to do nothing and repeated clicks / Force Quit can crash the app.

## Symptoms

- Hotkey works for a period, then is silently ignored with no error message.
- The Dock icon no longer raises the existing window.
- Repeated launches or Force Quit produce a crash.

## Initial hypotheses

- CMCP-1: rebuilding changes the ad-hoc signature, so macOS silently stops delivering the global hotkey.
- A stuck `Session.hotkey_down` latch after a controller panic.
- A main-thread run-loop wedge from the input-engine tap or a synchronous Tauri wrapper.

## Investigation

1. Traced the Fn/bare-modifier path: native CGEventTap callback -> Rust `on_fn_state`/`on_mod_state` -> `Event::HotkeyDown/Up` -> controller.
2. Compared Fn and mod taps against the permission watcher and single-instance Dock activation.
3. Checked git history: v0.2.16 lacked the permission watcher, bare-modifier mask fix, crash reports and worker panic containment.
4. Read the native tap latches (`g_fnDown`, `g_down`) and the Rust edge latches.

## Evidence

- `fn_tap.rs` and `mod_tap.rs` kept `LAST_DOWN` as a function-local static written only by the callback. Native `start()` resets `g_fnDown`/`g_down`, but restarting never reset the Rust latch.
- A tap disabled while a key is held can lose the release edge. On re-enable, the native latch was left set, so the next physical DOWN looked like a repeat; the Rust latch also stayed set.
- Release notes record CMCP-1: a rebuilt binary can register the hotkey while macOS delivers no events.
- Dock/Force-Quit symptoms are consistent with an unresponsive main thread, but no crash report was available on this host, so that part is not proven.

## Root cause

A lost release edge leaves both native and Rust down-latches set. Re-registering or re-enabling the tap resets only the native latch (and only in some paths), so the Rust edge detector swallows every subsequent press.

## Fix

- Added `hotkey_latch.rs::EdgeLatch`: reset on tap start/stop, and accept a down state older than 30 s as a fresh press (stale-edge recovery).
- Wired it into `fn_tap.rs` and `mod_tap.rs`; serialized native stop/start with a lifecycle mutex to avoid double-join.
- Native tap callbacks now clear `g_fnDown`/`g_down` whenever a disabled tap is re-enabled.
- Added a 2 s watchdog to `input_engine_tap.m`, which previously had no tap-level watchdog.

## Verification

- Added failing tests first for restart-reset and stale-down recovery; they failed to compile until `EdgeLatch` existed and now pass (2/2).
- Full `cargo test -p teletype-desktop` passes with the fixes.
- Live Mac repro of the original symptom is still needed to confirm the Dock/Force-Quit crash is eliminated.

## Reusable lesson

When a global input tap is disabled and re-enabled, the OS does not necessarily deliver the missing key-up. Every layer that caches an edge (native and Rust) must be reset together, and a stale-down timeout is needed when the layer that lost the edge cannot tell Rust.

## Evidence / sources

- User report: v0.2.16 hotkey stops, Dock icon unresponsive, Force Quit crash.
- Code: `crates/teletype-desktop/src/{hotkey_latch,fn_tap,mod_tap}.rs`, `src/{fn_tap,mod_tap,input_engine_tap}.m`.
- Documentation: `docs/release-notes-v0.2.17.md` CMCP-1 notes.

## Promotion

Verified by code evidence and unit tests; living-user confirmation still pending.

---
