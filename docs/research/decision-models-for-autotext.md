# Decision models for AutoText disambiguation (llama.cpp /v1/systemone)

Research + live spike, 2026-10-09. Follow-up to
`docs/research/jev-kev-for-symbol-disambiguation.md` and Decision D008.

Question: now that llama.cpp serves decision models natively
(`/v1/systemone`, PR #29818, b11488+), can a small local decision model
replace the deterministic AutoText disambiguation rule
(`crates/teletype-core/src/autotext/disambiguate.rs::entry_allowed`)?

**Short answer: no — not for text, at least not zero-shot.** The runtime
blocker that killed the Kev idea in the earlier doc is gone (these models run
on the same `llama-server` subprocess, D002-safe), but the *accuracy*
objection shows up instead: on the 16 real cases from the rule's own test
suite, the best model tested (Kev-4B) agrees with the rule only 13/16, and
the model it beats is a 60-line pure function with zero latency.

## What changed since the earlier research

| Earlier blocker | Status |
|---|---|
| Kev/Jev only run on Python + torch/MLX | **Gone.** llama.cpp b11488 (2026-10-08) serves decision models via `POST /v1/systemone`; GGUFs published by ggml-org. |
| No llama.cpp path for Qwen3.5 + DeltaNet | **Gone.** Kev-0.8B/4B/9B GGUFs are in the ggml-org decision-models collection and load in a plain llama-server. |
| Only hosted Jev had the right API shape | **Gone.** The `/v1/systemone` API *is* the Jev System One format (state + named questions → calibrated probabilities, zero output tokens). |

llama.cpp pin for this capability: **b11488 / commit `a657f7e9`** (PR #30114,
adds d1-omni-600M; PR #29818 added the endpoint; PR #29831, Clef support, is
also in). `scripts/build-llama-server.sh` is updated to this pin.

## The model landscape (HF, 2026-10-09)

ggml-org decision-models collection — all run via `/v1/systemone`:

| Model | Size | Base | Modalities | License | Notes |
|---|---|---|---|---|---|
| Julia-1 | 144M | mmBERT-small | text, 50+ langs | Apache 2.0 | 3 ms |
| Laya | 421M | ModernBERT-large | text, English | Apache 2.0 | 5 ms |
| **Kev-0.8B** | 0.8B | Qwen3.5-0.8B-Base | text | Apache 2.0 | tested |
| **Kev-4B** | 4B | Qwen3.5-4B-Base | text | Apache 2.0 | tested |
| lev | 4B | Qwen3.5-4B | text | Apache 2.0 | |
| Kev-9B | 9B | Qwen3.5 | text | Apache 2.0 | |
| Clef-Flash | 9B | Cloudflare/clef-flash | text+image | Apache 2.0 | |
| Clef | 27B | Cloudflare/clef | text+image | Apache 2.0 | |
| Bespoke-Nimble-9B-v3 | 9B | — | text | **CC BY-NC** | |
| OpenJev | 27B | Qwen3.8-27B | text+image | **CC BY-NC** | |

Non-collection GGUFs found: StartLux-Decision-0.8B/2B/4B/9B/27B (CC BY-NC,
needs a Python sidecar for the decision head — not a pure llama.cpp path),
OpenThai-SystemOne-GGUF (0.8B, Thai+EN, also needs a Python head client),
OOMU-SystemOne-0.6B (Apache 2.0, Qwen-0.6B + 256 decision tokens, 10–18 ms on
Apple Silicon — worth a look if a 600M-class text model ever matters).

**d1-omni-600M (Liquid AI, 2026-10-07):** 587M, LFM2.5-Encoder-350M + SigLIP2
vision + FastConformer audio. The only llama.cpp decision model with an audio
input path (≤30 s, "highly experimental"). Official GGUF:
`LiquidAI/d1-omni-600M-GGUF` (Q8_0 = 407 MB + 263 MB mmproj). License:
**LFM Open License v1.0** (Apache-based, free under $10 M revenue — fine for
Teletype, but not Apache 2.0). Experimental release; no published speed
numbers from Liquid AI.

## The spike

`crates/teletype-inference/src/bin/systemone-spike.rs` (new, standalone bin —
no app code touched). It:

1. Spawns the pinned `llama-server` for a model (`--serve <path|hf-ref>`, or
   `--base-url` for an existing server).
2. Runs the 16 built-in cases (the 13 from `disambiguate.rs` tests + the D008
   context examples) as a `noul` question: *"is the speaker asking to TYPE the
   character named '<phrase>'?"*
3. Scores each answer against the live rule (`entry_allowed`) and reports
   agreement + p50/p95 latency.

Reproduce:

```bash
scripts/build-llama-server.sh   # now builds b11488
hf download ggml-org/Kev-4B-GGUF --local-dir /tmp/kev4 --include "*Q8_0*"
cargo run -p teletype-inference --bin systemone-spike \
    --serve /tmp/kev4/Kev-4B-Q8_0.gguf
```

### Measured results (M5-class Mac, Q8_0, Metal, 16 cases)

| Model | Runtime | Agreement with rule | Wrong on | p50/avg | Load |
|---|---|---|---|---|---|
| Kev-0.8B | llama.cpp subprocess | **9/16 (56%)** | all 6 mid-sentence "keep" cases | 30 ms | ~2 s |
| d1-omni-600M-Q8_0 | llama.cpp subprocess | **11/16 (69%)** | "use a comma…", "the star…", "a plus…", "full stop ends", "plus sign" | 18 ms | ~5 s |
| Kev-4B | llama.cpp subprocess | **13/16 (81%)** | "use a comma…", "full stop ends in March", "two plus sign two" | 135 ms | ~10 s |
| **TinyDecide (10.4M)** | **in-process Rust** | **10/16 (62%)** | "insert a comma", "I need a comma here", "add a full stop", over-expands "a plus…" + "plus sign" | **9.9 ms** | **8 ms** |
| Deterministic rule | pure function | **16/16** (it is the ground truth here) | — | ~0 ms | ~0 ms |

**TinyDecide** (`TheREZOR/TinyDecide`, Apache 2.0, released 2026-10-05) is the
outlier: 10.4M params, 6.2 MB, a self-contained Rust crate with no ggml, so it
links in-process next to whisper.cpp (no D002 collision) and needs no
subprocess. It is the smallest and fastest option by an order of magnitude.

**Phrasing matters enormously for TinyDecide.** A 16-phrasing sweep (see
`tinydecide-sweep` / `tinydecide-sweep2`) found the best zero-shot operating
point is the short noul **"The speaker wants to insert the {phrase} symbol."**
at **11/16 (69%), ~9.5 ms** — up from 7/16 with a long question and 10/16 with
the first short one. The improvement is real but it plateaus at 11/16: five
more targeted phrasings ("means the symbol, not the ordinary word", "giving a
command to type the symbol", explicit "symbol-not-word" disambiguation, choice
variants) all scored 7-9/16, *worse* than the plain insert phrasing. The model
latches onto wording, but the plain imperative is the sweet spot.

The 5 misses at the 11/16 plateau are structural, not phrasing artifacts:
- **over-expands "plus"** ("that is a plus for the team", "two plus sign two")
  — "plus" is the most ordinary-English symbol word and the model can't hold it
  down mid-sentence;
- **under-expands cue-based comma/period** ("insert a comma", "I need a comma
  here", "the payment full stop ends in March") — all land just under 0.5; the
  model needs a naming cue to trigger expansion and the phrasing doesn't help it
  weight those cues.

It gets the unambiguous cases confidently right (0.95 "the symbol for a period",
0.87 "add a full stop here", 0.79 "close paren"). The failures are exactly the
ambiguous middle — the hard part the deterministic rule handles with its
contextual cues. This is why the rule still wins: the misses are not noise to
average out, they are the task.

Per-case pattern:

- Every model gets the unambiguous cases right ("period", "insert a comma",
  "the symbol for a period", "close paren").
- Kev-0.8B is a constant `expand` — it cannot do the discrimination at all.
- d1-omni-600M and Kev-4B both get the hard "keep" cases *partially* right,
  but inconsistently: "the payment period ends in March" is kept while "the
  star of the show" is expanded (same shape of sentence).
- The two failures the *best* model (Kev-4B) makes are interesting:
  - "the payment full stop ends in March" → kept. Defensible, but it is the
    exact bug D008 fixed ("the payment period ends in March" was the headline
    corruption). The model treats "full stop" as more ordinary-English than
    "period".
  - "two plus sign two" → expanded. The rule resolves this by seeing the
    longer phrase "plus sign" wins; a flat noul question over "plus" cannot
    express that. Better prompt engineering (asking about the *matched
    phrase*, i.e. "plus sign") would likely fix this one.
- "use a comma to separate the fields" is the case every model gets wrong.
  It is the documented reason `use` is not a naming cue in the rule.

The question phrasing is a single zero-shot attempt; a tuned prompt, per-case
criteria descriptions, or a fine-tune on Teletype's own override labels would
change these numbers. But the gap is in the wrong direction to close
casually: the rule is 100% on its own suite by construction, and the models
are 56–81%.

## Verdict

1. **Keep the deterministic rule as the implementation.** It is faster
   (0 ms vs 18–135 ms), more accurate on the cases that matter, and has no
   model-loading cost. D008 stands.
2. **The earlier doc's "take the architecture, not the runtime" is now
   half-obsoleted**: the runtime *is* compatible (same llama-server, same
   subprocess story, one new endpoint). What remains true is the
   architecture part — the seam (`entry_allowed`, one pure function) is
   exactly where a `SymbolDisambiguator` trait would sit if a model ever
   earns its keep.
3. **The realistic path to a model here is fine-tuning, not zero-shot.**
   The rule's 16 cases are the easy 16. The value of a decision model is on
   the long tail of utterances the rule cannot reason about, and that is only
   measurable with labelled real-world data — which the app should start
   collecting (every user override of an expansion is a label). Revisit when
   (a) a few hundred labels exist, and (b) a fine-tune demonstrably beats the
   rule on held-out corrections. Kev-4B is the most plausible base (best
   text accuracy, Apache 2.0, 135 ms p50 is affordable off the critical
   path).
4. **d1-omni-600M's audio path is the one genuinely new capability** in the
   llama.cpp lineup — decisions over ≤30 s raw speech, no ASR in between. It is
   experimental and its text quality (69%) is below Kev-4B (81%); do not plan
   around it until Liquid AI ships a non-experimental release.
5. **TinyDecide is the best *architectural* fit but the weakest *accuracy*.
   10.4M params / 6.2 MB / Apache 2.0 / in-process Rust / no ggml / 8 ms load /
   ~10 ms per decision. It sidesteps D002 entirely (no ggml to collide with
   whisper.cpp) and fits the ~13 MB binary constraint that d1-omni (407 MB)
   cannot. But zero-shot it scores 10/16 (62%) — below Kev-4B (81%) and
   d1-omni (69%) — and is text-only (no audio, 127-token cap). Its question
   phrasing is highly sensitive (7/16 with a long question, 10/16 with a short
   one), which is a sign it is prompt-tunable and possibly fine-tunable, not a
   hard ceiling. The realistic path is the same as for the Kev models: collect
   user-override labels and fine-tune; TinyDecide's size makes a fine-tune
   cheap, but it has no published fine-tune loop yet.
6. **License note:** Kev, Julia-1, Laya, lev, Clef, and **TinyDecide** are
   Apache 2.0 — all fine for Teletype (D001). OpenJev, Bespoke-Nimble, and
   StartLux are CC BY-NC — never bundle them. d1-omni-600M is LFM Open License
   v1.0 (free under $10 M revenue) — acceptable, but one more non-standard
   license to track.

## Open items / follow-ups

- [ ] **TinyDecide prompt-tuning: DONE (2026-10-09).** 16-phrasing sweep found
      the best zero-shot phrasing is "The speaker wants to insert the {phrase}
      symbol." at 11/16 (69%), ~9.5 ms. Targeted follow-ups ("command to type",
      "means the symbol not the word", choice variants) all scored 7-9/16 —
      worse. 11/16 is a prompt-tuning plateau; the 5 misses are structural
      (over-expands "plus", under-expands cue-based comma/period). Further
      zero-shot phrasing is unlikely to close the gap to the rule's 16/16.
- [ ] Label collection: record (text, phrase, user-chose-expand/keep) on every
      override of a System AutoText expansion. No code in this spike.
- [ ] If labels accumulate: fine-tune Kev-4B (or TinyDecide, which is cheaper
      to fine-tune at 10.4M) and re-run the spikes with held-out cases.
- [ ] Re-test d1-omni-600M when it leaves "experimental", and when
      llama.cpp's audio path matures — the audio-intent angle (deciding
      "symbol request?" from raw audio before ASR) has no competitor in the
      llama.cpp lineup today. TinyDecide has no audio path, so it cannot fill
      that role.
