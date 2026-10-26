---
id: L065
title: writes.run always resolves to a truthy object, so callers must branch on .ok
status: verified
type: convention
area: ui / writeGuard
agent/tool: Qoder (lead session)
date: 2026-10-25
---

# L065 — `writes.run(...)` never falsy-resolves: branch on `.ok`, not on the return

## Problem

While auditing hold-and-restore behaviour in the settings screens, the failure
branch of several writes was unreachable.

## Symptoms

A rejected save looked like a successful one: the optimistic value stayed on
screen and no error row appeared, even though the IPC call had thrown.

## Root cause

`useWrites().run(key, fn)` is declared
`(key, fn) => Promise<WriteResult>` (`ui/src/lib/writeGuard.ts:35`) and
`runWrite` catches instead of throwing, returning
`{ ok: false; error: string }` (`ui/src/lib/writeGuard.ts:18-25`). That is a
non-empty object, so it is always truthy. Any caller written as
`if (await writes.run(...))` takes the success branch on every failure. The
type makes the bug obvious; the API shape makes it tempting, because `run`
looks like it could resolve to nothing.

## Fix

Callers use the two-step form, which is what the hold-and-restore comment
assumes:

```tsx
const res = await writes.run("s1", () => invoke("set_s1_control", { ... }));
if (!res.ok) { setS1(before); refresh(); }
```

## Verification

`grep -rn "if (await writes.run" ui/src/` returns nothing; 20 `writes.run` call
sites in `ui/src/screens/*.tsx` all destructure and test `.ok`.

## Reusable lesson

A guard function designed never to throw has to signal failure through a field,
not through its own falsiness, and every consumer must read that field. When
adding a call site, copy the `const res = await writes.run(...); if (!res.ok)`
shape; a bare `if (await writes.run(...))` compiles, passes review by eye, and
is dead code for the error path.

Related: [[2026-10-25-use-resource-dep-reload-storm]] (the read half of the same
module).
