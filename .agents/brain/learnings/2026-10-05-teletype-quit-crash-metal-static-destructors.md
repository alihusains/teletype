# L015: Tray quit SIGABRT — ggml Metal device lives in a C++ static that dies during exit()

**ID:** L015
**Date:** 2026-10-05
**Agent:** Codex
**Project area:** teletype-desktop (lifecycle) / teletype-speech (parakeet) / parakeet-sys (whisper.cpp)
**Status:** verified
**Evidence:** crash report `teletype-2026-10-05-212537.ips` (pre-fix); post-fix live test: app launched with Parakeet + S1-mini warm, quit via `kAEQuitApplication` AppleEvent to the running process, clean exit, zero new crash reports in `~/Library/Logs/DiagnosticReports/`.

## Problem

Quitting Teletype from the tray (Quit menu item) SIGABRT'd: `ggml_abort` inside `ggml_metal_rsets_free` → `ggml_metal_device_free` → `~unique_ptr<ggml_metal_device>` → `__cxa_finalize_ranges` → `exit` → `-[NSApplication terminate:]`.

## Root cause

whisper.cpp's Metal backend parks the GPU device in a **function-local static** (`ggml_metal_device_get` in `ggml-metal-device.cpp`). When the process exits, C++ runs static destructors (`__cxa_finalize_ranges`). At that point:
1. `NSApplication terminate:` has already called `exit()` directly — the Tauri `RunEvent::Exit` hook may not have freed our model buffers.
2. The residency-set free asserts `[rsets->data count] == 0` ("if you hit this assert, most likely you haven't deallocated all Metal resources before exiting") and aborts.

The original `mark_teardown()` flag only skipped `parakeet_free()` in the provider's `Drop` — it did NOT stop the C++ static destructor from running during `exit()`.

## What did NOT work (tried, reverted)
- `mark_teardown()` flag alone (pre-existing): Drop guard skipped, but the static destructor still ran at exit with buffers alive → crash.
- `exit(0)` inside `mark_teardown()` from `RunEvent::Exit`: correct idea, but the tray Quit path can reach `exit()` via `terminate:` BEFORE `RunEvent::Exit` fires (NSApplication calls exit() directly).
- `NSApplication setTerminate:`: not a real selector (NSInvalidArgumentException).
- `NSAppleEventManager setEventHandler:forEventClass:andEventID:inTarget:`: not a real selector either (the ObjC API is `setEventHandler:forEventClass:andEventID:inTarget:` only on the delegate proxy; the manager's real method differs).
- Intercepting the Quit AppleEvent from Rust without the AppKit event-manager bindings was a dead end.

## Fix (final, minimal)

`RunEvent::Exit` hook (`crates/teletype-desktop/src/lib.rs`) now:
1. `state.speech.unload(); state.parakeet.unload();` — frees the parakeet context (buffers → residency sets empty) while the Metal runtime is still fully alive.
2. `teletype_speech::parakeet::mark_teardown()` — sets the flag (Drop guard no-ops) **and calls `exit(0)`**, so C++ static destructors run immediately on the main thread while Metal is up; the residency-set assert holds because our buffers are already freed.

`crates/teletype-speech/src/parakeet.rs`: `mark_teardown()` gained the `exit(0)` (macOS only; no-op on other targets) with a doc comment explaining the static-destructor ordering.

## Verification

- `cargo test -p teletype-speech -p teletype-desktop --lib`: 79 passed.
- Live: fresh build, launched, Parakeet + S1-mini loaded (log shows both), sent `kAEQuitApplication` via a Swift helper (`NSAppleEventDescriptor` + `sendEvent`), process exited, `ls -t ~/Library/Logs/DiagnosticReports/teletype*` shows no report newer than the pre-fix one.
- SIGTERM quit also clean.

## Reusable lesson

For any Rust app embedding C++ libraries with **function-local statics that hold OS-resource state** (Metal/CUDA contexts, GPU buffers): a "skip the free on drop" flag is not enough — the static destructor runs at `exit()` regardless. Either (a) free your resources from a hook that runs before `exit()` AND force the static destructors early with `exit()` from that hook, or (b) patch the C++ side to not use a static. Test the tray/AppleEvent quit path specifically — it reaches `exit()` via `terminate:` and bypasses framework-level exit hooks.
