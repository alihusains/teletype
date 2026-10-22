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

Base URLs, from `crates/teletype-speech/src/catalog.rs` (`WHISPER_BASE_URL`,
`PARAKEET_BASE_URL`, `FLUID_REPO_BASE_URL`, `SHERPA_MODELS_BASE_URL`, and the
per-entry `base_url` override):

- Whisper (whisper.cpp ggml): `https://huggingface.co/ggerganov/whisper.cpp/resolve/main`
- Parakeet (ggml-org Parakeet GGUF): `https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main`
- Parakeet.cpp (GGUF): the same `ggml-org/parakeet-GGUF` base, except
  `fluid-speech-mini`, which overrides it (see its row)
- Core ML / FluidAudio (`FLUID_REPO_BASE_URL` = `https://huggingface.co`):
  `https://huggingface.co/FluidInference/<repo>`; the `file` of a Core ML entry
  *is* the repo name, and the files inside are fetched by the FluidAudio bridge
  at load time, not by Teletype's downloader
- sherpa-onnx (`SHERPA_MODELS_BASE_URL`): `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models`
  (one `.tar.bz2` release asset per model, extracted under
  `<models_dir>/sherpa/<archive stem>`)

Which engine ships where matters for the license question below: the ggml
engines (`Parakeet`, `Whisper`, `ParakeetCpp`) and the Core ML bridge are
macOS-only, so Windows only ever offers the two `SherpaParakeet` rows.

