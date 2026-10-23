---
id: L017
title: Premium UI revamp — reuse the design tokens in ui/index.html, don't invent new palettes
status: candidate
type: design
area: ui (React/Tauri webview)
agent: Codex
tool: apply_patch
date: 2026-10-06
---

# L017: Premium UI revamp — reuse the design tokens in `ui/index.html`, don't invent new palettes

## Problem

The goal asked to "revamp and enhance the UI/UX of Insights … give it a fresh, premium look … without affecting existing functionality." The risk in a revamp is either (a) breaking data/IPC/refresh behavior, or (b) introducing ad-hoc colors/shadows that clash with the rest of the app and break in dark mode.

## Investigation

- Read the full `ui/src/screens/InsightsScreen.tsx` (data model `Insights`, IPC calls `get_insights`/`get_usage_stats`/`get_profile`/`list_dictionary`, `useTauriEvent("dictation-state")` refresh) and `ui/src/components/FixesCard.tsx`.
- Found the real design system is defined in `ui/index.html` `:root` (not a `.css` file): elevation `--shadow-1/2/3`, `--text-tertiary`, `--border-subtle`, `--accent-soft`, type scale `--text-xs..lg`, `--tracking-tight`, motion `--ease`/`--dur`. Light + dark palettes both defined there.
- Revamped only presentation: `Card` (added `section` eyebrow label + icon-chip header + `boxShadow: var(--shadow-2)`), `StatCard` (icon chip, tabular-nums, shadow), `BarChart` (gradient bars), hero (multi-layer radial+linear `HERO_GRADIENT` + shadow + hairline border), refined segmented range control, gradient top-phrase bars. `FixesCard` given matching elevation + header.
- Left every `Insights` field, all four `invoke` calls, the `useTauriEvent` subscription, and every empty-state message untouched.

## Evidence

- `npx tsc --noEmit` clean; `npm run build` (vite) succeeded.
- Fresh app launch (vite on 1420 up first), navigated to Insights via AX-press. Verified live with real data (4,161 words / 294 dictations / 349 fixes): premium hero, elevated Fixes card, Activity band (bar chart + heatmap), Habits band (5 stat cards), Where-you-dictate donut, Top phrases, Your records, Milestones all render correctly with the new styling.

## Reusable lesson

- The design system lives in `ui/index.html` `:root`, not a separate stylesheet. Before any "premium" restyle, read those tokens and build on `--shadow-*`, `--text-tertiary`, `--border-subtle`, `--accent-soft`, `--tracking-tight`, `--ease`/`--dur`. Inventing hex colors/shadows breaks dark mode (defined in the same file) and looks off-brand.
- "Premium" reads as: elevation (shadow tokens), a richer multi-layer gradient hero, consistent icon-chip headers, tabular-nums for figures, tight tracking on large numbers, and subtle hairline separators — not more color.
- A safe revamp touches presentation only. Keep the data interface, IPC calls, event subscriptions, and empty-state copy identical, and verify with `tsc` + a fresh build + a live screenshot across every band.

## Evidence / sources

- Files: `ui/src/screens/InsightsScreen.tsx`, `ui/src/components/FixesCard.tsx`, `ui/index.html`
- User confirmation: pending (Ali reviewing)

