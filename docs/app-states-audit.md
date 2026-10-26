# Teletype App States Audit

Scope: every screen, the pill, the readiness panel, and the download path.
Goal: find where the app is silent, stuck, or stale, so it feels responsive.
Evidence is cited as file and line. No file was modified while writing this.

Two conventions govern the findings:

1. Every claim names a real file and line.
2. "Silent" means the failure is visible only in the developer console, or the
   UI shows a success that did not happen.

## Status

One line per defect so this stays the single place to look. Updated as fixes
land; a defect is `fixed` only when a test proves the new behaviour.

| Defect | State | Where the proof is |
|---|---|---|
| A-1 deletes with no error | fixed | `ui/src/test/dictationStates.test.tsx` (A-1), `listScreenStates.test.tsx` (Styles, Dictionary), `settingsStates.test.tsx` |
| A-2 optimistic writes never rolled back | fixed | `listScreenStates.test.tsx` (auto-apply), `settingsStates.test.tsx` (keybind, pill position) |
| A-3 list loads fail silently | fixed | `homeStates.test.tsx`, `dictationStates.test.tsx`, `listScreenStates.test.tsx`, `settingsStates.test.tsx` |
| A-4 file drop has no queue | fixed | `dictationStates.test.tsx`, "queues three dropped files, processes one at a time" |
| A-5 model select has no warm state | fixed | `modelsOpenStates.test.tsx`, "counts up the elapsed time while a model is starting", "shows the slow-machine hint only for a model the host cannot run natively". Cancel stays out: the backend exposes no cancel for `select_model` |
| A-6 download done clears early | fixed | `modelsOpenStates.test.tsx`, "keeps a finishing state between a done download and the confirmed catalog"; `modelsScreen.test.tsx` |
| A-7 empty state before first load | fixed | `homeStates.test.tsx` (empty only after a successful empty read), `dictationStates.test.tsx` |
| A-8 runtime status lags 5 s | fixed | `runtimeStatusEvent.test.tsx` (7 tests: event path, poll restart, staleness label) |
| A-9 settings saves swallow failures | fixed | `settingsStates.test.tsx` (pill position), `listScreenStates.test.tsx` |
| A-10 copy reports success unconditionally | fixed | `dictationStates.test.tsx` (A-10) |
| A-11 no busy label or cancel on the drop zone | fixed | `dictationStates.test.tsx` (A-4 / A-11) |
| A-12 `get_settings` null fallback | fixed | `listScreenStates.test.tsx`, `settingsStates.test.tsx` |
| A-13 keybind save has no per-field pending | fixed | `settingsStates.test.tsx` (A-3b) |
| A-14 Ollama detection blocks the list | fixed | `modelsOpenStates.test.tsx`, "ends loading and names the failure when detect_ollama fails" |
| A-15 language override fires without feedback | fixed | `ui/src/test/personalizationLegible.test.tsx`, "rolls a failed per-app language override back" |

The shared primitives the fixes route through are new:
`ui/src/lib/writeGuard.ts` (`useResource`, `useWrites`, `runWrite`) and
`ui/src/components/StateRows.tsx` (`LoadFailed`, `WriteFailed`, `LoadingRow`).
A screen that invents its own error row is a defect, not a style choice.

## What the app already does well

These are recorded so the fix list does not re-flag them.

- `useRuntimeStatus` polls `get_runtime_status` every 5 s and pauses while the
  tab is hidden (`ui/src/lib/runtimeStatus.tsx:37`). It deliberately refuses to
  spin on a "downloaded but not loaded" model and renders `Checking what is
  loaded…` while null (`ui/src/lib/runtimeStatus.tsx:224`).
- The dictation pill drives itself from a state machine fed by real events
  (`ui/src/pillMachine.ts`) and its warm and recording timers are gated by
  `useNow` on the phase (`ui/src/pill.tsx:1292`), so it is event driven rather
  than guessed.
- Downloads survive screen unmount because progress is held in a shared store
  keyed by model id (`ui/src/lib/downloadStore.ts`), and the backend emits
  `model-download-progress` on a byte basis
  (`crates/teletype-desktop/src/commands/models.rs:1232`).
- Errors from Tauri are decoded into a human sentence, not `[object Object]`,
  by `errorMessage` (`ui/src/lib/errors.ts:37`).

## Defects

Each defect is numbered A-<n> in descending order of severity.

### A-1 Destructive deletes show no error and no pending state

The delete buttons on Dictation, Transforms, AutoText, Dictionary, and Styles
all fire a `delete_*` command and swallow the rejection with
`.catch(console.error)`. If the backend rejects (locked file, missing row),
the row stays on screen and the user sees nothing.