| Catalog id | Engine | File | URL | License | Attribution |
|---|---|---|---|---|---|
| `parakeet-tdt-v3` | Parakeet (ggml) | `ggml-parakeet-tdt-0.6b-v3-q8_0.bin` (~640 MB) | `…/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v3-q8_0.bin` | CC-BY-4.0 (base model: NVIDIA NeMo Parakeet TDT 0.6B v3, per plan §15; GGUF conversion terms `(unverified)`) | NVIDIA attribution |
| `parakeet-tdt-v3-q4` | Parakeet (ggml) | `ggml-parakeet-tdt-0.6b-v3-q4_0.bin` (~356 MB) | `…/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v3-q4_0.bin` | CC-BY-4.0 (base model, per plan §15; GGUF conversion terms `(unverified)`) | NVIDIA attribution |
| `parakeet-tdt-v3-f16` | Parakeet (ggml) | `ggml-parakeet-tdt-0.6b-v3-f16.bin` (~1260 MB) | `…/parakeet-GGUF/resolve/main/ggml-parakeet-tdt-0.6b-v3-f16.bin` | CC-BY-4.0 (base model, per plan §15; GGUF conversion terms `(unverified)`) | NVIDIA attribution |
| `fluid-speech-mini` | Parakeet.cpp (GGUF) | `fluid-speech-mini-q2k-q4k.gguf` (~347 MB) | `https://huggingface.co/FluidVoice-Labs/fluid-speech-mini-gguf/resolve/main/fluid-speech-mini-q2k-q4k.gguf` (entry `base_url` override; the engine's default base is `ggml-org/parakeet-GGUF`) | `(unverified)`: the terms live on the repo page in the URL column and were not re-checked in this pass. Base model: NVIDIA Parakeet family, mixed-quant (q2_k + q4_k) GGUF conversion by FluidVoice-Labs. Per plan §15, confirm before shipping | "Fluid Speech Mini by FluidVoice-Labs (base model: NVIDIA NeMo Parakeet)" |
| `parakeet-ultra-coreml` | Parakeet Core ML (FluidAudio, Neural Engine) | repo `parakeet-ultra-coreml` (~600 MB, multi-file checkpoint) | `https://huggingface.co/FluidInference/parakeet-ultra-coreml` | `(unverified)`: the terms live on the repo page in the URL column and were not re-checked in this pass. Base model: NVIDIA NeMo Parakeet TDT 0.6B (Ultra post-training); Core ML conversion by Fluid Inference (FluidAudio). Per plan §15, confirm before shipping | "NVIDIA NeMo Parakeet, Core ML port by Fluid Inference (FluidAudio)" |
| `parakeet-v2-coreml` | Parakeet Core ML (FluidAudio, Neural Engine) | repo `parakeet-tdt-0.6b-v2-coreml` (~600 MB, multi-file checkpoint) | `https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml` | `(unverified)`: same basis as `parakeet-ultra-coreml`. Base model: NVIDIA NeMo Parakeet TDT 0.6B v2 (English only) | "NVIDIA NeMo Parakeet v2, Core ML port by Fluid Inference (FluidAudio)" |
| `parakeet-v3-coreml` | Parakeet Core ML (FluidAudio, Neural Engine) | repo `parakeet-tdt-0.6b-v3-coreml` (~600 MB, multi-file checkpoint) | `https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml` | `(unverified)`: same basis as `parakeet-ultra-coreml`. Base model: NVIDIA NeMo Parakeet TDT 0.6B v3 | "NVIDIA NeMo Parakeet v3, Core ML port by Fluid Inference (FluidAudio)" |
| `parakeet-ctc-110m-coreml` | Parakeet Core ML (FluidAudio, auxiliary spotter) | repo `parakeet-ctc-110m-coreml` (~100 MB, multi-file checkpoint) | `https://huggingface.co/FluidInference/parakeet-ctc-110m-coreml` | `(unverified)`: same basis as `parakeet-ultra-coreml`. Base model: NVIDIA NeMo Parakeet CTC 110M. Not a dictation model: the bridge downloads it on demand for vocabulary boosting | "NVIDIA NeMo Parakeet CTC 110M, Core ML port by Fluid Inference (FluidAudio)" |
| `sherpa-parakeet-v3-int8` | SherpaParakeet (sherpa-onnx, CPU; the Windows default) | `sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` (~465 MB) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` | CC-BY-4.0 for the base model (NVIDIA NeMo Parakeet TDT 0.6B v3, per plan §15); the int8 export and release repackaging by k2-fsa `(unverified)` | "NVIDIA NeMo Parakeet v3, int8 ONNX export by k2-fsa (sherpa-onnx)" |
| `sherpa-parakeet-v2-int8` | SherpaParakeet (sherpa-onnx, CPU; English only) | `sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2` (~461 MB) | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2` | CC-BY-4.0 for the base model (NVIDIA NeMo Parakeet TDT 0.6B v2, per plan §15); the int8 export and release repackaging by k2-fsa `(unverified)` | "NVIDIA NeMo Parakeet v2, int8 ONNX export by k2-fsa (sherpa-onnx)" |
| `large-v3-turbo-q5` | Whisper (whisper.cpp ggml) | `ggml-large-v3-turbo-q5_0.bin` (~547 MB) | `…/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin` | MIT (per plan §15) | OpenAI |
| `large-v3-turbo` | Whisper (whisper.cpp ggml) | `ggml-large-v3-turbo.bin` (~1624 MB) | `…/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin` | MIT (per plan §15) | OpenAI |
| `small-en` | Whisper (whisper.cpp ggml) | `ggml-small.en.bin` (~466 MB) | `…/whisper.cpp/resolve/main/ggml-small.en.bin` | MIT (per plan §15) | OpenAI |
| `base-en` | Whisper (whisper.cpp ggml) | `ggml-base.en.bin` (~142 MB) | `…/whisper.cpp/resolve/main/ggml-base.en.bin` | MIT (per plan §15) | OpenAI |
| `base` | Whisper (whisper.cpp ggml) | `ggml-base.bin` (~142 MB) | `…/whisper.cpp/resolve/main/ggml-base.bin` | MIT (per plan §15) | OpenAI |
| `tiny` | Whisper (whisper.cpp ggml) | `ggml-tiny.bin` (~75 MB) | `…/whisper.cpp/resolve/main/ggml-tiny.bin` | MIT (per plan §15) | OpenAI |

Note: the speech catalog pins a SHA-256 for every single-file entry, taken
from the publisher's own metadata (the Hugging Face LFS `lfs.oid`, or the
GitHub release asset digest for sherpa archives) and verified before install.
Core ML entries have no single-file hash: they name a multi-file checkpoint
repo that the FluidAudio bridge fetches itself. The old `parakeet-tdt-v2`
ggml entry was removed because `ggml-org/parakeet-GGUF` publishes v3 only, so
the URL was a guaranteed 404.

