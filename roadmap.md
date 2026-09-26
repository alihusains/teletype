# Teletype Roadmap

Extract proven ideas from EnviousWispr (EW), keep Teletype's performance and privacy edge.

**Reference tree:** shallow clone at `/var/folders/w3/y5xsfv8x42v6qcn7gdmdt769hgrqq5/T/opencode/EnviousWispr` (Swift 6, macOS 14+, 576 source files / 768 test files).
**Baseline:** Teletype `d8979ed` + uncommitted batch (usage tracking, System AutoText, Home/Dictation polish, icons). ~11.8k LOC Rust, 165 tests, 13 MB release binary, no telemetry, on-demand models.

---

## 2026-09-26 — Reprioritized plan (high-impact, quick-win first)

> Re-sequenced to win against WisprFlow. The old P0–P4 below is preserved as the
> detailed reference; this section is the current order of attack. Full reasoning in
> `usp.md`. Rule: build the things that make a user switch, in the order they get
> noticed — perceived speed first, then the "it just knows" layer, then the cheap
> high-perceived-value wins.

### Tier 1 — The switcher (do first, biggest perceived win)

| # | Item | Why it wins | Effort | Status |
|---|---|---|---|---|
| T1.1 | **Streaming LLM + progressive paste** | Closes the #1 gap: WisprFlow shows text in ~1s, we take 3–8s (full-batch). This is what a user notices in the first 10 seconds. | High | **Done (true network-level streaming, 2026-09-26):** trait `generate_with_system_stream` + `ServerProvider::chat_stream` (raw TCP SSE reader, `SseFrameParser` incremental frames) + engine token sink + pill live preview. Tokens now fire as the LLM generates them (verified live: frames arrive over the wire progressively, not in a burst). All tested (235 core / 33 inference / 19 desktop, incl. `sse_frame_parser_streams_across_chunk_boundaries`). |
| T1.2 | **Live transcript preview in the pill** | "Words assembling as I speak" is the whoa moment. Requires T1.1. | Medium | Not started (pill has no live-text surface) |
| T1.3 | **500-word transcript splitter** (old P1.1) | Long dictations degrade quality without it; prerequisite for polishing long takes reliably. | Low-Med | Not started |

### Tier 2 — "It just knows" (differentiators a cloud app can't match)

| # | Item | Why it wins | Effort | Status |
|---|---|---|---|---|
| T2.1 | **Wire personalization feedback loop** (old P1.3) | Learns your edits (sign-offs, phrasing) → "it knows how I talk." Biggest paid-for-but-unwired gap (zero production callers today). | Medium | Not wired |
| T2.2 | **Context-aware auto-style** (old P1.2 per-app mode) | Gmail→email polish, Slack→casual, auto. "It formatted my email without me asking." | Low-Med | Partial (context + styles exist; auto-routing missing) |
| T2.3 | **Typed + voice AutoText** | Typed watcher shipped + tested; voice in pipeline. WisprFlow's snippets are weaker. | Done (typed) | Verify end-to-end in app |

### Tier 3 — Cheap, high perceived value (quick wins, do in parallel)

| # | Item | Why it wins | Effort | Status |
|---|---|---|---|---|
| T3.1 | **"Time saved" card on Home** (from W2) | Surface the existing `get_insights` Impact/Milestone on Home. WisprFlow has no such story. | Low | Not started (data exists, UI missing) |
| T3.2 | **Privacy as a front-page claim** (W1) | Onboarding copy + privacy badge. Zero code risk, positions the whole product. | Low | Not started |
| T3.3 | **ITN phase 2b** (old P3.1) | Spoken→written numbers/dates/money; huge perceived-quality win. | Medium | Partial (2a done, 2b deferred) |
| T3.4 | **Emoji restore + language detection** (old P3.2/P3.3) | Polished output feels professional. | Low-Med | Not started |

### Tier 4 - Reliability + parity (the old P2/P3, keep as backlog)
- AX-write paste cascade (P2.1), VAD auto-stop (P2.2), recovery spool (P2.3)
- Apple Intelligence connector (P1.4), model checksums (P1.5)
- ITN 2b, emoji, LID, updater, terminal context (P3 backlog)
- **Jev-class decision layer (P5):** polish gate, context-aware transform routing, voice-command intent classifier. Requires P0 stable (local LLM server). See P5 section + Sprint E.

### Suggested next 3 sprints
1. **Sprint — Speed:** T1.1 (streaming) → T1.2 (live preview) → T1.3 (splitter).
   *Demo: stop talking, watch the text land in ~1s, words assembling in the pill.*
2. **Sprint — It knows you:** T2.1 (feedback loop) → T2.2 (auto-style) → T3.1 (time-saved card).
   *Demo: it uses my sign-off, formats Gmail differently, shows my weekly minutes saved.*
3. **Sprint — Polish + position:** T3.2 (privacy copy) → T3.3 (ITN 2b) → T3.4 (emoji/LID) → Tier 4 reliability.

---
## 0. Non-negotiables (what we protect)

| Keep | Why |
|---|---|
| 13 MB release binary, on-demand model downloads | EW ships 37 MB bundled assets + 417 MB tree before models |
| Zero telemetry by default | EW ships Sentry + PostHog; `prompt.md` mandates local-first |
| Fast dev loop (`cargo tauri dev` + Vite HMR) | EW needs Xcode/Tuist release builds |
| Cross-platform path (macOS + Windows) | EW is AppKit macOS-only |
| Launch does not warm ASR by default | Protects cold start; offer as opt-in setting only if measured bad |
| Single shared ggml / no in-process llama.cpp | Our current link clash; subprocess is the fix |

**Do not adopt:** PostHog/Sentry-on-by-default, bundled 37 MB assets in the base install, Tuist dual build, warm-up-at-launch as default.

---

## EG-1 and S1-mini: model status and download links

### Do we have EG-1 today?

**No.** Teletype has no EG-1 weights and no reference to them. EW also does not bundle the weights; it downloads them at runtime from Envious Labs' CDN via `eg1-manifest.json` + `eg1-delivery-manifest.json`.