- `ui/src/screens/DictationScreen.tsx:139` deletes an entry, `catch(console.error)`, then `refresh()` re-reads the list. A failed delete quietly keeps the row.
- `ui/src/screens/TransformsScreen.tsx:384` asks `confirmDialog` first, which is good, but `delete_transform` on line 385 has no `.catch` and no error state, so a rejection is an unhandled promise rejection, not a visible message.
- `ui/src/screens/AutoTextScreen.tsx:438` guards with a confirm, but the delete call itself has no error path.
- `ui/src/screens/StylesScreen.tsx:246` `delete_style_profile(...).catch(console.error)`.
- `ui/src/screens/DictionaryScreen.tsx:307` `delete_user_pack(...).catch(console.error)`.

There is no confirm on the Dictation, Styles, or AutoText row deletes at all.
A single misclick on the trash icon removes the entry with no second thought.

Fix: route every delete through the shared confirm dialog, await the command,
and surface `errorMessage(e)` on failure. Keep the row visible when the delete
rejects.

### A-2 Optimistic writes are reverted, but several are not

Some screens optimistically flip a value and roll it back on failure. That is
the right shape. The gap is the screens that flip a value and never roll it
back.

- `ui/src/screens/KeybindsScreen.tsx:233` (`updateTransformShortcut`) sets the
  shortcut in state, then on reject restores `t.shortcut` on line 247. Correct.
- `ui/src/screens/TransformsScreen.tsx:318` saves `autoApplyTransform` and
  only swallows errors with `.catch(console.error)`. The toggle visually flips
  even when the save failed, so the UI claims the transform is auto-applied
  when it is not.
- `ui/src/screens/ScratchpadScreen.tsx:30` toggles `scratchpadEnabled` and
  swallows the save with `.catch(console.error)`. The enabled switch stays on
  in the UI after a failed save.
- `ui/src/screens/AutoTextScreen.tsx:319` and the `get_settings` reloads on
  line 271, 284 use `.catch(() => null)`, so a failed `get_settings` silently
  yields `null` and the toggles render from the last good value with no
  indication the read failed.

Fix: every optimistic toggle must hold the previous value and restore it on
reject, or refresh from the backend and only flip if the backend confirmed.

### A-3 List loads fail silently and read as "empty"

A load that rejects is indistinguishable from a list that is genuinely empty.
The empty state says "No dictations yet" when the truth may be "the read
failed".

- `ui/src/screens/DictationScreen.tsx:64` `list_dictation_history().catch(console.error)`. If this rejects, `entries` stays `[]` and the empty state at line 231 (`No dictations yet. Focus any app…`) is shown. The user is told to start dictating when the real issue is a failed read.
- `ui/src/screens/HomeScreen.tsx:86` loads history with `.catch(() => {})` and `get_insights` on line 87 with `.catch(() => {})`. A failure here blanks the recent dictations and the insights card with no signal.
- `ui/src/screens/AutoTextScreen.tsx:301` loads `list_autotext` and `list_system_autotext` with `.catch(console.error)`, then renders an empty list.
- `ui/src/screens/TransformsScreen.tsx:255` `list_transforms().catch(console.error)`; an empty render on failure.

Fix: track a `loadError` flag per list. Render an error row with a retry
button instead of the empty state. Distinguish "loaded and empty" from "failed
to load".

### A-4 File transcription drop has no queue and re-enters mid-flight

The Dictation drop zone accepts one file at a time and gives no feedback while
a second file is dropped or a new selection is made during an in-flight
transcription.

- `ui/src/screens/DictationScreen.tsx:103` the `onDragDropEvent` handler calls
  `transcribeFile(p)` for every path in the drop event. Dropping three files
  calls the command three times; each overwrites the `transcribe` state, so
  the user sees the last one only. There is no queue and no "N files pending".
- `ui/src/screens/DictationScreen.tsx:202` disables the "Choose file" button
  while `transcribe.status === "transcribing"`, but the drag path on line 103
  is not gated by that status, so a drag can still start a new transcription
  while one is running.
- There is no `transcription-progress` event wired to this screen. The
  backend `transcribe_file` command (`crates/teletype-desktop/src/commands/models.rs:1941`) does not emit incremental progress, so a long file shows a static "Transcribing <name>…" with no percentage and no elapsed time.

Fix: queue dropped files and transcribe them one at a time with a visible
counter. Either surface a progress event from the backend or show an elapsed
timer with `useNow` so the user knows it is still working.

### A-5 Model selection is slow and the UI has no warm state

Selecting a local LLM model is one of the genuinely slow operations in the app,
and the UI gives a single spinner with no sense of how long it will take.

- `crates/teletype-desktop/src/commands/models.rs:152` `select_model` stops the
  previous engine (line 184, a blocking `spawn_blocking` wait), then warms up
  the new llama-server (line 200). The comment at line 180 notes the old
  server is about 5 GB resident. This is a multi-second to multi-tens-of-
  seconds operation on CPU.
