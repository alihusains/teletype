# Sherpa-onnx `durations` are token LENGTHS, not end times

**ID:** L091
**Date:** 2026-11-11
**Agent:** pi (coding agent)
**Project area:** teletype-speech (word timings), teletype-core (segmentation)
**Status:** verified
**Evidence:** live meeting transcript `225406d9` showed
`{'Hello,': start 480, end 160}` — end before start for every word; after
the fix, live meetings produce sane intervals (`[400..1040]`,
`[5680..11120]`) and combined segments.

## Problem

Meeting reports showed **one word per line**. The report's segmenter splits
on gaps > 1 s between words; with every word's `end_ms` smaller than its
`start_ms`, every gap computed to > 1 s and every word became its own
segment.

## Symptoms

- Report rows: one word each, no combining.
- `transcript.json`: `endMs < startMs` on nearly every word.
- Dictation was unaffected text-wise (its final text doesn't use timings),
  so the corruption was invisible outside word-timing consumers.

## Initial hypotheses

1. Meeting segmenter threshold too aggressive → false (1 s is reasonable).
2. Word grouping (BPE boundary logic) wrong → false (words grouped fine).
3. The engine's end times are garbage → **true**, and the cause was our
   call site, not the engine.

## Root cause

`sherpa.rs::transcribe_detailed` passed `result.durations` straight into
`group_token_timings(tokens, start_seconds, end_seconds)`. sherpa-onnx's
`durations` is the **length** of each token in seconds; the token's end is
`timestamp + duration`. So every word's `end_ms` was actually its last
token's *duration* (e.g. 160 ms) — smaller than the word's start.

## Fix

`sherpa.rs`: compute `ends[i] = ts[i] + durs[i]` and pass that.
`group_token_timings` (lib.rs): defensive clamp `end_ms = end_ms.max(start_ms)`
so a future misbehaving engine can never emit an inverted interval.

## Verification

- Live CDP meeting on Windows: report rows now combine
  (`0:05 | Speaker 1 | I'm going to see that.`).
- `cargo test -p teletype-speech --lib` green for the timing tests (two
  pre-existing Windows platform failures in parakeet_cpp/transcribe.cpp are
  unrelated).

## Reusable lesson

When an engine exposes `timestamps` + `durations`, never assume the second
array is end times — check the semantics in the engine docs, and validate
that `end >= start` on a real recording, not just synthetic unit inputs.
Inverted intervals are silent until a gap-based consumer (segmenter,
rescorer, player) trips over them.
