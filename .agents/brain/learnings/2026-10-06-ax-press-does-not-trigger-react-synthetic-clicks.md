---
id: L018
title: AX-press / synthetic CGEvent clicks do not reliably trigger React onClick in the Tauri webview
status: candidate
type: testing
area: ui (React/Tauri webview) / test harness
agent: Codex
tool: AXUIElementPerformAction + CGEvent
date: 2026-10-06
---

# L018: AX-press / synthetic clicks do not reliably trigger React `onClick` in the Tauri webview

## Problem

While battle-testing tabs, some button actions worked via the AX helpers
(`AXUIElementPerformAction(el, kAXPressAction)`) and some did not — with no
obvious pattern. Dictionary "Add Manually" → `Add` worked (word appeared), but
Dictionary row `Delete` never removed the word, and the Developer log showed
**no** `remove_dictionary_word` IPC call ever fired.

## Investigation

- Confirmed the delete code path is correct: `DictionaryScreen.tsx:632`
  `onClick={() => remove(w.id)}` → `invoke("remove_dictionary_word", { id })` →
  `refresh()`. The command is registered: `crates/teletype-desktop/src/lib.rs:584`
  and implemented at `commands.rs:3258`.
- Tried three ways to fire the delete button: (1) `AXUIElementPerformAction`
  on the AXButton, (2) `axpress4 "Delete word Sorathiya"` (matches the
  `aria-label`), (3) a raw `CGEvent` mouse-down/up at the button's screen
  coordinates. None removed the word; the Developer log stayed clean (no error,
  no IPC), proving the React handler never ran.
- Contrast: form submissions (Transforms `Create`, AutoText `Add snippet`,
  Dictionary `Add`) all worked through the same AX-press path.

## Root cause

React attaches `onClick` through its synthetic-event system on the root
container, delegating from native DOM events. `AXUIElementPerformAction` and
posted `CGEvent` mouse clicks do not always produce the native DOM `click`
event sequence (with the correct `isTrusted`/target) that React's delegation
requires, so the handler is skipped. Which buttons work is inconsistent and
not a reliable signal.

## Reusable lesson

- **A button that "does nothing" under AX-press is not proof of an app bug.**
  First check the Developer-screen log for the expected IPC call: if it's
  absent and there's no error, the click never reached React — it's a
  test-harness limitation, not a defect.
- To verify a destructive action end-to-end through the real UI, use the
  `Toggle DevTools` button (Developer screen) to open the webview inspector and
  drive the element from the console, or rely on the unit/integration tests
  plus a code-path review. Do not "fix" the app based on a failed synthetic
  click.
- The reliable AX helpers remain: `axpress N` (nav), `axpress4/5 "text"`
  (title/aria-label match), `axfocus2 "placeholder"` / `axfocus2 FIRST_TEXTAREA`
  (focus a field, then `keystroke` to fill React controlled inputs).

## Bonus finding (real bug found while verifying): `confirm()` was broken

While verifying the delete path via DevTools, the console showed
`Unhandled Promise Rejection: dialog.confirm not allowed. Command not found`.
Root cause: four screens use the global `confirm()` for destructive actions
(`TransformsScreen.tsx:47`, `AutoTextScreen.tsx:208`,
`PersonalizationScreen.tsx:236`, `ScratchpadScreen.tsx:42`), and Tauri routes
the global `confirm` through `@tauri-apps/plugin-dialog`. But the JS
`confirm()` calls the **`message`** command (`plugin:dialog|message` with
`buttons: 'OkCancel'`), **not** a `confirm` command. The capability only had
`dialog:allow-open`, so every confirm() was silently denied and the delete
never proceeded.

Fix: add **`dialog:allow-message`** (NOT `dialog:allow-confirm`, which is a
deprecated alias that does not cover the `message` command the JS actually
calls) to `crates/teletype-desktop/capabilities/default.json`. Verified: the
native NSAlert ("Test / QA confirm works?" with Cancel/OK) now appears.

**Reusable rule:** when a Tauri plugin command is denied with "Command not
found", read the plugin's `dist-js` to find the *exact* IPC command name the
JS wrapper invokes, then add the matching `allow-<command>` permission.
`confirm()` → `message`; don't guess from the JS function name.

## Evidence / sources

- Files: `ui/src/screens/DictionaryScreen.tsx:632`, `crates/teletype-desktop/src/lib.rs:584`, `crates/teletype-desktop/src/commands.rs:3258`, `crates/teletype-desktop/capabilities/default.json`, `ui/node_modules/@tauri-apps/plugin-dialog/dist-js/index.js`
- User confirmation: pending