| | EG-1 | S1-mini |
|---|---|---|
| Publisher | Envious Labs LLC | Superwhisper (Qwen3-0.6B derivative) |
| Version | `eg1-1.2-c003` (display 1.2) | `1.0` @ `34add00a48a2e5d24e5a4ee5405a99620a3a240c` |
| Base model | Qwen3-4B-Instruct-2507 (Apache-2.0) | Qwen3-0.6B (Apache-2.0) |
| Quant / context | q5km / 16384 tokens | q4km / 8192 tokens |
| Layout | 8 GGUF shards, ~2.69 GB total (`2,889,512,608` bytes) | single GGUF, 462 MB (`484,219,808` bytes) |
| Runtime | `llama.cpp` (`llamacpp-eg1-v1`), EW's bundled `llama-server` | same |
| Prompt template | `eg1-v2` (envelope builder) | `s1-control-line-v1` |
| License | **EG-1 Community Model License 1.0 (restrictive)** | **Apache-2.0 + naming term** ("S1-mini" by "Superwhisper" must be preserved) |
| License URL | https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt | https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE |

Both URLs were verified live (HTTP 200) on 2026-09-22.

### Downloadable polish models (user picks any)

Labels below are taken from EnviousWispr's README ("On-device AI polish" + release notes). All weights are optional and on-demand; Teletype never re-hosts them. Drop files into `~/Library/Application Support/com.teletype.app/models/` (or use in-app Download when wired in P0.5).

**Product stance:** Teletype is and will remain **open source, never commercial**. Users may download and use any of these for personal use. We still must not re-host EG-1 weights (license 2.a) or bundle them inside the `.app`.

