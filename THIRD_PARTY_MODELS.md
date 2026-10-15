# Third-party models

Every model this tree references — the speech catalog
(`crates/teletype-speech/src/catalog.rs`), the LLM catalog
(`crates/teletype-inference/src/catalog.rs`), and the files committed under
`models/` — with its download URL, license, and attribution text.

Teletype never bundles or re-hosts model weights (except the two files
committed under `models/`, noted below). Models are downloaded by the user on
demand from the publisher's official URLs; the user is bound by the model's
license upon download. Allowed licenses are listed in the Beat Wispr Flow
plan §15 (Apache-2.0, MIT, BSD, CC-BY-4.0 with attribution, plus Superwhisper's
Apache-2.0 + naming clause for S1-mini). Anything that could not be verified
is marked `(unverified)` — no license is invented here.

This file covers **model weights only**. Library/license notices live in
`THIRD-PARTY-NOTICES.txt` (kept). Teletype's own source is MIT (`LICENSE`,
matching `license = "MIT"` in the root `Cargo.toml`).

## Speech models (`teletype-speech` catalog)

Base URLs (see `catalog.rs`):

- Whisper: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main`
- Parakeet: `https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main`

| Catalog id | File | URL | License | Attribution |
|---|---|---|---|---|
| `parakeet-tdt-v3` | `ggml-parakeet-tdt-0.6b-v3-q8_0.bin` (~640 MB) | `…/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v3-q8_0.bin` | CC-BY-4.0 (base model: NVIDIA NeMo Parakeet TDT 0.6B v3, per plan §15; GGUF conversion terms `(unverified)`) | NVIDIA attribution |
| `parakeet-tdt-v2` | `ggml-parakeet-tdt-0.6b-v2-q8_0.bin` (~631 MB) | `…/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v2-q8_0.bin` | CC-BY-4.0 (base model: NVIDIA NeMo Parakeet TDT 0.6B v2, per plan §15; GGUF conversion terms `(unverified)`) | NVIDIA attribution |
| `parakeet-tdt-v3-q4` | `ggml-parakeet-tdt-0.6b-v3-q4_0.bin` (~356 MB) | `…/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v3-q4_0.bin` | CC-BY-4.0 (base model, per plan §15; GGUF conversion terms `(unverified)`) | NVIDIA attribution |
| `parakeet-tdt-v3-f16` | `ggml-parakeet-tdt-0.6b-v3-f16.bin` (~1260 MB) | `…/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v3-f16.bin` | CC-BY-4.0 (base model, per plan §15; GGUF conversion terms `(unverified)`) | NVIDIA attribution |
| `large-v3-turbo-q5` | `ggml-large-v3-turbo-q5_0.bin` (~547 MB) | `…/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin` | MIT (per plan §15) | OpenAI |
| `large-v3-turbo` | `ggml-large-v3-turbo.bin` (~1624 MB) | `…/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin` | MIT (per plan §15) | OpenAI |
| `small-en` | `ggml-small.en.bin` (~466 MB) | `…/whisper.cpp/resolve/main/ggml-small.en.bin` | MIT (per plan §15) | OpenAI |
| `base-en` | `ggml-base.en.bin` (~142 MB) | `…/whisper.cpp/resolve/main/ggml-base.en.bin` | MIT (per plan §15) | OpenAI |
| `base` | `ggml-base.bin` (~142 MB) | `…/whisper.cpp/resolve/main/ggml-base.bin` | MIT (per plan §15) | OpenAI |
| `tiny` | `ggml-tiny.bin` (~75 MB) | `…/whisper.cpp/resolve/main/ggml-tiny.bin` | MIT (per plan §15) | OpenAI |

Note: the speech catalog pins no SHA-256 for any entry (`sha256: None` in
`catalog.rs`); the code comments flag this as a known gap. No `sha256`
values are recorded here because none could be verified.

## LLM / polish models (`teletype-inference` catalog)

| Catalog id | URL | License | Attribution |
|---|---|---|---|
| `s1-mini` ("S1-mini", 0.6B, ~462 MB) | `https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf` | Apache-2.0 + naming clause (license: `…/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE`) | Always display "S1-mini by Superwhisper" |
| `qwen2.5-3b` ("Qwen2.5 3B", ~1900 MB) | `https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf` | Apache-2.0 (`…/Qwen2.5-3B-Instruct-GGUF/blob/main/LICENSE`) | "Qwen2.5 by Alibaba (Apache-2.0)" |
| `fast` ("Fast (Qwen3 1.7B)", ~1400 MB) | `https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q5_k_m.gguf` | Apache-2.0 (`…/Qwen3-1.7B-GGUF/blob/main/LICENSE`) | "Qwen3 by Alibaba (Apache-2.0)" |
| `qwen3-0.6b` ("Qwen3 0.6B", ~400 MB) | `https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/qwen3-0.6b-q4_k_m.gguf` | Apache-2.0 (`…/Qwen3-0.6B-GGUF/blob/main/LICENSE`) | "Qwen3 by Alibaba (Apache-2.0)" |
| `quality` ("Quality (Qwen3 4B)", ~2600 MB) | `https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/qwen3-4b-q4_k_m.gguf` | Apache-2.0 (`…/Qwen3-4B-GGUF/blob/main/LICENSE`) | "Qwen3 by Alibaba (Apache-2.0)" |
| `qwen2.5-7b` ("Qwen2.5 7B", ~4900 MB) | `https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf` | Apache-2.0 (`…/Qwen2.5-7B-Instruct-GGUF/blob/main/LICENSE`) | "Qwen2.5 by Alibaba (Apache-2.0)" |
| `gemma2-2b` ("Gemma 2 2B", ~1500 MB) | `https://huggingface.co/google/gemma-2-2b-it-GGUF/resolve/main/gemma-2-2b-it-Q4_K_M.gguf` | Google Gemma Terms of Use (`https://ai.google.dev/gemma/terms`) — **not in the plan §15 allowed list** | "Gemma 2 by Google (Google License)" |
| `needle3` ("Needle 3 (Cactus Compute)", 121M, ~35 MB) | `https://huggingface.co/Cactus-Compute/needle3/resolve/main/needle3.cact` | Apache-2.0 claimed in catalog (`…/needle3/blob/main/LICENSE`); **weights license `(unverified)`** (per plan §15: ship only after confirmation) | "Needle 3 by Cactus Compute (Apache-2.0)" |

