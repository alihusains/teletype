# Task: Pill style must live-update when changed in Settings (UI-only)

## How to work this task
- Repo: `/Users/a.sorathiya/Documents/Ali/teletype` — work ONLY inside this repo. Do NOT touch
  the `enviouswispr/` reference tree or any sibling.
- Files you may edit: **`ui/src/pill.tsx` ONLY.** Do NOT edit any `.rs` file. A separate task
  is adding the Rust-side `settings-changed` emit; your job is only to make the pill listen.
- In your final report, paste the REAL output of every verification command below, not prose.

## Why (the real gap)
The floating pill is a separate Tauri webview (`ui/pill.html` -> `ui/src/pill.tsx`). It renders
its style from `settings.pillStyle` (pill.tsx:611, style switch at pill.tsx:641-643:
`classic` / `levelRail` / `well`). But it only fetches settings ONCE, on mount:

    // pill.tsx:556-558
    useEffect(() => { refreshSettings(); }, []);

So when the user changes "Pill style" in Settings, the already-open pill webview never learns
and keeps the old style. A companion task makes the Rust backend emit a `settings-changed`
event to the pill whenever settings are saved. This task makes the pill LISTEN for that event
and refresh.

Goal: when the `settings-changed` event fires, the open pill re-reads settings and re-renders
with the new `pillStyle`.

## Read first (before writing any code)
1. `ui/src/pill.tsx` — the `Settings` interface (~line 20-35), `refreshSettings` (532-542),
   the mount `useEffect` (556-558), and the existing `useTauriEvent` usages (566+, e.g.
   `useTauriEvent("pill-skip", ...)`, `useTauriEvent("pill-language", ...)`).
2. `ui/src/lib/useTauriEvent.ts` — the listener helper; match its exact usage pattern.

## What to build
Add ONE `useTauriEvent` call in `ui/src/pill.tsx`, placed next to the other `useTauriEvent`
calls (around line 566), that refreshes settings when the backend signals a change:

    useTauriEvent("settings-changed", () => {
      refreshSettings();
    });

Match the existing `useTauriEvent` generic/payload style (the other handlers take a payload
type; here the payload is irrelevant, so use `useTauriEvent<void>` or `useTauriEvent<unknown>`
whichever compiles cleanly given the helper's signature). Keep it to that one listener. Do not
refactor the style components or the render switch.

## Verification (run these, paste real output)
1. `cd /Users/a.sorathiya/Documents/Ali/teletype/ui && npx tsc --noEmit 2>&1 | tail -20`
   (must type-check, no errors).
2. `cd /Users/a.sorathiya/Documents/Ali/teletype && git diff ui/src/pill.tsx`

## Out of scope (do NOT touch)
- Any `.rs` file. Pill POSITION repositioning (separate task). The Developer tab, typing
  autotext, or any other settings. The `enviouswispr/` reference tree.
