# Project Brain

This file is the durable working memory for this project.

It is written for both humans and coding agents.

Keep it concise, factual, current.

---

## 1. Project identity

**Project name:**
Teletype

**One-line purpose:**
Local-first cross-platform voice dictation app: hold a hotkey, speak, get text
transcribed on-device and typed into the focused app, with deterministic cleanup,
AutoText snippets, text transforms, and optional local/remote LLM polish.

**Current status:**
Feature-rich and working on macOS. Full Wispr Flow parity roadmap implemented and
verified as of 2026-09-20 (`checkpoint.md`). The P0 "Polish" batch (subprocess LLM
server, OpenAI-compatible provider, keyring secrets, expanded model catalog) was
committed in `3fa2793` (2026-09-23), and the verified P0 fixes landed on `main` as
`9163631` (P0-8 scaled timeouts) and `e2a9ec9` (P0-2/4/5/6/7/9/11/12/13, 2026-09-25).
Note: per-bug worktree commits `0e8b94e..1658caf` exist on worktree branches but are
NOT on `main` (superseded by `e2a9ec9`), so any "uncommitted P0 batch" or
`0e8b94e..1658caf` citation is stale (findings.md P0-10 and P0-1 were corrected
2026-09-25). Main-line commits through 2026-09-25: `9163631` (S1 control line,
scaled timeouts, Apple Intelligence groundwork), `56dc123` (4 pill styles + S1 UI),
`1c9f3d9`, `e353e79` (S1 wire-token tests), `2d31981` (DeveloperScreen S1 readout),
`e2a9ec9` (9 P0 fixes), `8765f05` (download resume + disk probe, 246 tests),
`839dfb5` (findings.md status annotations), `9c066ae` (language picker Phase A UI).
Full test counts at `8765f05`: build clean, 246 passed / 0 failed, `tsc --noEmit`
clean (working tree dirty afterwards with in-flight ITN/T7 work - expected).
Independent verification of all completed-task claims (OpenCode2, 2026-09-25):
11/14 confirmed with file:line evidence; discrepancies were docs-level only
(worktree-vs-main citation, stale P1-15, 9163631 message scope overstating S1).

**Primary users / audience:**
Personal open-source project: built for the owner's daily use and the
open-source community (user-confirmed 2026-09-22).

**Repository / product owner:**
github.com/alihusains/teletype (origin remote), branch `main`, owner: Ali.

---

## 2. What we are building

### Problem

Typing (~40 wpm) is far slower than speaking (~130-160 wpm). Existing dictation
tools either ship audio to the cloud (privacy problem), charge subscriptions
(cost), or require internet (dependency). See `README-github.md`.

### Desired outcome

Hold a key anywhere in macOS (Windows planned), speak, and get clean written text
inserted at the cursor, entirely on-device, no account, no telemetry. Plus
AutoText expansion, AI transforms, dictionary, insights, and a transcript archive.

### In scope

- macOS dictation end-to-end: hotkey (Fn CGEventTap), mic capture, local ASR
  (Parakeet + Whisper), pipeline cleanup, clipboard-paste injection, floating pill
- Deterministic pipeline: capitalization, punctuation, fillers, dictionary,
  AutoText protection/restore, transforms, style profiles
- Local-first privacy: no telemetry, no network on default path
- Optional LLM polish via subprocess llama-server or OpenAI-compatible endpoint
- Cross-platform architecture (Rust core + Platform trait); Windows layer stubbed
- Personal open-source, never commercial (see Decisions)

### Out of scope

- Cloud speech APIs on the default path
- Telemetry/analytics (PostHog, Sentry-on-by-default explicitly rejected)
- Linux
- Commercial/subscription model
- Bundling large model weights in the base install (~13 MB binary target)

---

## 3. Important context from the user

Record durable context the user has explicitly provided.

| Topic | Context | Source | Date |
|---|---|---|---|
| Audience | Personal open-source project, never commercial | user answer | 2026-09-22 |
| Plan of record | `roadmap.md` is the authoritative plan; `checkpoint.md` is historical status | user answer | 2026-09-22 |
| Windows priority | Equal priority with macOS going forward | user answer | 2026-09-22 |
| Uncommitted P0 batch | ServerProvider / OpenAiCompat / keyring / catalog work needs review + verification before commit | user answer | 2026-09-22 |

Rules:
- Prefer direct user statements over assumptions.
- Do not copy sensitive information.
- Record only context that will help future work.

---

## 4. Current architecture

### High-level flow

```text
hotkey (Fn tap / global shortcut)
  → mic capture (cpal, 16 kHz mono)
  → local ASR (Parakeet or Whisper via SpeechProvider)
  → pipeline.rs: context → AutoText protect → personalization packet
      → transforms (deterministic; LLM only if a transform applies and a
        provider is available) → validation → AutoText restore
  → TextInjector (clipboard save → paste → restore)
  → history.json + day-wise .md transcript archive
LLM path: InferenceProvider = ServerProvider (child llama-server process,
  POST 127.0.0.1:<port>/v1/chat/completions) | OpenAiCompatProvider |
  LlamaProvider (stub, "runtime not linked") | Mock (tests)
  The provider instance is memory-only; it is rehydrated at startup from
  settings.selected_llm_provider by commands::rehydrate_provider (see G006)
```

State machine (`teletype-core/src/state.rs`, pure `decide()`):
`Idle → Listening → Stopping → Transcribing → Transforming → Inserting →
Completed | Cancelled | Error`.

### Main components

| Component | Purpose | Location |
|---|---|---|
| teletype-core | Platform-independent logic: state machine, audio, pipeline, autotext, transforms, personalization, context, dictionary, style, scratchpad, insights, stats, history, injector, storage | `crates/teletype-core/` |
| teletype-speech | SpeechProvider trait + Parakeet + Whisper providers, model catalog | `crates/teletype-speech/` |
| teletype-inference | InferenceProvider trait, model catalog/download, ServerProvider (subprocess), OpenAiCompatProvider, LlamaProvider stub, mock | `crates/teletype-inference/` |
| parakeet-sys | CMake build of whisper.cpp v1.9.4 (provides parakeet + ggml) | `crates/parakeet-sys/` |
| teletype-desktop | Tauri 2 app: commands (IPC), dictation controller, tray, overlay pill, Fn tap, platform impls, keyring secrets | `crates/teletype-desktop/` |
| ui | React 19 + Vite 6 + TS screens + floating pill | `ui/` |

