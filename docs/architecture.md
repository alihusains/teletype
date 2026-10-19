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
| Local STT | `engine/` — whisper-rs (whisper.cpp, Metal/CoreML on macOS) behind one `Engine` | `SpeechProvider` trait in `teletype-speech`; three providers (Whisper, Parakeet TDT, Parakeet.cpp), selected per model via the `SpeechEngine` setting |
| Text injection | `paste.rs` — dedicated clipboard thread, save/restore clipboard, enigo paste shortcut, 250 ms settle before paste, 350 ms before restore | Reused in `injector.rs` behind `TextInjector` |
| Global hotkeys | `tauri-plugin-global-shortcut` + macOS single-modifier CGEvent tap (`platform/macos/hotkey.rs`) | Reused; abstracted as `ShortcutService` with abstract modifiers |
| Tray / pill | `tray.rs` (menu bar + panel), `pill.rs` (small always-on-top window) | Reused pattern; pill becomes the dictation overlay |
| Model downloads | `models.rs` — Hugging Face streaming download with cancellation, `.part` files, size check | Reused for speech models; LLM models use the same store |
| Settings persistence | `settings.rs` — JSON in the app config dir, per-field defaults, corrupt file moved aside | Same mechanism via `storage` |
| Permissions | `platform/macos/permissions.rs` (objc TCC checks) | Reused in `platform/macos.rs` |

Not reused: the Swift `speaktype/` app (replaced by the Rust desktop app),
history/statistics screens
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
│   │       ├── storage.rs         # JsonStore (atomic JSON files, corrupt-quarantining)
│   │       ├── repo.rs            # Repo<T>: ArcSwap snapshot over a JsonStore + broadcast
│   │       ├── persist.rs         # SQLite durable backend (teletype.db, WAL) + JSON→SQLite migration
│   │       ├── shortcuts.rs       # abstract hotkey representation + conflict detection
│   │       └── platform/          # Platform trait + mock
│   ├── teletype-speech/           # SpeechProvider trait + Whisper / Parakeet / Parakeet.cpp providers + model catalog
│   ├── teletype-inference/        # InferenceProvider trait, ModelManager, llama-server sidecar provider, EngineHandle, mock
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
transcription worker result, pipeline readiness, VAD firing, edit checks)
are `dictation::Event` messages to it, so state has a single owner.
`decide(input, phase, mode)` is a pure function covered by unit tests.
The pipeline worker never injects text itself: it finishes bookkeeping
(history, transcript file, usage, edit-watch), then sends `PipelineReady`
with the prepared payload, and only the controller — the single owner of
the session state machine — performs the injection after checking the
session is still valid (not cancelled, not superseded). A `Cancel`
processed before `PipelineReady` discards the result; that FIFO ordering
is the whole cancellation guarantee.

```
Idle → Listening → Stopping → Transcribing → Transforming → Inserting → Completed → Idle
                 ↘ Cancelled (escape)            ↘ Error → Idle (original text inserted or dropped)
```

## Speech

`SpeechProvider` abstracts model loading/unloading/transcription/cancellation/
language. Three local providers ship behind it:

- `WhisperProvider` (whisper.cpp; Metal/CoreML on Apple Silicon, CPU/CUDA
  elsewhere) — the portable default.
- `ParakeetProvider` (Parakeet TDT v3, recommended on macOS).
- `ParakeetCppProvider` (GGUF-native loader for mixed-quant models).

The speech model catalog records which engine loads each model file.
`AppState` keeps one `SpeechModelManager` per engine (`speech`, `parakeet`,
`parakeet_cpp`) behind the idle-unload timer, plus a `speech_engine` setting
(`SpeechEngine`: `Whisper` / `Parakeet` / `ParakeetCpp`) as the fallback when
a selected model id is outside the catalog. A one-time migration infers the
engine from the selected model's catalog entry for settings files that
predate the field. English first; the data model carries a language code
(`"en"`, default) so multilingual is additive, with per-app language
overrides on top.

While recording, an interim-transcript loop re-decodes the audio-so-far on a
fixed cadence and emits an `interim-transcript` event to the pill as a
provisional preview (marked provisional because whole-take re-decodes revise
as the sentence develops; the authoritative text is the single full decode at
stop). The loop serializes on the speech-model lock with the final
transcribe and skips a tick rather than overlapping it.

## Inference

`InferenceProvider` abstracts `generate(prompt, params)`. Providers:

