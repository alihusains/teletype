---
id: L056
title: AutoText export needed dialog allow-save, the fourth missing-capability dead control
status: verified
type: debugging
area: "AutoText / Tauri capabilities"
agent: "qoder"
tool: "cargo build, vitest, chrome-devtools screenshot"
date: 2026-10-24
---

# AutoText export needed dialog allow-save, the fourth missing-capability dead control

## Problem

AutoText had no export/import while the dictionary did. Adding the buttons
exposed a second problem: the capability file granted `dialog:allow-open` and
`dialog:allow-message` but not `dialog:allow-save`, so the new Export button
(and the dictionary's existing Export button, which nobody had exercised on a
packaged build) would be denied by Tauri IPC.

## Symptoms

- `save()` from `@tauri-apps/plugin-dialog` rejects with a permission error;
  the UI catches it and shows an error, or with an older catch pattern shows
  nothing at all.
- `confirmDialog()` was already covered: plugin-dialog's `confirm()` is a
  wrapper over the `message` command (L018), which is why `allow-message` was
  enough and `allow-confirm` is not needed.

## Investigation

1. Read both JS implementation and capability list:
   `node_modules/@tauri-apps/plugin-dialog/dist-js/index.js` shows `confirm()`
   calling `messageCommand()` -> `plugin:dialog|message`, while `save()` calls
   `plugin:dialog|save`.
2. Checked `crates/teletype-desktop/capabilities/default.json`: `allow-open`
   and `allow-message` present, `allow-save` absent.
3. Searched the brain: L016 (missing `dialog:allow-open`), L018 (missing
   `dialog:allow-message`). This is the same class for the third dialog verb.

## Evidence

- Capability file before the fix listed exactly two `dialog:*` permissions.
- `cargo build -p teletype-desktop` accepts the added `dialog:allow-save`
  identifier (an unknown permission fails the Tauri build).
- A browser-preview screenshot of the Snippets screen shows Export, Import and
  Add new rendering as one header row; the interactive save/open behaviour is
  covered by the interaction tests, since the preview has no dialog backend.

## Root cause

In Tauri v2, registering the dialog plugin does not grant its commands. Each
verb needs its own capability, and nothing in the build or test suite exercises
the permission layer, so a missing verb stays invisible until a user clicks it.

## Fix

- `capabilities/default.json`: added `"dialog:allow-save"`.
- AutoText gained a real export/import path:
  - core: `AUTOTEXT_EXPORT_VERSION = 1`, `AutoTextExport {version, entries}`,
    `AutoTextImportCounts {imported, updated, skipped}`, `export_to_json`,
    `export_to_file`, `parse_export`, `merge_entries`, `import_from_file`.
  - merge semantics mirror the dictionary: parse the whole file first (a bad
    file never mutates the store), skip system rows, invalid triggers/snippets
    and empty replacements, dedupe case-insensitively, and on a trigger
    conflict the file's content wins while the stored `id` and `created_at`
    stay so editors and usage counters keep pointing at the same row.
  - IPC: `export_autotext` returns the number of entries written so the UI
    reports the file's content rather than possibly stale screen state;
    `import_autotext` returns the counts.
  - UI: Export/Import in the Snippets header with a `role="status"` result line
    and a `role="alert"` failure line; import reloads the list.

## Verification

- `cargo test -p teletype-core --lib autotext::tests` 11 passed (5 new
  import/export tests: roundtrip, merge identity, skip rules, id collision,
  bad envelope leaves the store untouched).
- `cargo test -p teletype-desktop --lib` 325 passed.
- `npx tsc --noEmit` clean; full UI suite 193 passed including 4 new
  interaction tests (export, cancel, import counts + reload, rejected import).
- `cargo build -p teletype-desktop` succeeds with the new permission.

## Reusable lesson

Three dialog verbs are separately gated: `open`, `save`, `message`. `confirm()`
rides on `message`, not on `confirm`. Before shipping any file-picker or
confirmation control, grep `capabilities/default.json` for the verb and add it;
a permission denial is a rejected promise that a `.catch(console.error)` makes
look like a dead button.

## Evidence / sources

- `crates/teletype-desktop/capabilities/default.json`
- `crates/teletype-core/src/autotext/mod.rs`
- `crates/teletype-desktop/src/commands/autotext.rs`
- `ui/src/screens/AutoTextScreen.tsx`
- `ui/src/test/autotextImportExport.test.tsx`
- L016, L018 (same failure class)