### Important integrations

| System | Purpose | Interface / location | Notes |
|---|---|---|---|
| llama-server (llama.cpp) | Local GGUF LLM for Polish | spawned subprocess, `crates/teletype-inference/src/server.rs` | Built via `scripts/build-llama-server.sh`, pinned commit, Metal on, curl/OpenSSL off; never linked in-process |
| OpenAI-compatible APIs | Remote/Ollama/LocalLLM polish | `crates/teletype-inference/src/openai_compat.rs` | base_url + optional key + model |
| OS keychain | API key storage | `keyring` crate, `crates/teletype-desktop/src/secrets.rs` | service `com.teletype.app`, accounts `llm:<provider>`; never in settings.json |
| Hugging Face / publisher CDNs | Model downloads | `crates/teletype-inference/src/download.rs`, catalog | SHA-256 verified; EG-1 requires license-accept gate |
| SpeakType (github.com/karansinghgit/speaktype, MIT) | Reference implementation | patterns reused (see `docs/architecture.md` Phase 0 audit) | Attribution required |
| EnviousWispr (EW) | Feature/quality reference for roadmap | temp clone path recorded in `roadmap.md` | Analysis source for P0-P4 plan |

### Data flow

```text
~/Library/Application Support/com.teletype.app/
  settings.json, dictation.json, dictionary.json, styles.json,
  scratchpad.json, autotext + transforms + personalization stores,
  models/ (speech + GGUF downloads), transcripts (day-wise .md archive)
```

---

## 5. Repository map

| Area | What it contains | Important files |
|---|---|---|
| `crates/` | Rust workspace (5 members) | `Cargo.toml` |
| `crates/teletype-core/` | Pure business logic | `pipeline.rs`, `state.rs`, `llm.rs`, `transforms/`, `autotext/`, `personalization/`, `context/` |
| `crates/teletype-inference/` | LLM providers + catalog | `server.rs`, `openai_compat.rs`, `catalog.rs`, `llama.rs` (stub) |
| `crates/teletype-desktop/` | Tauri app shell | `lib.rs`, `commands.rs`, `dictation.rs`, `overlay.rs`, `secrets.rs`, `fn_tap.m` |
| `ui/` | React frontend | `src/App.tsx`, `src/pill.tsx`, `src/screens/` (13 screens) |
| `docs/` | Architecture reference | `architecture.md` |
| `scripts/` | llama-server build | `build-llama-server.sh` |
| Root docs | Plans and status | `prompt.md` (original spec), `roadmap.md` (plan of record), `checkpoint.md` (historical status), `README-github.md` (public README; root `README.md` is the brain-starter doc) |
| `.agents/brain/` | This brain + learnings | `PROJECT_BRAIN.md`, `learnings/LEARNING_TEMPLATE.md` |

### Commands

**Install**

```text
cargo install tauri-cli --version "^2"
cd ui && npm install
```

**Run locally**

```text
cargo tauri dev          # dev log: /tmp/teletype-dev.log
```

**Test**

```text
cargo test --workspace
cd ui && npm run typecheck   # tsc --noEmit
```

**Lint / typecheck**

```text
cargo clippy --workspace --all-targets   # pre-existing warnings only
npm run typecheck                        # in ui/
```

**Build**

```text
cd ui && npm run build && cd ..
cargo tauri build        # release bundle (~13 MB binary target)
```

**Deploy**

```text
No deploy step (desktop app). Notarization/updater not yet set up (roadmap P3.5).
```

**Build llama-server (for local Polish)**

```text
scripts/build-llama-server.sh
# installs to crates/teletype-desktop/binaries/llama-server and target/debug/
# REQUIRED before selecting any local LLM model (EG-1, Qwen3, S1-mini), else
# select_model fails: llama-server binary not found (L004 / G009)
# Builds static + no OpenSSL (-DBUILD_SHARED_LIBS=OFF -DLLAMA_OPENSSL=OFF)
# so the single bundled resource is self-contained; clone+build takes minutes
```

---

## 6. Conventions

### Code conventions

- Rust workspace, edition 2021, rust-version 1.88, MIT
- `teletype-core` must contain no `#[cfg]` platform code and no Tauri dependency;
  platform access only via the `Platform` trait (see `docs/architecture.md`)
- Clippy lints are warnings (all, todo, dbg_macro, undocumented_unsafe_blocks);
  SAFETY comments required on unsafe blocks
