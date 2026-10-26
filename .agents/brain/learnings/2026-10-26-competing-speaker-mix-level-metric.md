---
id: L-20261026-competing-speaker-mix-level-metric
title: A two-talker eval mix is only as good as its level metric - whole-file RMS and a flipped SNR sign both produce plausible-looking WER
status: verified
type: debugging
area: evals / audio mixing / ASR benchmarking
agent: OpenCode lead session
tool: OpenCode
date: 2026-10-26
---

# A two-talker eval mix is only as good as its level metric

## Problem

Two claims needed the same artefact that the eval corpus did not have: no set
on disk put a second **human** over the dictation. Every `noise-*` tier is
LibriSpeech speech plus DEMAND/MUSAN noise, a stationary masker. Building
`competing-{snr}db` sets (target + a different LibriSpeech speaker) was the
only way to test "excels when others are talking nearby" and to score any
target-speaker-extraction idea.

The first build produced numbers, and the numbers were wrong.

## Symptoms

- At −5 dB the model's hypotheses were the **interferer's** text: WER 98.9%
  over 200 cases, with fluent, correctly spelled output — obviously not a
  decoding failure.
- The mixer printed clips being scaled down by up to 0.267 (≈ −11 dB) to
  avoid clipping, which no plausible two-talker mix should need.
- A verification guard added pre-emptively (`realised mix level` vs the level
  the requested ratio implies) refused the −5 dB set at +5.79 dB where
  +1.19 dB was expected.

## Initial hypotheses

1. The stereo/mono path was wrong, or the interferer was written twice.
2. The target audio was being overwritten by the interferer.
3. The ASR was genuinely collapsing at −5 dB.

## Investigation

- The mixture was audible and contained both talkers; hypothesis 1/2 dead.
- The sign check: `mix_noise.py` defines SNR as `20 log10(rms_signal /
  rms_noise)`, so a *negative* SNR means the noise is **louder**. Re-deriving
  the total-level formula showed the guard's expected value, not the mixer,
  was wrong: `10 log10(1 + 10^(-snr/10))`, which is +6.19 dB at −5 dB, +3.01
  at 0 dB, +1.19 at +5 dB.
- The remaining error was the level metric. Whole-file RMS includes leading
  and trailing silence, and LibriSpeech clips differ wildly in how much
  silence they carry. An interferer with generous pauses got boosted until
  its *speech* was several dB above the target's, so the "0 dB" mixture was
  not 0 dB and the louder talker won — which is exactly what a single-channel
  ASR should do, and exactly the misleading set the corpus had before.

## Root cause

Two independent mistakes, both invisible in the output WER:

1. **Level metric**: whole-file RMS instead of speech-active level.
2. **Sign convention**: the verification formula assumed positive SNR meant
   the interferer is louder, the corpus convention is the opposite.

## Fix

`scripts/eval/mix_speech.py` now measures **speech-active level**: 25 ms
frames, keep those within 25 dB of the loudest frame and above −50 dBFS, take
RMS over their power. Gain is computed from the *looped segment* that is
actually mixed, not the whole interferer file. Every mix writes a `.json`
sidecar with the applied gain, both source levels, the measured ratio, and the
clip scale.

`scripts/eval/build_competing_set.py` builds three tiers (0, +5, +10 dB; a −5 dB tier was built and discarded — see below),
refuses to pair clips whose speakers it cannot resolve, and aborts if the
mixer's reported ratio is more than 0.5 dB off the request.

A `--self-test` on the mixer validates the metric independently: halving a
tone's amplitude must read −6.02 dB.

## Verification

- `python3 scripts/eval/mix_speech.py --self-test` → `self-test ok: amplitude
  halving -> 6.02 dB`.
- Rebuild prints the realised speech-active ratio per tier: +0.00, +5.00,
  +10.00 dB (requested 0, +5, +10).
- The −5 dB tier was built correctly after the fix, then discarded: at
  interferer-louder, a single-channel ASR transcribes the louder talker
  (WER 98.9% over 200 cases, fluent interferer text), which is expected
  behaviour — the tier measures the mixer, not the engine. The default
  tiers are 0, +5, +10 dB.
- Paired, differing speakers only: 40 target speakers, 31 interferer
  speakers, 0 same-speaker pairs; provenance in
  `~/.cache/teletype-eval/competing-build.json`.
- End-to-end acceptance: at +10 dB a competent engine must still transcribe
  the *target*, not the interferer. That is the check that would have caught
  this without the guard — use it whenever a mix set is first used.

## Reusable lesson

When synthesising a multi-talker eval set, the level metric **is** the
experiment. Whole-file RMS is not loudness for speech with pauses; use
speech-active level, state the convention in the filename (`-5db` = interferer
louder, matching `noise-*`), and assert the realised ratio numerically before
running a model. A wrong set does not fail — it answers a different question
confidently.
