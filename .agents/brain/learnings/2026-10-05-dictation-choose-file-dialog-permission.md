# Dictation "Choose file" button dead — missing dialog capability permission

- ID: L016
- Date: 2026-10-05
- Agent: codex (Qwen3.8)
- Project area: Dictation tab / Tauri capabilities
- Status: verified
- Severity: user-reported broken feature (P3.17 file transcription)

## Problem
The "Choose file" button on the Dictation tab did nothing when clicked.

## Symptoms
- Clicking "Choose file" produced no visible effect and no console error
  (the `open().catch(console.error)` swallowed the IPC denial).

## Initial hypotheses
1. Wrong handler wired (no — `chooseFile` calls `open` from
   `@tauri-apps/plugin-dialog`).
2. Dialog plugin not registered (no — `tauri_plugin_dialog::init()` in
   `crates/teletype-desktop/src/lib.rs:241`).
3. Capability permission missing.

## Investigation
- Traced `chooseFile` → `open()` (dialog plugin) in
  `ui/src/screens/DictationScreen.tsx:84`.
- Checked `crates/teletype-desktop/capabilities/default.json`: it listed
  core/global-shortcut/opener permissions but NO `dialog:*` permission.
- Tauri v2 IPC denies any plugin command not granted in a capability, and the
  denial surfaces as a rejected promise — silently, because of `.catch()`.

## Root cause
`dialog:allow-open` was never added to the main-window capability, so the
dialog plugin's `open` command was denied at the IPC layer.

## Fix
Added `"dialog:allow-open"` to `crates/teletype-desktop/capabilities/default.json`.
No UI change needed.

## Verification
- Rebuilt (`cargo build -p teletype-desktop`), relaunched fresh.
- AX-pressed "Choose file" on the Dictation tab → native NSOpenPanel opened
  with the audio filter (screenshot evidence).

## Reusable lesson
In Tauri v2, registering a plugin in Rust is NOT enough — every plugin command
used from the webview must also be granted in `capabilities/*.json`. A denied
IPC call is just a rejected promise, so `.catch(console.error)` on UI handlers
hides the failure. When a UI button "does nothing", check the capability file
before the handler.