- Deterministic code owns everything except narrow AI tasks ("the model is not
  the application")
- Spoken-list rendering belongs to deterministic code: `format_spoken_lists`
  in `transforms/engine.rs` (ordinal narration + colon-announced lists) runs on
  polish/professional output after validation, so it holds on the EG-1 route
  (fixed prompt, no rule possible) and on model fallback. It also repairs model
  output that crams `- ` markers onto one line (the failure EW's judge calls
  "merges several items onto one line"). Format convention, exactly per EW's
  runtime prompt examples: lead-in ends with `:`, one item per line starting
  `- `, each item capitalized with a trailing period. EW has no equivalent
  pass; they rely on prompt rules plus model capability (see External sources)

### Naming

- Crates: `teletype-*`; Tauri commands are `snake_case` and registered in
  `lib.rs`; serde fields are camelCase in IPC (UI must match, e.g. `sizeMb`)
- Provider traits: `SpeechProvider`, `InferenceProvider`, `TextInjector`,
  `ContextProvider`, `Platform`

### Testing

- Pure unit tests without mic/display/network; all mock providers
  (`MockSpeechProvider`, `MockInferenceProvider`, `MockPlatform`, etc.)
- One end-to-end pipeline test (Gmail + Professional + `/email` scenario)
- Settings fields are serde-defaulted for backward compatibility

### Error handling

- Pipeline never loses the user's words: on any transform/inference failure,
  fall back to original text (AutoText still expanded); dictation never blocks
  on LLM failure
- `Result<_, String>` is common across the IPC boundary
- Corrupt settings files are moved aside, not crashed on

### Logging / observability

- `tracing` for operational events only (`transform_started`, `model_loaded`, ...)
- Never log transcripts, AutoText values, profile contents, or API keys
- In dictation paths use `log_line()`, never raw `eprintln!` (closed stderr on
  relaunch can abort the app, see Gotcha G002)

### API conventions

- LLM calls use `POST {base_url}/v1/chat/completions` with
  `Authorization: Bearer <token>`; local subprocess binds `127.0.0.1` only,
  ephemeral port, random API key
- Generation: temperature 0, no reasoning. EG-1 caps at max(input chars, 256);
  the generic path scales `max_tokens` with input length (clamped 300..2048).
  Hitting the cap makes providers return `Err`, which the engine treats as
  fallback-to-input, so an undersized cap silently drops the whole transform
  (see G007)

---

## 7. Decisions

Only record durable decisions here.

### Decision D001

**Title:** Open source forever, never commercial

**Date:** 2026-09-22

**Status:** accepted

**Decision:**
Teletype remains open source and non-commercial. EG-1 weights are user-directed
downloads with a license-accept checkbox; never re-host or bundle them in the
`.app`.

**Context:**
EG-1 Community Model License 1.0 forbids commercial/competitor use and
re-hosting; roadmap needed a permanent product stance.

**Alternatives considered:**
Commercial product (rejected: violates EG-1 clause 2.b and product intent).

**Reason:**
Keeps Teletype inside the EG-1 license and matches the local-first,
no-subscription positioning.

**Evidence / source:** `roadmap.md` "Open decisions" #1 (resolved) and EG-1
license analysis section.

### Decision D002

**Title:** LLM inference runs as a subprocess llama-server, never in-process

**Date:** 2026-09-20 (link-clash fix) / 2026-09-22 (subprocess direction)

**Status:** accepted (implemented and committed in `3fa2793`, 2026-09-23; timeout alignment in `9163631`)

**Decision:**
Do not link llama.cpp into the app process: its ggml collides with whisper.cpp's
ggml (duplicate-symbol link failure). Instead spawn a separately built
`llama-server` child process and talk to it over loopback HTTP.

**Context:**
In-process linking produced hundreds of duplicate symbols; `LlamaProvider` was
deliberately stubbed to return "runtime not linked" as the interim fix.

**Alternatives considered:**
Unify ggml in-process (rejected: unstable, explicitly deferred in roadmap P0.1);
static single-ggml rebuild (rejected for now).

**Reason:**
EW uses the same subprocess architecture; it is the stable path.

**Evidence / source:** `checkpoint.md` "Linker error" section; `roadmap.md`
P0.1; `crates/teletype-inference/src/server.rs`.

### Decision D003

**Title:** Local-first, zero telemetry by default

**Date:** 2026-09-22 (recorded; original in `prompt.md`)

**Status:** accepted

**Decision:**
No network on the default path, no analytics/telemetry, no remote transcripts.
Model downloads are explicit user actions. Crash reporting only if ever added
must be opt-in and scrubbed (roadmap P2.4); PostHog explicitly rejected.

**Evidence / source:** `prompt.md` (NO NETWORK DEPENDENCY, PRIVACY sections);
`docs/architecture.md` Security section; `roadmap.md` non-negotiables.

### Decision D004

**Title:** API keys stored in OS keychain, never in settings.json

**Date:** 2026-09-22

**Status:** accepted (implemented and committed in `3fa2793`, 2026-09-23)

**Decision:**
Use the `keyring` crate (macOS Keychain / Windows Credential Manager), service
`com.teletype.app`, account `llm:<provider>`. Secrets never reach settings.json,
logs, or the UI/IPC surface.

**Reason:**
Mirrors EW's KeychainManager; roadmap P0.3 requirement.

**Evidence / source:** `crates/teletype-desktop/src/secrets.rs`;
`roadmap.md` P0.3.

### Decision D005

**Title:** roadmap.md is the plan of record

**Date:** 2026-09-22

**Status:** accepted

**Decision:**
Priorities and sequencing come from `roadmap.md` (P0-P4, EW-derived).
`checkpoint.md` is historical status. Windows is equal priority with macOS
going forward.

**Evidence / source:** user answers 2026-09-22.

### Decision D006

**Title:** Learned corrections live on the dictionary word (aliases + provenance), not in a separate store; packs are corrector-lane only

**Date:** 2026-09-26

**Status:** accepted (planned; implementation per `T2.1-plan.md`)

**Decision:**
A learned correction is stored as an alias on the target `DictionaryWord`
(`aliases` + `learned_aliases` + `learned_at` provenance fields, EW
`CustomWord` model), with save-at-once + 3 s undo pill and a deterministic
rules judge in front of the write. Vocabulary pack terms enter only the
corrector (never the polish prompt), enforced by two distinct vocabulary
value types (EW `VocabularyLanes` model). The polish prompt gets a bounded,
frequency-ranked term subset, never pack terms, never the full word list.

**Context:**
T2.1 personalization loop; user asked whether aliases belong on
`DictionaryWord` or in a separate learned-corrections store, and for a plan
covering growth (dictionary + packs + learned words). Full plan, EW source
map, and impact analysis: `T2.1-plan.md`.

**Alternatives considered:**
Separate `learned_corrections.json` table (rejected: join logic, alias
ownership conflicts, third UI surface, breaks one-file-per-concept storage).

**Evidence / source:** EW reference tree
(`enviouswispr/EnviousWispr/Sources/.../CustomWord.swift`,
`LearnedCorrectionCoordinator.swift`, `RulesCorrectionJudge.swift`,
`VocabularyLanes.swift`, `VocabularyPackStore.swift`).

---

## 8. Known constraints

Record constraints that future agents must know.

| Constraint | Why it exists | Evidence / source |
|---|---|---|
| Never link llama.cpp in-process | ggml duplicate symbols vs whisper.cpp | `checkpoint.md`, D002 |
| `llama-server` must be one self-contained binary (static, `LLAMA_OPENSSL=OFF`) | `tauri.conf.json` bundles that single file: a shared build needs ~10 sibling dylibs via an absolute `/tmp` LC_RPATH, and the default link pulls Homebrew `libssl` | G009, L004, `scripts/build-llama-server.sh` |
| Never re-host or bundle EG-1 weights | EG-1 Community Model License 1.0 clause 2 | `roadmap.md` license analysis |
| S1-mini attribution: "S1-mini by Superwhisper" | Apache-2.0 naming term | `roadmap.md` |
| ~13 MB release binary, no bundled model assets | Performance/privacy non-negotiable | `roadmap.md` section 0 |
| `teletype-core` stays platform-free | Cross-platform boundary; tested via mocks | `docs/architecture.md` |
| English-first; Parakeet is English-only, `"auto"` resolves to `"en"` | Model capability; prevents mis-transcription | `checkpoint.md` language guard |
| macOS 14+, Rust 1.88+, Node 18+, CMake required | Build/toolchain floor | `README-github.md` |
| Windows platform layer stubbed | UIA/OCR, injection, hotkey, tray not implemented | `README-github.md` Status |
| No telemetry / analytics ever on by default | Product stance + EW contrast | `roadmap.md`, D003 |
| Never log transcripts, AutoText values, or keys | Privacy contract | `docs/architecture.md`, `prompt.md` |
| Never edit `EG1_SYSTEM_PROMPT` without retraining | The prompt text and the EG-1 artifact are one contract (canonical: `eg1-polish-prompt-v2.txt` in EnviousWispr); only the generic `CORE_RULES`/`build_prompt` path may absorb new rules | `crates/teletype-core/src/transforms/prompt.rs` header comment |
| Provider kinds must all be handled in `commands::rehydrate_provider` | Settings persist the provider *choice* only; a new provider type that is not rehydrated at startup silently disables transforms after every restart | this pass (G006) |

---

## 9. Gotchas

Record non-obvious behavior that frequently causes mistakes.

### Gotcha G001

**Title:** Linking llama.cpp alongside whisper.cpp breaks the build

**What happens:**
Hundreds of duplicate ggml symbol errors at link time (release and debug).

**Why:**
Both libraries embed their own copy of ggml.

**How to avoid it:**
Use the subprocess `ServerProvider` (or any out-of-process runtime). Keep
`LlamaProvider` stubbed; do not add `llama_cpp` as a linked dependency.

**Source / evidence:** `checkpoint.md` 2026-09-20 fix pass; D002.

### Gotcha G002

**Title:** `eprintln!` in the dictation controller can abort the app

**What happens:**
App process aborts when stderr is closed (e.g. after a relaunch).

**Why:**
Rust panics on write failure to a closed stderr in `eprintln!`.

**How to avoid it:**
Use `log_line()` in `dictation.rs` paths instead of `eprintln!`.

**Source / evidence:** `checkpoint.md` Insights Added, crash fix.

### Gotcha G003

**Title:** Weekday label epoch math

**What happens:**
Day labels render one day off (Sunday shows as Monday).

**Why:**
1970-01-01 was a Thursday; correct formula is `(3+days)%7`, not `(4+days)%7`.

**How to avoid it:**
Keep `(3+days)%7` in `stats.rs`/`insights.rs` day_label.

**Source / evidence:** `checkpoint.md` Insights Added, weekday fix.

### Gotcha G004

**Title:** Tauri events under React StrictMode

**What happens:**
Duplicate/missed event listeners in dev.

**Why:**
StrictMode double-invokes effects.

**How to avoid it:**
Always register Tauri listeners through `ui/src/lib/useTauriEvent.ts`.

**Source / evidence:** `checkpoint.md` pill fix.

### Gotcha G005

**Title:** Pill state phase ordering

**What happens:**
Pill appears before state updates, causing phase mismatches.

**Why:**
Show/hide raced ahead of the typed `PillState` emit.

**How to avoid it:**
`overlay.rs` must emit the typed `PillState` before show/hide.

**Source / evidence:** `checkpoint.md` pill fix.

### Gotcha G006

**Title:** The LLM provider instance is memory-only and used to start empty

**What happens:**
After an app restart, `AppState.inference` was `None` even though settings
still showed a selected model: every transform silently fell back to
AutoText-only output until the user re-picked the model in Settings.

**Why:**
Only the in-session `select_model` / `select_openai_provider` commands
installed a provider. `settings.selected_llm_provider` records the *choice*,
not an instance, and nothing rehydrated it in `lib.rs::run()`.

**How to avoid it:**
`commands::rehydrate_provider` runs on a background thread at startup and
rebuilds the provider (local-server: warm_up the child; openai-compat: rebuild
from the keyring key, no probe). Any newly added provider kind must be added to
that match too. The pipeline also flags the skip via
`PipelineResult.transform_skipped_no_model`, logged by `dictation.rs`.

**Source / evidence:** this pass (2026-09-22); `commands.rs`
`rehydrate_provider`, `pipeline.rs`, `dictation.rs`.

### Gotcha G007

**Title:** Hitting `max_tokens` is a silent no-transform, not a truncated result

**What happens:**
Medium-length dictations were passed through unchanged even though the model
and provider were loaded and healthy; only very short inputs transformed.

**Why:**
Both providers return `Err("stopped at max_tokens (truncated output)")` when
generation hits the cap, and the engine treats any `Err` as fallback-to-input.
The generic path used `GenerationParams::default()` (300 tokens), so once
prompt + output exceeded 300 tokens the transform silently vanished.

**How to avoid it:**
The generic path now scales `max_tokens` with input length
(`clamp(300, 2048)`), sized so prompt + output still fit the local server's
4096-token context. Never lower it blindly; check both providers' stop-at-cap
behavior first.

**Source / evidence:** this pass (2026-09-22); `openai_compat.rs:201`,
`server.rs:227`, `engine.rs` generic path.

### Gotcha G008

**Title:** Restarts can orphan `llama-server` child processes

**What happens:**
After one or more app restarts (especially `cargo tauri dev` rebuild cycles),
multiple `target/debug/llama-server` processes accumulate, reparented to PID 1,
each holding a full EG-1 model in RAM. Observed: 6 orphans at once
(2026-09-22).

**Why:**
Not fully root-caused (candidate): abrupt app termination (rebuild kill)
appears to skip `ServerProvider`'s `Drop`, so the child is never reaped. The
in-session `inference.replace()` path does kill its child correctly.

**How to avoid it:**
After restarting the dev app, check `pgrep -fl llama-server` and kill any
process that is not a child of the live `Teletype` PID. Also note
`cargo tauri dev` itself exits when the app quits, so a restart means
relaunching the whole dev stack, not just the binary.

**Source / evidence:** this restart pass (2026-09-22); cause unconfirmed.

### Gotcha G009

**Title:** A 0-byte `llama-server` stub silently wins binary lookup and fails as "Permission denied"

**What happens:**
Selecting a local LLM model (EG-1, Qwen3, S1-mini) errors with
`failed to spawn .../target/debug/llama-server: Permission denied (os error 13)`
under the misleading "Download failed:" UI label.

**Why:**
`tauri.conf.json` declares `bundle.resources.binaries/llama-server`, so a placeholder
file must exist for `cargo tauri build`; a 0-byte mode-644 stub was left at both
`crates/teletype-desktop/binaries/llama-server` and `target/debug/llama-server`.
`find_llama_server()` used `is_file()` only, so it picked the stub and `spawn()` returned
`EACCES`.

**How to avoid it:**
`is_runnable_binary()` (non-empty + exec bit) now gates every candidate in
`server.rs`, and `scripts/build-llama-server.sh` must have been run to install a real
binary. The build script passes `-DBUILD_SHARED_LIBS=OFF -DLLAMA_OPENSSL=OFF`: a shared
build resolves through an absolute `/tmp/llama.cpp-teletype/...` LC_RPATH and needs ~10
sibling dylibs the bundle does not ship, and the default OpenSSL link pulls Homebrew
`libssl.3.dylib`. Verify a candidate with `otool -L` (only `/usr/lib` + `/System`) and by
running it after moving the build dir aside.

**Source / evidence:** `learnings/2026-09-25-empty-llama-server-stub-shadowed-lookup.md` (L004).

### Gotcha G010

**Title:** VAD (and any sample-count-to-time math) must use the device's native sample rate, not the engine's

**What happens:**
"Stop after a pause" (VAD auto-stop) required ~2.4 s of real silence to fire
instead of the configured 800 ms. A 1+ second pause did not stop the take.

**Why:**
The capture stream runs at the device's native rate (`config.sample_rate()`,
commonly 48 kHz on Mac), and the VAD frame callback is fed audio at that rate.
But `VadDetector::silence_elapsed()` divided sample counts by a hardcoded
16 000 Hz. At 48 kHz each 512-sample chunk is 10.67 ms, not 32 ms, so the
detector counted 3x too few chunks per second and the 800 ms window stretched
to ~2.4 s of wall-clock silence.