## LLM / polish models (`teletype-inference` catalog)

| Catalog id | URL | License | Attribution |
|---|---|---|---|
| `s1-mini` ("S1-mini", 0.6B, ~462 MB) | `https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf` | Apache-2.0 + naming clause (license: `…/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE`) | Always display "S1-mini by Superwhisper" |
| `speakoflow-mini` ("SpeakoFlow Mini", 0.8B, ~834 MB), **default polish model** | `https://huggingface.co/SpeakoFlow/speakoflow-mini/resolve/main/SpeakoFlow-Mini-0.8B-Q8_0.gguf` | Apache-2.0 (`…/SpeakoFlow/speakoflow-mini/blob/main/LICENSE`) | "SpeakoFlow Mini by SpeakoFlow (Apache-2.0)" |
| `qwen2.5-3b` ("Qwen2.5 3B", ~1900 MB) | `https://huggingface.co/Qwen/Qwen2.5-3B-Instruct-GGUF/resolve/main/qwen2.5-3b-instruct-q4_k_m.gguf` | Apache-2.0 (`…/Qwen2.5-3B-Instruct-GGUF/blob/main/LICENSE`) | "Qwen2.5 by Alibaba (Apache-2.0)" |
| `fast` ("Fast (Qwen3 1.7B)", ~1400 MB) | `https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q5_k_m.gguf` | Apache-2.0 (`…/Qwen3-1.7B-GGUF/blob/main/LICENSE`) | "Qwen3 by Alibaba (Apache-2.0)" |
| `qwen3-0.6b` ("Qwen3 0.6B", ~400 MB) | `https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/qwen3-0.6b-q4_k_m.gguf` | Apache-2.0 (`…/Qwen3-0.6B-GGUF/blob/main/LICENSE`) | "Qwen3 by Alibaba (Apache-2.0)" |
| `quality` ("Quality (Qwen3 4B)", ~2600 MB) | `https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/qwen3-4b-q4_k_m.gguf` | Apache-2.0 (`…/Qwen3-4B-GGUF/blob/main/LICENSE`) | "Qwen3 by Alibaba (Apache-2.0)" |
| `qwen2.5-7b` ("Qwen2.5 7B", ~4900 MB) | `https://huggingface.co/Qwen/Qwen2.5-7B-Instruct-GGUF/resolve/main/qwen2.5-7b-instruct-q4_k_m.gguf` | Apache-2.0 (`…/Qwen2.5-7B-Instruct-GGUF/blob/main/LICENSE`) | "Qwen2.5 by Alibaba (Apache-2.0)" |
| `gemma2-2b` ("Gemma 2 2B", ~1500 MB) | `https://huggingface.co/google/gemma-2-2b-it-GGUF/resolve/main/gemma-2-2b-it-Q4_K_M.gguf` | Google Gemma Terms of Use (`https://ai.google.dev/gemma/terms`) — **not in the plan §15 allowed list** | "Gemma 2 by Google (Google License)" |
| `needle3` ("Needle 3 (Cactus Compute)", 121M, ~35 MB) | `https://huggingface.co/Cactus-Compute/needle3/resolve/main/needle3.cact` | Apache-2.0 claimed in catalog (`…/needle3/blob/main/LICENSE`); **weights license `(unverified)`** (per plan §15: ship only after confirmation) | "Needle 3 by Cactus Compute (Apache-2.0)" |

Plan P3.9 (EG-1 removal) is complete: the `eg-1` catalog entry, its 8 shards,
and the `models.enviouslabs.co` mirror URLs are gone, and `s1-mini` now points
at the Superwhisper Hugging Face repo. A user who had `eg-1` selected is
migrated to `speakoflow-mini` (the default cleanup model) with a one-time
notice on the Models screen (`migrate_removed_models` in
`crates/teletype-desktop/src/commands.rs`). Per plan §0.1 rule 5 the EG-1 license
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

