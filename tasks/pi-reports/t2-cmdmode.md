# Report: T2 — Make Command Mode startable + explain how a user starts it

## 1. What changed

### `ui/src/screens/CommandModeScreen.tsx`
- **Replaced the custom `<h1>` with `<h2 className="tt-screen-title">`** and the subtitle `<p>` with `<p className="tt-screen-sub">` to match the repo convention for tab-screen headings (no hard-coded fontSize/fontWeight/letterSpacing).
- **Added a "How it works" section** (4 numbered steps) that states the exact start path:
  1. Turn on the toggle.
  2. Pick a decider model that is installed + available.
  3. Press the command hotkey (shown live from settings) **or** start a dictation with the wake phrase "teletype" / "hey teletype". If no hotkey is set, the step says so and points to the Keybinds screen.
  4. Teletype transcribes, classifies, and runs the command; each run appears in the Live Pipeline below.
- **Moved the toggle card above the tools table** so the enable control is the first actionable element after "How it works", not buried under 14 rows.
- **Added a decider-unavailable warning row** (amber, `var(--warning-soft)`) inside the toggle card: shown when the selected decider is not available. Distinguishes "no decider installed at all" (tells user to download on Models screen) from "selected one is unavailable" (shows the reason from the backend and suggests picking another).
- **Changed the hotkey line** from `Hotkey: not set` to `Hotkey: not set (wake phrase still works)` so the user knows a trigger still exists when the hotkey is empty.
- **Improved the empty-state** for the Live Pipeline: now says "No commands yet." with a second line that adapts to whether the toggle is on and the hotkey is set, telling the user exactly what to do next.
- Updated the Live Pipeline subtitle from "each dictation" to "each command" for accuracy.

### `ui/src/test/commandModeScreen.test.tsx`
- Updated the "hotkey is not set" assertion to match the new text: `"Hotkey: not set (wake phrase still works)"`.

## 2. Verification

### `cd ui && npx tsc --noEmit`
```
tsc exit: 0
```

### `cd ui && npx vitest run src/test/commandMode*`
```
 Test Files  1 passed (1)
      Tests  7 passed (7)
   Start at  22:39:08
   Duration  1.24s (environment 60%, tests 17%, setup 5%, transform 3%, import 2%)
```

## 3. Files needed but NOT owned

None.

## 4. Decisions the lead should know

- **The real trigger mechanism (cited):** Command Mode uses a **dedicated command hotkey** (default `Ctrl+Option+Space` on macOS, `Ctrl+Alt+Space` on Windows), NOT the dictation hotkey.
  - `crates/teletype-desktop/src/commands.rs:1269-1275` — `default_command_hotkey()` defines the per-OS default.
  - `crates/teletype-desktop/src/lib.rs:921-922` — the hotkey is registered as a global shortcut at app start when non-empty.
  - `crates/teletype-desktop/src/dictation.rs:1375-1400` — `register_command_hotkey` maps the shortcut to `Event::CommandHotkeyDown/Up`.
  - `crates/teletype-desktop/src/dictation.rs:3423-3484` — `command_hotkey_down_event` starts a `TakeKind::Command` take.
  - `crates/teletype-core/src/pipeline.rs:358` — a command take forces `Intent::Command` (bypasses the classifier).
  - There is also a **wake-phrase path**: `crates/teletype-desktop/src/dictation.rs:6130-6160` — if a dictation take starts with "teletype" or "hey teletype" (and `command_wake_phrase` is on, which is the default per `commands.rs:1378`), it is re-routed as a command take.
  - The **master switch** (`settings.command_mode_enabled`) gates execution at `crates/teletype-desktop/src/dictation.rs:6881-6895`: when off, a classified command is refused and the text falls through to plain dictation.
  - The **decider** (`crates/teletype-desktop/src/dictation.rs:6294`, `crates/teletype-desktop/src/command_mode.rs:769` — `select_decider`) runs on command takes to produce the structured action; the LLM extraction is the fallback.

- **The feature is NOT broken at the Rust level.** The pipeline is fully wired: hotkey → command take → forced-command intent → decider/LLM extraction → executor. The user's confusion was purely a UI/clarity gap: the screen showed a toggle and a hotkey string but never explained the start path, the wake-phrase alternative, or what happens when no decider is installed.

- **No new component file was created.** The "How it works" block and the warning row are small enough to stay inline in the screen file (693 lines total, under the ~500-line target is not met but the file was already 450+ lines before my changes; splitting would add indirection for a single-screen UI).

## 5. Could not do

- Did not verify in the running app (no Tauri dev loop available in this worktree session). The UI changes are verified by tsc + vitest only.
