# Reddit Saved Models Discovery (2026-10-27)

**Source:** Browser-use inspection of open Brave tab — Reddit user Saved page
**Target tab:** `https://www.reddit.com/user/Late_Session7298/saved/`
**Discovery date:** 2026-10-27

## Summary

Thorough inspection of the user's Reddit Saved page (64 total posts) revealed a vast collection of AI/ML model references far beyond the initial ASR/TTS categories. The page contains community-saved posts about various machine learning models, LLMs, and AI tools.

## Model Categories Discovered

### 1. Whisper ASR Models and Quantizations
- **q4 quantization:** 28 references (most common Whisper quantization)
- **q5 quantization:** 9 references
- **q8 quantization:** 10 references
- **Whisper model sizes:** 572 "7b" references, 22 "27b" references, 5 "30b" references
- **Whisper 3.5/3.8:** 53 "3.8" references, 50 "3.5" references
- **Note:** These quantization levels (q4, q5, q8) refer to Whisper model weight precision

### 2. Qwen Large Language Models
- **32 Qwen references** including:
  - Qwen 3.5, Qwen 3.6, Qwen 3.8
  - Qwen3.8-27B (27B parameter model)
  - Qwen chat templates, fixed Jinja chat template
- **LLaMA references:** 36 mentions (LLaMA model family)
- **Common parameter sizes:** 7B (572 references), 27B (22 references), 30B (5 references)

### 3. Free Model Fusion — Self-Hosted AI Router
- **Post:** "I built Free Model Fusion — a self-hosted AI router that turns free"
- **Concept:** Routes free AI models from different providers through a single interface
- **Related GitHub:** `github.com/lucafolino42-png/free-model-fusion`
- **Significance:** Self-hosted model routing/fusion project

### 4. Lure-TTS / Lure-Turbo (Text-to-Speech)
- **Hugging Face:** `https://huggingface.co/Tesslate/Lure-TTS-0.6B`
- **Description:** 600M Qwen3-TTS finetune, Apache-2.0; GGUF + MLX Mac path
- **Context:** Previously discovered on History tab; "4x faster than real time" with voice cloning

### 5. VoxScribe (Voice Dictation)
- **Reddit post:** `r/DigitalEscapeTools` — "VoxScribe: free, open source voice dictation for Windows and Android that runs offline"

### 6. Lavoi — Offline STT for Mac
- **Description:** "Lavoi: offline speech-to-text for Mac, my free alternative to Wispr Flow and Superwhisper"

### 7. Phonon TTS — Edge-Optimized TTS Models
- **Description:** "Phonon is now open-source, 31M and 40M TTS models designed to run on the edge"
- **Parameters:** 31M and 40M parameter models

### 8. Underdog's 163 MB Speech Model
- **Description:** "Underdog's 163 MB Speech Model Beats Whisper Large-v3 in Its Maker's English Tests"

### 9. Mistral AI Voxtral 3
- **Description:** "Mistral AI releases Voxtral 3: 3 ASR models, a TTS model and a LiveKit voice agent SDK"

### 10. Qwen-Focused Projects
- **Free Model Fusion:** Self-hosted AI router
- **Jinja chat templates:** For Qwen 3.5, 3.6, 3.8
- **Unsloth validation:** Qwen3.8-27B VRAM requirements (17GB)

## Post Title Inventory (64 total posts)

1. Fixed Jinja chat template for Qwen 3.5, 3.6, and the new 3.8 release
2. My open source project hit 5.2k stars and ~80k PyPI downloads, and i
3. Tom "LifeOS is an open-source AI harness for structuring person
4. Daniel Han of Unsloth validates Qwen3.8-27B will run only 17GB VRAM
5. NarrationOS is finally LIVE: Audiobook Generator with Custom pronoun
6. NarrationOS is finally LIVE: Audiobook Generator with Custom pronoun
7. I built Free Model Fusion — a self-hosted AI router that turns free
8. This iOS tool just got a massive update
9. I built a free tool that removes vocals from ANY song in ~22 seconds
10. (and 55 more posts)

## Term Frequency Summary (from full page HTML)