**How to avoid it:**
`Recording::start_with_vad_rate` publishes the device rate to a shared slot
*before the stream plays* (no race with the first frame); the VAD frame
callback latches it via `VadDetector::set_sample_rate`. Any future code that
converts sample counts to time must take the actual feed rate, never assume
16 kHz. Regression test: `vad::tests::silence_timing_uses_actual_feed_rate_not_16k`.

**Source / evidence:** this fix pass (2026-09-26); `vad.rs` (`set_sample_rate`,
`silence_elapsed`), `audio/mod.rs` (`start_with_vad_rate`), `dictation.rs`
(rate slot).

### Gotcha G011

**Title:** Tauri IPC returns camelCase, but several UI screens read snake_case settings fields

**What happens:**
Clicking the "remove filler words" checkbox in Settings blanked the entire app
window (sidebar included). Other settings toggles (scratchpad, transforms,
styles) silently failed to persist.

**Why:**
The `Settings` struct is `#[serde(rename_all = "camelCase")]`, so the
`get_settings` / `save_settings` IPC carries camelCase keys (`removeFillerWords`,
`fillerWords`, `recordingMode`, ...). But `SettingsScreen`, `ScratchpadScreen`,
`TransformsScreen`, `StylesScreen`, `OnboardingScreen`, `App.tsx` and `pill.tsx`
read/wrote the **snake_case** names. On load every underscored field was
`undefined` (checkboxes showed unchecked, selects showed their first option by
coincidence); toggling "remove filler words" on then called
`settings.filler_words.map(...)` on `undefined`, which threw and unmounted the
whole React tree (no error boundary) -> blank window. `ModelsScreen` already
used camelCase correctly, which is why only some screens were broken.