- `ServerProvider` — the local provider. llama.cpp's ggml cannot be linked
  in-process alongside whisper.cpp's ggml, so the model runs in a separate
  `llama-server` sidecar subprocess: the provider locates the binary
  (`TELETYPE_LLAMA_SERVER`, next to the app exe, bundle `Resources/`, then
  `PATH`), spawns it on `127.0.0.1` at an ephemeral port with a random API
  key, polls `/health` until ready, serves `generate` via
  `POST /v1/chat/completions`, and kills the child on shutdown/`Drop`.
  A pid file beside the model lets the next launch reap an orphan from a
  crashed run (verified to still be our llama-server before signalling).
  `ServerProvider` reports `is_local() == true`, which is what the polish
  gate consults.
- `OpenAiCompatProvider` — remote OpenAI-compatible API connector (explicit
  user configuration; the only path where text leaves the machine).
- `MockInferenceProvider` — deterministic, for tests and dev.

`ModelManager` owns the lifecycle `Unavailable → Downloading → Loading →
Ready → Busy → Unloading → Error`, keeps the selected model warm, and never
loads per request. Generation is short (≤ 300 tokens), temperature 0,
no reasoning — transforms are not thinking tasks.

The loaded engine is held in an `EngineHandle` (ArcSwap, lock-free):
dictation clones the `Arc<dyn InferenceProvider>` and drops the lock
immediately, so no lock is held across the LLM call (which can take seconds),
and a model swap atomically installs a new `Arc` while in-flight dictations
keep the old one alive until they finish. The same clone-up-front discipline
applies to the snapshot stores read before `pipeline.run()`.

Streaming: the local server streams tokens over SSE (`stream: true`),
parsed incrementally by `SseFrameParser` over a raw loopback TCP connection
with a bounded read slice (`READ_SLICE`) under the overall generation
deadline, so a silent server fails instead of hanging. Each delta is
forwarded to the pill as a `transform-token` event carrying only
`{seq, tok}` — the pill concatenates the deltas, keeping IPC traffic linear
in output length instead of quadratic.

## Shared state: Repo<T> snapshot stores

Every settings-like document in `AppState` (`settings`, `autotext`,
`transforms`, `profile`, `history`, `dictionary`, `user_packs`, `styles`,
`usage`, `scratchpad`) is a `Repo<T>`: one `JsonStore` file plus one
lock-free `ArcSwap` snapshot. Reads are `Repo::get()` — no lock, no clone,
just an `Arc` load. Writes are clone-on-write (`Repo::update`): apply to a
clone, persist, then swap the snapshot in and broadcast the new `Arc` to
subscribers. On a persistence error the snapshot is not swapped, so memory
and disk stay consistent. Background threads (typing watcher, unload timer)
read snapshots without ever touching Tauri state.

## Persistence: SQLite + background actor

History and usage stats are durable in SQLite (`teletype.db` in the config
dir, WAL mode, `synchronous=NORMAL`) via `PersistStores` (tables `history`
and `usage`). The dictation hot path never does synchronous disk I/O: a
single background writer thread (`teletype-desktop::persist::PersistHandle`,
an mpsc actor) owns the connection and applies `History` / `HistoryRemove` /
`Usage` / `UsageReset` messages off-path, coalescing bursts (flush at most
once per ~250 ms, at a max batch, or on shutdown). The in-memory `Repo`
snapshots stay the read source of truth; the actor mirrors each change to
the durable store, with a bounded flush on quit and on `AppState` drop so a
panic cannot lose queued entries.

One-time JSON→SQLite migration: when the DB tables are empty and a legacy
JSON file exists, its rows are imported and the file is renamed (never
deleted) so a failed migration is recoverable. Settings, AutoText,
transforms, profile, dictionary, packs, styles and scratchpad remain JSON
files via `JsonStore` (atomic write, corrupt file moved aside).

## Settings updates: SettingsPatch + observers

Whole-document saves have a lost-update hazard (two tabs read-modify-write
concurrently; the second clobbers the first). The fix has two halves, both
serialized under `AppState::save_lock`:

- `SettingsPatch` — a partial update where every field is `Option`, and only
  `Some` fields are applied (`update_settings_patch` command). A caller with
  a stale snapshot can change one field without reverting the rest.
- `settings_observers` — one function per subsystem (hotkey, quick-add
  shortcut, pill position, app icon, tray visibility, typing toggle), each a
  no-op when its watched field is unchanged. Both `save_settings` and
  `update_settings_patch` diff old vs new and run only the observers whose
  field changed, then emit `settings-changed` so the webviews refresh.

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
