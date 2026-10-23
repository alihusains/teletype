---
id: L039
title: Pill meters looked dead during real dictation — dB-linear levels vs low macOS input gain, and a misleading synthetic preview
status: verified
type: debugging
area: ui/pill + teletype-core audio
agent: qoder
date: 2026-10-20
---

# Pill meters looked dead during real dictation

## Problem
User dictating loudly saw tiny waveform bars in every pill style, while the Settings
style previews showed big lively meters.

## Symptoms
- Live pill: bars barely rise above the floor line during speech.
- Settings preview: same components, big swings.

## Initial hypotheses
1. Frontend scaling bug introduced by the pill restore.
2. Backend `LevelMeter` emitting near-zero values.
3. Device gain too low for the meter's dB mapping.

## Investigation
- Traced the pipeline: `LevelMeter::push` → `level_from_amplitudes` (dB-linear,
  `FLOOR_DB=-58`, `NOISE_GATE=0.25`, RISE/FALL smoothing) → `pill-level` event →
  `pillMachine` reducer (raw passthrough) → linear `level * height` in every renderer.
- Ruled out (1): pill.tsx/pill.html are byte-identical to the pre-revamp commit.
- Measured (3): macOS default input `VolumeScalar` = 0.275 (≈ -11 dB), so normal
  speech RMS lands around 0.2..0.3 on the 0..1 display scale → bars at 20-30 % height.
- Compared the preview: `usePillPreviewLevels` synthesizes `v` up to ~1.0, so the
  preview over-represents what live audio can reach on this machine. The preview is
  not a calibration reference.

## Root cause
Two compounding issues, not one:
1. Perceptual mismatch (not a regression): the backend level is dB-linear and the
   renderers mapped it linearly to pixels; with a low system input gain, real
   speech occupies only the bottom third of the scale.
2. A refactor-era regression found while fixing (1): `WAVE_BARS` dropped from the
   first-release 34 to 15, and `LevelRailPill` guards with
   `hist.length === 24 ? hist[i] : 0` — with a 15-bar window the Level Rail
   rendered its floor stub for ALL bars (fully dead), and Dot Matrix only ever
   filled 15 of its 25 columns. Silent because no test drove the rail end-to-end.

## Fix
- `displayLevel = sqrt(clamp01(level))` applied once in the `pillMachine` `level`
  reducer case, so every consumer (Waveform, RainbowMeter, RainbowLips, DotGrid)
  gets the perceptual loudness law (half height at -6 dB) without touching the
  backend, VAD, or transcripts. Settings previews keep their own synthetic levels.
- `WAVE_BARS` restored to 34; the rail's padding guard changed to an index-safe
  `hist[i + max(0, hist.length - 24)] ?? 0`.
- Height bump across styles (user follow-up): waveform 22→30px, capsule rows
  44→52px, lips 20→26px, rail 24→28px, well meter 16→24px.

## Verification
- New unit tests in `ui/src/__tests__/pill-level-display.test.ts`; two existing
  raw-value assertions in `pill-state.test.ts` updated to go through the curve.
- `ui/src/test/pillLevelGeometry.test.tsx` renders each recording style through
  the real `pill-state`/`pill-level` event path and asserts minimum bar heights —
  this is the test that caught the dead Level Rail.
- `vitest run` 17 files green, `tsc --noEmit` clean, `vite build` OK; Settings
  previews visually confirmed taller in Chrome.

## Reusable lesson
When a level meter "feels dead" but the math is correct end-to-end, check the
input gain and the perceptual curve before suspecting the pipeline: dB-linear
levels always need a sqrt/log display law, and a synthetic preview that maxes
out at 1.0 is a misleading reference for live-signal amplitude.
