---
id: L-20261022-parakeet-cpp-quit-crash-shutdown-backend
title: Unloading a parakeet.cpp model is not enough to quit clean - the engine owns a process-global GPU backend
status: verified
type: debugging
area: lifecycle / macOS / Metal / speech engines
agent: Qoder CLI lead session
tool: Qoder CLI
date: 2026-10-22
---

# Unloading a parakeet.cpp model is not enough to quit clean

## Problem

Item 1 of Ali's request was "Fluid Speech Mini will not load" (`parakeet.cpp
engine unavailable: parakeet.cpp engine not built on this machine`). Fixing the
catalog/engine mapping made the model load and transcribe. That exposed a second,
user-visible defect: quitting the app from the tray crashed.

## Symptoms

- A real crash report, `~/Library/Logs/DiagnosticReports/teletype-2026-10-22-144615.ips`.
  The faulting thread ends `application_will_terminate` -> `mark_teardown` ->
  `exit` -> `__cxa_finalize` -> `ggml_metal_rsets_free`, SIGABRT.
- The same abort reproduced deterministically without the app:
  `cargo test -p teletype-speech --test real_parakeet_cpp -- --ignored` exited 134
  *after* printing `test result: ok. 1 passed`, and the pre-existing
  `examples/parakeet_cpp_proof.rs` exited 134 after printing a correct transcript.
- Only ever happened when a parakeet.cpp model had actually been loaded in the
  process. A whisper.cpp-only session quit fine.

## Initial hypotheses

1. The GGUF model itself leaks Metal resources, so the assert is a model bug.
   Disproved: the assert is about *residency sets*, a device-level object, not
   about model buffers.
2. L015's fix (unload every provider before `mark_teardown()`) was incomplete and
   needed extending to the third provider. Partly true but not the mechanism -
   `state.parakeet_cpp.unload()` was already called and the abort persisted.
3. Set `GGML_METAL_NO_RESIDENCY=1` to sidestep residency sets entirely. This does
   silence the abort, and is measured below, but it disables a performance feature
   to hide a lifetime bug. Rejected as the fix; kept as a documented fallback.

## Investigation steps

1. Reproduced the abort in two independent non-GUI harnesses (ignored test,
   example binary), which removed "maybe it is the app" from the question.
2. Read the vendored ggml source. `third_party/ggml/src/ggml-metal/ggml-metal-device.m:618`
   asserts `GGML_ASSERT([rsets->data count] == 0)` inside `ggml_metal_rsets_free`,
   and `ggml_metal_device_free` (line 870) frees rsets *first*. So a device whose
   residency sets still hold buffers aborts during static destruction.
   `GGML_METAL_NO_RESIDENCY` is read at line 781.
3. Read the vendored engine source for who owns that device.
   `parakeet.cpp/src/ggml_graph.cpp:27` declares `g_backend`, a **process-global**
   `pk::Backend`. `unload()` deletes the provider's context
   (`parakeet.cpp/src/parakeet_capi.cpp`, `parakeet_capi_free` = `delete ctx`) but
   never touches `g_backend`. Line 89 of `ggml_graph.cpp` is `pk::shutdown_backend()`,
   and the engine's own comment says it must be called before the process exits.
4. Confirmed the symbol is exportable from the built dylib, then wired it.
5. Added a temporary `PROBE:` eprintln to see whether the dlsym resolution worked.
   It printed `PROBE: capi present, shutdown_backend=false` - the hook resolved to
   null even though the name was copied from `nm`.

## Evidence

- `nm -gU libteletype_parakeet_cpp.dylib` prints the table form `__ZN2pk16shutdown_backendEv`
  (two underscores). Asking `dlsym` for that string returns null. A standalone C
  probe (`/tmp/dlsym_probe.c`) showed `_ZN2pk16shutdown_backendEv` (one underscore)
  resolves. Mach-O adds a single `_` prefix and the leading `_` of the Itanium
  mangling is already part of the mangled name, so `nm`'s output is one underscore
  too many for `dlsym`.
- Before the rename: exit 134 with the `PROBE:` line reporting `false`.
  After the rename: `REAL_EXIT=0`, and `test result: ok. 1 passed`.
- The proof example also moved from exit 134 to exit 0.
- `cargo test -p teletype-speech` (non-ignored): 61 passed + 1 passed, 0 failed.
- `cargo build -p teletype-desktop`: 0.
- Residency timing, 58 s of audio through Fluid Speech Mini: with residency sets
  (the shipped configuration after the fix) 1.12-1.19 s per take; with
  `GGML_METAL_NO_RESIDENCY=1` 1.04-1.05 s. Disabling residency was both safe and
  marginally faster on this workload, which is why it is recorded as a fallback
  rather than dismissed.
- Separate finding while testing the quit path: `kill -TERM` to the app exits with
  status 0 and does **not** run the `RunEvent::Exit` cleanup. `pgrep -fl llama-server`
  still listed pid 30085 afterwards and
  `~/Library/Application Support/com.teletype.app/s1-mini.llama-server.pid` still
  contained 30085. The tray-Quit path (AppleEvent `kAEQuitApplication` ->
  `application_will_terminate`) is the only path that reaches the hook.

## Root cause

Two independent defects, both on the exit path:

1. `pk::Backend` is a process global that the C API's per-context `free` never
   releases. Its Metal device survives every provider `unload()`, its residency
   sets stay non-empty, and the C++ static destructor that finally frees the
   device asserts. The engine prescribes exactly one remedy: call
   `pk::shutdown_backend()` after the last provider is unloaded and before the
   process starts tearing down.
2. The llama-server reaper sat *after* `mark_teardown()` in the `RunEvent::Exit`
   hook. `mark_teardown()` calls `std::process::exit(0)`, so it never returns and
   everything below it was dead code on macOS. Every quit orphaned the local
   model server. (L015 recorded the teardown ordering; it did not record that the
   marker call is terminal.)

And one mechanism that nearly sent the fix down the wrong path:

3. On macOS, a C++ mangled symbol is looked up with **one** leading underscore.
   Copying the name out of `nm` gives two and resolves to null, which makes a
   correctly written teardown call look like it does nothing.

## Fix

- `crates/teletype-speech/src/parakeet_cpp.rs`: new
  `pub type FnShutdownBackend = unsafe extern "C" fn()`, a `shutdown_backend`
  field on `CApi`, resolution via `sym("_ZN2pk16shutdown_backendEv")` inside
  `load_capi`, and `pub fn shutdown_backend()` (macOS-gated, no-op elsewhere) that
  calls it when present.
- `crates/teletype-desktop/src/lib.rs`: in `RunEvent::Exit`, order is now
  persist shutdown -> `speech.unload()` -> `parakeet.unload()` ->
  `parakeet_cpp.unload()` -> `parakeet_cpp::shutdown_backend()` -> inference
  `provider.shutdown()` + clear the slot -> `mark_teardown()` **last**. Comments
  state that nothing may follow the teardown marker because it never returns.
- `crates/teletype-speech/tests/real_parakeet_cpp.rs`: rewritten as a real
  conformance run against the downloaded GGUF (hello.wav -> non-empty text,
  silence.wav -> `SpeechError::NoSpeech`, unload, then `shutdown_backend()`), so
  the exit path itself is under test. The earlier version transcribed one second
  of synthetic zeros and asserted Ok, which contradicted the conformance contract
  and failed for a reason unrelated to the bug.
- `crates/teletype-speech/examples/parakeet_cpp_proof.rs`: unload + shutdown after
  printing the transcript, for the same reason.

## Verification

- `cargo test -p teletype-speech --test real_parakeet_cpp -- --ignored` ->
  `test result: ok. 1 passed` and `REAL_EXIT=0`.
- `cargo test -p teletype-speech` -> 61 + 1 passed, 0 failed.
- `cargo build -p teletype-desktop` -> exit 0.
- The example binary exits 0.
- Dev app boots, auto-loads the parakeet.cpp backend and s1-mini.
- **Not verified by me:** the live tray-Quit click. `tauri-pilot` is not installed
  or wired into this app (no binary, no socket, no plugin entry), and AppleScript
  `System Events ... quit` returned success without terminating the accessory app.
  The crash-report stack and the two deterministic harnesses are the evidence; a
  human quit-click on the built app is still worth one confirmation.
- **Not fixed:** the SIGTERM orphan. Delegated as
  `tasks/pi-tasks/t7-sigterm-orphan-llama-server.md`.

## Reusable lesson

- L015's rule "unload every provider before teardown so the residency sets are
  empty" is **engine-specific**. It holds for whisper.cpp, whose device is the
  static. It does not hold for parakeet.cpp, where the device lives in a
  process-global backend object that the C API never frees. Every new speech
  engine must be asked, at exit time: "what global state do you own, and who
  releases it?" - and the answer must be a call in `RunEvent::Exit`, not an
  assumption.
- A teardown hook that "does nothing" is more often a symbol-resolution failure
  than a wrong theory. Resolve the name in a five-line C probe before changing the
  design.
- Anything placed after a function that calls `std::process::exit()` is dead code.
  When reordering an exit hook, treat the terminal call as a boundary and put the
  rest above it.
- `kill -TERM` on the app is not a quit. Only the AppleEvent/tray path runs
  `RunEvent::Exit`, so process-manager kills, logouts, and `pkill` all skip
  cleanup.
