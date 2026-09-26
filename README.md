# Teletype

**Talk instead of type. Private, fast, and entirely on your machine.**

Teletype is a cross-platform voice dictation app. Hold a key, say what you mean,
and let go. Your words are transcribed on-device, tidied up, and typed straight
into whatever app your cursor is in. No cloud, no account, no subscription.

---

## The problem

Typing is the slowest way most of us get ideas out of our heads and into a
document, an email, or a chat. The average person types around 40 words a
minute, but speaks at 130 to 160. That gap is real time, lost every single day
across the dozens of times a person reaches for the keyboard.

The existing voice-typing tools close that gap, but they do it by shipping your
audio to a company's servers. That creates three problems:

1. **Privacy.** Everything you say is recorded and transmitted. For legal,
   medical, financial, or personal writing, that is a dealbreaker.
2. **Cost.** The private, high-quality tools are monthly subscriptions.
3. **Dependency.** They need the internet, so they fail in the airplane, the
   subway, or the offline office.

The core tension: the best dictation is fast and private, and fast and private
has historically meant either paying a subscription or trusting a vendor with
your words.

## The solution

Teletype runs the entire speech-to-text pipeline **locally**. It bundles two
open, on-device speech engines (NVIDIA Parakeet TDT and OpenAI Whisper) that
run on your own CPU and GPU. Your microphone audio is processed in memory and
never written to a server. The only network calls the app makes are one-time
model downloads, which you can skip entirely by staying offline after setup.

On top of raw transcription, Teletype adds the finishing touches that make
spoken text feel like written text:

- **Automatic cleanup** - capitalization, punctuation, and filler-word removal
  ("um", "uh") run deterministically on every dictation.
- **AutoText snippets** - speak a short phrase and have it expand to a full
  sentence or address, or type a shortcut that expands as you go.
- **Text transforms** - sentence case, title case, lower/upper case, strip
  markdown, and trim whitespace, applied automatically or on demand.
- **A personal dictionary** - teach Teletype names and terms it mishears, once.
- **Insights** - a local dashboard of words dictated, speaking speed, streaks,
  and the time you've saved versus typing.
- **A plain-text archive** - every dictation is also written to a day-wise
  folder of `.md` files you can browse, search, and back up anywhere.

## The impact

The math is simple. If Teletype transcribes at your speaking rate (~150 wpm)
and typing would have taken 40 wpm, every 1,000 words you dictate saves roughly
21 minutes of typing. The Insights screen tracks this for you in real time, so
the time saved is not a marketing number - it is measured from your own usage.

Beyond speed, the impact is control:

- Your recordings and transcripts stay in a folder **you** choose.
- There is no word limit, no trial, and no account.
- It works with no internet after the one-time model download.

## How to use

1. **Install and grant permissions.** On first run the onboarding wizard asks
   for Microphone and Accessibility access (Accessibility is what lets Teletype
   type into other apps).
2. **Pick a dictation model.** Go to **Models** and download one. Parakeet TDT
   v3 (~640 MB) is recommended for speed and accuracy. Whisper models range
   from Tiny (75 MB) to Large v3 Turbo (~1.6 GB). Downloads are local and
   one-time.
3. **Hold the hotkey and talk.**
   - macOS: `⌘⇧Space`
   - Windows: `Ctrl+Shift+Space`

   Release the key to transcribe and type. The floating pill shows the live
   state: listening, transcribing, and done. Change the hotkey in **Settings**.
4. **Shape your output.** Add AutoText snippets, transforms, and dictionary
   words in their respective screens. Everything is stored locally.
5. **Review and archive.** The **History** screen lists recent dictations
   (copy or delete any entry). The full day-wise text archive lives in the
   folder shown under **Settings → Transcripts**; click "Open folder" to browse
   it in Finder or Explorer.

## Requirements

