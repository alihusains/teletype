# NeuDecide (Neuphonic) — audio-native speech-to-tool-call model

Research, 2026-10-10. Follow-up to
`docs/research/decision-models-for-autotext.md` (Decision D015).

Question: what is NeuDecide (https://huggingface.co/neuphonic/neudecide,
https://www.neuphonic.com/models/neudecide), where does it fit in Teletype,
and does it replace Whisper?

## What it is

A tiny **audio-first decision model**: raw speech in + a JSON tool list in →
a structured tool call out. **No transcript in between.** That is the whole
point — it is *not* an ASR model.

- 55.5M params, 42.7 MB (three ONNX graphs: audio encoder 28.7M / tool
  encoder 11.8M / decoder step 15.0M), ~60M total.
- Apache 2.0 (repo + weights). Repo + PyPI `neudecide` released ~2026-10-01/08;
  PyPI is alpha (`0.1.0a1`). 8 days old at time of writing.
- 16 kHz, ≤30 s audio, English only. Limits: 1,536 tool tokens, 128 output
  tokens, ≤10 tools recommended.
- Runs on a single CPU thread via ONNX Runtime, no GPU, no internet after
  download. Measured: 46 ms/call on M3 (159 ms load), 82 ms on S24+, 206 ms on
  Pi 5; peak RAM 146–174 MB.
- Architecture: streaming English FastConformer encoder (NVIDIA
  `stt_en_fastconformer_hybrid_medium_streaming_80ms`) + a Needle-initialised
  tool-calling encoder–decoder, trained end to end.
- SLURP (10 tools): 75.5% tool accuracy, 37.5% exact match — beats every
  cascade tested (Whistle→Needle 3, Parakeet 660M→FunctionGemma) and
  Voxtral Mini 3B by ~5× on exact match.
- Known limitations (vendor-stated): **can fire on noise** (no calibrated
  confidence score — put VAD or a "no action" tool in front); free-form
  arguments are weaker than ASR cascades (use `enum`); English only; 30 s cap.

## Does it replace Whisper?

**No.** It produces zero text. Teletype's core job (speak a sentence → typed
into the focused app) still needs Parakeet/Whisper/Whistle. NeuDecide is a
different category: it is a decision model, not a transcription model.

## Where it fits in Teletype

It is the **audio-native version of D015's open item**. D015 tested text
decision models (Kev, TinyDecide, d1-omni-600M) and concluded: keep the
deterministic rule; the one genuinely new capability to watch is *decisions
over raw speech, no ASR in between* (d1-omni's experimental audio path); do
not plan around it until non-experimental. NeuDecide is exactly that,
non-experimental, Apache 2.0, and 43 MB vs d1-omni's 407 MB.

Fit points, in order of plausibility:

1. **Voice-activated commands (the real new feature).** "Insert a comma",
   "clear this", "switch to professional" → tool calls decided in ~46 ms
   *before* ASR runs. The Wispr-style voice-command layer, with the smallest
   credible local model found so far. Sits at the same seam D015 designed
   (`SymbolDisambiguator`-shape over `entry_allowed`, or a new
   `CommandGate` in front of the dictation pipeline).
2. **AutoText disambiguation from audio** — D015 follow-up, scoreable with
   the existing 16-case suite (English sentences, so language is fine).
3. **Intent gate** — "dictation vs command" as a 2-tool decision. Would need
   our Silero VAD in front given the fires-on-noise limitation.

## Blockers / costs (why it is not in the app today)

- **Python + ONNX Runtime only.** No Rust crate, no GGUF, no llama.cpp path.
  Integration options: (a) `llama-server`-style Python sidecar (fits D002's
  subprocess pattern but ships a Python runtime/PyInstaller blob — conflicts
  with the ~13 MB binary target), or (b) new `onnxruntime-sys` crate linking
  the ONNX Runtime C lib next to whisper.cpp (no ggml collision, but a big
  native dep + real integration work).
- **43 MB user-directed download** — fine as an optional model (EG-1
  precedent), not fine bundled.
- **No confidence score** — needs a VAD/"no action" gate (we have Silero VAD).
- **Alpha, 8 days old** — API may move.

## Verdict

1. **Keep Parakeet/Whisper/Whistle for transcription.** NeuDecide does not
   touch that lane.
2. **Track Neuphonic for updates** (repo, HF, blog). The moment a good
   non-alpha path appears (Rust/ONNX C bindings, GGUF export, or a stable
   Python sidecar we accept), spike it: score the 16-case AutoText suite and a
   voice-command prototype against the deterministic rule.
3. **Do not integrate now.** The integration cost (sidecar or ONNX-in-Rust)
   outweighs the benefit while it is alpha and Python-only. The seam is ready
   (D015); the model is the first credible audio-native candidate for it.

## Monitoring checklist

- [ ] NeuDecide releases a Rust crate or stable ONNX C buildable path.
- [ ] GGUF or llama.cpp-compatible export appears (would make it drop-in via
      the existing `llama-server` subprocess pattern, D002-safe).
- [ ] Confidence score / "no action" calibration ships (vendor lists this as
      missing).
- [ ] Multi-language support (we ship Windows + non-English Whisper models).
- [ ] PyPI leaves alpha.
- [ ] Re-score: 16-case AutoText suite + a small voice-command suite (insert
      comma / switch style / clear) as a spike bin, same shape as
      `tinydecide-spike.rs`.

## Artifacts

- Sources: https://huggingface.co/neuphonic/neudecide ·
  https://www.neuphonic.com/models/neudecide ·
  https://github.com/neuphonic/neudecide ·
  https://www.neuphonic.com/blog/neudecide
- Related: `docs/research/decision-models-for-autotext.md`, D008, D015,
  `crates/teletype-inference/src/bin/tinydecide-spike.rs`.
