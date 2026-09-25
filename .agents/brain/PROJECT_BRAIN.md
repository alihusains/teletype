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
verified as of 2026-09-20 (`checkpoint.md`). A P0 "Polish" batch (subprocess LLM
server, OpenAI-compatible provider, keyring secrets, expanded model catalog) is
written but **uncommitted and not yet end-to-end verified** (user-confirmed
2026-09-22). Tests green at time of writing: `cargo test --workspace` all passing,
`tsc --noEmit` clean, clippy warnings pre-existing only.

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

**Status:** accepted (implementation in uncommitted P0 batch)

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

**Status:** accepted (implementation in uncommitted P0 batch)

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

---

## 8. Known constraints

Record constraints that future agents must know.

| Constraint | Why it exists | Evidence / source |
|---|---|---|
| Never link llama.cpp in-process | ggml duplicate symbols vs whisper.cpp | `checkpoint.md`, D002 |
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

---

## 10. Verified learnings

Do not duplicate the full learning files here.

Keep only the most important summaries and links to detailed learnings.

| ID | Learning | Area | Status | File |
|---|---|---|---|---|
| L001 | Transforms silently did nothing: empty inference provider at startup + 300-token cap treated as fallback | transforms / inference | candidate (code-verified; app end-to-end run pending) | `learnings/2026-09-22-silent-transform-fallback.md` |
| L002 | Builtin dictionary seeds falsely rewrote ordinary words ("apt" -> "API", "its" -> "iOS") because they entered the edit-distance-2 sweeper; seeds are now exact-only via `DictionaryWord.fuzzy: false` with a version-2 migration | dictionary / pipeline | verified | `learnings/2026-09-22-dictionary-fuzzy-seed-collision.md` |

---

## 11. Open questions

Questions that affect future implementation.

| ID | Question | Why it matters | Owner | Status |
|---|---|---|---|---|
| Q001 | Is the uncommitted P0 batch (ServerProvider, OpenAiCompat, keyring, catalog) verified end-to-end, and should it be committed? | Blocks clean baseline for roadmap Sprint A; user flagged it "needs review/verification" | user + agent | open |
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
