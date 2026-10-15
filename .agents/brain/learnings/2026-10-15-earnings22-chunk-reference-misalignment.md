# L025 — Earnings22-VAD chunks ship no per-chunk transcripts: proportional word apportionment is a silent dataset artifact

- **ID:** L025
- **Title:** Scoring VAD-chunked audio against a duration-proportional word split fabricates WER (hypothesis = middle/tail of the reference); score the parent call instead
- **Date:** 2026-10-15
- **Agent/tool:** OpenCode lead session (`feat/beat-wispr`)
- **Project area:** evals / datasets
- **Status:** verified
- **Evidence:** commit `429e860`; run artifacts: per-chunk WER 0.8229 vs call-level 0.1218 on the same model and audio

## Problem

The `earnings22-vocab` set scored `verbatim_micro=0.8229` on its first real
run (partial-4), with 78/100 cases at WER ≥ 0.7. Side-by-side probes showed
the hypotheses were not garbage: each one matched the **middle or tail** of
its assigned reference (e.g. case 0015's hypothesis is a line from the middle
of the window; 0008 starts ~12 words into it).

## Symptoms

- WER distribution bimodal: good cases |Δ|≤5 words from the reference start,
  bad cases +6…+43 words (`Δ` measured by locating the hypothesis inside the
  reference transcript).
- All chunk durations, timings and file names looked consistent with the
  declared metadata.

## Initial hypotheses

1. "Our builder downloaded or renamed the wrong wav per case." **Ruled out**:
   case 0015's local 16 kHz mono PCM is md5-identical
   (`350305ff93c644e91ab4410e95084a6d`, 591,084 bytes) to a fresh HF download
   of `4468919_trimmed_chunk_50.wav`.
2. "The chunk timings are broken." **Ruled out**: chunk k's end equals
   chunk k+1's start for every pair, first start = 0, last end = parent
   duration; all local wav durations match the declared durations.

## Investigation steps

1. PCM equality probe (hypothesis 1) → mapping is clean.
2. Timeline audit of the chunk jsonl → the chunks tile the parent contiguously.
3. Read the dataset card: chunks are **Silero-VAD speech segments merged up to
   30 s**; "reference text … aligned during the Artificial Analysis evaluation
   pipeline". The repo ships **audio + start/end timings only** — no per-chunk
   text.
4. Conclusion: word position is not linear in elapsed audio (speech rate and
   pauses vary). Apportioning words to chunks by `duration / parent_duration`
   is therefore wrong wherever the local pace deviates from the call average —
   and the error shows up as *systematically shifted* references, not noise.

## Root cause

The old builder split the parent transcript proportionally to chunk duration.
Nothing in the dataset supports that assumption: the chunk boundaries come
from VAD (speech regions), while the proportional split assumes constant
words-per-second. The misalignment then punishes the model for hearing what
the audio actually contains (the neighbouring words).

## Fix

Score the unit the data actually supports: **the whole call**. The chunks tile
each parent losslessly, so concatenating a call's chunks in `chunk_index`
order reconstructs the cleaned parent audio; the cleaned parent transcript is
then an exact reference and no alignment of any kind is needed. This also
matches how AA itself scores Earnings22 (per-call WER). Rebuilt set: 6 calls,
738 vocab terms (`429e860`; same output contract: manifest.jsonl, vocab.json,
audio/, `.done` with a version marker so stale chunk-level layouts rebuild).

## Verification

- Pre-check on parent 4471586 (21 min): `verbatim_micro=0.1034` in 24 s —
  vs 0.82 with the chunk apportionment, same model, same audio.
- Full rebuilt set: 6 cases, `verbatim_micro=0.1218`, `vocab_recall=0.8277`,
  `entity=0.8447` in 1m56s for 115 minutes of audio (RTF ≈ 52×) — in line
  with published Parakeet numbers on Earnings22.
- Set integrity: 6 wavs + manifest + vocab.json + `.done`; assembled call
  durations within 6 s of the parents' declared durations.

## Reusable lesson

Before scoring chunked audio, verify the dataset ships the *reference
alignment*, not just audio + timings. A duration-proportional split assumes a
constant speech rate — VAD chunks do not satisfy it. When a metric collapses
and hypotheses are systematically shifted (tail/middle of their reference),
suspect the reference derivation before the model. When chunks tile their
parent losslessly, parent-level scoring removes the alignment problem
entirely, and often matches how the dataset's authors score it themselves.