**How to avoid it:**
UI settings field names MUST match the IPC casing (camelCase), not the Rust
struct field names. `InputDevice` has no `rename_all`, so its `is_default`
stays snake_case in the UI. A root `ErrorBoundary` in `main.tsx` now catches
render errors and shows a message instead of a blank window.

**Source / evidence:** this fix pass (2026-09-26); `ui/src/screens/*`,
`ui/src/pill.tsx`, `ui/src/App.tsx`, `ui/src/main.tsx` (new ErrorBoundary).

---

## 10. Verified learnings

Do not duplicate the full learning files here.

Keep only the most important summaries and links to detailed learnings.

| ID | Learning | Area | Status | File |
|---|---|---|---|---|
| L001 | Transforms silently did nothing: empty inference provider at startup + 300-token cap treated as fallback | transforms / inference | candidate (code-verified; app end-to-end run pending) | `learnings/2026-09-22-silent-transform-fallback.md` |
| L002 | Builtin dictionary seeds falsely rewrote ordinary words ("apt" -> "API", "its" -> "iOS") because they entered the edit-distance-2 sweeper; seeds are now exact-only via `DictionaryWord.fuzzy: false` with a version-2 migration | dictionary / pipeline | verified | `learnings/2026-09-22-dictionary-fuzzy-seed-collision.md` |
| L004 | Selecting a local LLM model failed with "Permission denied (os error 13)": a 0-byte stub kept for `bundle.resources` shadowed `find_llama_server()` (`is_file()` only); the shared llama-server build also depended on an absolute `/tmp` rpath + Homebrew OpenSSL | inference / build | verified | `learnings/2026-09-25-empty-llama-server-stub-shadowed-lookup.md` |
| L005 | Individual settings are half-wired: read once at startup (log in) + apply side effect (no reposition, no live-update) | settings / pattern | candidate (code-verified; app end-to-end run pending) | `learnings/2026-09-26-settings-individual-halves.md` |
| L006 | EG-1 model "Download failed: SIGKILL" was actually a macOS code-signing kill, not a timeout. The `llama-server` binary in `target/debug/` had an invalid ad-hoc signature (`codeSigningFlags=0x1000000`, `spctl` rejected). macOS killed it at launch before it could serve `/health`. Fix: `codesign --force --sign - target/debug/llama-server`. After re-signing, `codesign --verify` passes and fresh launches work. The 120s `READY_TIMEOUT` in `server.rs` is still useful as a cold-cache safety net but was NOT the root cause. **Always re-sign `llama-server` after a fresh `cargo build`** — the linker-signed ad-hoc signature from cargo is not accepted by macOS code-signing monitor. | inference / macOS code-signing | verified | — |
| L007 | "Expand autotext while typing" is implemented and verified: `crates/teletype-desktop/src/typing.rs` (306 lines) + `typing_tap.m` (passive `kCGSessionEventTap`/`ListenOnly` CGEventTap on the main run loop, mirrors `fn_tap.m`). The tap reports key-downs to a Rust buffer; on space/enter it looks up the trigger in `AutoTextStore` and expands via enigo backspace+type. Requires Accessibility permission (app already needs it). Research (pi, `tasks/pi-tasks/learnings/typing-autotext-research.md`) confirmed: slashanyware.com is a closed-source Chrome extension (no code to reuse); espanso is GPL-3.0 (incompatible with our MIT project, reference-only); the best optional refactor is swapping `typing_tap.m` for the `rdev` crate (MIT, 0.5.3, pure-Rust CGEventTap, layout-aware chars) to drop the hand-written ObjC. Current design ships as-is; no GPL exposure. | typing / autotext / macOS | verified | `tasks/pi-tasks/learnings/typing-autotext-research.md` |
| L008 | T1.1 streaming LLM + live pill preview is built and tested, now with **true network-level streaming (T1.1b done)**: `InferenceProvider::generate_with_system_stream` (default falls back to batch) + `ServerProvider::chat_stream` that opens a **raw TCP connection**, hand-writes the HTTP/1.1 request (`Connection: close`), and reads the SSE body **incrementally** via `SseFrameParser` (buffers partial frames, yields `data:` contents as they arrive, stops at `[DONE]`). `on_token` fires per token while the model is still generating, so the pill shows words landing progressively. Verified live against llama-server: frames arrive over the wire progressively (2 at 0.11s, 5 by 0.18s), not in a burst. Helpers `parse_localhost_base_url`, `read_status_and_headers`, `read_to_eof` are pure/testable. Tests: 235 core / 33 inference (incl. `sse_frame_parser_streams_across_chunk_boundaries`) / 19 desktop all pass. | streaming / inference / pill | verified | — |
| L009 | **Blank white window after a fresh build = the binary is loading `devUrl` (http://localhost:1420), not `frontendDist`, and vite is not running.** Root cause: plain `cargo build --release` (or `cargo build -p teletype-desktop`) does NOT set Tauri's production-mode internal flag, so the resulting binary serves the frontend from `devUrl` instead of `frontendDist`. With vite down, the webview gets nothing and the window is blank white. The fix is to build with the Tauri CLI, which sets the flag and embeds `frontendDist`: `cargo tauri build` for release (no vite needed) or `cargo tauri dev` for development (auto-starts vite on 1420). **Never launch a plain-`cargo build` binary expecting it to show the UI** without vite running. Also: `tauri.conf.json` `frontendDist` must stay the portable relative path `../../ui/dist` (an absolute machine path and a `dist` symlink are non-portable and were a dead-end). | build / Tauri / frontend serving | verified | — |

---

## 11. Open questions

Questions that affect future implementation.

| ID | Question | Why it matters | Owner | Status |
|---|---|---|---|---|
| Q001 | Is the uncommitted P0 batch (ServerProvider, OpenAiCompat, keyring, catalog) verified end-to-end, and should it be committed? | Blocks clean baseline for roadmap Sprint A; user flagged it "needs review/verification" | user + agent | resolved 2026-09-25: batch in `3fa2793`, P0 fixes on main in `9163631` + `e2a9ec9` (worktree commits `0e8b94e..1658caf` are NOT on main); baseline verified at `8765f05` (build clean, 246 tests pass) |
| Q002 | llama-server distribution: download on first local-model use vs optional installer? | Affects packaging and the 13 MB binary constraint | user | open (roadmap open decision #2) |
| Q003 | Default provider priority chain when Apple Intelligence and local server both exist (order + UI copy)? | Roadmap P1.4 / open decision #3 | user | open |
| Q004 | Windows: which seam lands first given "equal priority" (hotkey, injection, tray, mic)? | D005 says equal priority but no Windows plan exists yet | user | open |
| Q006 | "Detected <Lang>. Lock it?" chip after auto-detect (EW's LanguageChipView pattern)? | Follow-up to the 2026-09-25 language picker work; needs detected-language signal from ASR (P2-35) | user | open |
| Q005 | Repo hygiene: root `README.md` is the brain-starter doc while the public README lives in `README-github.md`; also stray untracked files exist | Confusing for contributors and agents | user | open |

Agents may answer an open question from reliable evidence.
When evidence is insufficient, ask the user rather than inventing an answer.

---

## 12. External sources

Track important external sources that shaped the project.

| Source | What we learned | Last checked | Notes |
|---|---|---|---|
| `github.com/karansinghgit/speaktype` (MIT) | Base patterns: audio, state machine, paste injection, hotkey, tray, model downloads | 2026-09-22 (via docs) | Attribution required; Phase 0 audit in `docs/architecture.md` |
| EnviousWispr source clone (persistent as of 2026-09-23 at `enviouswispr/EnviousWispr`, gitignored; older temp path in `roadmap.md`) | EW feature map, EG-1/S1 delivery, paste cascade, VAD, splitter, keychain. Spoken-lists implementation: prompt rules only, no deterministic pass. Cloud rule in `Sources/EnviousWisprLLM/Prompting/CloudFixedPromptBuilder.swift:119` ("never put two items on one line"); Apple on-device rule 11 with worked example in `AppleIntelligenceConnector.swift:316`; local/Ollama restraint rule in `LocalFixedPromptBuilder.swift:96` ("a single sentence is never a list; clauses joined by and/but/so are never a list"); EG-1 (`EGOnePromptBuilder`) has NO list rule, behavior lives in weights; S1-mini gets a user-settable control line `[Structure: lists\|prose]` from `S1ControlSettings.swift` (default lists, trained closed enum). Eval truth in `scripts/eval/behavior_judge.py`: `list_format_required` makes one-item-per-line a hard requirement (inline run = `major_fail` "wrong_format"); punctuation, capitalization and bullet-vs-numbered are allowed variants; measured 2026-08-15, EG-1 produced a list on 1 of 114 sealed `spoken_list` cases; every runtime prompt example ends items with a period (the website "•" chip without periods is marketing). Teletype equivalent: deterministic `format_spoken_lists` in `engine.rs` for polish transforms (incl. repairing model inline-hyphen output) + `CORE_RULES` for editable prompt paths | 2026-09-23 | Reference only |
| `https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt` | EG-1 usage/redistribution terms | 2026-09-22 (HTTP 200 verified per roadmap) | Drives D001 |
| EG-1 / S1-mini download URLs + SHA-256s | Catalog entries and checksums | 2026-09-22 (verified live per roadmap) | In `roadmap.md` and `catalog.rs` |
| `prompt.md` (original spec) | Product requirements, quality bar, privacy rules | 2026-09-22 | In-repo primary spec |
| LiveKit Agents UI (`github.com/livekit/components-js`, `packages/shadcn`, Apache-2.0) | Dot-matrix grid visualizer ported as the `dotGrid` pill style. Component is plain React+CSS (no WebGL); the LiveKit coupling is only `useMultibandTrackVolume`/`useAgent` wrappers, and the official `volumeBands` prop is the clean injection seam. Aura/Wave visualizers ARE WebGL shaders and are not cheap ports | 2026-09-26 | Vendored with attribution in `ui/src/pill.tsx` |
| `github.com/jaredpalmer/kev` (Apache-2.0) + `logicrw/awesome-jev-projects` + pinggy.io JEV-alternatives benchmark (2026-09-23) | Jev = TypeSafe's closed "System One" decision model: state + typed questions (Choice/Score/Noul) → probability per option in one parallel pass, no token generation, 50-100ms hosted. Open clones (Laya 421M BERT, Kev 0.8-27B Qwen+LoRA, SemIf frozen-logit wrapper, NanoJev 0.6B, jevlike trainer) reproduce the interface, NOT zero-shot accuracy: independent jabr 49-task benchmark = Jev 0.966 vs best-open 0.704. Kev-0.8B MLX on Apple Silicon: 47ms warm / 77ms fresh per multi-question request. Teletype fit: NOT a drop-in for polish (text generation, wrong job); plausible use = fast intent classifier (polish vs raw insertion), context-app detection, transform routing. Blocked on: Python sidecar vs Rust-only + llama-server D002 precedent, 13MB binary + no-bundled-weights constraint, zero-shot accuracy gap | 2026-09-26 | Research only, no code written; user asked "useful or not" before any implementation |
| `github.com/yijunyu/jev-rs` (Apache-2.0/MIT) + `jev-sdk` crate (docs.rs) | Rust-native Jev-class engine: reads next-token logprobs from any llama-server GGUF (Qwen3-4B: 75ms p50 warm, zero output tokens), wire-compatible `POST /v1/systemone`, `jev ask`/`serve`/`mcp`/`eval`/`calibrate`. Accuracy on dev_tasks: 0.71 overall (choice 0.92, noul 0.64, score 0.56) — same 0.70 open-class ceiling. Reuses the llama-server Teletype ALREADY bundles (D002 subprocess, no new runtime, no Python). `jev-sdk` = hosted-TypeSafe-only client (network, violates D003). Fit: strictly better than Python Kev for a future agent layer (same accuracy, Rust, no new sidecar), but still not useful for current deterministic pipeline | 2026-09-26 | Follow-up to Kev research; no code written |

---

## 13. User decisions that should not be lost

Record explicit user decisions that are likely to matter later.

| Date | Decision | Context |
|---|---|---|
| 2026-09-22 | Personal open-source project, never commercial | Brain initialization |
| 2026-09-22 | `roadmap.md` is plan of record; `checkpoint.md` is historical | Brain initialization |
| 2026-09-22 | Windows is equal priority with macOS | Brain initialization |
| 2026-09-22 | Uncommitted P0 batch needs review/verification before commit | Brain initialization |
| 2026-09-22 | Approved three-part transform fix: provider rehydration at startup, surfacing the silent pipeline fallback, EW-derived generic prompt rules + builtin dictionary seed ("yes please") | Transform quality pass |
| 2026-09-22 | Spoken enumerations must render as points: "first, second, third" narration and colon-announced lists ("Bring the following ...: apple, grapes, banana and onion") each become `- ` bullet lines, matching the EW website "Spoken lists" demo | User request, dictation + screenshot |
| 2026-09-23 | List format fixed to match EW exactly: lead-in line ending `:`, one `- ` item per line, capitalized, trailing period (their runtime prompt examples; judge treats punctuation as an allowed variant). Inline one-line hyphen runs from EG-1 are a bug fixed deterministically, not left to the model. EW clone kept persistent at `enviouswispr/` and gitignored | User, after live dictation produced a one-line hyphen run |
| 2026-09-25 | Port EW's Transcription-tab settings in this order: (1) language auto-detect + full picker, (2) stop-on-silence VAD + pause-duration slider, (3) spoken emoji + spoken punctuation toggles, (4) engine picker cards + tabbed Settings (Transcription/AI Polish/General), (5) unload-model-after timer. Skip streaming ASR and live preview (P2) | User, after EW vs Teletype settings gap analysis |
| 2026-09-25 | Language setting: `"auto"` now passes through to Whisper (auto-detect); Parakeet ignores the language arg in its wrapper. Settings picker is model-aware (Auto + the selected model's language list) | Item (1) of the EW settings port; `effective_language` in `dictation.rs`, `SettingsScreen.tsx`, `SpeechModelStatus.languages` |
| 2026-09-25 | Pill style picker with 4 choices: Teletype (default, original), Classic Capsule, Level Rail, Reading Well (ported from EW). `pill_style` setting (default `"default"`). Reading Well's live-preview well is chrome-only until streaming ASR (P2-29) | Item (pill port) of the EW settings port; `pill.tsx` (ClassicPill/LevelRailPill/ReadingWellPill + RainbowLips/RainbowMeter/RainbowHairline), `SettingsScreen.tsx`, `Settings.pill_style` in `commands.rs` |
| 2026-09-25 | VAD auto-stop: pure-Rust `silero-vad-pure` engine (no ONNX), stop policy in `teletype-core::vad` (fire only on silence after speech, default 800 ms, leading silence never stops). Settings `vad_auto_stop` (default off) + `vad_silence_ms` slider (300..2000). Active ONLY in hands-free hold mode (double-tap start): plain hold stops on key release and push-to-talk stops on the second tap, so both would preempt VAD. Capture thread feeds an `Arc<Mutex<Option<VadDetector>>>` slot via `Recording::start_with_vad`; on `UtteranceComplete` the slot is dropped (one-shot) and `Event::VadFired { session }` stops the take; session check is the second gate. VAD engine init failure falls back to manual stop. Latency gate: test asserts < 10 ms per 32 ms chunk (roadmap P2.2). **Sample-rate fix (2026-09-26):** the detector once hardcoded 16 kHz when converting silent chunks to wall-clock time, but the capture stream feeds the device's native rate (commonly 48 kHz on Mac), so an 800 ms pause took ~2.4 s of real silence to fire ("1+ second pause not working"). `VadDetector::set_sample_rate` + `Recording::start_with_vad_rate` now publish the device rate (stored before the first frame, no race) and the frame callback latches it; regression test `silence_timing_uses_actual_feed_rate_not_16k` | Item (2) of the EW settings port; `vad.rs` (`set_sample_rate`/`silence_elapsed`), `audio/mod.rs` (`start_with_vad_rate`, rate published pre-play), `dictation.rs` (`vad_enabled_for`/`start_with_vad`/`VadFired` + rate slot), `Settings.vad_auto_stop`/`vad_silence_ms` in `commands.rs`, `SettingsScreen.tsx` |
| 2026-09-26 | 5th pill style "Dot Matrix" (`dotGrid`): ported from LiveKit Agents UI `AgentAudioVisualizerGrid` (Apache-2.0, attribution + modified notice in `pill.tsx`). Vendored as inline-style React (no Tailwind/shadcn/livekit deps); LiveKit's FFT bands replaced by the scalar `pill-level` history (newest at right), agent states mapped warming→connecting (ring sweep), recording→speaking/listening (volume rows grow from middle row), processing→thinking (row scan). 24x3 strip inside the existing 520x84 window (no Rust resize); idle keeps the default bars. 100ms interval only runs while mounted. Verified by headless-Chrome screenshots of all three phases via a temporary `pill-preview.html` harness (deleted after) | User request after LiveKit Agents UI compatibility analysis; `pill.tsx` (DotGrid* block), `SettingsScreen.tsx` PILL_STYLES, `commands.rs` doc comment |

---

## 14. Brain maintenance notes

### When an agent should update this file

Update when:
- durable project context is learned
- architecture changes
- a decision is made
- a constraint is discovered
- a recurring gotcha is confirmed
- an existing entry becomes stale

### When not to update

Do not add:
- temporary thoughts
- ordinary implementation details
- one-off debugging noise
- guesses
- secrets
- duplicated information

---

## 15. Future company brain hook

This project intentionally keeps company-wide knowledge separate.

Future configuration may reference a shared company brain here:

```yaml
company_brain:
  enabled: false
  location: ""
  version: ""
```

When enabled:
- project knowledge remains authoritative for project-specific facts
- company knowledge provides shared guidance and reusable patterns
- conflicts must be surfaced, not silently merged
- company knowledge must not require changing this file's schema

## 16. Session preferences

- **Auto-compaction threshold: 10% minimum** (2026-09-26). Do not let context
  auto-compact below 10% remaining. If context is getting tight, compact
  proactively at ~20% remaining, not at the last moment. This prevents
  mid-task compaction that loses working state.

## 17. Project skills (.agents/skills/)

Reusable `/ali-*` skills for this repo (read the SKILL.md when the trigger fires):

| Skill | Trigger | Purpose |
|---|---|---|
| `ali-pi-team` | `/ali-pi-team` | The build team: decompose a task, delegate MECH units to pi (background), do QUICK units inline, verify before done. |
| `ali-verify-pi-work` | `/ali-verify-pi-work` | Verify a pi "done" claim: re-run its commands, read the real diff, check for crash artifacts. Never trust a pi report alone. |
| `ali-app-dev-loop` | `/ali-app-dev-loop` | Start/restart the Tauri dev app correctly (vite 1420 + binary + llama-server re-sign L006) and confirm the window serves. |

**Convention:** when Ali gives a build task, default to `/ali-pi-team`. After any pi
task, run `/ali-verify-pi-work` before reporting done. When the app needs a live check
or comes up blank, use `/ali-app-dev-loop`.
