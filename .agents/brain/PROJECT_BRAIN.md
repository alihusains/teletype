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
Feature-rich and working on macOS. **Do not describe the product as "at parity
with Wispr Flow"**: the honest position is measured per-axis, and the platform,
language, and streaming-preview gaps are real. `usp.md` §3/§4 is the current
verified competitive sheet.

**Full workspace test suite green (2026-10-06 re-run):** `cargo test --workspace`
→ **0 failures** across all crates (389 core, 73, 62, 50, 48 inference, 15, 13,
11, 6, 4, 2, 1 + doc-tests). `tsc --noEmit` clean, `vite build` green. This is
the verified "battle-tested, fully working" baseline for the production-readiness
pass.

**15-item overhaul (2026-10-07, v0.2.5):** All 15 items implemented and
committed. Key additions: (1) grouped sidebar nav (App/Record/Process/You/
System sections, item 11); (2) selection transforms with global shortcuts
(Wispr Flow style, item 2); (3) list-style setting bullets/numbered (item 3);
(4) create-your-own transform modal (item 4); (5)+(7) self-learning dictionary
wired into `record_dictation_edit` → `learn_terminology_to_dictionary` (items
5, 7); (6) vocabulary pack word chips with × remove (item 6); (8) Quick Add
global shortcut → `quick_add_selected_word` (item 8); (9) system-sound fix:
`pathForResource` never searches absolute dirs, path built directly
(item 9); (10) dedicated Keybinds screen with record/cancel/quick-add/
transform shortcuts + conflict detection (item 10); (12) responsive pill
previews (item 12); (13) real transcripts path via `get_transcripts_dir` +
`TranscriptsDirField` (item 13); (14) settings grouped into Dictation/Input/
Per-app/Floating Pill/App Icon/Appearance/Transcripts/About (item 14);
(15) user-created vocabulary packs: `UserPackStore` in `teletype-core`,
persisted to `user_packs.json`, fed into the correction tier alongside
built-in packs (item 15). New IPC commands: `list_user_packs`,
`create_user_pack`, `delete_user_pack`, `set_user_pack_enabled`,
`add_user_pack_word`, `remove_user_pack_word`, `quick_add_selected_word`.
New settings fields: `quickAddHotkey`. New Insights fields:
`dictionaryWords`, `learnedWords`, plus a "Fastest take"
personal record (the single best measured speaking rate, clamped to a
plausible range and shown only when a take was long enough to time).
Sound-cue fix (item 9) is learning L021: NSBundle `pathForResource:
inDirectory:` never searches absolute directories, so every system-sound
lookup returned nil and the cue was silently dead; the path is now built
directly. All 12 nav screens + Keybinds = 13.
Full test suite green: 483 core+ipc+eval, 79 desktop, tsc clean, clippy
clean, fmt clean.

Production-readiness pass (2026-10-06, uncommitted): all 12 tabs verified
rendering + key interactions; the Dictation "Choose file" button is fixed
(missing `dialog:allow-open` capability — see L016); the dev-tools auto-open
bug is removed in favor of a `devtools_toggle` command + Developer-screen
button; and the **Insights tab got a premium revamp** (richer multi-layer
gradient hero, elevated cards via `--shadow-*` tokens, icon-chip headers,
tabular-nums figures, gradient bar/donut/phrase bars, section eyebrows) with
no change to the data model, IPC calls, `dictation-state` refresh, or
empty-state copy — built on the `ui/index.html` design tokens (see L017).
During the tab-by-tab battle test a real bug was found and fixed: every
destructive action gated by the global `confirm()` (Transforms, AutoText,
Personalization, Scratchpad) was silently denied because the capability only
allowed `dialog:allow-open`. Tauri's `confirm()` invokes the dialog plugin's
`message` command, so the fix is adding **`dialog:allow-message`** (not
`dialog:allow-confirm`) to `capabilities/default.json` — verified the native
OK/Cancel NSAlert now appears (see L018). Tab-by-tab battle test complete
(2026-10-06): all 12 nav screens — Home, Dictation, Insights, Transforms,
AutoText, Dictionary, Style, Scratchpad, Personalization, Models, Settings,
Developer — verified rendering with real data and key interactions. Remaining
open items: a real-microphone end-to-end dictation pass (untested in this
environment) — now resolved via the file path: `transcribe_file` was driven
through the real app (DevTools `invoke('transcribe_file',{path})`) on a
synthesized WAV, and it produced a correctly transcribed + cleaned history
entry ("The quarterly report is ready to send to the team by end of day.",
tagged `File: hello.wav`), exercising decode → Parakeet ASR → ITN → AutoText →
filler removal → transform → emoji restore → history. Note: the symphonia
decoder rejects AIFC (`no suitable format reader found`) but accepts plain
LEI16 WAV. React duplicate-key `''` warnings: every `key=` across all screens
and settings sections was audited and none can be empty in the current data;
the one data-driven key with a theoretical empty-value path (the Insights
`Donut`, keyed by app label) was hardened to a stable index key
(`InsightsScreen.tsx`, `tsc`-clean). The exact dev-only warning could not be
captured because the Codex CLI terminal reclaims focus between keystrokes, so
DevTools-console capture is unreliable in this environment (L018) — it is
non-functional and dev-only, so it does not affect any feature.

S1-mini parity re-verified live (2026-10-06): spawned `target/debug/llama-server`
on the local `s1-mini.gguf` with the current spawn flags (`--jinja
--chat-template-kwargs '{"enable_thinking":false}'`) and the current `run_s1`
request shape; all three control axes (styling/structure/context) returned
correct, distinct output with `finish_reason=stop` and no empty-content
failures — matches the Better Stack reference video (see L013/L014). During
this pass a stray **installed** `/Applications/Teletype.app` instance was found
running in parallel with the dev build (goal item 3: no duplicate copies) and
was killed; only the dev build + vite remain. It kept relaunching (manual, not a
login item), so on 2026-10-06 it was **moved to Trash** — the dev build is now
the only Teletype on the machine, so the duplicate-process recurrence is gone.

Four changes landed 2026-09-27 (this session, uncommitted at time of writing):

1. **Direct AX injection tier** (`crates/teletype-desktop/src/ax_text.rs`). Tier 1
   writes at the caret through the Accessibility API with read-back verification,
   removing ~600 ms of fixed sleeps (250 ms pre-paste + 350 ms pre-restore) and the
   clipboard round trip on every dictation. Falls back to the clipboard when AX is
   unavailable or the write cannot be confirmed. New `Platform::insert_text` /
   `focused_text` default to the clipboard, so Windows and Linux are unaffected.
   Built against `objc2-application-services` rather than an `.m` bridge: the AX
   headers ship only in the full Xcode SDK, and the pre-generated bindings compile
   with the Command Line Tools SDK that CI uses.
2. **Personalization loop is actually wired** (`edit_watch.rs` +
   `dictation.rs::observe_pending_edit`). It was dormant: `record_dictation_edit`
   had no production caller. The observation point is the *next* dictation, which
   is when the user has finished editing the previous one. Every guard fails closed.
3. **Eval harness + CI gate** (`crates/teletype-eval/`, `evals/`). 445 tests pass.
   Replay mode runs in CI with no model; measure mode produced the first real
   numbers: **ASR 142 ms p50 / 665 ms p95, micro-WER 0.056** over a 17-case
   `say`-synthesised corpus. Human-recorded audio is still needed before any claim
   about real-speech accuracy.
4. **Signing, notarization, and updater** (`docs/signing-and-updates.md`,
   `release.yml`, `tauri.conf.json`). Tauri updater chosen over Sparkle. The
   updater keypair exists; the Apple credentials do not, so releases are still
   unsigned until those secrets are added.

