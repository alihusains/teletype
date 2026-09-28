# Jev / Kev as the disambiguator for System AutoText

Question from the user: can a Jev- or Kev-style model make the
"was that word a symbol request or an ordinary word?" decision, without
hurting latency?

Short answer: **the idea is right and the tool is the right shape, but Kev's
runtime does not fit Teletype today.** Here is what I verified, including the
parts that do not work.

## What Jev and Kev actually are

Not language models. **Discriminative decision models.**

- **Jev** (Typesafe AI): a "System 1" model that never generates text. You
  send the text plus one or more *questions*, each with a set of answer
  options, and it returns a **calibrated probability for each option**. Hosted
  API. 70-500 ms per request.
- **Kev** (github.com/jaredpalmer/kev, Apache-2.0, 7.4k stars): the open-source
  family. Qwen3.5/3.8 base + rank-16 LoRA adapter + a small pointer head. Same
  API. Sizes 0.8B / 4B / 9B / 27B.

The question types map onto this problem exactly:

```json
{
  "state": "the payment period ends in March",
  "questions": {
    "intent": {
      "type": "noul",
      "instructions": "Did the speaker ask for a full-stop character to be typed?"
    }
  }
}
```

which returns roughly `{"noul": 0.03}` - i.e. "no, that was the word".

That is a better shape than a hardcoded rule, in three ways:

1. It returns a **probability**, so the threshold is tunable and the
   confidence is reportable.
2. It is **trained**, not hand-written, so it generalises to phrasings I would
   not think of.
3. `kev-finetune` is a documented loop for training on your own labelled data,
   which is exactly what Teletype would need: a few hundred labelled
   utterances, one epoch, and the boundary stops being a guess.

## Measured latency (Kev's own numbers, Apple M5)

| Model | New text | Same text again (cached) |
|---|---|---|
| Kev-0.8B | 149 ms | 28 ms |
| Kev-4B | 721 ms | 136 ms |

For reference, Teletype's measured dictation is ASR 380 ms p50 / 1100 ms p95,
total 1650 ms p50 / 3800 ms p95. So **Kev-0.8B at 149 ms is affordable** on
the slow path, and 28 ms when the text repeats. Latency is not the objection.

## Why it does not fit today

**Kev's serving stack is Python, not llama.cpp.**

From the Kev README: Python 3.12/3.13, `uv`, PyTorch on GPU or **MLX** on
Apple Silicon, served by `python -m kev.serve` over HTTP. The model is a LoRA
adapter on a Qwen3.5 base.

Teletype's LLM path is the opposite: a statically linked `llama-server`
C++ subprocess, spawned by Rust, over loopback HTTP, in a ~13 MB binary. That
is not an accident, it is Decision D002: linking llama.cpp into the app is
prohibited because its ggml collides with whisper.cpp's, and D003 requires no
Python runtime and no cloud on the default path.

Adding Kev would mean shipping Python + torch-or-MLX. That is a different
product, not a component.

There is a second problem, and it is the one that decides this. Kev's models
are Qwen3.5/3.8, which "mix attention layers with Gated DeltaNet layers, which
are recurrent and ignore attention masks". The repo's own docs make a point of
this because it forces each question into its own forward pass. There is no
established llama.cpp path for that architecture; even HF's llama.cpp-quant
support post is explicit that it is *starting* with Qwen3.5. So the obvious
shortcut - quantise Kev to GGUF and run it on the `llama-server` Teletype
already spawns - is not available today.

A third, smaller one: routing through TypeSafe's hosted Jev API, or a Kev
deployment on Modal, would send the user's dictated text to a third party. That
directly contradicts D003 (no network on the default path) and D004. It could
only ever be an explicit opt-in, which means it is not the default answer.

## The recommendation

**Take the architecture, not the runtime.**

The insight worth having from Jev/Kev is: *this is a discrimination problem with
a tunable threshold, not a parsing problem*. Teletype can have that today with
a calibrated probabilistic classifier in about 200 lines of dependency-free
Rust, trained on the same features my prototype uses, and refined later from
real corrections.

Concretely, in order:

1. **Ship the deterministic rule now.** Already prototyped, 13/13 on the real
   cases. Expand a single ambiguous word only when the utterance is 1-2 words,
   or a naming cue sits within two words before it, or it starts/ends the
   utterance. `use` is deliberately not a naming cue. Zero latency, no
   dependency, no setting needed.

2. **Shape the seam so a real decision model drops in later.** Put the rule
   behind a `SymbolDisambiguator` trait returning
   `Expand { confidence: f32 }` or `Keep`. The 13-case suite in
   `tests/text_integrity.rs` becomes the gate for whatever replaces the rule.

3. **Collect labelled data from day one.** Every time a user overrides an
   expansion (or the app auto-corrects one after dictation), that is a training
   label. In six months Teletype owns a dataset no public model has, which is
   when a real 0.8B decision model starts to earn its keep.

4. **Revisit Kev then, not now.** If a GGUF path for Qwen3.5 + DeltaNet
   appears, or if Kev ships a llama.cpp-compatible export, the 0.8B model at
   149 ms is affordable on this dictation's latency budget, and step 3 means it
   could be fine-tuned on Teletype's own labels.

## Honest summary

| Option | Latency | Fits the architecture today? | Honest verdict |
|---|---|---|---|
| Deterministic rule | ~0 ms | yes | Ship it. 13/13 on real cases. |
| Calibrated classifier in Rust | <1 ms | yes | Same seam, learns instead of guessing. |
| Kev-0.8B local (Python/MLX) | 149 ms | **no** - new runtime, D002/D003 | Revisit when a GGUF path exists. |
| Kev-4B/9B local | 721 ms+ | no | Too slow for the fast path anyway. |
| Hosted Jev / Modal | 70-500 ms + network | no - breaks D003 | Opt-in only, never the default. |

The user instinct was right: a discriminative decision model is the correct
shape for this, and calibrated probabilities beat a hardcoded rule. The
constraint is not the idea, it is that the only implementation of that idea
today is a Python runtime, and Teletype's whole LLM story is built on not
having one.
