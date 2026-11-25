# Speech engine lives in a per-kind manager slot — never assume `state.speech`

**ID:** L090
**Date:** 2026-11-11
**Agent:** pi (coding agent)
**Project area:** teletype-desktop (speech model routing)
**Status:** verified
**Evidence:** live CDP meeting test on Windows: first finalize run failed with
"speech model not loaded" while the user's model (sherpa-parakeet-v3-int8)
was loaded; after routing through the correct slot the same flow produced a
report. `cargo build -p teletype-desktop` green.

## Problem

The new meeting finalize thread transcribed the recorded WAV through
`state.speech` (the whisper manager slot). The user's selected engine was
**sherpa-parakeet**, which loads into the **`state.neural`** slot. Result:
finalize errored with `speech model not loaded: Model load failed: the
whisper.cpp backend is not built for this platform` — the whisper slot was
empty because whisper is not built on Windows.

## Symptoms

- Meeting start/stop/recording all worked; only finalization failed.
- The error named whisper.cpp, an engine the user never selected — the
  misleading part.
- The app's own dictation transcribed fine (it routes correctly), so the
  bug was invisible outside the new code path.

## Initial hypotheses

1. Sherpa model not loaded → false: dictation was using it fine.
2. Meeting pipeline bug (wrong samples) → false: WAV on disk was valid.
3. Wrong manager slot → **true**.

## Investigation

- `AppState` has four speech manager slots: `speech` (whisper), `parakeet`,
  `parakeet_cpp`, `neural` (everything else).
- The warm-up thread and `dictation.rs` resolve the slot via
  `dictation::resolve_speech_model(settings, models_dir) -> (path,
  SpeechEngineKind)`: `Whisper → speech`, `Parakeet → parakeet`,
  `ParakeetCpp → parakeet_cpp`, `Sherpa | Fluid | Qwen3Asr | AppleSpeech |
  TranscribeCpp | Whistle → neural`.
- The new code had bypassed that mapping by reaching for `state.speech`
  directly.

## Root cause

Assuming the selected engine lives in the "obvious" slot instead of
reusing the existing resolver. The slot mapping is non-obvious (four slots,
one catch-all) and is only documented in code.

## Fix

`AppState::active_speech_manager()` (in `lib.rs`) wraps
`resolve_speech_model` and returns the right manager; the meeting finalize
thread calls it. Same mapping the warm-up thread uses, in one place.

## Verification

- Live CDP run: `meeting_start` → 8 s record → `meeting_stop` →
  `finalizing` → `done` with `report.html` + `transcript.json` + 8.0 s WAV.
- No-speech room produced a "No speech was captured." report (correct).
- `transcribe_file` on clean `hello.wav` → 4 words (engine healthy).

## Reusable lesson

Any new code path that transcribes audio must obtain the provider through
`AppState::active_speech_manager()` (or `resolve_speech_model` + the slot
match) — never `state.speech` directly. When a feature "works" in dictation
but fails in a new consumer, suspect slot routing before the new code.
