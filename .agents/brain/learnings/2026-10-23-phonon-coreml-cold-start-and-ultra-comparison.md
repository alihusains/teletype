---
id: L-20261023-phonon-coreml-cold-start-and-ultra-comparison
title: Phonon-2 Core ML pays ~60-90 s per encoder function on first call, and does not beat Parakeet Ultra on the Mac sets
status: verified
type: measurement
area: "evals / candidate ASR"
agent: "Qoder (Ali)"
tool: "scripts/eval/bench_candidate_asr.py, teletype-eval replay"
date: 2026-10-23
---

# Phonon-2 Core ML cold start and Ultra comparison

## Problem

The Phonon-2 candidate arrived with an upstream claim of "essentially Ultra's
accuracy" at ~1/4 the size. It needed a Mac measurement before it could be
considered for a catalog row, and its hypotheses run showed meaningless latency
(millisecond p50s next to a matching p99 in the tens of seconds).

## Symptoms

- `evals/runs/candidates/phonon2/*/hypotheses.jsonl` carried one or two cases
  per set at 57-88 s while the rest were 8-40 ms.
- `hypotheses.meta.json` RTF varied from 0.0047 (librispeech-clean) to 0.575
  (ami) for the same model, which cannot be throughput.
- The pre-existing `fleurs-en/report.json` disagreed with its own hypotheses
  file (max 373 ms vs 87.7 s) because the report predated the hypotheses.

## Initial hypotheses

1. The model has pathological decode loops on certain clips.
2. The host was contended while the hypotheses were produced.
3. Core ML is compiling or loading something once per input shape.

## Investigation

1. Re-scored all six sets from the current hypotheses with
   `teletype-eval run --engine none --model-label phonon2 --hypotheses ...`
   so every report matched the file it scored.
2. Summed per-case `asr_ms` and compared it with `hypotheses.meta.json`:
   the sums match `model_seconds` exactly, so the outliers are in the
   measurement, not the report.
3. Re-timed the slow FLEURS clips serially in a fresh process with Homebrew
   Python 3.11 + coremltools 9.0.
4. Repeated the same clip twice, and a different clip twice.
5. Read `phonon_coreml.py` and `manifest.json`: the encoder is a multifunction
   Core ML package with `enc_5s`, `enc_10s`, `enc_15s`, `enc_35s`; audio is
   bucketed into the smallest function that fits and the padded mel reaches
   that function's fixed length. `self.progs` caches the loaded function per
   process; the compiled package is cached on disk under
   `~/Library/Caches/phonon-coreml`.

## Evidence

- Re-timing the three slowest FLEURS clips serially: 60.6 s, 60.5 s, 65.8 s for
  clips of 7.3 s, 13.4 s and 16.2 s, while a 13.0 s clip in the same process
  took 58 ms.
- Repeating one clip: 64.8 s, then 32.6 ms. A new function in the same process:
  302 ms, then 22.6 ms.
- The slow cases in each original set are the first case that lands in a given
  bucket (5 / 10 / 15 / 35 s), not a property of the audio.
- One warm outlier remains (`cv-0006`, 75.1 s in a set whose first-call loads
  were all ~0.3 s), so host contention can add a stall even with warm functions.

## Root cause

The 57-88 s outliers are the per-process Core ML function load, paid once per
duration bucket (up to four), not model throughput and not a decode loop. Warm
calls are 8-45 ms per clip. The per-set RTF in `hypotheses.meta.json` therefore
measures cold start, not the model.

## Fix

No code change. The register now records warm latency separately from first-call
load, and the candidate's RTF is marked unusable. A future shipping integration
would need either an explicit warm-up at model load or a single 35 s function
(pad every window), before Phonon-2 could meet the dictation latency gate.

## Verification

Every report under `evals/runs/candidates/phonon2/` now scores the hypotheses
file beside it. Six-set accuracy (verbatim micro): LS clean 1.68, LS other 5.19,
cv 7.14, fleurs-en 4.77, ami 10.86, noise-0db 9.65. Parakeet Ultra on the same
sets: 1.61, 3.56, 5.89, 4.95, 10.46, 7.18. Close-talk mean 4.67 vs 3.69.

## Reusable lesson

For any Core ML multifunction ASR port, separate **first call per function** from
**warm call** before trusting a latency number. A per-clip harness that visits
each duration bucket once will charge a one-time compile/load to the first clip
of that bucket and report it as throughput. Time the same clip twice: if the
second call is three orders of magnitude faster, the outlier is the function
load. On this host the load is 60-90 s per function and is cached per process,
not across processes.

## Evidence / sources

- Reports: `evals/runs/candidates/phonon2/`
- Runner: `.tmp/asrbench/models/phonon-2-coreml/phonon_coreml.py`,
  `manifest.json` (`functions_s: [5, 10, 15, 35]`)
- Harness: `scripts/eval/bench_candidate_asr.py` (`make_phonon`)
- Scorer: `teletype-eval run --engine none --hypotheses`
- Ultra reports: `evals/runs/bakeoff/parakeet-ultra-coreml/<set>-off/report.json`
