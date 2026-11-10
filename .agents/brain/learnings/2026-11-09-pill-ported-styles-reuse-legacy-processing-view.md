# L091: Pill ported styles (well/classic/levelRail) reuse the legacy processing view during busy phases

**ID:** L091
**Date:** 2026-11-09
**Agent:** pi (lead)
**Project area:** ui / pill / pillMachine
**Status:** verified
**Evidence:** `usesPortedStyle` definition in `ui/src/pill.tsx`; agentic-cue test `ui/src/test/agenticCue.test.tsx` (default style shows the cue during "Transforming…")

## Problem

When asked to "show that polishing is actively happening" in the pill, the obvious
place to add the cue was the "well" style's recording view (`ReadingWellPill`),
because that is what the user sees while recording. That would have been wrong:
the "well" style never shows its recording view during the processing phases.

## Root cause

`usesPortedStyle` (the gate that selects a ported style's own chrome) is:

```ts
const usesPortedStyle =
  glow || dotGrid || (phase === "recording" && pillStyle !== "default");
```

So for `pillStyle` = "well" / "classic" / "levelRail", the ported chrome is used
**only while `phase === "recording"`**. The moment the take ends and the phase
becomes `processing` / `transcribing` / `streaming` / `done` / `error`,
`usesPortedStyle` flips to `false` and the pill falls back to the **legacy**
default chrome — which renders `LegacyProcessingBody` when busy.

Consequences:
- Adding a processing view to `LegacyProcessingBody` (and `DotGridProcessingPill`)
  covers **every** style, because all of them route through the legacy processing
  view during busy phases. No per-style processing view is needed.
- The phase that means "polish/transform is running" is `streaming`, derived from
  the backend message by `refineBusyPhase`: `/^transform/i` → `streaming`,
  `/^transcrib/i` → `transcribing`. Gate the agentic cue on `phase === "streaming"`.

## Tool quirk (JS)

`String.prototype.trim()` **removes U+FEFF** (zero-width no-break space, code 65279),
even though U+FEFF is a Format (Cf) character, not a spec WhiteSpace. Verified with
`"same words﻿".trim() === "same words"` → `true`. This is why the old
`polishDiffShowable` (`raw.trim() !== polished.trim()`) dropped a diff whose only
difference was a trailing U+FEFF. When relaxing that gate to "always show the card",
the whitespace-only / U+FEFF cases now render instead of being dropped, so the
"dropped card → `dismiss_pill_result`" safety path is only exercised by an empty
`raw` or `polished`.

## Reusable lesson

- To change what the pill shows while a take is being processed, edit
  `LegacyProcessingBody` (and `DotGridProcessingPill`), not the ported styles'
  recording views. The ported recording views are recording-phase-only.
- "Transforming…" (backend label) == `streaming` phase == the polish/transform
  stage. "Transcribing…" == `transcribing` phase.
- Don't assume `trim()` follows the spec's WhiteSpace list; test the exact
  characters (U+FEFF, U+200B, etc.) before relying on it to normalize diffs.
