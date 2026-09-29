# BUG-012 — Personalization screen is unreachable (no nav entry)

**Severity:** P1 (shipped feature is dead code; the off-switch for BUG-001 is not navigable)
**Area:** ui / App.tsx
**Status:** open
**Found:** 2026-09-28 QA audit (5-agent sweep), verified by reading `App.tsx:34-45` and `:225`

---

## Description

`NAV_BASE` (`ui/src/App.tsx:34-45`) has **no** `personalization` entry, so
nothing navigates to the Personalization screen.

The screen **is** rendered (`App.tsx:225`:
`{screen === "personalization" && <PersonalizationScreen />}`) and the `Screen`
union includes `"personalization"` (`App.tsx:29`), but there is no nav item to
reach it.

The whole screen — explicit preferences, learned preferences, and **all three
learning toggles** (`learnFromEdits` etc.) — is shipped and wired but
unreachable.

### Why it matters

This is the off-switch the user needs to undo a wrong correction recorded by
BUG-001. A learned preference that's wrong can be viewed and removed — but the
user cannot get to that screen.

## Files

| File | Lines | Role |
|------|-------|------|
| `ui/src/App.tsx` | 34-45 | `NAV_BASE` — no personalization entry |
| `ui/src/App.tsx` | 29 | `Screen` union includes `"personalization"` |
| `ui/src/App.tsx` | 225 | the screen is rendered but unreachable |

## Reproduction

1. Open the app.
2. Look through the sidebar nav.
3. There is no "Personalization" item.
4. The screen exists in code but no UI path reaches it.

## Unit test cases (must pass after fix)

```ts
// 1. Every Screen variant must have a NAV entry (or be explicitly dev-gated).
it("every Screen variant is reachable from NAV_BASE", () => {
  const screens: Screen[] = ["home", "transcription", "autotext", "personalization", "developer"];
  const navIds = NAV_BASE.map(n => n.id);
  for (const s of screens) {
    if (s === "developer") continue; // explicitly dev-gated
    expect(navIds).toContain(s);
  }
});

// 2. PersonalizationScreen must be reachable from NAV_BASE.
it("Personalization is in NAV_BASE", () => {
  expect(NAV_BASE.some(n => n.id === "personalization")).toBe(true);
});

// 3. Clicking the nav item routes to the personalization screen.
it("clicking Personalization nav shows PersonalizationScreen", () => {
  render(<App />);
  fireEvent.click(screen.getByText("Personalization"));
  expect(screen.queryByTestId("personalization-screen")).not.toBeNull();
});
```

## Acceptance criteria

- [ ] A "Personalization" nav item exists in the sidebar.
- [ ] Clicking it shows the Personalization screen.
- [ ] The three learning toggles (`learnFromEdits` etc.) are reachable.
- [ ] A wrong learned correction (from BUG-001) can be viewed and removed by
      the user.
- [ ] `npm test` (UI) passes.

## How to test (manual / smoke)

1. **Smoke (1 min):**
   - Open the app, look at the sidebar.
   - **Pass:** a "Personalization" item is present.
   - Click it. **Pass:** the Personalization screen renders with the learning
     toggles visible.
   - **Fail:** no nav item, or clicking it shows a blank screen.

2. **Regression (1 min):**
   - If a Developer tab exists, confirm it does not duplicate the
     Personalization entry.
   - **Pass:** exactly one nav path to Personalization.

## Fix direction

Add a nav entry to `NAV_BASE` in `App.tsx`:

```ts
{ id: "personalization", label: "Personalization", icon: "..." }
```

Pick an appropriate icon from the existing `IconName` set (e.g. a sliders or
user-preference icon). The screen, union member, and render branch already
exist — only the nav entry is missing.

## Related

- BUG-001 — the wrong-corrections bug this screen's off-switch is meant to
  mitigate
