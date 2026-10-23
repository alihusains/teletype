# Zero-shot decision models lose to the deterministic rule on AutoText disambiguation

- **ID:** L023
- **Date:** 2026-10-09
- **Agent:** pi
- **Project area:** AutoText / disambiguation (D008) / teletype-inference
- **Status:** verified (measured, reproducible via `systemone-spike`)
- **Evidence:** `docs/research/decision-models-for-autotext.md`,
  `crates/teletype-inference/src/bin/systemone-spike.rs`

## Problem

Decision D008 ships a deterministic rule (`entry_allowed`) for "was that word
a symbol request or an ordinary word?" The 2026-09-28 research doc rejected a
real decision model (Jev/Kev) because the only runtime was Python +
torch/MLX, incompatible with the subprocess-llama-server architecture (D002)
and the no-Python contract (D003).

On 2026-10-07/08 that blocker disappeared: llama.cpp b11488 added
`POST /v1/systemone` (PR #29818) and GGUFs of the Kev family plus Liquid AI's
d1-omni-600M (PR #30114) now run on the exact same `llama-server` the app
already spawns.

## Measurement

`systemone-spike` (dev bin, not app code) ran the 16 cases from
`disambiguate.rs` tests as a `noul` question against three models, Q8_0,
Metal, M5-class Mac:

| Model | Agreement with rule | p50 | p95 |
|---|---|---|---|
| Kev-0.8B | 9/16 (56%) | 30 ms | 193 ms |
| d1-omni-600M-Q8_0 | 11/16 (69%) | 18 ms | 79 ms |
| Kev-4B | 13/16 (81%) | 135 ms | 254 ms |
| Deterministic rule | 16/16 | ~0 ms | ~0 ms |

Failure pattern: all models nail the unambiguous cases ("period",
"insert a comma"). Kev-0.8B is a constant `expand` (no discrimination).
Kev-4B and d1-omni-600M get the mid-sentence "keep" cases only *partially*
right, inconsistently ("the payment period ends in March" kept, "the star of
the show" expanded — same sentence shape). "use a comma to separate the
fields" (the documented reason `use` is not a naming cue) is wrong for every
model.

## Root cause

Zero-shot decision quality on a narrow, domain-specific discrimination task
does not beat a purpose-built rule. The models were trained on general
decision benchmarks (tickets, intent, toxicity), not on "is this word a
symbol request in dictation." The rule encodes exactly the four contextual
cues that matter for this task and nothing else.

## Fix / decision

No code change. D008 stands: the deterministic rule stays the
implementation. The seam (`entry_allowed`, one pure function) remains the
drop-in point for a `SymbolDisambiguator` trait if a model ever earns its
keep.

## Reusable lesson

1. **A runtime blocker being removed does not make the feature viable.**
   The Kev rejection was runtime-shaped, but the real deciding factor was
   accuracy, which the spike finally measured. When a "right shape, wrong
   runtime" decision gets re-asked, the first job is to measure the shape on
   the real cases, not to celebrate the runtime fix.
2. **Ground truth = the existing test suite.** The 16-case set came from
   `disambiguate.rs` tests; the spike calls `entry_allowed` live, so the
   comparison is model-vs-rule, not model-vs-assertion. A decision-model
   spike that hand-writes its own expected values can drift from the rule it
   is supposed to replace.
3. **TinyDecide (10.4M, 6.2 MB, Apache 2.0) is the best architectural fit but
   the weakest zero-shot accuracy (10/16, 62%).** It is a self-contained Rust
   crate with no ggml, so it links in-process next to whisper.cpp and
   sidesteps D002 entirely. It is also the smallest (~6 MB vs d1-omni's 407 MB
   GGUF) and fastest (~10 ms per decision, 8 ms load). But it is text-only,
   127-token-capped, and below Kev-4B (81%) and d1-omni (69%).
4. **Question phrasing is a tunable dial for TinyDecide, but it plateaus.** A
   16-phrasing sweep found the best zero-shot question ("The speaker wants to
   insert the {phrase} symbol.") scores 11/16 (69%), ~9.5 ms — up from 7/16
   with a long question. But five more targeted phrasings ("command to type",
   "means the symbol not the word", choice variants) all scored 7-9/16, worse.
   11/16 is a prompt-tuning plateau; the 5 misses are structural (over-expands
   "plus", under-expands cue-based comma/period), not phrasing artifacts.
5. **The realistic path for every model here is fine-tuning on collected
   labels, not zero-shot.** Every user override of a System AutoText expansion
   is a training label. Revisit a fine-tune when a few hundred labels exist.
   TinyDecide's 10.4M size makes a fine-tune cheap, but it has no published
   fine-tune loop yet; Kev-4B does.
6. **d1-omni-600M's audio path is the only genuinely new capability** in the
   llama.cpp lineup (decisions over ≤30 s raw speech, no ASR in between).
   TinyDecide has no audio path and cannot fill that role. d1-omni is
   experimental; do not plan around it until Liquid AI ships a
   non-experimental release.
7. **License watch:** Kev, Julia-1, Laya, lev, Clef, and TinyDecide are all
   Apache 2.0 — safe to bundle. OpenJev, Bespoke-Nimble, and StartLux are CC
   BY-NC — never bundle. d1-omni-600M is LFM Open License v1.0 (free under
   $10 M revenue) — acceptable but one more non-standard license to track.

## Operational notes (how the spike was run)

- llama.cpp pin bumped to `a657f7e9` (b11488) in
  `scripts/build-llama-server.sh`; build takes a few minutes, binary lands in
  `crates/teletype-desktop/binaries/llama-server`.
- This llama.cpp build is `-DLLAMA_OPENSSL=OFF`, so `-hf <repo>` **cannot
  download** (no HTTPS in the server). Download GGUFs first with `hf
  download` and pass a path; `systemone-spike --serve <path>` handles that.
- `LiquidAI/d1-omni-600M` (the PyTorch repo) is **not** directly loadable by
  llama.cpp; the GGUF is a separate repo, `LiquidAI/d1-omni-600M-GGUF`
  (Q8_0 407 MB + 263 MB mmproj).
- Models are ~0.8–4.5 GB each; they live in
  `crates/teletype-desktop/binaries/decision-models/` (gitignored territory,
  never commit).