**2026-10-01 placeholder audit (EW v2.5.2 delta review).** Verified against the
tree; task files `tasks/honesty-block.md`, `tasks/deferred-settings-ui-misc.md`,
`tasks/deferred-lifecycle-hardening.md` were stale and are now corrected.
Verdicts:
- BUG-002 (spoken emoji/punctuation toggles) — **fixed**: `transforms/spoken_emoji.rs`
  + `autotext/system.rs`, both wired via `pipeline.rs`.
- BUG-003 (unload-model timer) — **fixed and hardened**: `end_dictation(delay)`
  arms the deadline on session end; a 5 s tick thread in `lib.rs` drives
  `SpeechModelManager::tick()` for both speech managers. 2026-10-01: the
  launch-time warm-up path now disarms a pending deadline
  (`end_dictation(None)`) before loading, so a warm-up cannot be undone by the
  tick thread; new end-to-end test
  `end_dictation_arms_unload_that_tick_fires_on_a_real_provider` in
  `teletype-inference/src/manager.rs`.
- BUG-013/014/015/016 (lifecycle) — **all fixed** (commit `d34f6d0` + T1.1
  streaming + P1-A `llm_loading` gate + `.part`→verify→rename downloads).
- BUG-017/018 (fillerCounts, save race) — **fixed** (`d34f6d0`).
- P5.1 polish gate — **done as a deterministic rule** (`transforms/gate.rs`,
  on by default), not the logprob `DecisionProvider` the P5 section describes.
- Still open: language-drift validator (NOTE in `validator.rs`), ITN 2b
  (`itn_parity_fixtures` `#[ignore]`d), P1.4 Apple Intelligence provider (dep
  removed, never built), VAD is the pure-Rust Silero port (no ONNX model).

**Streaming ASR was measured and rejected, not skipped.** Parakeet TDT has no
streaming state in this binding, so a sliding window re-derives the entire
utterance every call. The measured provisional tail is the whole window
(p50 11.4 s), so there is no stable prefix to commit and the roadmap's
"11 s chunk / 1 s hypothesis" design cannot work. The interim pill preview
therefore shows a *provisional* whole-take re-decode, and the inserted text is
still the single authoritative full decode. Details and the viable alternatives:
`tasks/pi-tasks/streaming-asr-spike-findings.md`. The reference project's numbers
come from a different engine arrangement and do not transfer to us.

Earlier status (kept for history): the P0 "Polish" batch (subprocess LLM server,
OpenAI-compatible provider, keyring secrets, expanded model catalog) was
committed in `3fa2793` (2026-09-23), with verified P0 fixes on `main` as
`9163631` (P0-8 scaled timeouts) and `e2a9ec9` (P0-2/4/5/6/7/9/11/12/13,
2026-09-25). Recent `main` commits (2026-09-25/26): `38843a8` (T2.1
personalization backend), `b7ff674` (T3.2 privacy front-page), `9a1969d`
(P3.7/P3.9/P2.3 placeholders + import/export + recovery spool), `903eab0` (P5.1
rule-based polish gate), `3d9f6f7` (P3.2/P3.3 emoji restore + per-app language
overrides), `3c9783a` (W1: Windows compile gate + WH_KEYBOARD_LL typing hook +
cross-platform speech). Independent verification of the then-completed-task claims
(OpenCode2, 2026-09-25): 11/14 confirmed with file:line evidence.

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
| Personalization retention priority | The more the user uses the app, the more it learns, and the more visible that learning is. "Less effort, high impact, quick wins" is the sequencing rule for the T5 sprint. | user answer | 2026-10-02 |
| T5 sprint in roadmap | `roadmap.md` "2026-10-02 — Personalization & Insights visibility sprint" (T5.1-T5.13) is the current active plan for making personalization + insights visible. | user answer | 2026-10-02 |

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
| `docs/` roadmaps | Platform roadmaps (2026-09-26) | `roadmap-windows.md` (Windows port plan + measured learnings from the reference), `roadmap-macos.md` (macOS completion/parity/hardening plan) |
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
cargo clippy --workspace --all-targets   # 0 real lints (2026-09-28 re-verify); 10 clang linker-noise lines only
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

### Decision D007

**Title:** Windows ships via native `windows-latest` CI builds; typed-AutoText works on Windows via a `WH_KEYBOARD_LL` hook

**Date:** 2026-09-26

**Status:** accepted (implemented in `3c9783a`; full plan in `docs/roadmap-windows.md`)

**Decision:**
The Windows layer is built natively, not cross-compiled from macOS. CI runs a
`windows-latest` job (`cargo check --workspace` under MSVC) because whisper.cpp/ring C
dependencies cannot cross-compile from a macOS host (they need a native MSVC C
toolchain). The typed-AutoText watcher runs on Windows through a `WH_KEYBOARD_LL`
low-level keyboard hook (`typing_windows.rs`) on its own message-pumping thread, feeding
the same `i32` channel the macOS CGEventTap uses; `decode_key()` unifies the two
encodings (space/enter sentinels shared, `char` codepoint on Windows vs virtual-key-code
on macOS). Speech on Windows resolves to the Whisper (whisper.cpp) path because Parakeet
is CoreML/macos-only.