Plan P3.9 (EG-1 removal) is complete: the `eg-1` catalog entry, its 8 shards,
and the `models.enviouslabs.co` mirror URLs are gone, and `s1-mini` now points
at the Superwhisper Hugging Face repo. Per plan §0.1 rule 5 the EG-1 license
only allows use inside Envious Labs apps and bans use in competing products,
and no code from EnviousWispr (GPLv3) may be copied.

## Files committed under `models/`

| File | What it is | License / attribution |
|---|---|---|
| `models/tinydecide/meta.json` (kept, ~187 KB text) | TinyDecide model config/tensor table by REZOR: `https://huggingface.co/TheREZOR/TinyDecide` | Apache-2.0 (verified on the Hugging Face repo page, 14 Oct 2026; base model `google/electra-small-discriminator`). Runtime: `tinydecide::TinyDecide` (`crates/tinydecide`, Apache-2.0). |
| `models/tinydecide/model.bin` (**removal staged in P0.1**, ~6.2 MB) | TinyDecide 10.4M 4-bit weights, same repo as above | Same Apache-2.0 as above. Re-download: `curl -L -o models/tinydecide/model.bin https://huggingface.co/TheREZOR/TinyDecide/resolve/main/model.bin` (see `crates/teletype-inference/src/tinydecide.rs`). Nothing needs it at build/test time: the app skips loading when it is absent (`ensure_decision_provider`), and all tests that use it skip with download instructions. |
| `models/needle3.cact` (kept, ~35 MB) | Needle 3 weights (see `needle3` catalog entry above) | Same caveat as the catalog entry: Apache-2.0 claimed, weights license `(unverified)`. Out of scope for P0.1; left untouched. |

No `vendor/` directory exists in this tree (confirmed 14 Oct 2026) — there is
nothing to deduplicate.
