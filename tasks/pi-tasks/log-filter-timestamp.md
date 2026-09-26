# Task: Log view - fix timestamp, add level filter, improve readability

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Files you may edit: `ui/src/screens/DeveloperScreen.tsx` only.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
The current log view (after the previous enhancement pass) has three problems:
1. **Timestamps are not visible** — the `lineTimesRef` captures `new Date().toLocaleTimeString()`
   but the timestamp span is rendered with `color: "#5c6470"` on a dark background, making it
   nearly invisible. The timestamp should be clearly readable.
2. **No filter** — the user wants to filter logs by level (Info, Success/Warn, Error).
3. **Overwhelming** — with badges, timestamps, duration chips, and colored text all at once,
   the view is busy. It needs to be calmer and easier to scan.

## Read first
1. `ui/src/screens/DeveloperScreen.tsx` — the whole file. Note:
   - Lines 6-10: `LogEntry` interface.
   - Lines 12-18: `LEVEL_COLORS`.
   - Lines 22-28: `LEVEL_BADGES`.
   - Lines 30-33: `durationColor()`.
   - Lines 59-100: component state, `activeModel`, `lineTimesRef`, `useTauriEvent`.
   - Lines 191-280: the log-line render and empty state.
2. `ui/src/screens/SettingsScreen.tsx` — for the app's visual style tokens.

## What to build

### 1. Fix the timestamp visibility
- Change the timestamp color from `#5c6470` to `#6b7280` (slightly lighter) and add
  `fontSize: 11` so it's smaller but readable. Keep it on the left, muted, tabular-nums.
- The timestamp should be the first element in each row, before the level badge.

### 2. Add a level filter row
- Add a row of filter chips below the header (between the header and the S1 box / log container).
- Four chips: **All**, **Info**, **Warn**, **Error**. (Success counts as Info for filtering.)
- Each chip is a small rounded button. The active chip has `background: var(--accent)`,
  `color: white`. Inactive chips have `background: var(--surface)`, `color: var(--text-secondary)`,
  `border: 1px solid var(--border)`.
- Add `const [filter, setFilter] = useState<"all" | "info" | "warn" | "error">("all")`.
- Filter logic:
  - `"all"`: show all lines.
  - `"info"`: show lines where `level === "Info"` or `level === "Success"`.
  - `"warn"`: show lines where `level === "Warn"`.
  - `"error"`: show lines where `level === "Error"`.
- Apply the filter in the `lines.map(...)` by computing `visibleLines = lines.filter(...)`
  before the map. Keep the original index for the `key` and `lineTimesRef` lookup.

### 3. Calm the visual design
- Remove the `background` from the level badge. Instead, just show the badge text in the
  level color, with `fontSize: 10`, `fontWeight: 600`, `letterSpacing: 0.5`. No pill
  background. This reduces visual noise.
- Keep the duration chip but make it more subtle: `fontSize: 11`, no background, just
  colored text with `fontVariantNumeric: "tabular-nums"`.
- Reduce the gap between elements from `gap: 10` to `gap: 8`.
- Keep `marginBottom: 2` on each line and `marginBottom: 6` after Error lines.

### 4. Do NOT change
- The S1 control box, the header row, the `clear` function, the polling `useEffect`,
  the `get_logs` call, the `useTauriEvent` for `settings-changed`, any Rust code.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype/ui && npx tsc --noEmit 2>&1 | tail -20`
   (must exit 0, no errors)
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff ui/src/screens/DeveloperScreen.tsx`

## Out of scope (do NOT touch)
- Any `.rs` file.
- `ui/src/pill.tsx`, `ui/src/App.tsx`, `SettingsScreen.tsx`.
- The `enviouswispr/` reference tree.
