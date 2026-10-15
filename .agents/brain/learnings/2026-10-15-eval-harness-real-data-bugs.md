# L024 — The eval harness only lies calmly: synthetic smoke corpora hid three real-data panics

- **ID:** L024
- **Title:** Eval harness must be exercised on a real corpus before any gate trusts it — three panics surfaced on the first real run
- **Date:** 2026-10-15
- **Agent/tool:** OpenCode lead session (`feat/beat-wispr`)
- **Project area:** evals / measurement
- **Status:** verified
- **Evidence:** commits `9b54c2c`, `34df465`; red-pass proof below

## Problem

The Gate 0 baseline could not be produced. `teletype-eval run --engine parakeet`
against the real LibriSpeech sets crashed repeatedly, one bug at a time:

1. First run: `wer.rs:216` — `attempt to subtract with overflow` on the very
   first real LibriSpeech utterance (after a clean 1.9 s model load).
2. Second run (after fix 1): `earnings22-vocab-0058` — `No speech detected`
   aborted the whole 500-case run.
3. Third run (after fix 2): `polish.rs:272` — `range end index 1 out of range
   for slice of length 0` when the judge scored the empty hypothesis of the
   failed case.

## Initial hypotheses

- (1) "The model or the wav decoder is broken." Wrong — the model loaded and
  decoded fine; the panic was in the WER alignment backtrace.
- (2) "Parakeet found no speech, so the file is silence/corrupt." Wrong — the
  clip is 4.3 s at mean -19.4 dB: normal audio. It is a real ASR failure on
  that segment, not a dataset defect.
- (3) "The judge is fine; the failure is elsewhere." Wrong — the judge's
  `contains_word` sliced `h[i..i+n]` without a length guard.

## Investigation steps

1. Reproduce → read the exact panic line → map the DP: the forward pass writes
   `decisions` only for columns 1..=n, so column 0 is always 0; the backtrace
   read that 0, took the "consume column" branch and did `j -= 1` on a usize.
2. Ported the exact algorithm to Python, brute-forced all string pairs over
   `{a,b,c}` up to length 4: **384 of 14,641 pairs** underflow (e.g.
   `"a b a"` vs `"b a c"`); the fixed boundary logic fails 0.
3. Fixed per-case failure handling, re-ran: the judge then panicked on the
   empty hypothesis — same *class* of bug (slice/index arithmetic assuming
   lengths), second instance found at `polish.rs:244` for short references.

## Root cause

All three are the same failure mode at different layers: **index arithmetic
that silently assumed real-world invariants (non-empty inputs, edges of a DP,
short references) that the 17-clip `say`-synthesized smoke corpus never
violated.** The gate infrastructure itself had never been driven by real data.

## Fix

- `9b54c2c` — explicit boundary moves in the wer backtrace + guards in the
  polish judge; regression tests: the minimal trigger pair, an exhaustive
  14,641-pair agreement test vs a naive Levenshtein, empty-hypothesis and
  short-reference judge cases.
- `34df465` — per-case decode/ASR failures are recorded (`case_errors` in
  report.json), scored as total errors, and never abort a run; `load_sets`
  no longer double-prefixes pre-prefixed ids/vocab keys (found in the same
  output: `earnings22-vocab-earnings22-vocab-0058`).

## Verification

- Red-pass: both new wer tests panic with the exact original message when the
  fix is reverted (temporarily) — proven, not assumed.
- Green: `cargo test -p teletype-eval` 96/96; clippy 0 errors; rustfmt clean
  on every touched file (old-vs-new per-file comparison).

## Reusable lesson

Run every measurement tool against the *real* corpus before trusting any
number it produced on synthetic data. And when an index/slice bug is found in
one place, grep for the same pattern (`saturating_sub` + indexing, `[i..i+n]`)
elsewhere in the file — this class came in threes.