- `ui/src/screens/ModelsScreen.tsx:396` (`selectLocal`) sets `selectingId` and
  renders a single spinner on the chosen row. There is no elapsed timer, no
  "this can take a minute on CPU" note, and no cancel. The user can only guess
  whether it is progressing or hung.

Fix: show an elapsed timer with `useNow` during selection, add a "can take a
while on this machine" hint when `runsOnHost` is false, and consider a cancel
path that matches the backend stop.

### A-6 Download "done" clears the store before the catalog re-reads

When a download finishes, the UI clears the download store and re-reads the
catalog. Between those two, the row shows neither the progress bar nor the
downloaded state, so a fast refresh can briefly render a stale "Download"
button for a model that just landed.

- `ui/src/screens/ModelsScreen.tsx:387` on success calls `clearDownload(id)`
  then `refresh()`. The progress bar unmounts the instant `clearDownload`
  runs, before `list_models` returns the new `downloaded: true`. For a few
  frames the row is neither downloading nor downloaded.
- `ui/src/components/DownloadProgress.tsx:72` clears local state on
  `status === "done"` or `cancelled`, so the bar disappears from the event,
  independent of the catalog read. Two sources clear the bar at slightly
  different times.

Fix: clear the download store only after the catalog re-read has applied the
new `downloaded` flag, or keep a "just finished" state on the row until the
refresh lands.

### A-7 The empty state is shown before the first load settles

Several screens render the empty state on the first paint, before the initial
`invoke` has resolved, so a non-empty list flashes "empty" for one frame.

- `ui/src/screens/DictationScreen.tsx:231` renders the empty block whenever
  `entries.length === 0`. `entries` is initialised to `[]` (line 53), so the
  empty state paints on mount before `list_dictation_history` returns.
- `ui/src/screens/TransformsScreen.tsx` and `AutoTextScreen.tsx` follow the
  same shape (state initialises empty, list loads in an effect).

Fix: add a `loading` flag that is true until the first load resolves. Show a
skeleton or nothing while loading; show the empty state only when loaded and
empty.

### A-8 Runtime status lags by up to 5 seconds and can read "ready" after a failure

The readiness panel is the app's main "is it working" surface. It polls every
5 seconds, so a model that dies or loads is reflected up to 5 s late.

- `ui/src/lib/runtimeStatus.tsx:37` `POLL_MS = 5000`. The comment on line 34
  says the fast state (dictation phase) arrives as an event, which is true for
  the pill, but the readiness panel itself has no event path. A model load
  that finishes on the next poll is still shown as "loading" for up to 5 s.
- On a failed `get_runtime_status`, the hook keeps the previous value (line
  63, the empty `catch`). A transient IPC failure therefore freezes the panel
  at the last good reading with no "stale" marker.

Fix: emit a `runtime-status` event from the backend on state change and use it
to invalidate the poll immediately. Add a "last checked" timestamp so a stale
panel is legible.

### A-9 Settings saves swallow failures across the board

Settings is the largest consumer of `save_settings` and nearly every save is
`.catch(console.error)`. A write that fails (disk full, corrupt settings file)
leaves the UI showing the value the user set.

- `ui/src/screens/TransformsScreen.tsx:282`, 318, 400, 447, 477, 496 all save
  settings with `.catch(console.error)`.
- `ui/src/screens/SettingsScreen.tsx` repeats the same pattern throughout the
  hotkey, permissions, and AI polish sections.
- `ui/src/screens/ModelsScreen.tsx:287` `saveOpenaiSettings` awaits
  `get_settings` with no catch, so a failed read throws out of the handler.

Fix: centralise settings writes in one helper that reports failure with
`errorMessage`, and surfaces a toast or inline error rather than the console.

### A-10 Copy actions report success unconditionally

Copy buttons flip to a "Copied" check even when the clipboard write failed.

- `ui/src/screens/DictationScreen.tsx:133` `navigator.clipboard.writeText(text).catch(() => {})` then sets `copiedId` unconditionally. On a clipboard permission denial the user sees a green check that did not happen.
- `ui/src/screens/ScratchpadScreen.tsx:37` copies the scratchpad with
  `.catch(() => {})` and reports success.

Fix: only set the "copied" flag in the `.then`; on reject show a brief
"Copy failed" instead of the check.

### A-11 The drop zone and choose button have no distinct busy label

While a transcription is in flight the only signal is the line that changes to
"Transcribing <name>…". There is no progress bar, no cancel, and no elapsed
time, so a long file is indistinguishable from a hang.

- `ui/src/screens/DictationScreen.tsx:193` is the single busy label.
- The "Choose file" button is hidden during transcription (line 202) but there
  is no way to stop the in-flight job.

Fix: add an elapsed timer and a cancel button to the transcription row.

### A-12 AutoText and Dictionary reload from `get_settings` with null fallback