| Label (from EW README) | What it is / best for | Runs on | Extra download | Get it |
|---|---|---|---|---|
| **EG-1** (recommended by EW) | Envious Labs' own model, custom fine-tuned for dictation cleanup: spoken lists → real lists, wall-of-speech → paragraphs, keeps only the self-corrected wording. Works macOS 14+. EW benchmark: 93.7% on 1,890 cleanup cases. | On-device, macOS 14+ | ~2.9 GB (8 GGUF shards) | [Download EG-1](https://models.enviouslabs.co/eg1/eg1-1.2-c003/) · [License](https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt) |
| **S1-mini** by Superwhisper | Small open model for dictation cleanup, happiest in English; EW pairs it with Tone / Structure / Context style settings. Free, 484 MB. | On-device, macOS 14+ | ~484 MB (1 file) | [Download (Envious Labs)](https://models.enviouslabs.co/s1/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf) · [Backup (Hugging Face)](https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf) · [License](https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE) |
| **Qwen3 1.7B "Fast"** (Teletype catalog) | Quick local transforms; Polish and short rewrites. | On-device | ~1.4 GB | Already in Teletype Models screen · [Hugging Face](https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q5_k_m.gguf) |
| **Qwen3 4B "Quality"** (Teletype catalog) | Better quality for Professional and Prompt Engineer transforms; same 4B class as EG-1's base (Qwen3-4B-Instruct). | On-device | ~2.6 GB | Already in Teletype Models screen · [Hugging Face](https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/qwen3-4b-q4_k_m.gguf) |
| **Apple Intelligence** | Apple's on-device model, no extra download. | On-device, macOS 26+ (EW); we wire when FoundationModels is available | none | No download; enable in System Settings → Apple Intelligence (roadmap P1.4) |
| **Ollama** | Use a model on your Mac or one hosted by Ollama. | On-device or Ollama servers | varies | [Install Ollama](https://ollama.com) → base URL `http://127.0.0.1:11434/v1` |
| **OpenAI / Gemini / Claude** (via OpenAI-compatible key) | Bring-your-own-key cloud polish, text only. | Cloud (your key) | none | [OpenAI](https://platform.openai.com/api-keys) · [OpenRouter](https://openrouter.ai/keys) · any OpenAI-compatible endpoint (roadmap P0.2) |

#### EG-1 shard files (all 8 required; folder download preferred)

Base directory: `https://models.enviouslabs.co/eg1/eg1-1.2-c003/`

| Shard | File | Size (bytes) | SHA-256 |
|---|---|---|---|
| 1/8 | `eg1-1.2-c003-00001-of-00008.gguf` | 399884640 | `8f05b91acb93ed9c0ef833d617f4cefb492f697c4fe783f6ba26ce4add14e3c0` |
| 2/8 | `eg1-1.2-c003-00002-of-00008.gguf` | 395007008 | `011892fa153cf95f3dc68c049860ad35c752a990f582e4b33c974205df2486fa` |
| 3/8 | `eg1-1.2-c003-00003-of-00008.gguf` | 393972896 | `5769143e7fc28cf4b052237da617cfcbebf2f16bf1500acc4c7ed483fbc18a6f` |
| 4/8 | `eg1-1.2-c003-00004-of-00008.gguf` | 397618336 | `52724cb4812115d993c5f4e4f4ae5e87a44e005fd05b51e14bf427d353dd9d78` |
| 5/8 | `eg1-1.2-c003-00005-of-00008.gguf` | 389508864 | `9b414a065c40e25dea077ae9fe8198ae9c13965cb5df5e997df0580aef452450` |
| 6/8 | `eg1-1.2-c003-00006-of-00008.gguf` | 388606432 | `00ccd4bcf1507db1ca57252ed3465672a3acd1ec49eb450512183bd20e67f25e` |
| 7/8 | `eg1-1.2-c003-00007-of-00008.gguf` | 397168384 | `3a7b065f44ab398f9eb9656aa263f43a9be1acd6af55a0790c7960162675e729` |
| 8/8 | `eg1-1.2-c003-00008-of-00008.gguf` | 127746048 | `91160e7d004150b44a360a7db69237b5405f34d29855c15480324a4abfdf5c0c` |

Individual first shard (browser download button): `https://models.enviouslabs.co/eg1/eg1-1.2-c003/eg1-1.2-c003-00001-of-00008.gguf`
Install name after download (llama-server entrypoint): keep shards together; `-m` points at shard 1.

**S1-mini** single file: `https://models.enviouslabs.co/s1/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf`
SHA-256: `3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634`
Install file name: `s1-mini-1.0.gguf`. Attribution in UI: **S1-mini by Superwhisper**.

### EG-1 license: what this means for Teletype

Read in full from `https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt` (EG-1 Community Model License v1.0, 2026-07-02). Key clauses:

1. **Permitted:** download/use on own devices; personal, educational, research, evaluation use (including benchmarks).
2. **Forbidden without written permission:** (a) re-host/mirror/redistribute the weights; (b) incorporate/bundle/embed/**invoke** the Model in a commercial product or any product that competes with EnviousWispr; (c) distill outputs into another distributed model; (d) remove license/attribution.
3. **Trademarks:** "EG-1", "Envious Engine", "EnviousWispr", "Envious Labs" are trademarks; factual identification only.
4. Permission requests: `hello@enviouslabs.co`.

**Teletype policy (open source forever, never commercial):**

- **Allowed:** link the user to the official Envious Labs URL so *they* download the weights to their machine. Show the license URL and require an explicit "I accept the EG-1 license" checkbox before starting the download. Personal use by the user is fine under clause 1.b; we are not re-hosting.
- **Still forbidden:** mirror any shard on our servers/CDN, or bundle EG-1 inside the `.app` release artifact.
- **No commercial-product conflict:** because Teletype will not be commercial, clause 2.b (commercial product / competitor) does not apply to us; we keep the product non-commercial to stay inside the license.
- Keep SHA-256 verification on every file (digests above).
- Always attribute: EG-1 is by Envious Labs; S1-mini by Superwhisper; base models Qwen3 (Alibaba, Apache-2.0).

**llama-server engine (separate from weights):** EW's `llama-server` is built from `ggml-org/llama.cpp` (MIT), commit `fdb1db877c526ec90f668eca1b858da5dba85560`, static, Metal on, OpenSSL/curl off, ~15 MB. We rebuild our own the same way for macOS arm64 + Windows x64; do not copy their binary (supply chain + platform).

---

## P0 - Ship working Polish (highest user value)

Goal: "Polish this" works end to end with a real model. Unblocks everything the user asked for (Polish, any model, Ollama, OpenAI-compatible key).

### P0.1 Subprocess local LLM server

**Why:** `LlamaProvider::warm_up`/`generate` currently return "runtime not linked" because llama.cpp's ggml collides with whisper.cpp's ggml when linked in-process. EW's answer is architecture, not a link fix: a separate process.

**EW reference:** `Sources/EnviousWisprLLM/EGOne/EGOneServerManager.swift`, `LocalPolishTransport.swift`, `EGOneRuntime.swift`, `LocalPolishServerCoordinator.swift`, `Resources/llama-server-PROVENANCE.md`.

**Build:**

1. New crate or module `teletype-llm-server` (or `teletype-inference::server`) that:
   - Locates a `llama-server` binary (bundled next to the app after first download, or downloaded like models).
   - Spawns `llama-server -m <gguf> --port <p> --host 127.0.0.1 -c <ctx> -fa on --cache-type-k q8_0 --cache-type-v q8_0` with a random bearer token.
   - Health-checks `GET /health` (or first chat call) until ready; expose `ready | starting | failed | paused`.
   - Exposes `POST http://127.0.0.1:<port>/v1/chat/completions` with `Authorization: Bearer <token>`.
2. New `InferenceProvider` impl `ServerProvider` implementing the existing trait in `teletype-core/src/llm.rs` (`model_id`, `model_name`, `generate(prompt, GenerationParams)`).
3. Kill the server on app exit and under memory pressure (macOS: `os_proc_available_memory` / simple RSS threshold; Windows: job object). Never let the LLM starve ASR.
4. On any server failure: silent fallback to raw text (EW's rule). Surface status only in Models screen, never block dictation.
5. Rebuild llama-server with deployment target pinned (macOS 14.0, not host SDK default). Verify `otool -l` / `dumpbin` shows correct minos. `-DGGML_METAL=ON -DLLAMA_OPENSSL=OFF -DLLAMA_CURL=OFF` on macOS.

**Do not** attempt to unify ggml in-process in this phase; subprocess is the stable path.

**Acceptance:** select a local GGUF in Models → warm-up succeeds → Polish transform returns cleaned text → server dies on quit → dictation never blocks on LLM failure.

### P0.2 OpenAI-compatible connector (covers "any model" + Ollama)

**Why:** One connector covers OpenAI, OpenRouter, Groq, Together, DeepSeek, LM Studio, **Ollama** (`http://127.0.0.1:11434/v1`), and any `/v1/chat/completions` endpoint. EW ships separate `OpenAIConnector`, `OllamaConnector`, `GeminiConnector`, `ClaudeConnector`; we collapse most value into one first.

**EW reference:** `OpenAIConnector.swift`, `OllamaConnector.swift`, `OllamaSetupService.swift`, `LLMModelDiscovery.swift`, `LLMNetworkSession.swift`, `LLMRetryPolicy.swift`.

**Build:**

1. `OpenAiCompatProvider` in `teletype-inference`:
   - Config: `base_url`, `api_key` (optional for Ollama/local), `model`, `timeout`, `extra_headers`.
   - Uses `reqwest` (already in tree) against `{base_url}/chat/completions`.
   - Implements `InferenceProvider::generate`.
2. Preset row in Models screen: OpenAI, Ollama (local), OpenRouter, Custom… (custom = user-entered base URL).
3. Ollama niceties (optional same sprint): probe `http://127.0.0.1:11434/api/tags`, list models, guided "install / start / pull" states like `OllamaSetupService` (detecting → notInstalled → installedNotRunning → runningNoModels → pulling(progress) → ready).
4. Model discovery: `GET {base_url}/models` where supported (OpenAI, Ollama) to fill a dropdown; free-text model id always allowed.
5. Retry policy: one retry on 5xx/timeouts within the `GenerationParams::timeout` budget; never infinite.

**Acceptance:** Polish works against (a) local subprocess GGUF, (b) Ollama with `qwen3` pulled, (c) OpenAI with a real key, (d) arbitrary custom base URL.

### P0.3 Secure API key storage

**EW reference:** `KeychainManager.swift` (Keychain generic passwords, service id, lazy migration from legacy files).

**Build:**

1. Add `keyring` crate (macOS Keychain, Windows Credential Manager, Linux Secret Service).
2. Service name: `com.teletype.app`, keys: `openai-api-key`, `custom:<provider-id>`, etc.
3. Never write keys to `settings.json`. Debug builds may use a file store only if needed to avoid ACL prompts, mirroring EW's debug exception, but default to keyring.
4. UI: password input, "Test connection" button, clear-key button.

**Acceptance:** key survives restart, does not appear in `settings.json` or logs, Test connection returns model list or clear error.

### P0.4 Wire Models screen + Polish pipeline

**Current state:** `list_models` / `select_model` / `download_model` exist; `select_model` calls `LlamaProvider` which fails; `MockInferenceProvider` is test-only; UI LLM section is display-only.

**Build:**

1. Provider registry: `local-server` | `openai-compat` | `apple-intelligence` (later) selected in settings (`selected_llm_provider`).
2. `select_model` for local catalog installs the GGUF path and points `ServerProvider` at it; warm-up talks to subprocess.
3. Remote provider: store config in settings + keyring; no large download.
4. Transforms screen: each transform (Polish, Professional, Casual, Rewriter, Prompt Engineer) runs through active provider; show provider name + latency; on failure keep raw text (never drop the dictation).
5. Progress: download shows bytes; server start shows starting→ready.

**Acceptance:** Models screen fully functional; no path reaches `MockInferenceProvider` in production; Polish works from the Transforms UI and from an optional post-dictation toggle.

### P0.5 Model catalog entries (EG-1 + S1-mini links)

**Build:**

1. Extend `teletype-inference/src/catalog.rs` (or a new `polish_catalog.rs`) with:
   - Existing Qwen3 `fast` / `quality` entries (keep).
   - **`s1-mini`**: single-file download from primary + HF backup URL above, size_mb ~462, sha256 above, license Apache-2.0 naming term, attribution string "S1-mini by Superwhisper".
   - **`eg-1`**: multi-file (8 shards) entry, license gate checkbox, deep-link to `https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt`, download only after accept; **no mirror**. Teletype is open source / never commercial, so user-directed download under EG-1 clause 1 is fine; still never re-host or bundle the weights in the `.app`.
2. Multi-file download support in `download_model` (or new `download_polish_model`): sequential/resumable shard download into `models/eg-1/`, verify each SHA-256, only then mark ready. Entrypoint file = shard 1 (llama-server `-m` accepts sharded GGUF when all parts share the directory).
3. Models screen section: "Local polish models" with size, license chip, "Download" / "Open license" link.
4. Docs: README "Models" section lists both download URLs for manual install (drop-in folder support).

**Acceptance:** user can download S1-mini in-app with checksum verify; user can open EG-1 license and download (or be told to download manually) without us re-hosting; bad hash fails the install.

**P0 exit criteria:** Polish works locally and remotely; keys in keyring; catalog shows Qwen3 + S1-mini + EG-1 (license-gated); no ggml clash crash.

---

## P1 - Make Polish quality match EW

### P1.1 Transcript splitting (500-word ceiling)

**EW reference:** `Sources/EnviousWisprPipeline/TranscriptSplitter.swift` (measured: 53% quality on 45-min transcript vs 96% when split; sentence-first via `NLTokenizer`, word fallback; word ceiling 500 + byte ceiling for non-space scripts; parts are verbatim slices).

**Build:**

1. Pure Rust splitter in `teletype-core` (use `unicode-segmentation` for words; sentence split can start with regex on `.!?` + newline, upgrade to a real sentence segmenter later).
2. Apply before any LLM transform; join parts with `\n\n` (or single space for `inline` mode).
3. Unit tests: round-trip non-whitespace equality; run-on sentence fallback; multi-byte scripts.

### P1.2 Polish modes aligned to EW

**EW reference:** `PolishMode.swift`: `inline`, `message`, `structured`, `edit`. Note: EG-1/cloud fixed prompt builders ignore mode (tuned model); mode matters for generic cloud models.

**Teletype today:** transforms `Polish`, `Professional`, `Casual`, `Rewriter`, `Prompt Engineer` + Style profiles (`style.rs`).

**Build:**

1. Map: EW `inline` ≈ our Polish short; `message` ≈ Casual; `structured` ≈ Professional; `edit` ≈ Rewriter. Keep our names (users already see them); document mapping in code.
2. Per-app default mode: reuse `AppType` from `context/mod.rs` + personalization store (Slack → message, Gmail → structured, etc.).
3. Prompt builders per provider family: `local_fixed` (S1/Qwen control-line), `eg1_envelope` (if EG-1 allowed), `cloud_fixed` (OpenAI-compat), `apple_intelligence` (P1.4). Mode is an input; some builders ignore it by design.

### P1.3 Wire personalization feedback loop

**Why:** `extract_signals` / `apply_signals` in `personalization/learn.rs` have **zero production callers**. Biggest paid-for-but-unwired gap. EW: `LearnFromEditsWiring`, `ObservedCorrectionWatcher`, `CoreMLCorrectionJudge` (judge is optional later).

**Build:**

1. After each dictation, if the user edits the inserted text within N seconds (or before next dictation), diff raw vs final and call `extract_signals` → persist via `PersonalizationStore`.
2. Feed signals into polish prompts (custom words, casing, preferred phrasing) and dictionary.
3. Command: `record_dictation_edit` or automatic via history update path in `dictation.rs`.
4. Tests: edit detection, signal extraction, prompt injection.

### P1.4 Apple Intelligence connector (macOS only)

**EW reference:** `AppleIntelligenceConnector.swift` (FoundationModels, 4096-token context preflight, silent skip on unsupported language / context overflow → fall back to raw).

**Build:**

1. `cfg(target_os = "macos")` module using `FoundationModels` (Swift interop or objc2 as available; may need a tiny Swift helper via `swift-bridge` / separate cdylib if Rust bindings are insufficient).
2. Context window preflight (approx tokens); on overflow or unsupported language: silent raw fallback (never error dialog during dictation).
3. Provider priority chain: Apple Intelligence (if available) → local server → openai-compat → raw.
4. Availability probe + Models screen badge "On-device (Apple)".

**Fallback chain is the product:** users with no key still get Polish on Apple Silicon Macs.

### P1.5 Model delivery checksums (all models, not just EG-1)

**EW reference:** `DeliveryManifest` schema-v1: path, sizeBytes, sha256, component, installPath, ordered sources, admission (layout, diskHeadroomFactor 2.2, evictPreviousRevisions), manifestDigest.

**Build:**

1. Add optional `sha256` + `size_bytes` to `SpeechModel` and LLM catalog entries (start with new entries; backfill popular Whisper/Parakeet files from HF etags or recompute).
2. `sha2` crate; verify after download; on mismatch delete + retry once from next source.
3. Atomic install: download to `.part`, verify, rename.
4. Disk headroom check before large downloads (2.2× factor like EW).

**Acceptance:** corrupted download never becomes "ready"; speech model downloads verified where hashes exist.

---

## P2 - Reliability (keep latency, fix silent failures)

### P2.1 AX-write paste cascade (macOS)

**EW reference:** `PasteCascadeExecutor` (AX insert first, paste fallback, caret revalidation), plus measured `ClipboardCleanup` 200 ms restore *outside* the awaited delivery future.

**Build:**

1. `injector.rs`: try `AXUIElementSetAttributeValue` / `AXUIElementSetParameterizedAttributeValue` for focused editable; verify caret/text changed; else clipboard+⌘V path (current behavior).
2. Never block dictation completion on clipboard restore (EW lesson: restore on a timer, change-count guard).
3. Windows: keep/enhance SendInput path; no AX.
4. Tests: mock platform for cascade order; manual matrix for Terminal, VS Code, Slack, Chrome.

### P2.2 Silero VAD auto-stop

**EW reference:** bundled `silero-vad-unified-256ms-v6.0.0.mlmodelc`, `DeadAirStreamingDetector`, dead-air latch tests.

**Build:**

1. Optional model (~1 MB ONNX) via `ort` crate, on-demand download with sha256.
2. Run on frames already captured in `audio/mod.rs`; after N ms below probability threshold → auto-stop + flush to ASR.
3. Settings: VAD on/off, sensitivity, min utterance ms. Default on for hold-to-talk release-and-tail mode only if it does not add perceptible latency (measure: VAD must be << 10 ms per frame).
4. Keep energy-based fallback if model missing.

### P2.3 Escape Recovery spool

**EW reference:** `RecoverySpool`, encrypted, survives crash; recovery text processor falls back to raw.

**Build:**

1. While recording, append PCM to a spool file under app data (`recovery/spool-<id>.pcm`), optionally encrypted (OS keychain-wrapped key or plain with 0600 if encryption is overkill for V1).
2. On clean stop: delete spool. On next launch: if spool exists and non-trivial, offer "recover last dictation".
3. Bound disk use (e.g. max 60 s audio).

### P2.4 Opt-in crash reporting (optional)

Only if users ask: `sentry-rust` behind a setting, default off, scrub personal text (EW's `SentryEventSanitizer` is the reference for what to strip). Never ship PostHog.

---

## P3 - Feature parity (value / effort ordered)

| # | Feature | EW reference | Effort | Notes |
|---|---|---|---|---|
| P3.1 | ITN (spoken→written numbers, dates, money, ordinals) | `InverseTextNormalizer` | M | Pure Rust, huge perceived quality win |
| P3.2 | Emoji restore | `Resources/emoji-dictionary.json` (127 KB) + post-processing | S | Check data license before copying; else ship our own short list |
| P3.3 | Language detection + per-app language | `LanguageDetector`, lock-chips | M | We hardcode `auto`→`en` today; Parakeet already lists 29 languages |
| P3.4 | Recording sound cues | 10 WAV start/stop packs + `RecordingSoundCue` | S | Ship 2-3 neutral cues first; respect Reduce Motion / mute setting |
| P3.5 | Auto-updater | Sparkle 2.9.6 | S | `tauri-plugin-updater` + signed updates; replaces manual rebuilds |
| P3.6 | Terminal + browser address-bar context | `TerminalContextResolver`, `BrowserAddressBarDetector` | M | Improves prompt style + AutoText protection; AX evidence tokens |
| P3.7 | Snippet placeholders `{{date}}` `{{time}}` `{{clipboard}}` | closed-set `SnippetPlaceholder` | S | Closed set only; no code execution |
| P3.8 | Vocabulary packs (tech/legal/medical/brands/names) | `Resources/Packs/*.json` + `VocabularyLanes` compile-time split | M | Pack terms → corrector only, **never** into polish prompt (EW law; we copy the lane split) |
| P3.9 | Custom words import/export | EW custom words + bulk import | S | JSON import/export for migration from other apps |
| P3.10 | Contacts import | `EnviousWisprContacts` | M | Names improve name capitalization; permission prompt |
| P3.11 | Quick Add / typed AutoText polish | QuickAdd, typed watcher | M | We have typed watcher; polish triggers + capitalization rules |
| P3.12 | Other-audio hold (duck Spotify while recording) | media remote adapter | S-M | macOS only; optional |
| P3.13 | Bluetooth / input device awareness | `BluetoothAwarenessOverlay` | M | Show which mic; warn when AirPods connect mid-session |
| P3.14 | Live preview of partial transcript | `EnviousWisprLivePreview` | L | Only after streaming ASR exists |
| P3.15 | Streaming/incremental ASR | EW streaming decode + live preview | L | Architecture change; measure first with hold-to-talk full utterance |
| P3.16 | Speaker diarization | 4 CoreML speaker models, 21 MB | L | Defer; niche for solo dictation |
| P3.17 | File transcription | audio file decode path | M | Drag-drop audio → history entry |
| P3.18 | Dark mode / theme | `AppearanceController` | S | CSS variables; system + light + dark |
| P3.19 | Reduce Motion | accessibility | S | Honor `prefers-reduced-motion` |
| P3.20 | Practice dictation in onboarding | EW onboarding | S | Mic check + one sample phrase |
| P3.21 | What's New on update | EW whats-new | S | Small modal after updater lands |
| P3.22 | Output classifier / edit judge | `OutputClassifier.mlpackage`, `CoreMLCorrectionJudge` | L | Defer until personalization loop is live; needs CoreML or ONNX port |
| P3.23 | Third-party notices file | EW `THIRD-PARTY-NOTICES.txt` + CI sync check | S | Required once we ship llama-server + packs |
| P3.24 | CI: build + test + macOS/Windows | EW PR check, macos-14 launch assert | M | GitHub Actions: `cargo test`, `tsc`, release build; add Windows runner when Windows path matures |

---

## P4 - Process and quality (parallel track)

1. **Commit the dirty tree** first (usage tracking, System AutoText, Home polish, icons) and delete stray root files (`aaaaaa.pdf`, `*.mhtml`, `*.jpeg` screenshots) or move to `docs/assets/`.
2. **Performance harness** (protect our edge): cold launch ms, time-to-first-dictation (cold/warm), RSS at idle and during record, polish p50/p95 latency per provider. Store numbers in `docs/benchmarks.md`; fail CI on large regressions later.
3. **Test depth:** add EW-style contract tests for critical seams (pipeline order, injector cascade, VAD thresholds, server crash fallback). Not 768 files; target the failure modes EW named (abandoned decode, stop fences, empty results).
4. **Windows validation:** manual matrix for global shortcut, mic, inject, tray (currently untested).
5. **Manual mic verification** (from `checkpoint.md`): pill phases, language, scratchpad routing.
6. **License hygiene:** EG-1 gate, S1-mini attribution, llama.cpp MIT notice, emoji/pack data licenses, `THIRD-PARTY-NOTICES.txt`.

---
## P5 - Jev-class decision layer (fast typed decisions on the existing LLM)

**Research:** 2026-09-26. The "Jev" ecosystem (TypeSafe's closed System One model + open clones: Kev, Laya, SemIf, jev-rs) answers *typed questions* (Choice / Score / Noul) against a text state by reading **next-token logprobs** in one prefill — zero output tokens, ~75 ms p50 warm. It is a *decision* primitive, not a text generator.

**Why Teletype could use it:** the pipeline is fully deterministic today (rules pick the transform, the style, whether to polish). A decision layer adds a probabilistic judgment at the seams where a rule is too brittle, **without a second LLM call and without a new process** — it reuses the `llama-server` subprocess already spawned for Polish (D002). No new runtime, no new model, no new network path, no new API key.

**Reference implementation:** [`jev-rs`](https://github.com/yijunyu/jev-rs) (Apache-2.0/MIT, Rust). Reads logprobs from any `llama-server` GGUF, wire-compatible `POST /v1/systemone`, exposes `ask` / `serve` / `mcp` / `eval` / `calibrate`. The scorer is ~200 lines and can be vendored into `teletype-inference` rather than shelling out to the CLI. The hosted-`jev-sdk` crate is **not** a fit (requires network + TypeSafe API key, violates D003).

**Accuracy reality (zero-shot, independent + project benchmarks):**

| System | Macro accuracy (out-of-domain) |
|---|---|
| TypeSafe Jev (hosted, closed) | 0.966 |
| Best open (Von, 395M) | 0.704 |
| jev-rs + Qwen3-4B (choice / noul / score) | 0.92 / 0.64 / 0.56 |
| Laya (421M) | 0.583 |

The open class tops out near **0.70 zero-shot**. This is fine for *gating* (skip the expensive path when confident) and *routing* (pick the most likely of N), not for *replacing* a deterministic rule. Every use case below falls back to the existing rule when confidence < threshold.

### P5.1 Polish gate (highest ROI, smallest surface)

**Problem:** when a local LLM is selected, every dictation is a candidate for the polish path. Short, clean utterances ("ok", "yes", "the file is in downloads") don't need a 500 ms–3 s generation pass.

**Build:**
1. New `DecisionProvider` in `teletype-inference` (or a module in `teletype-core` if the scorer is vendored): `decide(state, questions) -> DecisionResult` where `DecisionResult` carries `choice` / `noul` / `score` + per-option probabilities + confidence.
2. One Noul question: `"Does this text need polishing (punctuation, casing, structure)?"` with criteria `yes: "Has filler words, run-on sentences, missing punctuation, or spoken-list narration"`, `no: "Already clean, short, or a single complete sentence"`.
3. In `pipeline.rs`, before the transform step: if a local LLM is active and the decision returns `noul < 0.4`, skip the LLM transform and insert raw (AutoText still expands). Log `transform_skipped_decision`.
4. Threshold is a setting (`polish_gate_threshold`, default 0.4). Off by default until measured.

**Acceptance:** short clean utterances skip the LLM (verified by `transform_skipped_decision` log + wall-clock), long messy ones still polish. No regression in the 246-test suite. Latency: +~75 ms for the decision, −500 ms to −3 s for the skipped generation. Net win on the common short-utterance case.

### P5.2 Context-aware transform selection (per-utterance, not per-app)

**Problem:** T2.2 (auto-style) routes by *app* (Gmail → email, Slack → casual). The same app can host both registers ("quick note to Sam" vs "formal reply to the client").

**Build:**
1. One Choice question over the existing transform set: `"Which register best matches this text?"` with criteria = the active transform names + descriptions from `transforms/`.
2. In `pipeline.rs`, after context detection but before the transform: if the decision confidence ≥ 0.7, override the app-default transform with the chosen one. Otherwise keep the app default.
3. Must respect the user's explicit transform selection (explicit > decision > app-default > global-default).

**Acceptance:** "hey sam the deploy is busted" in Gmail routes to Casual, not Email-formal. Explicit user selection always wins. Falls back to app-default when confidence < 0.7.

### P5.3 Voice-command intent classifier (product expansion, largest scope)

**Problem:** Teletype is a *dictation* tool. The natural next product is *voice control*: "open the jira ticket for the login bug" should trigger an action, not insert text.

**Build (the decision half only; action handlers are a separate P5.4):**
1. One Choice question: `"What is the user asking for?"` with criteria `dictation: "Transcribe and insert this text"`, `app_action: "Perform an action in another app (open, send, create)"`, `search: "Search for something"`, `timer: "Set a reminder or timer"`.
2. If `app_action` ≥ 0.8, emit a new pipeline event `IntentDetected { kind, confidence }` instead of `Inserting`. The dictation controller routes to an `ActionHandler` trait (new) instead of `TextInjector`.
3. **No action handlers in this phase.** `ActionHandler` is a trait with a `MockActionHandler` for tests and a `LogActionHandler` (logs the intent, inserts a "[action: …]" placeholder) so the decision path is testable end-to-end before any real automation exists.

**Acceptance:** "open chrome" does NOT insert text; it emits `IntentDetected { kind: AppAction, confidence: 0.9+ }`. "the file is in downloads" still inserts text. The decision is logged with probabilities for inspection.

### P5.4 Action handlers (separate, larger build — not scoped here)

App automation (AX / UIA), search, timers, Jira/Slack integrations. Each is its own work item. P5.3 is the prerequisite: it produces the typed intent that P5.4 handlers consume. **Do not start P5.4 before P5.3 is verified.**

### P5.5 Transcript tagging for insights (low priority)

**Build:** after transcription, one Score (`"Emotional tone"`, ["neutral", "frustrated", "urgent"]) + one Choice (`"Topic"`, ["work", "personal", "shopping", "health"]) per transcript, stored in `history.json`. Feeds the existing `insights` / `stats` screens. Off by default.

### Constraints and non-negotiables for P5

- **No new process.** The decision layer queries the *existing* `llama-server` subprocess. If no local LLM is selected, the decision layer is inert (no model to read logprobs from). Never spawn a second model for this.
- **No new network path.** Loopback only. The OpenAI-compat path *can* work via `top_logprobs` but is less exact (server applies its own chat template); the raw llama-server path is preferred. Hosted APIs (OpenAI, Anthropic) do **not** expose per-label logprobs and are **not** supported backends.
- **RAM:** 0 MB added when a local LLM is already loaded (the common case for Polish users). +0.5–2.5 GB *only* if a user has no local LLM and we load one solely for decisions — **do not do this**.
- **Binary size:** vendoring the jev-rs scorer adds < 100 KB. Well inside the 13 MB budget.
- **Fallback:** every decision falls back to the existing deterministic rule when confidence < threshold. The pipeline never blocks on a decision failure (same contract as the existing "on any transform failure, fall back to original text").
- **License:** jev-rs is Apache-2.0/MIT. If vendoring, keep the attribution + license file (P3.23 `THIRD-PARTY-NOTICES.txt`).
- **Windows:** no additional cost. llama-server has a Windows build; jev-rs is cross-platform Rust. Same RAM/latency math.

**P5 exit criteria:** P5.1 (polish gate) is the shippable unit. P5.2 is a quality improvement. P5.3 is the product expansion and requires an explicit product decision (voice commands vs. dictation tool) before P5.4.

---


## Suggested execution order

### Sprint A - Polish MVP (P0)
1. Commit current batch + tidy repo root.
2. Build/obtain `llama-server` (macOS arm64 first) + subprocess manager + `ServerProvider`.
3. `OpenAiCompatProvider` + presets (OpenAI, Ollama, Custom) + `keyring`.
4. Wire Models screen + transform pipeline + failure fallback.
5. Catalog: S1-mini (checksummed) + EG-1 (license gate, multi-shard, hashes above).
6. Docs: how to point at Ollama; EG-1 manual download link + license.

**Demo:** Polish a sentence locally with S1-mini and remotely with an OpenAI key.

### Sprint B - Quality (P1)
1. Transcript splitter + mode mapping + per-app default mode.
2. Personalization feedback loop wiring.
3. Apple Intelligence connector + provider chain.
4. Checksums for speech models.

### Sprint C - Reliability (P2)
1. AX-write cascade + clipboard hygiene.
2. VAD auto-stop (measure latency impact).
3. Recovery spool.
4. Performance harness baseline.

### Sprint D+ - Pick from P3 by user feedback
Start with ITN, emoji, LID, updater, terminal context.

### Sprint E - Decision layer (P5, after P0 is stable)
1. P5.1 Polish gate (ship first; measurable latency win on short utterances).
2. P5.2 Context-aware transform selection (quality improvement; needs P5.1's `DecisionProvider`).
3. P5.3 Voice-command intent classifier (product decision required before starting; see Open decisions #6).
4. P5.4 Action handlers (only after P5.3 verified; each handler is its own work item).
5. P5.5 Transcript tagging (low priority; can land anytime after P5.1).

**Prerequisite:** P0 (local LLM server) must be stable. The decision layer is inert without a local model.
**Demo:** speak a short clean phrase → it inserts raw in <200 ms (no polish pass). Speak a messy run-on → it polishes. The pill shows which path was taken.

---

## Feature → EW source map (quick index)

| Teletype work item | EW file(s) |
|---|---|
| Subprocess LLM | `EGOne/EGOneServerManager.swift`, `LocalPolishTransport.swift`, `EGOneRuntime.swift`, `Resources/llama-server-PROVENANCE.md` |
| EG-1 download | `Resources/eg1-manifest.json`, `Resources/eg1-delivery-manifest.json` |
| S1-mini download | `Resources/s1-manifest.json`, `Resources/s1-delivery-manifest.json` |
| OpenAI / Ollama | `OpenAIConnector.swift`, `OllamaConnector.swift`, `OllamaSetupService.swift`, `LLMModelDiscovery.swift` |
| Keys | `KeychainManager.swift` |
| Apple Intelligence | `AppleIntelligenceConnector.swift` |
| Polish modes | `Core/PolishMode.swift`, `Prompting/*.swift` |
| Splitter | `Pipeline/TranscriptSplitter.swift` |
| Personalization | `LearnFromEditsWiring`, `ObservedCorrectionWatcher` (search under Pipeline/Services) |
| Paste cascade | `Pipeline/PasteCascadeExecutor.swift`, `Pipeline/ClipboardCleanup.swift` |
| VAD | `Audio/BundledVADModelLoader.swift`, `DeadAirStreamingDetector` |
| Recovery | `RecoverySpool` (search Pipeline/Storage) |
| Delivery integrity | `ModelDelivery/DeliveryManifest.swift` |
| LID | `ASR/LanguageDetector.swift` |
| ITN | `PostProcessing/InverseTextNormalizer.swift` |
| Emoji | `PostProcessing/Resources/emoji-dictionary.json` |
| Packs | `PostProcessing/Resources/Packs/*.json`, `Core/VocabularyLanes.swift` |
| Terminal/browser | `Services/TerminalContextResolver.swift`, `BrowserAddressBarDetector.swift` |
| Diarization | `Pipeline/SpeakerLabeler.swift`, `Resources/SpeakerModels/*` |

---

## Open decisions (need product call)

1. ~~Is Teletype commercial?~~ **Resolved: open source forever, never commercial.** EG-1 user-directed downloads + license checkbox are fine; still never re-host or bundle EG-1 weights.
2. **llama-server distribution:** download on first local-model use vs optional installer. Never in the base 13 MB binary.
3. **Apple Intelligence vs local priority:** default chain order and UI copy when both exist.
4. **Streaming ASR:** measure first; only commit to P3.15 if hold-to-talk full-utterance latency is actually a complaint.
5. **Updater signing:** Apple notarization + Tauri updater pubkey before P3.5 ships.
6. **Voice commands vs. dictation tool (P5.3/P5.4):** Does Teletype stay a pure dictation tool, or expand to voice control ("open the jira ticket" → action, not text)? P5.3 (intent classifier) is the decision half and is low-risk; P5.4 (action handlers: AX automation, search, timers, integrations) is a large separate build. **Do not start P5.4 without this product call.** P5.1 (polish gate) and P5.2 (transform routing) are safe to build regardless of this answer.

---

## Deferred follow-ups (left out of the current build wave, 2026-09-25)

Work deliberately not included in the current commits; each entry verified against git and the current tree.

- **Language-drift detection (P1-17):** Commit `9163631`'s message claims "Validator: expanded failure taxonomy (language drift, truncation)", but the diff only adds the `Truncated` variant — no `LanguageDrift` variant or language check exists in `crates/teletype-core/src/transforms/validator.rs` (confirmed by reading the commit diff and current code; the file now carries an in-code NOTE documenting the discrepancy). Implement per findings.md P1-17 (linguist-based input/output language compare, fail-open threshold).
- **Timeout wrapping + Cancelled-token polish (T7, in flight):** The Week 3 "Polish failure taxonomy + user-visible status" task (task `01a0d8b9-0849-7b31-9913-0e131d3f94d1`) deferred wrapping the transform in a user-cancel-aware timeout and polishing the Cancelled-token path. Current state: `ServerProvider` wraps generation in `tokio::time::timeout` with the scaled budget (`crates/teletype-inference/src/server.rs`), but `dictation.rs` `CancelPipeline` handling is coarse (drops the result, no explicit token-cancellation plumbing) — finishing this is part of the in-flight T7 work.
- **P1-19 per-manifest checksum verification:** `8765f05` (download resume + disk-space probe) added `Range: bytes=N-` resume with prefix re-hashing and a 2.2× disk probe, and `download.rs` verifies SHA-256 *when a hash is present* on the catalog entry — but full per-manifest (DeliveryManifest-style, findings.md P1-19 / roadmap P1.5) checksum coverage for all speech-model entries remains unverified/backfilled.
- **P2-28 Apple Intelligence provider:** `9163631` added the `tauri-apple-intelligence = "0.2.1"` dependency to `crates/teletype-desktop/Cargo.toml`, but no Rust code references it (grep of `crates/` finds zero usages) — the provider (P1.4) is not wired; rehydration in `commands.rs` only handles `local-server` / `openai-compat`.
- **`InsightsScreen.tsx` tsc errors (expected transient):** `npx tsc --noEmit` in `ui/` currently reports 2 errors (`polishStatus` on `Insights`, lines 444/459). These are omp2's in-flight T7 (failure taxonomy + user-visible status) working-tree edits, not a committed regression — expected to resolve at T7's commit.
- **Worktree-branch audit verdict:** No stranded work across the 16 `worktree-*` branches. All work is on main. Optional low-priority cherry-picks (not blockers): `1658caf` edit_distance/closest_model, `92d167b` SilenceStopper, `0e8b94e` cancelled_stop test.
- **ITN phase 2b (deferred):** Compound ordinals ('seventy second' -> 72nd), 'and' inside cardinals ('five thousand and eighty five' -> 5,085), spoken punctuation (applyPunct equivalent: 'hello comma world period' -> 'Hello, World.'), money-before-cardinals ordering. Parity currently 2071/2416 (85.7%); `itn_parity_fixtures` is intentionally `#[ignore]` until 2b lands.

---

*Generated 2026-09-22 from EnviousWispr source analysis + Teletype tree audit. EG-1/S1 URLs and hashes verified live that day. Deferred follow-ups section added 2026-09-25 (git-verified).*