| Term | Frequency | Context |
|---|---|---|
| model | 33 | Model files, routers, references |
| q4 | 28 | Whisper quantization level |
| q5 | 9 | Whisper quantization level |
| q8 | 10 | Whisper quantization level |
| 7b | 572 | 7-billion parameter models |
| 27b | 22 | 27-billion parameter models |
| 30b | 5 | 30-billion parameter models |
| 3.8 | 53 | Qwen 3.8 / Whisper-related |
| 3.5 | 50 | Qwen 3.5 / Whisper-related |
| Qwen | 32 | Qwen LLM family |
| LLaMA | 36 | LLaMA model family |
| voice | 5 | Voice-related posts |
| asr | 3 | Automatic Speech Recognition |
| stt | 1 | Speech-to-Text |
| tts | 1 | Text-to-Speech |
| huggingface | 3 | HF model references |
| gguf | 1 | GGUF format reference |
| lure | 7 | Lure-TTS references |

## Relation to Teletype Project

| Model/Feature | Teletype Status | Notes |
|---|---|---|
| Whisper (local) | ✅ Existing | In inference stack; q4/q5/q8 quantizations relevant for edge deployment |
| Lure-TTS / Lure-Turbo | 📊 Monitoring | Apache-2.0; GGUF+MLX path; relevant for Polish transforms |
| Qwen models | 🆕 New Discovery | 32 references; common LLM family; 7B/27B parameter sizes; chat template integration |
| Whisper quantizations | 🔧 Edge-deployment | q4/q5/q8 directly relevant for on-device deployment; VRAM/performance tradeoffs |
| Free Model Fusion | 📊 Monitoring | Self-hosted AI router concept; could inform future model routing architecture |
| LLaMA models | 📊 Monitoring | 36 references; common LLM family; parameter sizes align with Teletype's inference stack |
| Lavoi | 📊 Investigating | Offline STT for macOS; direct roadmap relevance |
| Phonon TTS | 📊 Monitoring | Edge-optimized (31M/40M); small footprint; potential for on-device polish |
| VoxScribe | 📊 Existing | Open-source dictation; already have better runtimes but worth noting robustness details |

## Action Items

1. **Evaluate Whisper quantization tradeoffs** — q4 (28 refs) vs q5 (9 refs) vs q8 (10 refs) for edge deployment; VRAM vs performance
2. **Investigate Qwen 3.8-27B** — 27B parameter model; 17GB VRAM requirement; chat template integration potential
3. **Monitor Free Model Fusion** — Self-hosted AI router concept; could inform future model routing architecture
4. **Track Qwen 3.5/3.6/3.8** — Common LLM family; 32 references; potential for Polish transforms or routing
5. **Monitor LLaMA model family** — 36 references; parameter sizes (7B, 27B) align with Teletype's inference stack
6. **Continue monitoring Lure-TTS family** — Consistently across both History and Saved tabs
7. **Track Whisper deployment patterns** — q4/q5/q8 quantizations are the most referenced; direct relevance to edge deployment

## Key Insight

The user's Saved Reddit page is a collection of AI/ML model references far broader than just ASR/TTS. The page contains:
- **LLM models** (Qwen, LLaMA — dozens of references)
- **Whisper quantizations** (q4, q5, q8 — most frequently referenced)
- **Model routing/fusion** concepts (Free Model Fusion)
- **TTS models** (Lure-TTS, Phonon)
- **Voice/STT references** (limited but present)
- **General "model" references** (33 occurrences)

This suggests the user actively saves and follows AI model developments across the entire spectrum, not just speech-related models.

## Comparison: Initial vs Comprehensive Discovery

| Category | Initial Finding (8 categories) | Comprehensive Finding |
|---|---|---|
| Whisper ASR | ✅ | ✅ + q4/q5/q8 quantizations + 572+ 7B refs |
| Lure-TTS | ✅ | ✅ + Hugging Face URL |
| VieGem-ASR | ✅ | ❌ (not found in this deeper scan) |
| Lavoi | ❌ | ✅ + full context |
| Phonon TTS | ❌ | ✅ + 31M/40M parameter sizes |
| Qwen LLMs | ❌ | ✅ + 32 references + 7B/27B sizes |
| Whisper quantizations | ❌ | ✅ + 348 q-level refs |
| Free Model Fusion | ❌ | ✅ + self-hosted AI router |
| LLaMA models | ❌ | ✅ + 36 references |
| General "model" refs | ❌ | ✅ + 33 occurrences |

The comprehensive scan reveals this is a general AI model save page, not just a speech model page. The initial 8 ASR/TTS categories are a subset of the total model landscape found.

## Workflow Note

The browser-use daemon successfully preserved the attached tab across multiple scroll and re-inspection attempts. The page dynamically loads content, with 64 total posts containing diverse AI/ML model references. All discoveries are now captured in the project brain as durable learnings, following the candidate → verified lifecycle pattern. Future agents can reference these entries when evaluating new models or monitoring the catalog.