**Context:**
W1 was originally a cross-compile gate that failed on GitHub runners (ring C code). The
user chose the native `windows-latest` job. The reference project
(`enviouswispr/enviouswispr-windows/`) carried a real Windows port through 23 phases with
measured evidence; its mechanisms and numbers inform `docs/roadmap-windows.md`, but the
language (C#/WinUI 3) does not carry over to Rust/Tauri.

**Alternatives considered:**
Cross-compile from macOS (rejected: C deps need native MSVC toolchain); stub the Windows
speech providers (rejected: native builds make real providers viable).

**Evidence / source:** `3c9783a`; `.github/workflows/ci.yml`;
`crates/teletype-desktop/src/typing_windows.rs`; `docs/roadmap-windows.md` (learnings
section).

### Decision D008

**Title:** An ambiguous System AutoText phrase is decided by a deterministic context rule, not an LLM call, and ships with no settings switch

**Date:** 2026-09-28

**Status:** accepted (implemented; `crates/teletype-core/src/autotext/disambiguate.rs`)

**Decision:**
A System AutoText entry whose phrase is also ordinary English expands only when
the utterance shows the user was naming a character: the utterance is 1-2 words,
a naming cue is within two words before it, it opens or closes the utterance, or
an insertion marker follows it. `use` is deliberately not a naming cue. The
decision is one pure function, `disambiguate::entry_allowed`, called from both
`expand_snippets_with` and `protect_snippets_with`. A **custom** entry is exempt.
**No settings switch is added.**

**Context:**
The bug was the corruption of running text, not a preference: "the payment
period ends in March" typed `the payment. ends in March`, and because
`protect_snippets_with` runs first, the model was asked to polish
`the payment{{AUTOTEXT_0}} ends in March`. Measured, the polish gate returns
`short_clean` for every one of these sentences, so an LLM-based fix would have to
add a brand-new model call to the fastest path (500 ms-3 s on a 1.65 s median
dictation) for a bug that fires on roughly 1 word in 50.

The no-switch decision is a deliberate departure from the earlier recommendation
to keep one as a safety net. A switch would be a way to get the bug back, nothing
that worked correctly stops working, "full stop" and "insert a comma" are already
the escape hatch, and seven persisted settings already have no UI, each one a
place a user cannot undo a decision.

**Alternatives considered:**
- LLM disambiguation (rejected: new call on the fastest path; the polish gate
  cannot serve it).
- Restricting `DEFS` to phrases with no ordinary-word reading (rejected: deletes
  "comma", "period" and "star", which are the words people actually use).
- A real decision model (Jev / open-source Kev). Right shape, wrong runtime: its
  only implementation is Python + torch/MLX on Qwen3.5/3.8 with no llama.cpp
  path, which would break D002 and D003. Latency was never the objection
  (measured 149 ms on an M5). What was taken is the *decision point*: one small
  pure function that a calibrated model can replace later without touching the
  AutoText engine. See `docs/research/jev-kev-for-symbol-disambiguation.md`.
- A settings switch (rejected, above).

**Evidence / source:** `disambiguate.rs`; `text_integrity.rs`
(`protected_system_autotext_leaves_ordinary_words_alone`,
`protected_symbol_requests_still_expand`,
`protected_autotext_placeholder_is_not_inserted_for_prose`,
`protected_custom_snippets_are_never_ambiguous`).

### Decision D009

**Title:** Intel macOS ships as a separate x86_64 cross-compiled build, non-blocking, and the updater manifest is arch-verified

**Date:** 2026-09-28

**Status:** accepted (implemented; `ci.yml` `macos-intel-build`, `release.yml`
`release-macos-intel`)

**Decision:**
Two macOS builds, not a universal binary. Intel is cross-compiled from the same
Apple Silicon runner (`--target x86_64-apple-darwin`), so there is one macOS
build story. The job is `continue-on-error` in CI and in release: Intel is an
option, not a promise, and must never be able to block an arm64 or Windows
release. README says Intel is built but **not supported** and slower.

The updater manifest advertises `darwin-x86_64` **only** when the Intel artifact
exists and its own recorded architecture says x86_64.

**Context:**
A universal `.app` would need a `llama-server` that runs on both architectures,
and llama.cpp is compiled per architecture, so "universal" means two
llama-servers in one bundle, twice the size, and not code-signable as a single
file.

Before this, `release.yml` pointed `darwin-x86_64` at the arm64 payload. That is
the worst kind of bug: the signature is genuine over the arm64 tarball, so it
verifies, the app installs, and then refuses to launch on the Intel Mac that
asked for it.

**Alternatives considered:**
- Universal binary (rejected above).
- A native `macos-13` Intel runner (rejected: that runner label is a
  deprecation risk, and cross-compiling Darwin to Darwin needs no extra
  infrastructure).
- Advertising `darwin-x86_64` unconditionally (rejected: that is the original
  bug).

**Evidence / source:** verified locally, not assumed. `cargo check --workspace
--target x86_64-apple-darwin` passes (2m11s). `llama-server` cross-builds to
`Mach-O 64-bit executable x86_64`, 15,983,296 bytes, CPU and Metal backends. The
manifest assembly is exercised by a harness over 8 scenarios, including 5 that
must refuse to publish.

### Decision D010

**Title:** Ship unsigned builds instead of failing on missing signing secrets

**Date:** 2026-10-03

**Status:** accepted (implemented; `release.yml`, `scripts/check-updater-manifest.sh`, commit `ffca250`)

**Decision:**
An empty-but-present signing secret (absent or whitespace-only) is treated as
absent. Preflight and build steps trim before deciding whether to sign, and
clear the signing env vars before `cargo tauri build` so Tauri takes its
unsigned path. The updater manifest omits unsigned platforms (with a notice)
instead of refusing to publish; a fully unsigned release writes an
empty-platforms manifest. `pub_date` is stamped in UTC rather than deleted,
because the updater plugin treats a missing `pub_date` as "no update available."

**Context:**
The Apple and updater secrets are not yet configured, so every tagged release
was blocked even though an unsigned build installs fine (Gatekeeper asks the
user to right-click open). An empty-but-present secret was worse than an absent
one: Tauri does not distinguish them, and an empty value fails `security import`
/ minisign decode deep into a 20–30 minute build.

**Alternatives considered:**
Fail the release when signing is absent (rejected: blocks a valid, installable
artifact); distinguish absent vs. empty at the Tauri layer (rejected: not
controllable from the workflow side).

**Evidence / source:** full reasoning in `adr/ADR-010-unsigned-builds-ship.md`.

### Gotcha G014

**Never `cp` a built binary over the path an app executes. Rename into place.**

**Symptom:** `llama-server` is SIGKILLed instantly, writes a 0-byte log, and the
app reports only "Polish skipped: no model loaded". Nothing names the cause.
Worse, it happened *after* the user selected a model, so it reads as the model
or the selection being broken.

**Why:** `cp` writes into the existing inode. When a `llama-server` is already
running from that path, which is the normal case in a dev loop, overwriting the
inode leaves a file macOS refuses to execute. The kill is `SIGKILL` with the
reason **`Code Signature Invalid`** (visible only in
`~/Library/Logs/DiagnosticReports/llama-server-*.ips`: `EXC_CRASH`, `signal:
SIGKILL (Code Signature Invalid)`, `bug_type 309`, before any frame). `file`
still reports a valid Mach-O and the executable bit is still set, so every
plausible check passes.

**Reproduced, then fixed, and both directions verified:**
- `cp` onto a file a process is exec'ing from → next exec exits 137.
- `codesign --force --sign -` on the same bytes → works again. Identical
  content, so it is the inode's state, not the bytes.
- `mv -f` a fresh temp file into place → works, and the running server keeps
  the old inode and is undisturbed.

**How to apply:** install binaries with a temp file plus `mv`, never `cp`. In
`server.rs`, `wait_until_ready` now names this cause whenever the child dies
with a 0-byte log, and `build-llama-server.sh` execs the binary it just
installed and fails with the `codesign` remedy if it cannot start. The second
one matters: the size check the script already had could not have caught this,
because the file is a perfectly good 14 MB Mach-O.

**Also fixed in the same pass:** a lying test. `StreamingMock` returned a bare
`"Hello"` for a 7-word input and its test asserted the validator must *accept*
it, so the nonsense mock had become the specification. It now returns a real
rewrite of the input, streamed word by word. Found because the new `OffTopic`
check rejected it, which was the check working correctly and the test being
wrong.

### Gotcha G015

**A timeout that is only checked *after* the blocking call is not a timeout.**

`stream.set_read_timeout(None)` with a `params.timeout` deadline checked after
`read()` returned. The deadline looked like it protected the call and did not:
a server that accepts the connection and then says nothing means `read()` never
returns, so the check is never reached. The symptom is "Transforming…" forever
with no error, which is the worst one available because it looks like a hang in
the app rather than a bug in the client.

**Why:** a deadline enforced between events only bounds the time spent *processing*
events. It says nothing about time spent waiting for the next one.

**How to apply:** give the blocking read its own short timeout (`READ_SLICE`,
5 s here) and treat a timeout as "no data yet": re-arm, and only fail once the
overall budget is spent. That distinguishes a slow model from a wedged one,
which a single long timeout cannot. Covered by
`a_silent_server_fails_instead_of_hanging`, which asserts both that it errors and
that it polls more than once, because a fix that gives up on the first timeout
would break every legitimately slow generation.

### Gotcha G016

**`.gitignore` cannot un-track a file, so a negation is a standing invitation to
commit a build artifact.**

`.gitignore` had `crates/teletype-desktop/binaries/*` then
`!crates/teletype-desktop/binaries/llama-server`. The negation un-ignored
exactly the file the build replaces, which is the file most likely to be 15 MB,
so `git add -A` staged a real binary. Nothing about the negation looked wrong:
it carried a comment explaining a legitimate-looking reason.

**Why:** the reason the placeholder was tracked at all ("the mapped path must
exist") was true but not sufficient, because every build path creates the file
first. A tracked file that a build overwrites is a build artifact that has been
given commit rights.

**How to apply:** when a tracked path is written by a build, untrack it rather
than protecting it with a negation, and add a guard. `scripts/check-no-built-binaries.sh`
checks the *index*, not the working tree, so building locally does not trip it
and only a staged artifact does. Verified by staging the binary and confirming
it fails. Note this cannot catch a file that is both ignored *and* force-added
with `git add -f` without also checking tracked size, so it does both.

### Gotcha G017

**A test comment can be the reason a bug is invisible, and a wrong comment is
worse than no comment.**

`stats.rs` computed the weekday as `(4 + days) % 7`, one day ahead of the correct
`(3 + days) % 7`, and its test passed because the comment said "2026-09-21 is a
Tuesday". It is a Monday. The test asserted the buggy value, so the suite was
green on a defect for as long as it existed.

**Why:** a reviewer reading `assert_eq!(day_label(d), "Tue 21 Sep")` has no way
to tell the code from the comment is wrong, and the comment reads as a fact
rather than a claim.

**How to apply:** where a value comes from an external fact (a calendar, a
protocol, a published constant), assert it against that fact independently, not
against the formula that produces it. Here that meant computing the real date in
Python and pinning "Mon 21 Sep", then deleting the duplicated constant so the two
modules share `insights::weekday_name` and cannot drift again. Same shape as
L003 and the lying `StreamingMock`: a test that documents a false premise is
worse than no test, because it converts a bug into a specification.

### Gotcha G018

**Whole-value read-modify-write via AX assumes replace semantics; terminals
append.**

`insert_into` read the field's entire value, spliced, and wrote the whole
string back. Terminal.app treats a value-set as *type this*: appends to the
display while reporting the set string as its value. Every take re-sent all
previous takes (`t1 | t1t2 | t1t2t3`), and the read-back matched every time, so
the duplication was verified as "ok". A read-back that compares against the
sent string cannot catch an app that parrots the sent string.

**Why:** the verification checked "the app accepted the string", not "the
field now contains old+new". Against a parroting app those differ, and the
check passed on the wrong one.

**How to apply:** insertion primitives must send only new text
(`AXSelectedText` replacement), never re-send existing content. Then an
appending app can produce at most one copy. Classify a changed-but-unexpected
read-back as landed (Normalized), and reserve fallback for byte-identical
(silent discard). Never cascade from selection-set back to value-set: an app
that discards the former but appends on the latter re-opens the bug. Pinned by
`whole_value_resend_duplicates_in_a_terminal_and_selection_does_not`, which
asserts the old strategy reproduces the exact reported shape.

### Gotcha G019

**Committing from a tree with someone else's uncommitted work needs hunk
provenance, not file provenance.**

Two sessions' edits shared `dictation.rs`/`commands.rs`/`SettingsScreen.tsx`.
File-level `git add` would have committed unfinished work under the wrong
name and message. The procedure that worked: classify hunks by keyword sets,
hand-split mixed hunks, scan for orphan lines (a dropped block can leave
keyword-less stragglers that still belong to it), `--check` the patch,
re-scan the staged diff for the other party's keywords before committing.

**Why it bit:** a trivial one-line follow-up fix was staged via a fresh
whole-file `git diff` patch, which silently re-included the other party's
hunks and contaminated the commit. The fix for the fix: reset and re-stage
from the saved per-file patches, applying the one-liner directly to the index
blob (`hash-object -w` + `update-index --cacheinfo`).

**How to apply:** keep per-file mine-patches as artifacts until the commit
lands; never derive a staging patch from a dirty worktree without
re-classifying every hunk in it.

### Gotcha G021

**A macOS pasteboard is a stack for normal pastes: Cmd+V reads the *last*
item, not item 0.**

**Symptom:** after dictating, every ⌘V pasted the *previous* clipboard
history instead of the new dictation. The dictation was on the pasteboard the
whole time, unreachable without Paste Special.

**Why:** the 2026-09-28 fix ("the dictation is pasteboard item 0") was built
on the false premise that `NSPasteboard` pastes item 0. Item 0 is what Paste
Special iterates first; a normal paste reads the most recent item, which is
the **last** one on the stack. The premise was pinned in a test comment
(`clipboard_safety.rs`: "NSPasteboard pastes item 0"), so the suite was green
on the defect — same shape as G017 (a comment documenting a false fact that
the test then asserts).

**How to apply:** in `ClipboardSnapshot::restored_with` (core `injector.rs`),
the user's items are written first in their original order and the dictation
goes **last**, so it is the current entry. `clipboard_safety.rs` pins the
order through `pasted_item`, which now takes the *last* item. Do not "fix"
this back to item 0 without re-measuring against a real pasteboard: the
symptom is only observable in a live app, not in unit tests.

**Verification:** `cargo test -p teletype-core --test clipboard_safety`
(13 pass) + `-p teletype-desktop --lib clipboard` (4 pass). Live in-app
verification still recommended (dictate twice, ⌘V must give the latest
take, ⌘⇧V / Paste Special must still reach the user's old copy).

### Gotcha G020

**Tiny instruct models cannot serve as discriminative judges until measured.**

Qwen3-0.6B-Instruct looked ideal on paper (600M params, 100+ languages,
Apache 2.0, 51 ms per judgment) and failed the golden gate decisively:
flag rate 0.400 with clean preservation 0.833. Zero-shot was pure yes-bias
(0.000). Few-shot barely dented it. A judge below ~0.98 clean preservation
actively destroys dictation, so "small but powerful" needs a number behind
it, not a benchmark table from another task.

**How to apply:** the golden set (`evals/wordjudge_golden.py`, gate
0.98/0.90) runs before any judge wires into the pipeline. Model identity,
quant, prompt template, and gate numbers are recorded with the result, so a
future swap re-proves rather than re-argues.

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

### Gotcha G012

**Cross-compiling macOS to Intel breaks in three arch-naming places, and one
host-probe trap.** All four were hit while building the Intel job, and three of
them are silent-looking.

1. `GGML_NATIVE` (on by default) probes the **host** CPU and writes the answer
   into the compile flags. Cross-building from an M4 wrote `-mcpu=apple-m4` into
   an x86_64 compile: `error: unknown target CPU 'apple-m4'`. `ggml-metal`
   itself cross-compiles fine; it is the CPU backend only. Fix: `-DGGML_NATIVE=OFF`
   plus an explicit baseline (`-DGGML_AVX2=ON`; every Mac that runs macOS 15 is
   2018-or-newer). **Scope it to cross-builds**: the arm64 build is native, its
   probe is correct, and changing its flags would change the shipped binary.
2. clang's `-arch` flag takes `arm64`, not Rust's `aarch64`:
   `clang: error: invalid arch name '-arch aarch64'`.
3. `CMAKE_OSX_ARCHITECTURES` is turned into `-arch <value>` verbatim, so it needs
   the clang spelling, not the Rust one.
4. `set -u` with `"${EMPTY_ARRAY[@]}"` is an unbound-variable error on bash 3.2,
   which is what macOS ships and what the GitHub macOS runners use. This one
   would have passed locally in a zsh-run shell and failed in CI.

Also: a failed CMake run leaves the bad flags in `CMakeCache.txt`, and a cached
value survives a reconfigure, so the fix appears not to work. Clear the cache
**before** configuring, not after.

**Why:** each of these produces either a build failure with a message that
points somewhere else, or worse, a build that succeeds and is the wrong
architecture.

**How to apply:** for any macOS cross-arch build, set the arch through
`CLANG_ARCH` (one variable, mapped from the Rust triple) and use it for the
`-arch` flag, `CMAKE_OSX_ARCHITECTURES` and the exported `CFLAGS*`. Then assert
on the artifact: `file <binary> | grep -q x86_64`.

### Gotcha G013

**Two payloads with the same filename turn a `glob(...)[0]` into a coin flip,
and two release assets with the same name silently ship one architecture.**

**Why:** the updater manifest picked the first `*.app.tar.gz` under `dist/`. With
one macOS artifact that was fine. Adding a second one, both named
`Teletype.app.tar.gz`, made the choice arbitrary, and a wrong choice advertises
`darwin-x86_64` pointing at the arm64 tarball.

**How to apply:** scope every glob to one artifact directory and treat more than
one match as an error. Have each build job write an `ARCH` marker next to its own
binary and have the manifest **refuse to publish** unless the marker matches the
platform key. Note the naming split: `file` prints `arm64`, but the Rust target
triple, `CMAKE_OSX_ARCHITECTURES` and the Tauri key all say `aarch64`. Before
uploading, check asset basenames for duplicates. Give the second architecture
distinct asset names (`Teletype-x86_64.app.tar.gz`) rather than renaming the
existing ones, so the primary platform's release does not change.

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
| L013 | S1-mini (Qwen3 fine-tune) returns empty output for every polish unless the Qwen3 thinking mode is disabled: llama-server must be spawned with `--jinja` + `--chat-template-kwargs '{"enable_thinking":false}'` (or `--reasoning off`), else the assistant turn opens inside a live think block and the model emits an empty think block and stops. Applied at spawn AND per-request in `ServerProvider` (model-id keyed); status chip now reports "model server stopped" via `InferenceProvider::is_alive()`. | inference / s1-mini | verified | `learnings/2026-10-05-s1-mini-needs-enable-thinking-false.md` |
| L014 | S1-mini control axes (Styling/Structure/Context) verified live against the local q4_k_m weights with the exact app request shape: all three axes produce visibly different, correct output (email addresses, list structure, styling shifts match the Better Stack video behavior). Known model-level limits (not app bugs): `casual` register keeps lowercase "i" and skips punctuation; ITN can drop digits ("two point one point three" → "2.13"). Candidate product decision: post-S1 deterministic pass for casual output. | transforms / s1-mini | verified | `learnings/2026-10-05-s1-mini-live-behavior-verified.md` |
| L015 | Tray quit SIGABRT in `ggml_metal_rsets_free`: whisper.cpp parks its Metal GPU device in a C++ function-local static; at `exit()` (called directly by NSApplication's `terminate:` on the tray-Quit path) the static destructor asserts residency-set buffers are empty and aborts. Fix: `RunEvent::Exit` unloads both speech managers **before** `mark_teardown()`, and `mark_teardown()` now calls `exit(0)` so the static destructors run immediately while Metal is still up. Verified: quit via `kAEQuitApplication` AppleEvent with both models warm → clean exit, no crash report. Tried-and-reverted alternatives (NSApplication `setTerminate:`, `NSAppleEventManager` selector) are documented in the learning. | lifecycle / macOS / Metal | verified | `learnings/2026-10-05-teletype-quit-crash-metal-static-destructors.md` |
| L016 | Dictation "Choose file" button did nothing: the dialog plugin is registered in Rust but `dialog:allow-open` was missing from `capabilities/default.json`, so Tauri v2 denied the `open` IPC command silently (rejected promise swallowed by `.catch(console.error)`). Fix: add `"dialog:allow-open"` to the capability. Verified: native NSOpenPanel now opens with the audio filter. Reusable: in Tauri v2, registering a plugin is not enough — every plugin command used from the webview needs a capability permission; denied IPC is just a rejected promise. | dictation / tauri capabilities | verified | `learnings/2026-10-05-dictation-choose-file-dialog-permission.md` |
| L017 | A "premium UI" revamp means richer use of the **existing** `ui/index.html` design tokens (`--shadow-1/2/3`, `--text-tertiary`, `--border-subtle`, `--accent-soft`, `--tracking-tight`, `--ease`) — multi-layer gradient heroes, elevated cards, icon-chip headers, tabular-nums figures, section eyebrows — **not** a new color palette, and **without** changing the data model, IPC calls, `dictation-state` refresh, or empty-state copy. Proven by the 2026-10-06 Insights rebuild. | frontend / design | verified | `learnings/2026-10-06-insights-premium-revamp-design-tokens.md` |
| L018 | Two coupled lessons from the tab-by-tab battle test. (1) Tauri's global `confirm()` invokes the dialog plugin's `message` command, so a destructive action's native OK/Cancel dialog silently fails to appear (no error) when the capability lacks **`dialog:allow-message`** — the deprecated `dialog:allow-confirm` does NOT cover it. Fixed in `capabilities/default.json`; verified the native NSAlert appears. (2) AX-press / synthetic clicks do **not** reliably trigger React `onClick`: a button that "does nothing" under AX-press is not proof of an app bug — confirm via the Developer log for the expected IPC call, or drive from the DevTools console. | frontend / tauri / testing | verified | `learnings/2026-10-06-ax-press-does-not-trigger-react-synthetic-clicks.md` |
| L019 | A stray installed `/Applications/Teletype.app` can run in parallel with the dev build, each spawning its own EG-1 `llama-server` (violates the "no duplicate copies" goal invariant). The stray's PID changes between kills (manual relaunch, not a watchdog; no launchd/login-item). Fix: kill the installed app **and** its llama-server child; re-check. Before any live-app check, `ps -eo pid,command | grep -E "teletype|llama-server" | grep -v grep` and kill anything that is not `./target/debug/teletype` / vite. | dev environment / process hygiene | verified | `learnings/2026-10-06-stray-installed-app-duplicates-dev-build.md` |
| L020 | Tauri bakes the app version into DMG/EXE filenames and the reported version from `tauri.conf.json` + workspace `Cargo.toml`, **not** from the git tag. A `v0.2.4` tag shipped `Teletype_0.2.2_aarch64.dmg` because the two files had drifted. Fix (2026-10-06): every job in `release.yml` rewrites both files to `${GITHUB_REF_NAME#v}` right after checkout, so artifact names and the app version always match the tag. | release / CI | verified | `8ff97ae` (release.yml) |
| L010 | **A `teletype_desktop_lib` test is flaky.** During the 2026-09-28 ground-truth test-state re-run, `cargo test -p teletype-desktop` showed `58 passed; 2 failed` once and `59 passed; 1 failed` once in early runs, then **never reproduced across 6 consecutive clean runs** (including 3× dedicated `-p teletype-desktop`). The failing test's name was not captured before it turned green. Steady state is green (578 pass / 0 fail / 1 ignored workspace-wide). **Not yet root-caused** — candidate causes: a timing/ordering-dependent desktop test, or an environment race (AX permission, a live llama-server child, or the re-sign state from L006). How to apply: if a desktop test fails in one run and passes on re-run, do not trust either result — re-run the single test in isolation (`cargo test -p teletype-desktop <name>`) 5× to confirm flakiness, capture the name the first time it fails, and check for environment races before blaming the code. | desktop / testing / flaky | candidate (observed 2×, not reproduced since) | — |

| L022 | A chain of `git reset --soft` on the shared `main` branch silently dropped a sibling teammate's unpushed commit (`91c838c`): the reset rewound HEAD past it, so it vanished from the branch while the tree stayed clean and tests passed. Recovered from `git reflog` (`git show <sha>`), content re-applied as `7a399b3`. Rule adopted: no reset/amend/history rewrite on a shared branch without lead approval; when a reported commit is missing, check reflog before assuming it was never made. | git / multi-agent coordination | verified | `learnings/2026-09-25-shared-branch-reset-drops-sibling-commits.md` |

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
| EnviousWispr source clone (persistent as of 2026-09-23 at `enviouswispr/EnviousWispr`, gitignored; older temp path in `roadmap.md`) | EW feature map, EG-1/S1 delivery, paste cascade, VAD, splitter, keychain. **The `enviouswispr/enviouswispr-windows/` subfolder is the reference's real Windows port (23 documented phases with measured evidence)** and is the source for `docs/roadmap-windows.md` (thread-pinning, per-frame decode-loop hazard, Win32 single-item clipboard, UIPI/RDP, model residency, native-build requirement). Key files: `notes/spike-s1.md` (ASR latency by tier), `notes/load-bearing-constraints.md` (the two promises + threats), `notes/windows-native-stack.md` (M-series -> WinML/NPU mapping), `docs/plans/windows-master-plan.md` (24 phases), `docs/performance/windows-laptop-readiness.md` (measured budgets), `docs/phase-zero/porting-ledger.md` (retain/adapt/leave-behind). Spoken-lists implementation: prompt rules only, no deterministic pass. | 2026-09-26 | Reference only; a reference claim is a prior, not Teletype evidence |
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
| 2026-09-28 | QA audit: 19 verified bugs filed in `bugs/` (see `bugs/README.md`). Headline: a class of EW-parity features advertised in this brain / README had no engine path — items (3) and (5) of the EW settings port above (spoken emoji/punctuation toggles = BUG-002, unload-model-after timer = BUG-003) and the live autotext toggle (BUG-010) were advertised but absent or dead. Fixed 2026-09-28: BUG-001 (personalization mishearing judge + count-1 injection gate + in-place edit diff), BUG-004 (ITN second→2nd noun guard), BUG-005 (splitter partial-echo duplication), BUG-006 (AX allowlist URL bars/search fields), BUG-007 (select_model kill-before-spawn), BUG-008 (full whisper.cpp language table, UI label derived from list length), BUG-009+010 (dead toggles now live-apply on save), BUG-011 (interim preview in Hold mode), BUG-012 (personalization nav), BUG-019 (`contract_06` in ipc_contract.rs: every persisted setting must have a production caller). Remaining: BUG-002+003 build-or-remove (the honesty block — needs a product decision), BUG-013..018 deferred (task files in tasks/). Do not treat items (3)/(5) of the EW port as done until BUG-002/003 close. | QA audit, `bugs/` tickets, `tasks/` |
| 2026-09-25 | Language setting: `"auto"` now passes through to Whisper (auto-detect); Parakeet ignores the language arg in its wrapper. Settings picker is model-aware (Auto + the selected model's language list) | Item (1) of the EW settings port; `effective_language` in `dictation.rs`, `SettingsScreen.tsx`, `SpeechModelStatus.languages` |
| 2026-09-25 | Pill style picker with 4 choices: Teletype (default, original), Classic Capsule, Level Rail, Reading Well (ported from EW). `pill_style` setting (default `"default"`). Reading Well's live-preview well is chrome-only until streaming ASR (P2-29) | Item (pill port) of the EW settings port; `pill.tsx` (ClassicPill/LevelRailPill/ReadingWellPill + RainbowLips/RainbowMeter/RainbowHairline), `SettingsScreen.tsx`, `Settings.pill_style` in `commands.rs` |
| 2026-09-25 | VAD auto-stop: pure-Rust `silero-vad-pure` engine (no ONNX), stop policy in `teletype-core::vad` (fire only on silence after speech, default 800 ms, leading silence never stops). Settings `vad_auto_stop` (default off) + `vad_silence_ms` slider (300..2000). Active ONLY in hands-free hold mode (double-tap start): plain hold stops on key release and push-to-talk stops on the second tap, so both would preempt VAD. Capture thread feeds an `Arc<Mutex<Option<VadDetector>>>` slot via `Recording::start_with_vad`; on `UtteranceComplete` the slot is dropped (one-shot) and `Event::VadFired { session }` stops the take; session check is the second gate. VAD engine init failure falls back to manual stop. Latency gate: test asserts < 10 ms per 32 ms chunk (roadmap P2.2). **Sample-rate fix (2026-09-26):** the detector once hardcoded 16 kHz when converting silent chunks to wall-clock time, but the capture stream feeds the device's native rate (commonly 48 kHz on Mac), so an 800 ms pause took ~2.4 s of real silence to fire ("1+ second pause not working"). `VadDetector::set_sample_rate` + `Recording::start_with_vad_rate` now publish the device rate (stored before the first frame, no race) and the frame callback latches it; regression test `silence_timing_uses_actual_feed_rate_not_16k` | Item (2) of the EW settings port; `vad.rs` (`set_sample_rate`/`silence_elapsed`), `audio/mod.rs` (`start_with_vad_rate`, rate published pre-play), `dictation.rs` (`vad_enabled_for`/`start_with_vad`/`VadFired` + rate slot), `Settings.vad_auto_stop`/`vad_silence_ms` in `commands.rs`, `SettingsScreen.tsx` |
| 2026-09-26 | 5th pill style "Dot Matrix" (`dotGrid`): ported from LiveKit Agents UI `AgentAudioVisualizerGrid` (Apache-2.0, attribution + modified notice in `pill.tsx`). Vendored as inline-style React (no Tailwind/shadcn/livekit deps); LiveKit's FFT bands replaced by the scalar `pill-level` history (newest at right), agent states mapped warming→connecting (ring sweep), recording→speaking/listening (volume rows grow from middle row), processing→thinking (row scan). 24x3 strip inside the existing 520x84 window (no Rust resize); idle keeps the default bars. 100ms interval only runs while mounted. Verified by headless-Chrome screenshots of all three phases via a temporary `pill-preview.html` harness (deleted after) | User request after LiveKit Agents UI compatibility analysis; `pill.tsx` (DotGrid* block), `SettingsScreen.tsx` PILL_STYLES, `commands.rs` doc comment |
| 2026-09-26 | Windows CI gate = native `windows-latest` job (not macOS cross-compile), after the cross-compile gate failed on runners (ring/whisper.cpp C code needs a native MSVC toolchain). Removed W1's stub speech providers. | User chose "Native windows-latest job (Recommended)" via AskUserQuestion; D007 |
| 2026-09-28 | "yes please do both": ship the deterministic symbol-disambiguation rule (no settings switch) **and** add the Intel build job. The no-switch part departs from the earlier recommendation; D008 records why | After the QA audit showed "the payment period ends in March" typing `the payment. ends in March`; D008 |
| 2026-09-28 | Intel macOS: separate x86_64 build rather than universal, built but **not supported** and slower, non-blocking in CI so it can never gate an arm64 release; D009 | User asked for universal-or-separate, and whether a working Intel build is possible |
| 2026-09-28 | `TYPING_WPM` is 52, not 40. Dhakal et al. CHI '18 (168k volunteers, 136M keystrokes) gives 40 as a low bound. `words_per_minute`/`times_faster` became `Option`, clamped to `MAX_PLAUSIBLE_WPM = 300`, measured on a monotonic `Instant`, and the UI shows no rate when unmeasured | User: "check the 40 wpm baseline, from the internet and the ew folder"; `docs/research/speaking-speed-and-time-saved-claims.md` |
| 2026-09-28 | Clipboard: never `clear()`; unreadable clipboard left untouched; dictated text published as an extra `NSPasteboard` item so it is **added on top of** clipboard history. New setting `keepTextOnClipboard` (default on) beside the existing `restoreClipboard` | User: "add a setting; we don't wipe their files or images strictly; add on top of clipboard history" |
| 2026-09-28 | Clipboard: the **dictation** is pasteboard item 0 and the user's own items follow it. `NSPasteboard` pastes item 0, so appending the dictation last left the old clipboard at the front, which is "it keeps pasting the previous clipboard text". Each restore also re-adds the dictation, so the pasteboard grew by one item per dictation forever; items we add carry a `org.teletype.dictation` marker and are dropped from the next snapshot, with a 16-item cap | User report, called critical; `clipboard_safety.rs` (13 tests) — **order premise was wrong, corrected 2026-10-02: see G021** |
| 2026-09-28 | A built binary must be `mv`-ed into place, never `cp`-ed over the path an app executes. `cp` onto a running binary's inode leaves a file macOS kills with SIGKILL / "Code Signature Invalid", 0-byte log, surfaced to the user as "Polish skipped: no model loaded" | User report: EG-1 selected but never polishing; G014 |
| 2026-09-28 | Model bullets: leave the model's own formatting alone. `format_spoken_lists` only rewrites *spoken* markers, so EG-1's `- apple` stays lowercase. Open question raised with the user rather than decided: should the deterministic pass capitalise and punctuate a model's bullets | Asked directly, "go ahead with all of it", and recorded as an open question in `docs/manual-test-2026-09-28.md` section 8 rather than silently decided |
| 2026-09-28 | All work moved onto branch `qa-audit-fixes` (from `main` at `ab53b08`) as 9 reviewable commits, directory-at-a-time. Intermediate commits were not each independently CI-verified; only the slice tips for `teletype-core`, `teletype-desktop` and `teletype-inference` were | User: "go ahead with all of it", after being shown the 86-path uncommitted tree |
| 2026-09-28 | `.agents/` stays gitignored ("kept locally, not published"), so PROJECT_BRAIN.md and the learnings are local-only. Do not force-add them | `.gitignore:53`; the comment there is deliberate |
| 2026-09-26 | Create two platform roadmaps (Windows + macOS) in Project Brain, using the reference's `enviouswispr` folder (both the Windows port and the macOS app) as the source for learnings | User request; produced `docs/roadmap-windows.md` + `docs/roadmap-macos.md` |

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
| `ali-agent-health` | `/ali-agent-health` | Validate the agent system + brain for drift: registry/reality mismatch, dead brain references, stale decisions, orphaned or overlapping specialists. A report, not a fixer. |
| `ali-evolve-agents` | `/ali-evolve-agents` | After meaningful work, decide whether the brain, a specialist, or the team needs an evidence-based update. Promote a learning to a gotcha/rule when it recurs; add a specialist only for a genuinely new recurring domain. |

**Convention:** when Ali gives a build task, default to `/ali-pi-team`. After any pi
task, run `/ali-verify-pi-work` before reporting done. When the app needs a live check
or comes up blank, use `/ali-app-dev-loop`. When the team or brain feels stale, run
`/ali-agent-health`; after a big or novel task, run `/ali-evolve-agents`.

---

## 10. QA audit 2026-09-27

108 prioritized findings with file:line evidence: `docs/qa-audit-2026-09-27.md`.
Read that before planning work; this section records only what is not obvious
from it.

**The three that matter most are not subtle.** Every dictated price is
corrupted (`itn::normalize("it will cost fifty dollars")` returns
`"it will cost$50"`), saying the word "period" or "comma" or "star" in a
sentence types a symbol instead, and `insights::compute` on a full 1000-entry
history takes **64 s** while holding the mutex that the dictation worker needs
before it injects. All three were invisible to 446 unit tests and to `tsc`
because each corrupts text in a way that keeps the words present.

**Smoke test convention.** `crates/teletype-core/tests/journey_smoke.rs`
(54 journey tests + 4 `#[ignore]`d defect repros), `text_integrity.rs`
(11 regression pins + 6 `#[ignore]`d acceptance tests), `ipc_contract.rs`
(5), and `crates/teletype-desktop/tests/edit_watch_index.rs` (4). 519 pass.

**Two test shapes, and the choice matters.**
- A test that pins today's *wrong* output is a ratchet: green, and it fails the
  day someone fixes the bug, which is the signal to promote the case. Use it
  when the tree is known-dirty and you need CI green without losing the signal.
- A test that pins the *invariant* belongs `#[ignore]`d with a `BUG-nn` reason,
  so it is the acceptance test for the fix and cannot be quietly rewritten to
  match a bug.
Never write a passing test that asserts a defect is correct. That makes the
defect a specification.

**`ipc_contract.rs` reads both sides of the wire** because `tsc` cannot: a
camelCase/serde regression of the kind that broke four shipped features merged
green. It parses `invoke()` call sites out of `ui/src`, `#[tauri::command]` out
of the backend, `generate_handler!` out of `lib.rs`, and Rust struct fields out
of source, then diffs them. Verified by injecting a casing bug and confirming
the test caught it.

**CI was not running any of this.** `ci.yml` used `cargo test -p
teletype-core --lib`, and `--lib` skips `tests/`, so all 71 integration tests
ran only locally. Now whole-crate, plus `cargo fmt --check` and `cargo clippy`,
plus a non-blocking job that reports whether the confirmed defects still
reproduce. "445 tests pass" in section 1 was a local number, not a CI fact.

**Fixed on 2026-09-28** (see the Fix log at the end of the audit doc): all
six ITN text-corruption bugs, the personalization-loop panic, the
off-topic-model-output hole in the validator, the serde-default upgrade data
loss, the cross-host API key leak, the 64 s Insights lock, the Windows build
break, and the two dead diagnostic flags. 519 tests pass, 3 ignored defects
remain (System AutoText, the Insights headline, and the weekday-label
disagreement).

**The mocks lie too.** Four existing tests had mocks whose output was not a
rewrite, so when the new `Failure::OffTopic` check landed they failed, and the
tempting move would have been to loosen the check. In every case the mock was
wrong. `EchoLlm` interleaved style instructions into the sentence;
`StreamingMock` returned the bare string "Hello" for a seven-word sentence;
`ScriptedLlm` returned a fixed line. Lesson for this codebase: when a new
validation rule rejects a test, first ask whether the *mock* or the *rule* is
wrong, and prefer capturing the real artifact (the prompt) over inferring it
from `final_text`.

**Round 2, 2026-09-28.** Clipboard: new `ClipboardGuard` trait in core plus an
`NSPasteboard` implementation in `teletype-desktop/src/clipboard.rs` that
round-trips every representation; `clipboard.clear()` is never called again; the
dictation is added as an extra pasteboard item so it lands in the system
clipboard history; new `keepTextOnClipboard` setting. Speaking rate:
`DictationEntry.duration_ms` measured on a monotonic `Instant`; `TYPING_WPM` is
now 52 (Dhakal et al. CHI '18, 168k volunteers), not the 40 lower bound;
`words_per_minute` is `Option` and clamped to 300; time saved is monotone by
construction. Research in `docs/research/speaking-speed-and-time-saved-claims.md`.

**Jev / Kev evaluated 2026-09-28** (`docs/research/jev-kev-for-symbol-disambiguation.md`).
The user is right that a discriminative decision model is the right shape:
Jev (Typesafe) and Kev (jaredpalmer/kev, Apache-2.0, 7.4k stars) take text
plus questions with answer options and return *calibrated probabilities*, never
generating text. Kev-0.8B is 149 ms on an M5 (28 ms cached), which fits the
1.65 s p50 dictation. But Kev's runtime is **Python + torch/MLX**, and its
models are Qwen3.5/3.8 with Gated DeltaNet layers, for which no llama.cpp path
exists yet. Shipping it would add a Python runtime, contradicting D002 (never
link a second ggml) and D003 (no cloud, no Python). Take the architecture, not
the runtime: put the rule behind a `SymbolDisambiguator` trait returning a
confidence, collect labelled overrides, and revisit when a GGUF path appears.

**macOS floor is 15.0 and Intel is effectively out.** `CMAKE_OSX_DEPLOYMENT_TARGET`
and `RUSTFLAGS` are both 15.0 because ggml-metal's device checks use
`@available(macOS 15.0)` and clang dangles the symbol below that. macOS 15 does
still run on 2018-2020 Intel Macs (macOS 26 is the one that drops them), so a
universal build is technically possible, but Intel has no Neural Engine so
Parakeet is unavailable there and the dictation engine would be a different
one. The README claimed "macOS 14+ (Apple Silicon or Intel) - fully supported",
which was false on both counts; corrected to macOS 15 / Apple Silicon.

**System AutoText is 5 words, not 79 entries.** Only `comma`, `period`, `quote`,
`star`, `plus` (arguably `colon`, `minus`) collide with prose; the other 20
single-word entries are not things people say. Do not "fix" this with an LLM
call: the polish gate returns `short_clean` for every breaking sentence, so a
model-based fix is a new 500 ms-3 s call on the fastest path. A deterministic
rule (expand a single ambiguous word only when the utterance is 1-2 words, or a
naming cue is within two words before it, or it starts/ends the utterance) was
prototyped at 13/13. `use` must never be a naming cue.

**The pattern worth naming.** A dozen of these findings are a guard that exists,
is documented, and does not evaluate the condition it was written for: six ITN
passes match a leading `\s` and drop it; the validator's `Failure` taxonomy has
no variant for off-topic output; section 6 of this brain documents serde
defaults that five structs do not have; `transform_skipped_no_model` is
documented as "the UI must surface this" and cannot be true on its own path;
`Insights::polish_status`, documented as the polish-state message, is
hardcoded to `None`. **A guard that cannot be observed failing is not a
guard.** When reviewing, do not accept a doc comment as evidence, and trace the
value that actually reaches each check.

**Verified clean, do not re-audit.** UTF-16 splicing in `ax_text.rs` is
surrogate-safe and clamped; `selected_range` cannot underflow;
`resample_to_target` is safe at every device rate; VAD chunking cannot panic or
drop samples; the cpal downmix cannot divide by zero; mutex poisoning is
handled uniformly via `into_inner`; SHA-256 verification works on every path
that has a hash and an empty-string hash does not pass vacuously; the local
llama-server is confined to loopback with `--api-key` on every request and the
model path passed as a separate `.arg()`; the EG-1 license gate is
backend-enforced; the child-process pipe deadlock does not exist (stdout is
`/dev/null`, stderr is a file); the Windows release job does a real
`cargo tauri build`; there is no ReDoS (search boxes are literal `includes`) and
no path traversal in the archive; D006's pack-privacy contract holds; the
recovery spool is correctly bounded and `recover_last_dictation` does not
inject.

**Not bugs, verified.** `correct_with_dictionary` turning "ten" into "10" is ITN
working. `AutoTextScope::Application` matching is correct.
`validate_snippet("")` returning `Ok` is deliberate. `get_insights` with no
`range` argument correctly yields `None` for the `Option`.

**Brain action item.** Gotcha G003 says to keep `(3+days)%7` in
`stats.rs`/`insights.rs`, but only `insights.rs` was fixed and
`stats.rs:57` still has `(4 + days) % 7`, so the gotcha reads as satisfied when
it is not. Same shape as L003, inverted. `stats::dashboard` has no production
caller today, so it is latent. **Left unfixed**: it is outside the two items the
user approved, and there is no user-visible impact while the function has no
caller. It has a `BUG-21` acceptance test in `stats.rs` that fails with
`left: "Tue 21 Sep", right: "Mon 21 Sep"`, plus a `regression_*` pin.

What made this survive a re-read is worth keeping: the old test's comment said
"2026-09-21 is a Tuesday", and 2026-09-21 is a **Monday**. The test agreed with
the bug, so it passed. A test that documents the bug's own false premise is
worse than no test.

**Round 3 (2026-09-28), the two user decisions.** BUG-12 (System AutoText
rewriting ordinary English) is **fixed**, so its `regression_*` pin was deleted
and its `#[ignore]` promoted to green, per the convention above. That leaves
`stats.rs`'s weekday labels as the only known-live defect with an executable
reproduction. 557 tests pass, 2 ignored.

**Verified test state, 2026-09-28 (re-run, not assumed).** `cargo test
--workspace` on `qa-audit-fixes`: **578 passed, 0 failed, 1 ignored** (verified
6× — the count grew +21 from the 557 above as tests landed across rounds).
The single ignored test is `itn_parity.rs:41` (`itn_parity_fixtures`,
"phase-2: 405/2416 rows failing… TODO remove when phase-2b lands") — it is the
suite's only slow part (~42 s/run). BUG-21 (stats weekday) is **fixed**
(`stats.rs:224` "BUG-21, fixed"; `day_labels_use_the_real_weekday` runs green),
so its repro is no longer ignored. Clippy is now **0 real lints** — the "74
pre-existing warnings" noted below is stale; the only 10 `warning:` lines are
macOS clang linker noise (`-framework Cocoa unused`), not code. `tsc --noEmit`
clean. One caveat: a `teletype_desktop_lib` test failed transiently in 2 early
runs (2 failed, then 1 failed) and never reproduced across 6 clean runs — a
**flaky desktop test**, name not captured; watch it, not currently blocking.

**The rule needed three fixes the prototype did not have**, all found by
running it against the real `DEFS` table rather than against the 13 hand-written
cases: the ambiguous set is **not** derivable from "is this a common English
word" (that puts `at sign` and `new line` in the same class when they are
opposites, and `at sign` is required for "email at sign example dot com"); a
rejected two-word phrase used to leave its own one-word prefix expanded
(`two + sign two = four`); and `INSERTION_MARKERS` starting with `in`/`on`/
`after` made ordinary continuations count as requests, which brought the
original bug straight back. Recorded because the tempting version of this rule
is the one that does not work. See D008 and G012.

**`ci.yml`'s clippy step cannot fail.** It runs
`cargo clippy --workspace --all-targets` with no `-D warnings`, and the tree
carried 74 pre-existing warnings at the time, so the step is informational. A
warning I
introduced this round (four `unsafe_op_in_unsafe_fn` hits in `clipboard.rs`) was
not caught by it and had to be found by reading a cross-compile log. **Updated
2026-09-28:** the tree is now at **0 real clippy lints** (the 74 were cleared in
later rounds), so the step *could* now be tightened to `-D warnings` with no
pre-existing debt to clear first — that is still its own task, but the blocker
is gone. The only 10 `warning:` lines are macOS clang linker noise, not lints.

**`binaries/llama-server` is tracked and the working tree now holds a real
14.6 MB arm64 binary, because running `scripts/build-llama-server.sh` is what
a local dev loop requires.** `.gitignore` ignores `binaries/*` but then
un-ignores `binaries/llama-server`, which is exactly the file the build
replaces, so the real binary is committable. The 74-byte placeholder is what is
committed on purpose (`tauri.conf.json` maps that path, and the Windows job
fails the build if the placeholder is still under 1 MB). **Do not stage the
built binary.** The clean fix is to commit the placeholder under a different
name and let the script write the real one, which needs the Windows fail-check
changed to "missing or real" as well; not done here because it is a design
change, not a bug fix.
