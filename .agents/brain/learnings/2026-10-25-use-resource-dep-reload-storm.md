---
id: L062
title: A useResource container in a useCallback dep is a reload storm, and the IPC mock cannot see it
status: verified
type: debugging
area: "ui / lib/writeGuard / screens/StylesScreen"
agent: "qoder"
tool: "CDP console capture over the ?demo preview, vitest"
date: 2026-10-25
---

# A useResource container in a useCallback dep is a reload storm, and the IPC mock cannot see it

## Problem

The Style tab issued **29,902 `get_settings` calls in 12 seconds** while Home was
the visible screen, and React logged `Maximum update depth exceeded` 167 times in
the same window. Four unit tests covering that exact screen passed throughout.

## Symptoms

- Boot console: repeated `Maximum update depth exceeded`.
- Real cost only visible by counting IPC: `get_settings`, `get_profile`,
  `list_style_profiles`, `get_app_style_overrides` and `style_resolution` each at
  ~30k/12s, in lockstep.
- The app stayed interactive and rendered correctly, so nothing looked broken.

## Initial hypotheses

1. The `useResource` return object was a fresh literal each render, poisoning any
   dep array. **Partly right, and already fixed by memoising it, which is why this
   bug survived that fix.**
2. A runaway `requestAnimationFrame` in the pill's `DotGridMatrix` (it does call
   `requestAnimationFrame` from inside a state updater). Rejected: the pill is a
   separate window entry and is not mounted by `App.tsx`.
3. `useTauriEvent` re-subscribing. Rejected: deps are `[event]`, a string.

## Investigation steps

1. Reproduced in a real headless Chrome over CDP rather than in jsdom, and
   installed a `console.error` hook via `Page.addScriptToEvaluateOnNewDocument`
   **before** the document existed, so boot-time warnings were not missed.
2. Deduped the captured stacks on the deepest `src/` frame, not the first frames:
   react-dom frames are identical for every occurrence, so grouping on them
   collapsed 167 events into one line and named nothing.
3. The surviving frame was `writeGuard.ts:70`, which is `setData(d)` inside
   `useResource`'s loader resolution. That said "something reloads forever" but
   not who.
4. Wrapped the shim's `__TAURI_INTERNALS__.invoke` in a counter. This is the step
   that found it: five commands at 30k each is one call site, and those five are
   exactly `StylesScreen.refresh()`.

## Evidence

`ui/src/screens/StylesScreen.tsx` before:

```tsx
const refresh = useCallback(() => {
  profileList.reload();
  ...
}, [profileList, refreshResolution]);   // <-- the bug

useEffect(refresh, [refresh]);
```

`profileList` is the memoised resource object, and memoising it on `data` is
correct: its identity changes whenever a response arrives. So `refresh` changed
on every reload, the mount effect re-fired, and called `reload()` again.

Why tests missed it: `ui/src/test/tauriMock.ts` `mockInvokeResponses` returns
`invokeResponses.get(cmd)`, the **same reference every call**. `setData(sameRef)`
bails out, `data` never changes, `profileList` identity never changes, the loop
never starts. Real IPC deserialises a fresh object on every call.

Proved by adding a guard that returns a fresh object per call: against the old
dependency it reports `list_style_profiles was called 384 times by a single
mount`; against the fix it passes.

## Root cause

Depending on a hook's *container* object rather than the *stable callbacks* it
exposes. The container is intentionally unstable, because that is how consumers
learn the data changed.

## Fix

Depend on the stable `reload` callback:

```tsx
const reloadProfiles = profileList.reload;
const refresh = useCallback(() => { reloadProfiles(); ... },
  [reloadProfiles, refreshResolution]);
```

Audited the other four `useEffect(refresh, [refresh])` sites
(`InsightsScreen`, `DictionaryScreen`, `PermissionsUpdatesLicenseSections`,
`KeybindsScreen`); all memoise on `[]` or primitives and are clean.

## Verification

- Boot IPC `get_settings`: 29,902 -> 20 per 12s. `Maximum update depth exceeded`:
  167 -> 0. Full 12-screen sweep at 1440 and 390 clean.
- `npx tsc --noEmit` exit 0; `stylesModelCapability` 5/5 including the new
  storm guard; the guard was confirmed to fail against the pre-fix dependency.

## Reusable lesson

- Never put a `useResource` result in a `useCallback`/`useEffect` dep array. Take
  `reload` or `patch` off it first; those are stable.
- A mocked IPC layer that returns a constant reference **cannot** detect a reload
  loop. Any test meant to prove "reads itself once" must return a fresh object per
  call, mirroring deserialisation.
- When a page renders fine but feels heavy, count IPC calls per command. A
  correct-looking screen with one command in the tens of thousands is a loop, and
  no amount of screenshot review will show it.
