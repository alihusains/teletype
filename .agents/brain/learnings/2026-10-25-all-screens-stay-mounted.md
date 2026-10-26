---
id: L063
title: App.tsx keeps every screen mounted, so hidden screens keep doing work
status: verified
type: architecture
area: "ui/App.tsx screen mounting"
agent: "qoder"
tool: "CDP IPC counter over the ?demo preview"
date: 2026-10-25
---

# App.tsx keeps every screen mounted, so hidden screens keep doing work

## Fact

`ui/src/App.tsx` renders **all** screens on every render and hides the inactive
ones with `style={{ display: screen === "x" ? "block" : "none" }}` (App.tsx:574-612).
Mounting is not tied to visibility, so every `useEffect`, poll and subscription in
every screen runs for the whole life of the window regardless of which tab the
user is on.

## Evidence

Measured on a 12-second boot sitting on Home, with the Style loop already fixed:

| command | calls / 12s | why |
|---|---|---|
| `plugin:event\|listen` | 39 | every screen subscribes; StrictMode doubles it |
| `get_logs` | 25 | a poll that belongs to a hidden screen |
| `get_runtime_status` | 20 | `useRuntimeStatus` polls at 5s per mounted consumer |
| `get_settings` | 20 | several screens read settings on mount |
| `plugin:event\|unlisten` | 12 | listeners torn down and rebuilt, i.e. churn |
| `stop_hotkey_capture` | 10 | the Keybinds recorders are live while Keybinds is hidden |

This is also how L062 became severe instead of merely embarrassing: a runaway loop
on a tab the user had never opened cost the whole app.

## Existing precedent

`SettingsScreen` already takes the right shape: App.tsx:605 passes
`active={screen === "settings"}` and `useRuntimeStatus(active)` short-circuits its
effect when false. The pattern exists; most screens just do not use it.

## Options, and what was NOT done

1. **Conditional mount** (`{screen === "style" && <StylesScreen />}`) - cheapest
   and most thorough, but discards in-screen state on every tab switch. The
   always-mounted design is plausibly deliberate, to avoid re-fetching on each
   switch, so this was not changed unilaterally.
2. **Propagate `active`** to the polling screens, as SettingsScreen does. Keeps
   state, stops background work. This is the recommended next step.
3. Leave it. Rejected: it is the mechanism behind the worst performance defect
   found in this pass.

## Reusable lesson

Before treating "the app feels heavy" as a model or backend problem, count IPC
calls per command with the active screen unchanged. Anything that ticks up while
nothing is happening is background work from a hidden screen, and the fix is to
gate it on visibility rather than to optimise the call.

When adding a screen with a poll or a subscription, take an `active` prop and
short-circuit the effect, following `SettingsScreen`/`useRuntimeStatus(active)`.