These screens read settings with `.catch(() => null)` and then branch on the
result. A failed read silently falls back to `null`, and the subsequent code
treats that as "feature off" rather than "read failed".

- `ui/src/screens/AutoTextScreen.tsx:271`, 284 `get_settings().catch(() => null)`.
- `ui/src/screens/DictionaryScreen.tsx:124` same pattern.

Fix: distinguish "settings unavailable" from "feature disabled". Show an error
and a retry instead of rendering as off.

### A-13 Keybind save has no per-field pending state

Saving a hotkey is awaited but the recorder does not show which field is being
written, and a slow save can be re-triggered by a second keystroke.

- `ui/src/screens/KeybindsScreen.tsx:252` `saveBinding` awaits the write, but
  the hotkey recorder row has no `saving` flag, so a fast user can start
  editing the same field again before the first write returns.

Fix: disable the recorder for a field while its write is in flight and show a
saving state on that row.

### A-14 Ollama detection blocks the model list refresh

On the Models screen, Ollama detection and `list_ollama_models` run in the same
mount effect. They are non-blocking, but `setOllamaChecking(false)` only runs
when the detect promise settles, so if `detect_ollama` is slow the "checking"
spinner persists and the user cannot tell whether the screen is done loading.

- `ui/src/screens/ModelsScreen.tsx:226` the mount effect starts
  `detect_ollama` and the settings read together. `refresh()` is also called on
  line 227. There is no overall "models loaded" flag tying them together.

Fix: track a single `modelsReady` flag set only when both the catalog refresh
and the Ollama detection have resolved.

### A-15 Personalization language overrides fire without error feedback

Changing a per-app language override is a fire and forget write with no pending
or error state.

- `ui/src/screens/PersonalizationScreen.tsx:179` and 210 call
  `set_app_language_override(...).catch(console.error)`. A failed override
  leaves the picker showing the new language with no signal it did not stick.

Fix: hold the previous language, restore it on reject, and surface the error.

## Where the app is actually slow

Ranked by user-perceived cost, not by code complexity.

1. Local LLM model selection. `select_model`
   (`crates/teletype-desktop/src/commands/models.rs:152`) tears down the
   previous ~5 GB engine and warms the new one synchronously. This is the
   longest single operation in the app and has no elapsed timer or cancel in
   the UI (see A-5).
2. Model downloads. Multi-gigabyte, verified by SHA-256
   (`crates/teletype-desktop/src/commands/models.rs:1232`). Well instrumented
   with a byte-level progress bar, but the "done" race (A-6) and the lack of a
   resume affordance outside the LLM path (`canPause` is LLM-only,
   `ui/src/components/DownloadProgress.tsx:83`) leave gaps.
3. File transcription. No progress event from the backend
   (`crates/teletype-desktop/src/commands/models.rs:1941`), so a long file is
   a static label (A-4, A-11).
4. Readiness panel polling. Up to 5 s of staleness with no event invalidation
   (A-8).
5. Settings writes. Each `save_settings` is a round trip to disk; the failures
   are invisible (A-9).

## Priority call list

Ordered by cost per unit of fix. The first five are cheap and high signal.

Cheap, do first:

1. A-1 Add confirm and error surfacing to every destructive delete. Reuse the
   existing `confirmDialog` and `errorMessage` helpers. No backend change.
2. A-10 Only mark copy as "copied" in the `.then` path. Two lines per screen.
3. A-7 Add a `loading` flag before rendering empty states. Prevents the
   empty-flash on first paint in Dictation, Transforms, AutoText.
4. A-3 Track a `loadError` per list and render an error-plus-retry row instead
   of a false empty state.
5. A-2 Make the remaining optimistic toggles (autoApply, scratchpad, language
   override) revert on reject, matching the pattern already used in
   `updateTransformShortcut`.

Medium effort, high feel:

6. A-4/A-11 Queue dropped files, add an elapsed timer and cancel to the
   transcription row.
7. A-8 Emit a `runtime-status` event on backend state change and add a "last
   checked" marker to the readiness panel.
8. A-6 Clear the download store only after the catalog re-read applies
   `downloaded: true`.
9. A-9 Centralise `save_settings` in one helper that surfaces failures.

Larger effort, do when time allows:

10. A-5 Add elapsed timer, slow-machine hint, and cancel to local model
    selection.
11. A-13 Per-field saving state in the hotkey recorder.
12. A-12 Distinguish "settings read failed" from "feature off" in AutoText and
    Dictionary.
13. A-14 A single `modelsReady` flag on the Models screen.

Verification notes: the two silent-failure classes (A-1, A-2, A-3, A-9) are all
`.catch(console.error)` or `.catch(() => null)` today, so they can be found and
counted mechanically. Every fix keeps the failure on screen instead of in the
console, and keeps the previous value on screen when a write rejects.
