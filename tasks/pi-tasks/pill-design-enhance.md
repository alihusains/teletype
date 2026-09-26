# Task: Enhance the three non-Teletype pill styles (Classic, LevelRail, ReadingWell)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo.
- Files you may edit: `ui/src/pill.tsx` only.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why
The three ported pill styles (Classic Capsule, Level Rail, Reading Well) are functional but
visually flat. They need a design pass to feel intentional and polished, matching the quality
of the default Teletype pill. The goal is: calm, readable, audio-reactive, with clear hierarchy.

## Read first
1. `ui/src/pill.tsx` — the whole file. Focus on:
   - Lines 100-130: `RainbowLips` component (audio-reactive bars).
   - Lines 260-300: `RainbowHairline` component.
   - Lines 418-445: `ClassicPill` component.
   - Lines 447-476: `LevelRailPill` component.
   - Lines 479-513: `ReadingWellPill` component.
   - Lines 515-650: `Pill` component (the main wrapper that selects which style to render).
   - The `DARK_SURFACE`, `DARK_BORDER` constants near the top.
2. `ui/src/screens/SettingsScreen.tsx` lines 40-45: the `PILL_STYLES` array with labels and hints.

## Design direction
Each pill should feel like a small, self-contained instrument. The design language:
- **Dark, glassy surface** with a subtle border that catches light on the top edge.
- **Audio-reactive elements** that breathe with the voice level (not just static).
- **Clear hierarchy**: the timer is the primary element, the audio visualization is secondary.
- **No visual noise**: at most 2 accent colors per pill, no rainbow overload.
- **Smooth transitions**: CSS transitions on all dynamic properties (150ms ease-out).

## What to build

### ClassicPill (lines 418-445)
Current: 185x44 capsule with RainbowLips, timer, "Listening…", CancelButton, RainbowHairline.
Enhance:
- Add a subtle top-edge highlight: `boxShadow: "inset 0 1px 0 rgba(255,255,255,0.08)"`.
- Make the timer the visual anchor: `fontSize: 14`, `fontWeight: 600`, white.
- Dim "Listening…" to `color: "rgba(255,255,255,0.35)"`, `fontSize: 11`.
- The RainbowLips should be slightly smaller (height 20 instead of 24) to not compete with the timer.
- Add `transition: "opacity 150ms ease-out"` to the CancelButton so it fades in/out smoothly.
- Keep the RainbowHairline but reduce its opacity to 0.6.

### LevelRailPill (lines 447-476)
Current: 24-bar rainbow level meter with timer.
Enhance:
- Add the same top-edge highlight as ClassicPill.
- Make the bars slightly thinner (width 3 instead of 4) with `gap: 2` for a finer look.
- The timer should be below the bars, `fontSize: 12`, `color: "rgba(255,255,255,0.7)"`.
- Add a subtle gradient overlay on the bar container: `linear-gradient(180deg, rgba(255,255,255,0.05), transparent)`.
- The overall pill should be slightly taller (height 52 instead of 44) to fit the bars + timer.

### ReadingWellPill (lines 479-513)
Current: wider panel with header (timer + meter + badge) over a well.
Enhance:
- Add the same top-edge highlight.
- The header row: timer `fontSize: 13`, `fontWeight: 600`; the meter badge `fontSize: 10`,
  `padding: "2px 8px"`, `borderRadius: 10`, `background: "rgba(255,255,255,0.08)"`.
- The well area: add a subtle inner shadow `boxShadow: "inset 0 1px 3px rgba(0,0,0,0.3)"` to
  give it depth. The well background should be slightly darker than the pill surface:
  `background: "rgba(0,0,0,0.2)"`.
- Add a thin separator line between header and well: `borderTop: "1px solid rgba(255,255,255,0.06)"`.

### Shared
- Do NOT change the `Pill` wrapper component, the `RecordingStyleProps` interface, or the
  style-selection logic.
- Do NOT change the `RainbowLips` or `RainbowHairline` component internals (only their
  usage/props in the three pill components).
- All changes are CSS/style props only. No new state, no new imports, no new components.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype/ui && npx tsc --noEmit 2>&1 | tail -20`
   (must exit 0, no errors)
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff ui/src/pill.tsx`

## Out of scope (do NOT touch)
- The default Teletype pill (the `Pill` wrapper's default rendering).
- Any `.rs` file.
- `ui/src/screens/SettingsScreen.tsx`, `ui/src/App.tsx`.
- The `enviouswispr/` reference tree.