- **macOS 14+** (Apple Silicon or Intel) - fully supported
- **Windows 10/11** - platform layer in progress (see [Status](#status))
- **Rust 1.88+**
- **Node.js 18+** (for the UI build)
- **CMake** and a C/C++ toolchain (`xcode-select --install` on macOS) - used to
  build the speech engines

## Build and run

```bash
# 1. Install the Tauri CLI (if you haven't already)
cargo install tauri-cli --version "^2"

# 2. Build the UI
cd ui && npm install && npm run build && cd ..

# 3. Run the app (first build compiles whisper.cpp + parakeet, a few minutes)
cargo tauri dev
```

To build a distributable bundle:

```bash
cargo tauri build
```

> The first `cargo build` downloads and compiles whisper.cpp v1.9.4 (the
> Parakeet engine). This is slow once; subsequent builds are incremental.

## Architecture

A Rust-first workspace with a Tauri 2 + React frontend. All inference is local.

```
crates/
  teletype-core        Pure Rust: pipeline, text transforms, auto-text,
                       shortcuts, settings, audio capture, platform traits,
                       insights, and the day-wise transcript writer
  teletype-speech      Local STT providers (Parakeet + Whisper) and model catalog
  parakeet-sys         Builds whisper.cpp v1.9.4 (parakeet + ggml) via CMake
  teletype-inference   LLM model catalog + GGUF provider slot + mock provider
  teletype-desktop     Tauri app: commands, dictation controller, platform impls
ui/                    React screens (Home, History, Insights, Transforms,
                       AutoText, Dictionary, Personalization, Models, Settings,
                       Onboarding) + the floating progress pill
```

**Dictation flow:** hotkey → record mic → transcribe (Parakeet or Whisper) →
deterministic cleanup (punctuation, fillers) → dictionary correction → AutoText
expansion → text transforms → type into the focused app. Every transcript is
also appended to a day-wise `.md` file in the user's chosen folder. The
floating pill shows the live state and a running timer.

## Models

All models download to `~/Library/Application Support/com.teletype.app/models/`
(macOS) and never leave your machine.

### Speech (dictation)

| Engine  | Models                                                        | Source                     |
|---------|---------------------------------------------------------------|----------------------------|
| Parakeet| TDT v3 (recommended), v3 compact, v3 high precision, v2 (English) | `ggml-org/parakeet-GGUF`  |
| Whisper | Large v3 Turbo, Large v3 Turbo (compressed), Small.en, Base.en, Base, Tiny | `ggerganov/whisper.cpp` |

### Polish / transforms (optional, download any)

Labels describe what each engine is best at (EG-1 and S1-mini labels are the
publishers' own; Qwen3 entries are Teletype's own catalog). Teletype is open
source and non-commercial: you may download and use these for personal use.
We do not re-host EG-1 weights; the links below go to the publisher.

| Label | Best for | Size | Download |
|-------|----------|------|----------|
| **EG-1** (recommended) | Dictation cleanup fine-tune: spoken lists → real lists, speech walls → paragraphs, keeps only your self-correction. macOS 14+. | ~2.9 GB (8 shards) | [EG-1 folder](https://models.enviouslabs.co/eg1/eg1-1.2-c003/) · [license](https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt) |
| **S1-mini** by Superwhisper | Small open cleanup model, happiest in English; pairs with Tone / Structure / Context styles. | ~484 MB | [Primary](https://models.enviouslabs.co/s1/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf) · [Hugging Face backup](https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/s1-mini-q4_k_m.gguf) · [license](https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/34add00a48a2e5d24e5a4ee5405a99620a3a240c/LICENSE) |
| **Qwen3 1.7B "Fast"** | Quick local Polish and short rewrites. | ~1.4 GB | [Hugging Face](https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/qwen3-1.7b-q5_k_m.gguf) (also in-app) |
| **Qwen3 4B "Quality"** | Higher quality Professional / Prompt Engineer transforms (same 4B class as EG-1's base). | ~2.6 GB | [Hugging Face](https://huggingface.co/Qwen/Qwen3-4B-GGUF/resolve/main/qwen3-4b-q4_k_m.gguf) (also in-app) |
| **Apple Intelligence** | Apple's on-device polish, no extra download (when available on your macOS). | none | Enable in System Settings |
| **Ollama** | Local or hosted Ollama models over an OpenAI-compatible API. | varies | [ollama.com](https://ollama.com) → `http://127.0.0.1:11434/v1` |
| **OpenAI-compatible keys** | Bring-your-own-key cloud polish, text only (OpenAI, OpenRouter, etc.). | none | e.g. [OpenAI keys](https://platform.openai.com/api-keys) |

EG-1 shards live in one folder; download all 8 `.gguf` parts into the models
directory and select shard 1. SHA-256 digests are listed in
[roadmap.md](roadmap.md). In-app: Models screen → accept the EG-1 license
checkbox → Download (shards install under `models/eg-1/`). S1-mini installs as
`models/s1-mini.gguf` with SHA-256 verification. Manual drop-in still works:
put the file(s) in `~/Library/Application Support/com.teletype.app/models/`.

## Testing

```bash
cargo test              # tests across core, speech, and inference
cd ui && npm run typecheck
```

To rebuild the local LLM server binary used for Polish:

```bash
scripts/build-llama-server.sh
```

## Status

- **macOS** - fully implemented (dictation, injection, hotkeys, tray,
  onboarding, insights, transcript archive, app icon).
- **Windows** - platform layer stubbed; UIA/OCR reading, keyboard injection,
  hotkey registration, and tray are not yet implemented.
- **Linux** - not targeted.

## License

MIT
