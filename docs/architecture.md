# Teletype — Architecture

Teletype is a local-first desktop application that unifies **voice**, **typing**,
**AutoText** and **AI transforms** into one text-input system. Everything runs on
the user's machine; no transcript, AutoText value or preference ever leaves it
unless the user explicitly configures a remote inference provider.

## Reference audit (Phase 0)

The product starts from the ideas in
[karansinghgit/speaktype](https://github.com/karansinghgit/speaktype) (MIT).
Its `desktop/` tree is already a Tauri 2 + Rust + React app, which matches the
required stack, so those patterns are reused deliberately (not translated):

| Concern | Reused from SpeakType desktop | Teletype approach |
| --- | --- | --- |
| Voice capture | `audio.rs` — cpal capture thread, mono f32, resample to 16 kHz | Reused approach in `teletype-core::audio` |
| Dictation state machine | `dictation.rs` — one controller thread owns all events; `decide()` is a pure function tested without a microphone | Same shape, extended with `Transforming`/`Inserting`/`Completed`/`Cancelled`/`Error` phases |
| Local STT | `engine/` — whisper-rs (whisper.cpp, Metal/CoreML on macOS) behind one `Engine` | `SpeechProvider` trait in `teletype-speech`; whisper.cpp is one provider, not the architecture |
| Text injection | `paste.rs` — dedicated clipboard thread, save/restore clipboard, enigo paste shortcut, 250 ms settle before paste, 350 ms before restore | Reused in `injector.rs` behind `TextInjector` |
| Global hotkeys | `tauri-plugin-global-shortcut` + macOS single-modifier CGEvent tap (`platform/macos/hotkey.rs`) | Reused; abstracted as `ShortcutService` with abstract modifiers |
| Tray / pill | `tray.rs` (menu bar + panel), `pill.rs` (small always-on-top window) | Reused pattern; pill becomes the dictation overlay |
| Model downloads | `models.rs` — Hugging Face streaming download with cancellation, `.part` files, size check | Reused for speech models; LLM models use the same store |
| Settings persistence | `settings.rs` — JSON in the app config dir, per-field defaults, corrupt file moved aside | Same mechanism via `storage` |
| Permissions | `platform/macos/permissions.rs` (objc TCC checks) | Reused in `platform/macos.rs` |

Not reused: the Swift `speaktype/` app (replaced by the Rust desktop app),
Parakeet/ONNX engine (whisper.cpp covers V1), history/statistics screens
(deferred), update service (deferred), legacy v1 import (N/A).

## Workspace layout

```
teletype/
├── Cargo.toml                     # workspace
├── crates/
│   ├── teletype-core/             # platform-independent business logic
│   │   └── src/
│   │       ├── audio/             # mic capture (cpal), resampling
│   │       ├── state.rs           # dictation state machine (pure core)
│   │       ├── autotext/          # entries, tokenizer/protector, expander
│   │       ├── transforms/        # definitions, repository, engine, prompt, validator
│   │       ├── personalization/   # profile, explicit + learned prefs, comparator
│   │       ├── context/           # ApplicationContext + normalization
│   │       ├── pipeline.rs        # UnifiedInput → ... → final text
│   │       ├── injector.rs        # TextInjector (clipboard+paste, preservation)
│   │       ├── storage.rs         # JSON persistence
│   │       ├── shortcuts.rs       # abstract hotkey representation + conflict detection
│   │       └── platform/          # Platform trait + mock
│   ├── teletype-speech/           # SpeechProvider trait + whisper.cpp provider
│   ├── teletype-inference/        # InferenceProvider trait, ModelManager, llama.cpp GGUF provider, mock
│   └── teletype-desktop/          # Tauri app
│       └── src/
│           ├── lib.rs             # AppState, command wiring
│           ├── commands.rs        # #[tauri::command] surface
│           ├── dictation.rs       # controller thread driving the state machine
│           ├── tray.rs            # menu bar / system tray
│           ├── overlay.rs         # floating pill window
│           ├── typing.rs          # direct-typing AutoText watcher (enigo)
│           └── platform/          # Platform impls (macos.rs, windows.rs)
├── ui/                            # React 19 + Tailwind 4 frontend
│   ├── index.html                 # settings main window
│   ├── pill.html                  # dictation overlay
│   └── src/
│       ├── screens/               # transforms, autotext, personalization, models, settings, onboarding
│       └── ...
└── docs/
```

**Boundary rule:** `teletype-core` has no `#[cfg]` platform code and no Tauri
dependency. Platform capabilities are reached only through traits:

```rust
pub trait Platform: Send + Sync {
    fn active_application(&self) -> Option<ApplicationContext>;
    fn permissions(&self) -> Vec<Permission>;
    fn request_permission(&self, kind: PermissionKind);
    fn permission_settings_url(&self, kind: PermissionKind) -> Option<String>;
    fn default_hotkey(&self) -> Hotkey;
    fn paste_shortcut(&self) -> Option<PasteShortcut>;  // None ⇒ injector falls back to key events
    fn setup_notes(&self) -> Vec<String>;
}
```

`teletype-desktop` provides the concrete `Platform` per OS; tests and the core
use `MockPlatform`.

## The model is not the application

Deterministic Rust owns: input state, app context, AutoText, personalization
retrieval, transform selection, prompt construction, output validation,
persistence, shortcuts, injection. AI providers only perform narrow tasks:

```
UnifiedInput (transcript | typed text)
   ↓  ContextProvider          → ApplicationContext (app, category, confidence)
   ↓  AutoText protect()       → "send it to {{AUTOTEXT_1}}"
   ↓  Personalization          → small preference packet (relevant prefs only)
   ↓  TransformEngine          → prompt → InferenceProvider → validation
   ↓  AutoText restore()       → {{AUTOTEXT_1}} → exact configured value
   ↓  TextInjector             → clipboard + paste, clipboard restored
```

No LLM is involved when no transform applies (plain dictation, or an
AutoText-only expansion). If inference fails or the model is unavailable, the
pipeline returns the original input (with AutoText expanded) — never an error
that loses the user's words.

## Dictation state machine

One controller thread owns the session; all events (hotkey, toggle, escape,
transcription worker result) are messages to it, so state has a single owner.
`decide(input, phase, mode)` is a pure function covered by unit tests.

```
Idle → Listening → Stopping → Transcribing → Transforming → Inserting → Completed → Idle
                 ↘ Cancelled (escape)            ↘ Error → Idle (original text inserted or dropped)
```

## Speech

`SpeechProvider` abstracts model loading/unloading/transcription/cancellation/
language. V1 ships `WhisperCppProvider` (whisper-rs; Metal/CoreML on Apple
Silicon, CPU/CUDA elsewhere). English first; the data model carries a language
code (`"en"`, default) so multilingual is additive.

## Inference

`InferenceProvider` abstracts `generate(prompt, params)`. V1 providers:

- `LlamaProvider` — llama-cpp-rs, GGUF. Catalog: `fast` (Qwen3 ~1.7B class),
  `quality` (Qwen3 ~4B class), plus any user-placed GGUF (BYOM).
- `MockInferenceProvider` — deterministic, for tests and dev.

`ModelManager` owns the lifecycle `Unavailable → Downloading → Loading →
Ready → Busy → Unloading → Error`, keeps the selected model warm, and never
loads per request. Generation is short (≤ 300 tokens), temperature 0,
no reasoning — transforms are not thinking tasks.

## AutoText

`/trigger` → exact replacement. Deterministic, no LLM. Values are protected
around AI transforms via `{{AUTOTEXT_n}}` placeholders, so email addresses and
phones can never be altered by a model. Works in both voice and direct typing
(a background watcher expands triggers typed in any app; disabled by default
until the user turns it on). Entries have a scope: `everywhere` or a specific
application.

## Personalization

Structured, not a growing memory string:

- `UserProfile { language, tone, formatting, vocabulary, global }`
- **Explicit preferences** — user-stated, highest priority, inspectable/removable.
- **Learned preferences** — derived locally by comparing AI output with the
  user's final edit (greeting, sign-off, terminology, …), with confidence
  `Weak → Medium → Strong`. Candidates are surfaced in Settings; nothing is
  uploaded or auto-applied without a scope.

Retrieval builds a **preference packet**: only preferences relevant to the
current context are included in the prompt, in priority order
(current instruction > explicit > learned > app context > transform default >
model default).

## Context

`ContextProvider` reports the active application, normalized into
`ApplicationContext { application_id, application_name, application_type,
window_title, confidence }` with categories: `email, chat, coding, document,
browser, terminal, social, unknown`. Detection is best-effort (macOS:
NSWorkspace frontmost app + bundle id map; Windows: foreground window title +
process name). Unknown is a valid, honest answer; context is a *baseline*,
never a forced style.

## Security and privacy

- Local-first: no network on the default path; model downloads are explicit.
- No telemetry, no analytics, no remote transcript.
- AutoText values and the profile are never logged; transcripts are not logged
  by default (operational events only: `transform_started`, `model_loaded`, …).
- Prompt injection: dictated text is wrapped and explicitly marked as data;
  the validator rejects prompt-echo / instruction-echo outputs and falls back
  to the original text.
- Meaning > style: prompts forbid changing facts, names, numbers, dates, URLs,
  identifiers, ownership, and pronouns; when uncertain the original is kept.

## Testing

- Core is fully testable without a microphone, display or model download:
  `MockSpeechProvider`, `MockInferenceProvider`, `MockContextProvider`,
  `MockTextInjector`, `MockPlatform`.
- Unit tests cover transforms, prompting, validation, AutoText,
  personalization, context, shortcuts, state machine, and injection logic.
- One end-to-end pipeline test runs the exact product scenario: Gmail +
  Professional + `/email` + a spoken sentence, asserting the email survives
  intact, meaning and pronouns are preserved, and the injector received the
  final text.
