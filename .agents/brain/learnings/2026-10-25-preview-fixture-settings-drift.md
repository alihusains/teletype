---
id: L064
title: The dev-preview fixture and the TS Settings interface both drifted from the real get_settings contract
status: verified
type: debugging
area: ui / dev preview / settings contract
agent/tool: Qoder (lead session)
date: 2026-10-25
---

# L064 — The dev-preview fixture and the `Settings` interface both drifted from the real `get_settings` contract

## Problem

A boot-time `console.error` survived every other fix in this session, and the
only way to see it was `ui/src/dev/browserPreview.ts`, the dev-only IPC shim
that lets the Vite URL render in a normal browser. The question was whether the
app genuinely returns those fields and the fixture lied, or the other way round.

## Symptoms

- React warned that a controlled input was going uncontrolled, at boot, with no
  screen selected. The stack bottomed out in a `src/` frame that was a *reader*
  of the setting, not a writer of the fixture.
- The warning appeared only in the `?demo` browser preview, never inside the
  Tauri webview, and never in the jsdom suite.

## Initial hypotheses

1. A screen initialised state from a field that the preview fixture omitted.
2. The preview fixture carried fields the backend never sends, and some reader
   trusted them.
3. Both fixture and interface were missing the field, so nothing could catch
   the drift at all.

## Investigation steps

1. Reproduced in the preview with the pre-document `console.error` hook
   (`.qwen/tmp/stack.mjs`) so the warning was captured before React mounted.
2. Traced the warned field to its reader, then to the real backend:
   `crates/teletype-desktop/src/commands.rs:537` (`scratchpad_enabled`) and
   `:549-550` (`openai_base_url`, `openai_model`). So hypothesis 1 was correct:
   the backend does send them, the fixture did not.
3. Tried to type the fixture as `Settings` to make the omission a compile error.
   That failed with `TS2353: 'scratchpadEnabled' does not exist in type
   'Settings'`, which revealed hypothesis 3: the TypeScript interface at
   `ui/src/screens/SettingsScreen.tsx` was *itself* missing all three fields.
   The fixture could not be validated because the contract it was supposed to
   match was already incomplete.
4. Added the three fields to `Settings`. That immediately produced three more
   `TS2739`/`TS2741` errors in other people's test fixtures
   (`ui/src/__tests__/settings-fields.test.ts` near :97 and :120,
   `ui/src/__tests__/settings-interface-test.test.ts` near :67), which each
   built a full `Settings` literal and were silently missing the same fields.

## Evidence

- Before: 1 controlled-input `console.error` at boot in the preview; `Settings`
  interface had no `scratchpadEnabled` / `openaiBaseUrl` / `openaiModel`.
- After: `BASE_SETTINGS: Settings` typecheck passes, `npx tsc --noEmit` exit 0,
  and boot console is 0 flagged errors.
- No full `Settings` object literal exists in production code, only
  `Partial<Settings>` and `invoke<Settings>`, so widening the interface could
  not break a writer.

## Root cause

Two independent copies of the same contract (the real IPC response and the
preview fixture) with a third copy (the TS interface) that was *less* complete
than either. Because the fixture was typed `Record<string, unknown>`, an
omitted key was invisible until runtime, and because the interface omitted the
key too, no compile-time check could have caught it. A screen doing
`setX(s.field)` then assigned `undefined` over a defined initial value, which is
exactly what React reports as a controlled input going uncontrolled.

## Fix

- `ui/src/dev/browserPreview.ts`: `const BASE_SETTINGS: Settings = { ... }`,
  type-only import so the shim adds no runtime edge, plus the three missing keys.
- `ui/src/screens/SettingsScreen.tsx`: the three fields added to `Settings` with
  the backend line numbers in the comment.
- The three test fixtures that build a complete `Settings` updated to match.

## Verification

- `cd ui && npx tsc --noEmit` exit 0.
- `npx vitest run` on the touched files: 20/20 passed.
- Preview boot console re-measured with `.qwen/tmp/stack.mjs`: 0 errors.

## Reusable lesson

A dev-preview IPC fixture typed as a loose map is a second source of truth that
nothing enforces. Type it against the same interface the real response uses, and
make the interface exhaustive enough to be worth typing against, so drift
becomes a `tsc` error instead of a runtime warning in a webview nobody opens.
When the typed-fixture fix does not compile, the missing property is the finding
*about the interface*, not an obstacle to the fix.

Related: [[2026-10-25-use-resource-dep-reload-storm]] (same boot trace),
[[2026-10-25-insights-learning-was-never-populated]] (the preview fixture
hiding a backend gap, the same class of lie in the opposite direction).
