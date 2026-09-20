# Teletype

Local, private, AI-powered voice dictation for your Mac.

Hold a hotkey, talk, and your words — transcribed on-device, optionally
cleaned up by a local LLM, and expanded via AutoText — are typed into whatever
app you're using. No audio or text ever leaves your machine.

## Features

- **Voice dictation** — hold-to-talk transcription with a floating progress pill.
- **Two local speech engines** — NVIDIA Parakeet TDT v3 (fastest, most
  accurate, recommended) or Whisper GGML models of any size.
- **LLM cleanup** — optional local LLM transform slot (GGUF catalog); the
  deterministic cleanup pipeline (capitalization, punctuation, filler words)
  always runs.
- **AutoText** — type `@team` and get a full email address; `[[name]]` becomes
  your name. Optional smart-correct for common typos.
- **Text transforms** — sentence case, title case, lower/upper case, strip
  markdown, trim whitespace.
- **Cross-platform core** — Rust backend with a macOS implementation; the
  Windows platform layer is a stub (see [Status](#status)).

## Requirements

- **macOS 14+** (Apple Silicon or Intel)
- **Rust 1.88+**
- **Node.js 18+** (for the UI build)
- **CMake** and a C/C++ toolchain (`xcode-select --install`) — used to build
  the speech engines

## Build & run

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

## Using Teletype

1. **Grant permissions** — the app asks for *Microphone* and *Accessibility*
   access on first run (the onboarding wizard walks you through it).
2. **Pick a dictation model** — Models → Dictation. Parakeet TDT v3 (~637 MB)
   is recommended; Whisper models range from Tiny (75 MB) to Large v3 Turbo
   (~547 MB). Downloads are one-time and local.
3. **Hold the hotkey and talk** —
   - macOS: `⌘⇧Space`
   - Windows: `Ctrl+Shift+Space`

   Release to transcribe and type. Change the hotkey in Settings.

**LLM cleanup** — the Models → Writing assistant screen lists local GGUF
models, but the llama.cpp runtime is not linked in this build (it conflicts
with the speech engine's math library). Selecting a model reports that
clearly, and transforms fall back to the deterministic cleanup pipeline.

## Architecture

A Rust-first workspace with a Tauri 2 + React frontend. All inference is local.

```
crates/
  teletype-core        Pure Rust: pipeline, text transforms, auto-text,
                       shortcuts, settings, audio capture, platform traits
  teletype-speech      Local STT providers (Parakeet + Whisper) and model catalog
  parakeet-sys         Builds whisper.cpp v1.9.4 (parakeet + ggml) via CMake
  teletype-inference   LLM model catalog + GGUF provider slot + mock provider
  teletype-desktop     Tauri app: commands, dictation controller, platform impls
ui/                    React screens (Transforms, AutoText, Personalization,
                       Models, Settings, Onboarding) + floating progress pill
```

**Dictation flow:** hotkey → record mic → transcribe (Parakeet or Whisper) →
optional LLM cleanup → AutoText expansion → text transforms → type into the
focused app. The floating pill shows the live state (listening → transcribing
→ typing → done).

## Models

All models download to `~/Library/Application Support/com.teletype.app/models/`
and never leave your machine.

| Engine | Models | Source |
|---|---|---|
| Parakeet | TDT v3 (recommended) | `ggml-org/parakeet-GGUF` |
| Whisper | Large v3 Turbo, Small.en, Base.en, Base, Tiny | `ggerganov/whisper.cpp` |
| LLM | Qwen3 1.7B / 4B GGUF (runtime not linked in this build; see above) | Hugging Face |

## Testing

```bash
cargo test        # 84 tests across core, speech, and inference
cd ui && npm run typecheck
```

## Status

- **macOS** — fully implemented (dictation, injection, hotkeys, tray, onboarding).
- **Windows** — platform layer stubbed; UIA/OCR reading, keyboard injection,
  hotkey registration, and tray are not yet implemented.
- **Linux** — not targeted.

## License

MIT
