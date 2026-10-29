---
id: L083
title: A 2B audio LLM does not beat a specialized ASR on dictation; its edge is language modeling, and that edge is too narrow to ship
status: verified
type: measurement
area: speech / eval / model selection
agent: main session (Qoder)
tool: llama.cpp b11960 llama-server, scripts/eval/bench_candidate_asr.py, teletype-eval
date: 2026-10-28
---

# A 2B audio LLM does not beat a specialized ASR on dictation; its edge is language modeling, and that edge is too narrow to ship

## Problem

Liquid AI released LFM3-Audio-2B (2026-09-08), a 2B any-to-any audio model
that claims English ASR WER 6.23 on the ESB/Open ASR suite and 6.03
multilingual on FLEURS, with streaming ASR (LiveASR) and 18 languages
including Polish. The question was whether it should join Teletype's speech
catalog.

## Claims tested

- Vendor headline: 6.23% English, 6.03% multilingual (FLEURS), several peers
  listed (Qwen2.5-Omni 3B 6.30, Voxtral Mini 3B 5.51).
- Implied product benefits: punctuation and casing included, streaming mode,
  Polish coverage, 1.4 GB on disk (within the 2.6 GB catalog ceiling).

## Investigation

1. Verified the model card and GGUF repo directly (not a search summary).
   llama.cpp support starts at b11960 / v0.8.0, including a `/v1/realtime`
   WebSocket; the repo's bundled `llama-server` is b11488, so a pin bump would
   be required to ship it.
2. Downloaded Q4_K_M (1.10 GB) + mmproj Q8 (0.34 GB) and ran the official
   b11960 macOS build. A resident `llama-server` with `--mmproj` transcribes
   through the OpenAI `input_audio` content part, system prompt
   `Perform ASR in <language>.`.
3. Added a `lfm3` adapter to `scripts/eval/bench_candidate_asr.py` so the
   Python bencher produces hypotheses and the Rust harness
   (`teletype-eval run --engine none`) scores them with the same normalizer
   and WER code as every shipping engine.
4. Ran 1,100 cases over 8 sets on an M4 Pro (24 GB), plus Polish baselines.

## Evidence

Verbatim micro WER, LFM3 vs `parakeet-ultra-coreml` (the Apple Silicon default):

| set | LFM3 | Ultra | note |
|---|---|---|---|
| librispeech-clean | 1.95 | 1.61 | loses |
| librispeech-other | 6.38 | 3.56 | loses |
| cv | 10.48 | 5.89 | loses |
| ami | 11.26 | 10.46 | loses |
| noise-0db | 12.79 | 7.18 | loses |
| competing-0db | **35.15** | 43.30 | **wins by 8.2 pp** |
| fleurs-en | **4.23** | 4.95 | **wins by 0.7 pp** |
| fleurs-pl | 11.32 | 7.79 | loses (Ultra is the Polish baseline) |

- Latency: p50 167-577 ms, p95 454-2041 ms, versus Ultra 38-75 / 57-185 ms.
  RTF 0.042-0.082, so decode is 12-24x faster than real time; the cost is
  per-utterance, not throughput.
- Memory: server RSS 1.70 GB at 16K context.
- Zero case errors; output is cased and punctuated, unlike the raw CTC/TDT
  engines.
- The `competing-0db` win is the only engine-level improvement ever measured
  on that set (raw-mix Fluid Mini 44.57, Ultra 43.30), and it comes with
  entity recall 0.80 vs 0.40. But 35% WER is still unusable dictation, and
  Focus + Ultra already reaches 29.93 on the same set.
- The Whisper Polish number from this harness (117.55%) is a fake: the eval
  driver hardcodes `transcribe(&samples, "auto")`, Whisper auto-detected
  English and emitted English-ish text. Do not quote it as a Whisper result.

## Root cause of the pattern

A 2B audio LLM spends capacity on language modeling. That helps exactly where
the acoustics are ambiguous but the language prior is strong (two talkers,
accented read speech). It does not help, and can hurt, where a specialized
acoustic encoder is simply better (clean read speech, stationary noise,
conversational Polish). The model is also 5-8x slower per utterance than a
0.6B TDT engine because every word is a decode step.

## Fix / verdict

No catalog row. Nowhere is LFM3 both better and usable; the two wins are one
narrow acoustic condition and one 0.7 pp accent-set gain. Additional gates
that would matter even if accuracy were better: LFM Open License v1.0 allows
commercial use only below $10M annual revenue, and shipping needs a new
speech provider plus a llama.cpp pin bump from b11488 to b11960+.

## Verification

Reports: `evals/runs/candidates/lfm3/<set>/report.json`; Polish baselines in
`evals/runs/bakeoff/{parakeet-ultra-coreml,large-v3-turbo-q5}/fleurs-pl-off/`.
Register rows: `.agents/brain/asrs.md` (candidate lane + run log 2026-10-28).

## Reusable lesson

When a speech-LLM vendor quotes an Open ASR average, do not compare it to a
clean-set WER and do not assume the average transfers. Run it through the
project harness on the sets that decide the product. Expect speech LLMs to
win on language-prior-heavy conditions and lose on clean/noisy/other-language
conditions; count the per-utterance latency, not just RTF. Check the runtime
pin and the weights license before spending time on a provider seam.