## NeuDecide

| Model | Source | License | Notes |
|---|---|---|---|
| `neudecide-q4` (catalog id, ~43 MB, ONNX) | Upstream: `https://huggingface.co/neuphonic/neudecide` @ `fb0157e…` (repo is **gated**: download requires a signed-in HF account accepting a contact-information agreement — affiliation/role form; observed 18 Oct 2026, no redistribution restriction in the gate itself) | Apache-2.0 (upstream `LICENSE` shipped in the re-host release; hash-verified against snapshot `fb0157e…`) | **Re-hosted publicly (A.3.1, done 18 Oct 2026):** https://github.com/alihusains/teletype-models/releases/tag/neudecide-v1-q4 — catalog `neudecide-q4` shards point at the release download URLs; upstream HF `resolve/main` URLs kept as `backup_url`. Assets are bit-identical: all 6 files re-downloaded from the release match the pinned SHA-256s below. Release also ships `LICENSE`, `NOTICE.md`, `SHA256SUMS`. Runtime: `teletype_decider::NeuDecide` (`crates/teletype-decider`, `onnx` feature). |

Pinned artifacts (verified 17 Oct 2026 against HF snapshot
`fb0157e06da16d887caf041ca7929f96da419e6b`, all present in the local HF cache;
re-host release assets re-downloaded and re-verified bit-identical 18 Oct 2026):

| File | Size (bytes) | SHA-256 |
|---|---|---|
| `config.json` | 1,002 | `a1eb5783b7843541b193258c1296d616a4f3f9f5893745bac74c5ef19b2e53a0` |
| `audio_encoder_q4.onnx` | 22,519,972 | `2ac5c48e1766382736ed8dd4d468568ee583536550fb741ac187219f8cd8da9b` |
| `tool_encoder_q4.onnx` | 10,090,217 | `4a5075ee2868d90ea4be3234eae04001be6886c9730e8746421fa17e84313d85` |
| `decoder_step_q4.onnx` | 9,951,225 | `ee68bbb4bbca552b909b41cb1fdbfdf0f9f3c852561e18036044ee2ff461e50d` |
| `tokenizer.json` | 228,594 | `2b31f2b79506cca36b3c63f0e8549e8401dd8d768175c57aa1e67432d69dee0d` |
| `tokenizer.model` | 124,892 | `b39d6892d3b27e66a1782ee624d965e5e656c03e9226cbe036a80389090947ad` |

Source URLs (catalog shards): `https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/<file>` for each file above; backup: `https://huggingface.co/neuphonic/neudecide/resolve/main/<file>`.

## DeepFilterNet3-LL (VAD/endpointing noise suppression, plan §5.5)

| Model | Source | License | Notes |
|---|---|---|---|
| `dfn3_ll` (3 ONNX sub-models: `enc.onnx`, `erb_dec.onnx`, `df_dec.onnx`) | `https://github.com/Rikorose/DeepFilterNet/releases` (file `DeepFilterNet3_ll_onnx.tar.gz`) | Apache-2.0 (repo `LICENSE`) | 48 kHz, 480-sample frames, 0-frame lookahead. Runtime: `teletype_core::audio::dfn3` (`crates/teletype-core/src/audio/dfn3.rs`), ONNX inference via `ort` 2.0.0-rc.13 (`load-dynamic` feature, CPU only). Weights are NOT bundled; `build_dfn3_slot` skips loading when the directory is absent and the detection path falls back to raw audio. |

### ort (ONNX Runtime binding)

| Crate | Version | License | Notes |
|---|---|---|---|
| `ort` | 2.0.0-rc.13 | MIT OR Apache-2.0 | Pinned to rc.13 (not rc.14) because rc.14 requires rustc 1.92; workspace rust-version is 1.90. Features: `std`, `ndarray`, `load-dynamic`. Shared with `teletype-decider` (same version/features). |
