# Reddit History Voice Models Discovery (2026-10-27)

**Source:** Browser-use inspection of open Brave tab — Reddit user history page
**Target tab:** `https://www.reddit.com/user/Late_Session7298/history/`
**Discovery date:** 2026-10-27

## Summary

Inspecting the user's open Brave browser tab (Reddit history) revealed multiple references to ASR, STT, TTS, and voice dictation models across Reddit comments and posts. The page contained 8,274 accessibility tree nodes and 2,950 non-ignored elements.

## Models and References Found

### 1. Lure-TTS and Lure-Turbo (Text-to-Speech)
- **Hugging Face:** `https://huggingface.co/Tesslate/Lure-TTS-0.6B`
- **Description:** 600M Qwen3-TTS finetune, Apache-2.0 code and weights, GGUF + MLX Mac path
- **Use case:** Text-to-Speech synthesis, voice cloning
- **Context:** Found in Reddit post `r/LocalLLaMA` with title "Lure-TTS and Lure-Turbo: 600M voice models to run 4x faster than real time, with inline (tags), direction, and voice cloning. Trained for under $200."
- **Occurrences:** 16 references for "TTS", 7 references for "voice model"

### 2. VoxScribe (Voice Dictation)
- **Reddit post:** `r/DigitalEscapeTools` — "VoxScribe: free, open source voice dictation for Windows and Android that runs offline"
- **Post ID:** `1xbpzyy`
- **Post date:** 2026-10-21
- **Description:** Free, open source voice dictation for Windows and Android that runs offline
- **Context:** Found in user's Reddit history with 7 non-ignored AX node references
- **Related:** Also referenced `voxscribe-ai-voice-dictation` in candidate scout list (rejected — we already have better runtimes)

### 3. OpenAI Whisper (ASR)
- **Mentioned in:** Reddit comment discussing speech recognition
- **Description:** "the speech recognition is OpenAI's Whisper model with open weights, run locally with faster-whisper on Windows and sherpa-onnx on Android"
- **Context:** Found in 1 reference for "speech recognition" — discusses local Whisper implementation with faster-whisper (Windows) and sherpa-onnx (Android)
- **Significance:** Confirms local Whisper usage pattern matching Teletype's existing inference stack

### 4. ASR-related terminology
- **"AsR"** (Automatic Speech Recognition): Found 6 times in faceplate loader scripts (Reddit UI class names)
- **"STT"** (Speech-to-Text): Found 1 time in server timing metrics
- **"dictation"**: Found 12 times — primarily referring to Reddit community posts about voice dictation tools
- **"voice model"**: Found 7 times — primarily the Lure-TTS model references

## Technical Notes

- The Reddit page is heavily dynamic (React-based), so the AX tree reveals the underlying data model
- Model names and Hugging Face URLs were extractable from the page's HTML/JS rendered content
- The Whisper reference confirms the local-ONNX/faster-whisper deployment pattern already in Teletype's roadmap
- Lure-TTS represents a new TTS model family worth monitoring for potential Polish/transformation use

## Relation to Teletype Project

| Model/Feature | Teletype Status | Notes |
|---|---|---|
| Whisper (local) | ✅ Existing | Already in inference stack (faster-whisper on Windows, sherpa-onnx on Android) |
| Lure-TTS / Lure-Turbo | 📊 Monitoring | New TTS model; Apache-2.0 license; GGUF+MLX path; could be relevant for Polish transforms |
| VoxScribe | 📊 Monitoring | Open-source dictation; already have better runties (Ultra, sherpa); worth copying robustness details |
| ASR models generally | 📊 In catalog | See `asrs.md` for full catalog; this Reddit scan adds external discovery data |

## Action Items

1. **Monitor Lure-TTS family** — Apache-2.0 licensed, GGUF+MLX paths available; worth adding to catalog watch list
2. **Note VoxScribe robustness details** — capture-then-resample, single-instance guard, per-key typing delay (3 details mentioned as "worth copying")
3. **Track Whisper deployment patterns** — confirm local faster-whisper + sherpa-onnx remains the preferred path
4. **Add to brain learnings** — this entry captures durable discoveries from browser inspection