- [ ] `systemone-spike` and `tinydecide-spike` are dev tools, not part of the
      app. They live in `crates/teletype-inference/src/bin/` and are not
      referenced by any app code. Delete or promote them when the follow-ups
      above decide their fate.

## Artifacts

- llama.cpp pin: `scripts/build-llama-server.sh` → `a657f7e9` (b11488)
- Spikes (dev bins, no app code):
  - `crates/teletype-inference/src/bin/systemone-spike.rs` (llama.cpp models via HTTP)
  - `crates/teletype-inference/src/bin/tinydecide-spike.rs` (TinyDecide in-process, best phrasing)
  - `crates/teletype-inference/src/bin/tinydecide-sweep.rs` (10-phrasing sweep)
  - `crates/teletype-inference/src/bin/tinydecide-sweep2.rs` (6 targeted phrasings)
- TinyDecide vendored:
  - `crates/teletype-inference/vendor/tinydecide/` (Rust crate, Apache 2.0)
  - `crates/teletype-inference/vendor/tinydecide-model/` (meta.json + model.bin, 6.2 MB)
- llama.cpp model downloads (local only, not committed):
  `crates/teletype-desktop/binaries/decision-models/`
  (Kev-0.8B-Q8_0, Kev-4B-Q8_0, d1-omni-600M-GGUF)
