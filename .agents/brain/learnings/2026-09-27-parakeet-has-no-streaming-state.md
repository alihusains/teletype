---
id: 2026-09-27-parakeet-has-no-streaming-state
title: Sliding-window re-decode cannot produce a stable transcript with Parakeet TDT
date: 2026-09-27
agent: OpenCode (Space Bunny Free)
area: speech / streaming
status: verified
evidence: crates/teletype-speech/examples/stream_spike.rs; tasks/pi-tasks/streaming-asr-spike-findings.md
---

# Sliding-window re-decode cannot produce a stable transcript with Parakeet TDT

## Problem

The roadmap treated "live words assembling in the pill" as a port of the
reference project's design: re-decode a sliding 11 s window every 1 s, keep a
committed prefix, show the tail as provisional. Before building it, we measured
whether a re-decode is affordable and whether it produces stable text.

## Symptoms

None reported, because the design was never built. The question was whether it
*could* be built.

## Initial hypotheses

1. Re-decode is too expensive, so the cadence must be long.
2. Re-decode is affordable but the text churns near the window's leading edge, so
   a commit boundary a few seconds back would be enough.
3. The binding is missing a streaming entry point that exists in whisper.cpp.

## Investigation

1. Read the FFI in `crates/teletype-speech/src/parakeet.rs`. The module exposes
   `parakeet_full` (whole buffer) plus callbacks. There is no online or partial
   decode. Hypothesis 3 was wrong: it is not a binding gap.
2. Wrote `crates/teletype-speech/examples/stream_spike.rs` to measure both cost
   and stability against a full decode of the same audio.
3. Generated a 22.03 s clip with `say` at 16 kHz mono, natural speaking rate of
   3.68 words/s, and ran the spike on an M4 Pro.

## Evidence

Cost is not the problem. A warm re-decode of an 11 s window is 188 to 532 ms p50,
and a sustained 1 s cadence over an 11 s window costs 0.164x of one core
(0.638x at a 0.5 s cadence, where back-to-back decodes contend on the Metal
queue). Model load is 1.8 to 8.4 s, which is the real cold-start cost.

Stability is the problem. Comparing each window hypothesis to the authoritative
full decode, counting the trailing words the full decode confirms:

| t | words | agreed tail |
|---|---|---|
| 11s | 44 | 0 |
| 12s | 44 | 0 |
| 13s | 42 | 0 |
| ... | | 0 |
| 21s | 40 | 0 |
| 22s | 38 | 38 |

Every intermediate hypothesis agrees with the final decode on **zero** trailing
words. Only the last one, where the window happens to cover the whole utterance,
agrees completely. The provisional tail is the entire window: p50 11.4 s, p90
13.3 s, max 14.1 s.

A metric bug was suspected first, because a "shared tail = 0" on every step
looked wrong. Printing the raw hypotheses explained it and corrected the metric:
consecutive windows are offset in time, so the older window's tail is the newer
window's *head*, and prefixes cannot align. The raw text shows the same sentence
with leading words trimmed as the window slides, plus punctuation that appears
and disappears (`t=13s` inserts "Um, I mean," that the final decode lacks).

## Root cause

Parakeet TDT in this binding is a whole-buffer decoder with no state carried
between calls. Every re-decode re-derives the utterance from scratch, conditioned
on whatever context the window happens to contain, so it re-decides punctuation,
capitalisation, and wording each time. The instability is a property of the
engine, not of the window geometry, which is why no choice of window length or
cadence fixes it.

## Fix

Not a code fix: the design was rejected and the interim path was redefined. The
pill shows a *provisional* whole-take re-decode, clearly marked as still
changing, and the text that actually gets inserted remains the single
authoritative full decode on stop. The committed interim loop documents the
700 ms cadence as decode-bound rather than a taste choice.

Viable follow-ups, in order: a marked-provisional interim preview (what shipped),
a stateful Whisper partial decoder via `whisper-rs`' partial mode, or
non-overlapping segments with no cross-segment context.

## Verification

- Spike output above, reproducible with `cargo run -p teletype-speech --example
  stream_spike`.
- The interim loop's cadence constant now cites these numbers.
- `usp.md` §4 states the gap and names the measurement, so the claim is not
  re-asserted without evidence.

## Reusable lesson

Before porting a streaming design from a reference implementation, measure
whether the *engine* supports incremental decoding. "The competitor does
sliding-window re-decode" is a statement about their engine arrangement, not a
specification. The failure mode is subtle: compute is cheap, the code is
plausible, and the text looks approximately right in a screenshot, so the
instability only shows up when you measure agreement with the authoritative
decode.

Second lesson, about metrics: when a measurement returns a uniform extreme value
(zero, everywhere), treat it as a possible bug before treating it as a finding.
Printing the raw intermediate values cost ten minutes and changed the conclusion
about *why* the number was zero.
