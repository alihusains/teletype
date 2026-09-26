# Task: Enhance the Developer tab log view

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  the `enviouswispr/` reference tree or any sibling.
- Files you may edit: `ui/src/screens/DeveloperScreen.tsx` only.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
The current log view is a flat monospace list with no visual hierarchy. Every line looks the
same weight; the level color is the only differentiator. Duration numbers float right with no
context. There is no timestamp, no visual grouping, and no way to scan for errors quickly.

## Read first
1. `ui/src/screens/DeveloperScreen.tsx` — the whole file. Note:
   - Lines 6-10: `LogEntry` interface (`level`, `message`, `durationMs`).
   - Lines 12-18: `LEVEL_COLORS` palette.
   - Lines 137-191: the log container div and the `lines.map(...)` render.
   - Lines 58-74: the polling `useEffect` that calls `get_logs`.
2. `crates/teletype-desktop/src/lib.rs` lines 100-165: `LogEntry` struct, `LogLevel` enum,
   `LOG_BUFFER`, `log_entry`, `log_entry_ms`. Note: `LogLevel` is `Info | Success | Warn | Error`.
   There is NO timestamp field in `LogEntry`. Do NOT add one to the Rust struct (out of scope).
3. `ui/src/screens/SettingsScreen.tsx` — for the app's visual style: it uses CSS custom properties
   like `var(--surface)`, `var(--border)`, `var(--text-secondary)`, `var(--accent)`. Match these.

## What to build
Enhance the log view in `DeveloperScreen.tsx` with these specific improvements:

1. **Level badges.** Instead of just coloring the text, add a small pill/badge before each line
   showing the level name (INFO, OK, WARN, ERR) in a compact uppercase font. The badge should be
   colored with the level color on a subtle background (e.g. `background: color + '18'` for ~10%
   opacity). This makes errors scannable at a glance.

2. **Timestamp.** Add a `new Date().toLocaleTimeString()` timestamp to each log line, rendered
   in a muted color on the left. Since the `LogEntry` has no timestamp field, capture the time
   in the UI when the line first appears (use a `useRef<Map<number, string>>` keyed by array
   index, populated on first render of each index). This is a display-only enhancement; it does
   not require a Rust change.

3. **Duration chip.** When `durationMs` is present, render it as a right-aligned chip with a
   subtle background (`background: var(--surface)`, `borderRadius: 4`, `padding: 1px 6px`)
   instead of plain text. Color the number: green (<500ms), yellow (500-2000ms), red (>2000ms).

4. **Line spacing and grouping.** Add `marginBottom: 2` to each line and a slightly larger gap
   (4px) after Error-level lines to visually separate them. Use `lineHeight: 1.5`.

5. **Empty state.** Keep the "No log lines yet." text but center it vertically in the container.

6. **Scroll behavior.** Keep the existing follow-on-scroll logic. Do not change the polling
   interval or the `get_logs` call.

7. **Do NOT change:** the S1 control box (a separate task handles that), the header row, the
   `clear` function, the `useEffect` polling logic, or any Rust code.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype/ui && npx tsc --noEmit 2>&1 | tail -20`
   (must exit 0, no errors)
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff ui/src/screens/DeveloperScreen.tsx`

## Out of scope (do NOT touch)
- Any `.rs` file.
- `ui/src/App.tsx`, `ui/src/pill.tsx`, `SettingsScreen.tsx`.
- The S1 control box visibility (a separate task handles that).
- The `enviouswispr/` reference tree.
