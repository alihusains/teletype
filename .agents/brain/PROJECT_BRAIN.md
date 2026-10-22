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

Production-readiness pass (2026-10-06; landed with the v0.2.6 release, `8ff97ae`): all 12 tabs verified
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

**Update (2026-11-01, Decision D020):** the shipped successor is **commit-while-you-go** — per-segment incremental decode with a live preview (P4.1 endpointer/segmenter, P4.2 preview resync; `fcae04b` on `feat/beat-wispr`). It fixes the 2026-10-10 "miss in the stretch" gap while respecting the two-promises constraints. It is NOT a token-by-token live stream.

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

**Release line (verified 2026-10-18, live UAT + self-test + two-tier command battery — verified, not claimed):**
`feat/beat-wispr` @ `5aba0a4` (2026-10-18 20:11) IS the v0.2.7 line; `main` = v0.2.6 release line; last tag `v0.2.7-rc1`, prior stable release `v0.2.6`. What was re-verified against the live running app that day (worktree binary, started 23:03, observed to 23:1x):
App boots clean into running state (LLM s1-mini warm in ≈0.53 s; no crash report produced; controller thread `teletype-dictation` alive); **default decider is `neudecide` when `neudecide-v1` model is present in `~/Library/Caches/com.teletype.app/models`** (live log `command decider: neudecide (model present)`) — the Gate A parity gaps (0/50 direct, 45/50 conflate) were measured on TTS clips and remain a *known risk*, while the live-mic 3-line battery all landed through the same path, so it is not treated as a release blocker; the D019 VAD gate is what protects takes shorter than 300 ms. End-to-end delivery chain verified live (dictation→transcribe→polish→inject, with the raw-transcript fallback never blocked); recovery spool then sticks across restart and clears on the next clean delivery; no crash after sustained uptime + restart exercise. **Kill/restart contract: SIGTERM only** (app ignores SIGINT outright; TERM exits ≈0.35 s clean — see D017, G024). Two stale brain entries from sandbox lineage are **not** in this repo: the `SIGINT-limits` planned harness and `list_tests.rs`/`probe_sigint.rs` (described in the session/entries.md restore fragments, L05x–L07x-style) — verified absent from worktree and main tree and from history; do not cite as live. Spoken-phrase card rescans do **not** persist across restart (`recordings/` never populated post-restart; in-memory + trace only) — that an open product micro-call, not a blocker. Nightly CI on this line: gate-a/matrix/red-green + coverage-report jobs green; 4 non-blocking reds pre-existing (cross-8 delay test, mixed-sweep, nightly deploy report, unsigned-app + secret-gate on mask) — no release value inside them.

**Update 2026-10-19 (v0.2.7 line; supersedes the Gate-A risk note above):** `feat/beat-wispr` FF-merged into `origin/main`; tag `v0.2.7` = `797f4f1`. The 2026-10-18 tokenizer defect is fixed (`79d89dc` + follow-up) and the re-run (`evals/RESULTS.md` run `command_suite_2026-10-18_c`) flipped the A.5 gates: suite **61/185 (33.0%)** vs needle **15/185 (8.1%)** = **+24.9 pts MET**; parity **46/50 (92%) MET**; latency p95 **87.5 ms** under the recalibrated **≤ 200 ms** budget (Ali approved 2026-10-18; the 80 ms bound retired) — then `86ad110` made NeuDecide the default (`default = ["neudecide"]`). CI on this line (run `39208405054`): 9 jobs green; 3 real reds — Lint (`clippy`), Checks (third-party-notices sync: `neudecide`, `arc-swap`, `@testing-library/jest-dom` missing from `THIRD-PARTY-NOTICES.txt`), macOS Intel `cargo check` — plus benches (non-blocking); re-check `gh run list` before citing CI. Gate-A human spoken battery remains outstanding.

---

**Current work topology (2026-10-19, v0.2.7 line):** one governing plan; the execution branch is FF-merged into `main` and tagged `v0.2.7`.

- `main` — release line at **v0.2.7** (`797f4f1`; tag `v0.2.7`). FF-merged from `feat/beat-wispr` on 2026-10-19, carrying the full Beat-Wispr plan execution, the NeuDecide default decider (see the D018 update below), the refactor merge (`b8b8f38`), and the 2026-10-16/19 QA battery fixes; the EG-1 catalog removal (D019) is on this line.
- `feat/beat-wispr` — plan execution source branch (`888ca81`, one docs/brain commit ahead of `797f4f1`; shippable code identical to `v0.2.7`). `PROGRESS.md` on that branch is the plan's gate/phase source of truth. **Task-level state (2026-10-19):** all plan phases have their tasks implemented and committed — P0.1–P0.5; P1.1–P1.12; P2.1–P2.9; P3.1–P3.9 incl. the 86-cell bake-off + `docs/decisions/D-engine-defaults.md`; P4.1–P4.4; P5.1–P5.8 code; P6.1–P6.5; P7.1–P7.8 + P7.10; P8.1–P8.7; P9 hardening + Phase-10 harness. Optional P6.6/P7.9 not started. **P1.1/P1.2 ARE landed**: `crates/teletype-core/src/injector.rs` uses `BEFORE_PASTE_CAP` 150 ms + `wait_for_modifiers_released` (no fixed pre-paste sleep) and `BEFORE_RESTORE` 350 ms runs on a detached thread. **Gate evidence still outstanding:** human corpus 0/120 clips; live release→paste latency traces never captured; Windows real-hardware sherpa run; the 32-case e2e delivery matrix not executed (`evals/runs/e2e_matrix_2026-10-17/MATRIX.md` is a BLOCKED dry-run); no `WISPR_COMPARISON.md` (Phase 10 run not performed).
- `refactor` — the `Teletype Refactoring and Feature Roadmap.md` workstream (`efbd0d5`), merged into `feat/beat-wispr` as `b8b8f38` (recorded in PROGRESS `d0b6a69`). 4 stashes still present on the working checkout.
- The main checkout (`/Users/a.sorathiya/Documents/Ali/teletype`) now sits on `release/v0.2.7` (tracks `origin/main`, in sync at `797f4f1`). Worktrees: `ahsorathiya7-fuzzy-happiness` = `feat/beat-wispr` @ `888ca81`; `release-main-027` = `main-release` @ `797f4f1`; `copilot-prewarm-upgraded-waddle` = detached HEAD at the stale local `main`. Local `main` has 2 commits not on `origin/main` (`727c3f0`, `6694138`) — do not fast-forward it blindly.

Plan work has landed through the v0.2.7 merge; the remaining gate-evidence work (human corpus, live traces, Windows hardware, e2e matrix) continues from the `feat/beat-wispr` lineage.

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

Model evaluation: the living ASR/audio eval register is
[`asrs.md`](asrs.md). It holds every catalog row, the candidate lane (including
Needle 3/Whistle), the first 5-engine/86-cell bake-off snapshot, and the
cadence. The measured top 3 per host class drive the recommended ranking (Ali,
2026-10-22). The human corpus is still 0/120, so the primary metric is null and
no recommendation may change until it is recorded.

### Important integrations

| System | Purpose | Interface / location | Notes |
|---|---|---|---|
| llama-server (llama.cpp) | Local GGUF LLM for Polish | spawned subprocess, `crates/teletype-inference/src/server.rs` | Built via `scripts/build-llama-server.sh`, pinned commit, Metal on, curl/OpenSSL off; never linked in-process |
| OpenAI-compatible APIs | Remote/Ollama/LocalLLM polish | `crates/teletype-inference/src/openai_compat.rs` | base_url + optional key + model |
| OS keychain | API key storage | `keyring` crate, `crates/teletype-desktop/src/secrets.rs` | service `com.teletype.app`, accounts `llm:<provider>`; never in settings.json |
| Hugging Face / publisher CDNs | Model downloads | `crates/teletype-inference/src/download.rs`, catalog | SHA-256 verified; `requires_license_accept` gates entries whose license needs an in-app accept (no catalog entry sets it today) |
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
# REQUIRED before selecting any local LLM model (S1-mini, Qwen3), else
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
  polish/professional output after validation, so it holds on the S1-mini route
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

### UI conventions

- A tab screen's top-level page heading is always `<h2 className="tt-screen-title">`
  (defined in `ui/index.html`, `--text-2xl` 28px / weight 650 / `--tracking-display`).
  Never hard-code `fontSize`/`fontWeight`/`letterSpacing` on it: every screen used to
  pick its own (18, 20, 22, 26) so switching tabs visibly resized the title. Non-typographic
  inline styles that the heading already relies on (`flex: 1` on Developer, `marginBottom: 4`
  on Personalization) stay; section headings further down the page keep their own styles.
  (2026-10-21, applied to Dictation/Dictionary/AutoText/Models/Personalization/Styles/
  Scratchpad/Developer/AI Polish/Keybinds/Settings)

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
- Generation: temperature 0, no reasoning. S1-mini caps at max(input chars, 256);
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
Teletype remains open source and non-commercial. Publisher weights (when a
catalog entry offers any) are user-directed downloads; never re-host or bundle
them in the `.app`.

**Update (2026-10-15, P3.9):** EG-1 was removed from the catalog entirely (its
license only allows use inside Envious Labs apps and bans competing products —
plan §0.1 rule 5; commit `1ebd339`). Selections migrate to S1-mini with a
one-time notice. The open-source/non-commercial stance stands independent of
EG-1.

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
is CoreML/macos-only. [SUPERSEDED 2026-10-21: Windows speech now runs sherpa-onnx
Parakeet (`sherpa-parakeet-v3-int8`, default via `catalog::pick_engine`); whisper-rs and
the ggml Parakeet/whisper.cpp backends are macOS-only dependencies and error on load
under Windows (`teletype-speech/Cargo.toml`, `whisper/stub.rs`). The native-MSVC CI
rationale still holds.]

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
`crates/teletype-desktop/src/typing/typing_windows.rs` (relocated to `typing/` in `8f86c5c`); `docs/roadmap-windows.md` (learnings
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

### Decision D011

**Title:** The controller/event loop is the sole authority for the final user-visible side effect (text injection); workers never inject

**Date:** 2026-10-08

**Status:** accepted (implemented in `dictation.rs`: `Event::PipelineReady`, `Session::pipeline_ready`, `cancelled_sessions`)

**Decision:**
The pipeline worker produces a result tagged with `session_id` and sends a
`PipelineReady` event to the single-threaded controller/event loop. Only the
controller validates `session_id` against `cancelled_sessions` and performs
the text injection. `Cancel` is processed by the same event loop, so FIFO
event order is the cancellation decision:

- `Cancel(session)` processed before `PipelineReady(session)` → session is
  tagged cancelled, the result is discarded, no injection.
- `PipelineReady(session)` processed before `Cancel(session)` → injection
  proceeds; the later cancel transitions the session state.

**Context:**
The original code called `insert_text` directly from the pipeline worker
closure. A check-then-act race meant a cancel arriving between the worker's
cancellation check and its injection could not be honored. The fix moves the
side effect into the event loop, where cancel and ready are serialized.

**Alternatives considered:**
- A cancellation flag checked by the worker (rejected: the flag check and the
  injection are not atomic; a cancel between check and inject is lost).
- A mutex around the injection (rejected: adds synchronization to a
  single-threaded loop that already serializes all events).

**Evidence / source:** `dictation.rs` — `Event::PipelineReady`,
`Session::pipeline_ready`, `Session::cancel_session`, `cancelled_sessions:
HashSet<u64>`; five concurrency tests in `dictation::tests` covering all
orderings.

### Decision D012

**Title:** AI Polish "Customize your Polish prompt" is a Wispr-style instruction list (base prompt is the first locked item), stored in `Settings.polishCustomInstructions`

**Date:** 2026-10-08

**Status:** accepted (implemented in `TransformsScreen.tsx` + `Settings.polish_custom_instructions: Vec<PolishInstruction>`)

**Decision:**
The Polish preset's "Customize your Polish prompt" is a list, not a single
free-text field. The base prompt (core rules + active rule fragments) is the
first, locked list item — no trash, no toggle. Each user-added instruction is
a `PolishInstruction { id, text, enabled }` row with a trash button and an
on/off toggle. The UI composes the stored transform instruction from the base
assembly + the *enabled* user instructions; Rust never reads the field
directly (same pattern as `polishRules`, hence in `SETTINGS_UI_ONLY` /
`SETTINGS_WITHOUT_UI`).

**Non-Polish presets** (Professional, Prompt Engineer, custom) keep a single
editable textarea bound to the preset's own `instruction` — the stored
instruction IS the prompt for those. `runPreview` ("See Updates") must NOT
re-assemble Polish rules over a non-Polish preset's instruction (it would
clobber it); it only refreshes the assembly for the auto-apply Polish preset.

**Context:**
The user's screenshots showed Wispr Flow's list UI (added instructions with
delete + toggle) and reported Teletype had no way to add instructions, that
the prompt stayed stuck on "polish" after switching presets, that the
keyboard-shortcut chooser was a dead `prompt()` dialog, and that the
learned-preference toast's Undo/Keep did nothing.

**Evidence / source:** `TransformsScreen.tsx` (`PolishInstruction`,
`assembleInstruction`, `persistInstructions`, `addInstruction`, the list
render + `HotkeyRecorder` shortcut card), `commands.rs`
(`PolishInstruction`, `Settings.polish_custom_instructions`),
`ipc_contract.rs` ratchet entries.

**Superseded 2026-10-08 (D013):** the instruction list is now available for
*every* preset, not just Polish. Each preset's base prompt is its own stored
instruction (Polish's is the rule assembly); user instructions are shared
across presets and appended to whichever preset runs. `runPreview` composes
the instruction for the selected preset. The Professional and Rewriter base
prompts were rewritten to the user's wording (Professional = the "professional
text editor" prompt; Rewriter = "well-structured, formal, simple… English" + a
user-instruction slot that the UI's added instructions fill).

### Decision D013

**Title:** Insights "Fixes made" counters are lifetime-only; impact baseline is 40 wpm; the "written" flourish is tiered by total words

**Date:** 2026-10-08

**Status:** accepted

**Decision:**
- **Usage counters (filler removed, AutoText expansions) are lifetime totals,
  not range-scoped.** Typed AutoText expansions happen in other apps and never
  create a dictation history entry, so they cannot be recomputed from the
  range-filtered history; the persisted `UsageStats` counters have no
  timestamps. `get_insights` always uses the persisted counters for these two
  (the old range recompute undercounted, so "AutoText expansions" read 0 in
  every bounded view). The card caption says "lifetime total".
- **Typing baseline is 40 wpm** (was 52, the CHI '18 mean). `insights.rs
  TYPING_WPM = 40`; the hero copy says "40 wpm average typing pace". The user
  asked for 40 specifically.
- **The "You've written N college essays!" flourish is tiered by total words**
  (essay 500 → article 1.5k → blog post 5k → short story 12k → novella 30k →
  book 90k), so praise escalates as the user writes more. `writtenTally()` in
  `InsightsScreen.tsx`.
- **The habit heatmap is 26 weeks** (was 12) so it fills the card width and
  leaves no gap. `HEAT_WEEKS = 26`; card title "last 26 weeks".

**Evidence / source:** `commands.rs` `get_insights`, `insights.rs`
(`TYPING_WPM`, `HEAT_WEEKS`), `InsightsScreen.tsx` (`writtenTally`),
`FixesCard.tsx`.

### Decision D014

**Title:** Dictionary page uses the reference-app rail layout; word rows carry aliases + usage counts; a master switch gates the correction pass

**Date:** 2026-10-09

**Status:** accepted

**Decision:**
- **Layout:** the Dictionary screen is a fixed banner (title + enable toggle +
  word counts) above a left `TabRail` (Your Words / Vocabulary Packs / Learn
  from / Quick Add) and an independently scrolling `TabPane`. Both are the
  shared `ui/src/components/TabRail.tsx` components, reused by any future
  rail/detail page. The Keybinds screen uses the same row pattern (icon tile +
  title + short line + "?" `Explainer` + trailing control) in grouped cards.
- **Data:** `DictionaryWord` gained `aliases: Vec<String>` (user-taught
  sound-alikes, distinct from `learned_from`) and `usage_count: u32`. Both are
  `#[serde(default)]`, so pre-existing `dictionary.json` files load unchanged.
  `Dictionary::update(word) -> Option<()>` is id-preserving (a hostile client
  cannot rename a word's identity); `Dictionary::record_usage(text)` is
  phrase-aware and case-insensitive, so multi-word entries like "VS Code"
  count. The dictation pipeline calls `record_usage` after each take, gated by
  the new `dictionary_enabled` setting (default true), which also empties the
  dictionary handed to the pipeline when off.
- **IPC:** `update_dictionary_word`, `record_dictionary_usage` (plus the
  pre-existing `export_custom_words` / `import_custom_words`, now surfaced in
  the UI via native dialogs).

**Evidence / source:** `crates/teletype-core/src/dictionary.rs`,
`crates/teletype-desktop/src/commands.rs` (+ `dictation.rs` usage hook),
`ui/src/screens/DictionaryScreen.tsx`, `ui/src/screens/KeybindsScreen.tsx`.

### Decision D015

**Title:** The refactor roadmap is executed on branch `refactor`; the parakeet-cpp/magnitude spike is committed alongside it because the S9 `speech_engine` migration is entangled with it

**Date:** 2026-10-14

**Status:** accepted (in progress)

**Decision:**
The `Teletype Refactoring and Feature Roadmap.md` (analyzed at `916f666`, v0.2.6)
is being executed on branch `refactor`. Progress as of 2026-10-14:
- **Phase 0 (quick wins): 10/10 DONE** (committed `05679e1`): P1 ITN LazyLock,
  B1 byte-level SSE, B3/B4/B5 storage, P3 delta tokens, B6/B7 tempfile, S5
  test-support→dev-deps, S8 repo hygiene, P8 whisper thread cap.
- **Phase 1 (measurement) + Phase 2 A1/A3/S9 (state/concurrency): committed**
  (`0d28caf`): criterion benches, cargo-fuzz, proptest, Vitest scaffold,
  llvm-cov CI floor, Windows `cargo test`; `Repo<T>` ArcSwap snapshot store
  wired into `AppState` for all 10 stores; engine behind `ArcSwap`
  (`EngineHandle`), no lock held across the LLM call (P2); `use_parakeet` bool
  → `speech_engine` enum with one-time migration (S9).
- **Phase 2: COMPLETE** (committed `6132ceb`): A4 background persistence actor
  + history/usage → SQLite (WAL, `teletype.db`) with one-time JSON→SQLite
  migration (JSON renamed to `*.migrated.json`, never deleted); dictation hot
  path is off disk (in-memory `Repo` `set_snapshot` + actor channel send;
  startup hydrates from SQLite). B8 patch-based `update_settings(patch)` +
  `SettingsObserver` (6 observers) fix the lost-update; `updateSettingsPatch`
  command, pill/Onboarding migrated.
- **Phases 3, 4, 5: not started** (structure, perf depth, features).
  - **Phase 3 first batch: DONE** (committed `1740289`, on `refactor`):
    S3 typed settings enums (`RecordingModeSetting`, `PillPositionSetting`,
    `ListStyleSetting`, `MatchStrictness`, `AppIcon`, `UiTheme`,
    `AutotextTiming`, `PillStyle`, `WarmEnginePolicy` — wire-identical JSON,
    `Unknown` fallback) + `CommandError{code,message}` at the IPC edge
    (`E_REQUEST`/`E_NOT_FOUND`/`E_MODEL`/`E_INTERNAL`; `thiserror` in desktop);
    S6 lib `eprintln!` → `tracing::warn!` (bins untouched); A2 unwired
    `dictation_stages.rs` (live/stub stages documented in-module; WIRED in
    `5c9e787` — worker spans + controller deliver + trace store + command); A7 `usePillMachine` reducer (behavior-identical);
    `docs/architecture.md` refreshed. Full `cargo test --workspace` green +
    85 vitest pass. Known follow-ups: UI `String(e)` error display now gets an
    object (`MicrophoneSoundsSections.tsx:103`) — FIXED via `errorMessage()`;
    commands split — DONE (`e460f70`, see below); specta,
    platform crate, tracing Layer still pending.
  - **A2 wiring: DONE** (committed `5c9e787`): worker records 8 spans,
    controller finishes `deliver`/`cancelled`, `AppState.traces` (cap 20),
    `get_dictation_trace` command, Developer waterfall + Copy JSON.
    Also fixed 3 release/non-mac `String`-Err compile bombs in cfg'd-out
    branches. Workspace green, 90 vitest, tsc clean.
  - **Phase 3 continued: DONE** (committed `e460f70`): S1 commands domain
    split (5372-line `commands.rs` → 1334-line root + 9 modules, re-exports
    keep all `commands::X` paths, `lib.rs` handler list byte-identical;
    `ipc_contract.rs` scanners cover `commands/*.rs`, 11/11 green) + UI
    `errorMessage()`/`errorCode()` helper (90 vitest pass). 118 desktop lib
    green; 1 transient flake in 11 runs, unreproduced in 10 reruns (tracked
    in `tasks/phase3-self-implementation.md`).

The `parakeet-cpp-sys` + `magnitude` inference spike (its own workstream, see
`spike/magnitude-inference` branch) is **wired into the build** and was
committed together with the roadmap work in `0d28caf`. It is NOT separable at
the file level without rewriting ~15 sites: the S9 `speech_engine` migration
references `SpeechEngine::ParakeetCpp`, the `state.parakeet_cpp` manager, and
`catalog::Engine::ParakeetCpp` throughout `dictation.rs`/`lib.rs`/`commands.rs`.
Stripping the spike would break the S9 work. If the spike is later dropped,
remove the `ParakeetCpp` variant + `parakeet_cpp` manager + `magnitude`/
`parakeet_cpp` mod declarations together, then regenerate `Cargo.lock`.

**Context:**
The roadmap audit (see `Teletype Refactoring and Feature Roadmap.md`) found the
working tree on `refactor` mixed roadmap work with the spike. Committing
roadmap-only required editing `lib.rs`/`Cargo.toml` to drop the spike, which
would have broken S9. The spike is on its own branch, so it is a separate
workstream that was merged into `refactor`'s tree.

**Evidence / source:** `git log` on `refactor` (`05679e1`, `0d28caf`);
`crates/teletype-core/src/repo.rs`; `crates/teletype-inference/src/engine.rs`;
`crates/teletype-desktop/src/commands.rs` (`SpeechEngine`, `default_speech_engine`);
`Cargo.toml` (workspace members).

### Decision D016

**Title:** Remove the EG-1 catalog entry; S1-mini + GGUF + Sonnet gen-sift become the shipping model set

**Date:** 2026-10-30

**Status:** accepted (implemented `1ebd339` on `feat/beat-wispr`; the EG-1 removal also landed on `main`)

**Decision:**
EG-1 — dual-licensed with usage terms that can no longer be verified after the 2026-10-30 egg.com/HF ask-front-charge (L017 closure) — is removed from the model catalog (`eg-1`), with a settings migration for users still on it. S1-mini (public EN) + local GGUF (+ optional Sonnet gen-sift, non-default) is now the full shipping model set. D001 stays live; its model-posture paragraph and gotcha G014 remain accurate for the downloads that still ship (S1-mini via models.enviouslabs.co; GGUF as before).

**Evidence:** `1ebd339` (P3.9); gate-3 rg proofs from that commit: `eg-1` zero references in code/UI; no EG-1 entry in the catalog JSON.

**Note:** This is legal hygiene, not a model upgrade. A command-decision model (NeuDecide, Apache-2.0) is a separate queue gated behind the plan's gate 2 — see the NeuDecide addendum and `PROGRESS.md` on `feat/beat-wispr`.

### Decision D017

**Title:** The "Beat Wispr Flow" plan is the governing plan; streaming preview ships as commit-while-you-go (P4.1/P4.2), and plan work lands only on `feat/beat-wispr`

**Date:** 2026-11-01

**Status:** accepted (in progress — gates/phases per `PROGRESS.md` on `feat/beat-wispr`)

**Decision:**
1. The root planning docs (`Teletype Agent Plan Beat Wispr Flow.md` + `Teletype Agent Plan NeuDecide Addendum.md`, desk-checked 2026-11-01) are the plan of record through gate 0–7: strict gates, baseline-ways-before-claims ("сваячьные считы со ссылки после изменений" = baseline measurements re-verified post-change), never break delivery.
2. The 2026-09-28 "streaming ASR rejected" learning stands for token-by-token streaming, but the plan's shipping approach is **commit-while-you-go**: per-segment incremental decode with live preview (P4.1 endpointer/segmenter landed; P4.2 preview UI landed `fcae04b`; live-preview UI present in `ui/src/pill.tsx`).
3. Plan work is executed on branch `feat/beat-wispr` (checked out from `main` HEAD per the plan's strict rule). Do not land plan items on `refactor` or `main`.
4. NeuDecide (55.5M ONNX command decider, Apache-2.0) is queued behind gate 2: hybrid decider — tool from audio, arguments from transcript; re-host to either HuggingFace or a public CAGOS location (plan addendum).

**Evidence:** root plan docs (desk-checked 2026-11-01); `fcae04b` (P4.1/P4.2, segmenter 9/9 + committer 9/9 green); `1ebd339` (P3.9); `PROGRESS.md` (gate state: gate 0 5/5 P0 done, one baseline row outstanding; G1–G2 drained; P3 7/9; P5.1/P5.3 done).


### Decision D018

**Title:** NeuDecide becomes the default Command Mode decider (hybrid: model for tool + enum args, transcript for free-form args)

**Date:** 2026-10-16

**Status:** accepted. Update 2026-10-17 (this branch: `b32c7ec`, `48d80b1`): A.3.1 (catalog), A.3.2 (ort runtime; `tools_token_id` root-cause fix, learning L034), A.4 (`tests/a4_no_mic.rs` plus the SPM merge-protocol tokenizer `src/spm.rs` with an 18-case encode/decode parity oracle against the real HF-cached model), and A.5 item 1 landed: 185-clip frozen corpus (`scripts/eval/gen_command_clips.py`, regenerable) + `examples/command_suite.rs` suite; run-5 needle baseline 15/185 (8.1%), open_app 8/30 strict — 18 case-only + 4 arg-shape misses + 148 hallucinated tool calls (table: `evals/RESULTS.md` run 5). **Update 2026-10-19: A.3.3 `command_mode.rs` wiring (unit c20) LANDED** — the startup warm-up thread (`lib.rs`) now populates the `DeciderSlot` from the on-disk `neudecide-q4` model (`NeuDecideDecider::new` + `command_decider.set`); previously the slot was created empty and never populated (the documented `ensure_decision_provider` was never implemented), so `select_decider` always returned `None` and every command take silently fell through to Needle even with the model present and command mode enabled. Live log now shows `command decider: neudecide (model present)` at startup. Remaining: model-gated Gate A (re-host `neudecide-v1-q4` → parity/latency/noise + human spoken battery) and end-to-end verification that a live command take fires the decider (per-take `command decider: neudecide: tool=…` line); the `tools_json` is still `v1_tools(&[], &[], &[])` (empty tool list), the prime suspect if the decoder still emits no tool call.

**Update 2026-10-18 (Gate A evidence, commits `1b15b10`, `0e3ccb2`, `d0ce765`):** the default flip is **blocked by the runtime, not the model**. A.4 measured against the real model (HF snapshot `fb0157e…`, re-host verified bit-identical): `parity_with_python` FAIL 45/50 TTS clips (all `rust=[]`); `noise_does_not_trigger` and `ort_and_sherpa_same_process` PASS; latency p95 531 ms vs the ≤ 80 ms gate. Root cause: `runtime.rs::build_tokenizer` can't parse the model's custom-format `tokenizer.json` and falls back to a zero-merge BPE, so the tool JSON encodes as byte soup and the decoder never emits a tool call (0/185 suite rows emit one — the 155/185 rubric score in `evals/RESULTS.md` 2026-10-18 is a degenerate always-silent model, NOT accuracy). **Per A.5: Needle stays the Command Mode default** until the tokenizer fix + a re-run flips these gates. Probe + evidence: `tasks/pi-tasks/bw-a44-modelrun-report.md` (worktree scratch, gitignored).

**Update 2026-10-18 (fix + flip; commits `79d89dc` → `bc7fd23` → `86ad110`):** the tokenizer root cause is fixed (byte-identical tools JSON for empty-param tools, structural-token strip in `detokenize`, house rubato resampler in the parity lane, ORT intra-op threads = 4) and the re-run met the gates: suite **61/185 (33.0%)** vs needle **15/185 (8.1%)** = **+24.9 pts MET**; parity **46/50 (92%) MET**; latency p95 **87.5 ms** under the recalibrated **≤ 200 ms** budget (Ali approved 2026-10-18). **A.5 verdict: NeuDecide becomes the default** (`default = ["neudecide"]`, the feature now really enables the ONNX runtime; `86ad110`). Noise note: the shipped q4 model false-triggers on ~97% of MUSAN clips; production protection is the A.1 VAD gate (documented in `evals/RESULTS.md`). **Still open:** (1) the Gate-A human spoken battery; (2) in-app `tools_json` is still built with `v1_tools(&[], &[], &[])` (`crates/teletype-desktop/src/command_mode.rs:296` — empty app/snippet/style enums) while the numbers above used populated app names (`crates/teletype-decider/examples/command_suite.rs:450`); the populated-enum path has not been verified live end-to-end; (3) the `command_suite_2026-10-18_c` run dir is not committed in the `main`/`feat/beat-wispr` trees and does not exist in this checkout (it lives only in the `ahsorathiya7-fuzzy-happiness` worktree) — do not cite it as committed evidence here.

**Decision:**
Command Mode decisions run through: (1) **NeuDecide** when the model files are present and the language is English (or `auto`) — the default; (2) the existing **Needle** path; (3) the **LLM JSON-schema fallback** (`command_llm.rs`). NeuDecide supplies the tool + enum args from the take audio (~46 ms); the transcript fills free-form args by deterministic rules first, LLM fallback second. It is skipped for non-English and never runs on dictation takes. Four serde-defaulted trace fields (`decider`, `decider_ms`, `tool`, `args_source`) record the chosen path.

**Context:**
The 2026-10-10 research verdict ("do not integrate now: Python-only, alpha") was superseded by the plan addendum (A.1). The Rust crate (`crates/teletype-decider`) reproduces the model's Python `json.dumps` tool format byte-for-byte (20-fixture compat test), the decode loop is unit-tested with a fake runner, the ONNX runtime sits behind a default-off `onnx` feature, and `parse_tool_calls` is fuzz-covered — while the model itself is Apache-2.0 and ~43 MB. Until the gated source is re-hosted to the `teletype-models` release (`neudecide-v1-q4` with recorded SHA-256), availability is `false` and every command take falls through to the existing deciders (no user-visible regression).

**Alternatives considered:**
Stay on Whistle → Needle (rejected: the addendum mandates NeuDecide; tool-native decoding covers enum args the extractor does not); keep research-only (rejected: the integration cost that motivated the old verdict is gone); run NeuDecide on the dictation path too (rejected: it is a command decider; non-English is explicitly skipped); hosted inference (rejected: D003 offline stance).

**Evidence / source:** full reasoning in `adr/ADR-015-neudecide-default-decider.md`; `docs/research/neudecide.md`; `crates/teletype-decider`; `fuzz/fuzz_targets/decoder_output.rs`; plan addendum A.1–A.5.

### Decision D019

**Title:** A.1 — gate the NeuDecide command decider with a Silero VAD speech-duration check (skip takes with < 300 ms of speech)

**Date:** 2026-10-18

**Status:** accepted (this branch)

**Decision:** `NeuDecideDecider::decide` first measures the take buffer with the in-tree Silero VAD (`teletype_core::vad::VadDetector`: 512-sample/32 ms chunks, floor 0.5). A take carrying < `MIN_SPEECH_MS` (300) of above-floor speech returns `Ok(no_action)` — no model load, no tool call, transcript delivered as plain dictation via the existing no-command path — before any model work. VAD init failure returns `None` and **fails open** (old behaviour). The gate lives inside `NeuDecideDecider::decide`, not `select_decider` (the take buffer is not visible at selection time); the Needle/LLM paths and the feature-off build are untouched.

**Context:** the 2026-10-18 audit (task `bw-vadgate.md`) verified **no** gate existed: the decider ran on every command take with no duration/VAD check, and `dictation.rs`'s `speech_ms` hashmap is a wall-clock take span (trace field only, never a signal). NOTE for future work on that filepath: it is take-duration, not VAD speech — do not confuse the two. Plan addendum A.1 wording: the model fires on noise without confidence, so command takes must be gated with silence vs. speech first. Supplier: Auden (A.1 audit), wires per the D018 plan priority.

**Alternatives considered:** gate in `dictation.rs` before `decide` (rejected: that file is read-only for this task, and the gate belongs with the decider it protects); energy/RMS stand-in (rejected: real Silero is already a dependency and is the supplier-sanctioned choice); skip = `Err` to re-rank needle fallback (rejected: A.1 says *skip*; `Err` would let the provider re-extract, i.e. the model still "fires" — `no_action` plain-dictation semantics match `Decision`'s existing design).

**Implementation:** `command_mode.rs` — `MIN_SPEECH_MS`/`SPEECH_PROB`, `vad_speech_ms(&mut VadDetector, &[f32])` (scripted-engine-testable), `take_speech_ms(&[f32]) -> Option<u64>` (real Silero), gate at the top of `decide()`. 4 new unit tests (boundary at 288/320 ms via a `ScriptedVad` `VadEngine` impl, real-engine silence/empty buffers, quiet take → `no_action` with an unbuildable model dir proving the model is never loaded). All gate commands green (2026-10-18): desktop lib 234/234 (+ feature-off 18/18 command_mode), decider pass, clippy `-D warnings` clean both feature states, fmt clean.

**Known caveat (pre-existing, not introduced here):** `teletype_core::vad::silero_16k` ignores `SampleRate::Hz8000` (returns the 16 kHz engine); command takes are 16 kHz so unaffected. Precondition for scaling: handle multi-command takes if product ever allows one.

### Decision D020

**Title:** Termination contract — the app is kindable by SIGTERM only; SIGINT is ignored process-wide; any kill/restart/smoke-test path must use TERM (or `app.exit()`), never INT, never `-9`.

**Date:** 2026-10-18

**Status:** accepted

**Decision:** The app's clean-exit path is the Tauri `core:app` exit capability (window close / tray Quit / updater restart paths) — nothing registers a SIGINT handler on any thread (verified in source: no `signal`/`sigaction`/`pthread_signal` registration anywhere in `crates/`), so SIGINT is unhandled and ignored. Live verification 2026-10-18: SIGINT for 45 s produced no exit (thread sample showed the controller parked in `run_loop` with no teardown in flight, 5 threads total, no new syscalls), while SIGTERM produced a clean exit in ≈0.35 s. The contract therefore: `kill -TERM <pid>` (or `pkill -TERM teletype`) in the dev loop, the e2e matrix helper, CI smoke, any future agent script; SIGKILL is not a replacement either (it skips the `DeadOnExit`/clipboard-restore/shutdown-hook paths — same reason G014-style damage occurs). Rationale: G022-class signal experiments otherwise burn minutes twice; a one-line contract turns "is the app down?" into a 1-second, side-effect-free check.

**Alternatives considered:** registering a SIGINT handler (would require touching the teletype-dictation controller teardown — out-of-scope risk, zero feature benefit); relying on `wait4`-style documentation without a test (rejected: last session's 8-minute investigation exists precisely because nothing was documented).

**Verification:** thread sample after SIGINT (parked in `run_loop`, no exit); SIGTERM exit in 0.35 s; `self_test.rs` (`--self-test`) already exits via the clean path. Any future SIGINT handling must supersede this decision with a new ADR, not a code change.

### Decision D021

**Title:** SpeakoFlow Mini (0.8B) is the default transform/cleanup model; S1-mini (0.6B) is kept as a recommended alternative.

**Date:** 2026-10-19

**Status:** accepted

**Decision:** The default local LLM for the Polish transform is **SpeakoFlow Mini** (`speakoflow-mini`, Q8_0, 0.8B, Apache-2.0, `Speakoflow/speakoflow-mini`), selected in `Settings::default()` (`selected_llm_provider: "local-server"`, `selected_llm_model: "speakoflow-mini"`) and as the EG-1 migration target. S1-mini (`s1-mini`) remains in the catalog (`recommended: false`, still pickable) for anyone who prefers it. SpeakoFlow gets its own card-faithful path (`run_speakoflow` in `transforms/engine.rs`, `SPEAKO_SYSTEM_PROMPT` + `build_speakoflow_messages` in `transforms/prompt.rs`), routed by the `is_card_cleanup` dispatch (either `s1-mini` or `speakoflow-mini` + Polish preset). Unlike S1-mini's protocol, SpeakoFlow uses a fixed system prompt + BARE transcript (no control line) and no max_tokens cap (the card warns capping truncates long inputs; we use `polish_max_tokens` ceiling only as a worst-case bound).

**Reason (measured, 2026-10-19):** a 12-case head-to-head battery on identical transcripts, both models on fresh llama-servers with their own intended prompts, scored **S1-mini 1/12 vs SpeakoFlow 11/12**. S1-mini was functionally broken in the harness: empty output on 7/12 cases (C1, C2, C6, C8, C9, C11, C12), a `<think` reasoning leak (C5), a 256-token "you know" hallucination loop on filler-only input (C10), and over-normalization that altered content ("420000 dollars"→"$420,000 dollars", C7). SpeakoFlow: 0 empty, 0 loops, 0 content damage; perfect restraint (C1 char-for-char), correct grammar/spelling fixes (C2/C3), correct self-corrections (C6/C11), exact name/number preservation (C7/C12), reliable on long input (C8). This matches the published card (SpeakoFlow 70.7% vs S1-mini 15.3% overall; S1-mini 55.3% content damage). SpeakoFlow is a *cleanup* model, not a rewriter: it fixes what is broken and leaves correct text, names, numbers, and code identifiers untouched.

**Verification:** `cargo test --workspace` green (747 core / 259 desktop / 79 inference incl. the new `polish_with_speakoflow_uses_card_faithful_path` + `non_polish_with_speakoflow_does_not_use_card_path` dispatch tests); `cargo clippy -p teletype-core -p teletype-inference -p teletype-desktop` 0 warnings. Model file `speakoflow-mini.gguf` (sha256 `696769bb…41fc9`, matching the catalog pin) installed in `~/Library/Application Support/com.teletype.app/models/`.

**Alternatives considered:** keep S1-mini as default and add SpeakoFlow as an opt-in (rejected: the measured 1/12 vs 11/12 gap makes S1-mini an unreliable default); route SpeakoFlow through the generic `build_prompt` path with no card-faithful handler (rejected: that sends the model an off-distribution "transformation engine" prompt instead of its fixed cleanup prompt, undermining its restraint); replace S1-mini entirely (rejected per user: keep it available).


### Decision D022

**Title:** The first-release pill (classic capsule + the 4 ported styles) is the shipped default; the 2026-10-20 token-based pill revamp is reverted.

**Date:** 2026-10-20

**Status:** accepted (implemented; `ui/src/pill.tsx` + `ui/pill.html` restored from `af6d2de^`, `default_pill_style()` → `Classic`)

**Decision:** `ui/src/pill.tsx` and `ui/pill.html` were restored to their pre-`af6d2de` ("revamp dictation pill and onboarding") state, which carries the v0.1.0 first-release designs: the LiveKit-ported classic/levelRail/well/dotGrid styles with their dark translucent surfaces and rainbow hairlines. The new token-based "Teletype" design was NOT deleted — it remains selectable as `pillStyle: "default"` — but `default_pill_style()` in `commands.rs`, the `pill.tsx` fallback, and the `SettingsScreen` picker fallback now resolve to `classic`. The restore keeps the post-v0.1.0 architecture: mounting lives in `pillEntry.tsx` (088c8d4), so the self-`createRoot` block was stripped from the restored file, and `rainbowColor`/`SIZES` re-exports come from `lib/rainbow.ts`.

**Context:** Ali rejected the revamped pill the same day it landed ("I'm not liking this pill user interface") and asked for the first-release pill plus the other first-release styles back. His stored setting was `well`; it was set to `classic`. The revamp's own `PILL_STYLES` picker already offered all five, so "bring back the other pills" was satisfied by restoring their original rendering, not by re-adding options.

**Verification:** `tsc --noEmit` clean; `vitest run` 144/144; `cargo test -p teletype-desktop --lib commands::settings` + `typed_settings_enums` green (the wire-value assertion updated to `"classic"`); `vite build` emits `dist/pill.html`; live check in Chrome: `pill.html` renders the restored idle capsule and the Settings picker shows all five styles with classic selected. Note: `commands::settings::tests::hotkey_capture_commands_are_available_on_every_platform` aborts with a foreign Obj-C exception in this environment — pre-existing, unrelated to settings serialization.

**Alternatives considered:** revert to the v0.1.0 tag file directly (rejected: it predates the pillMachine refactor and the `pillEntry` mount fix and would not compile against current `pillMachine.ts`); keep the revamp visuals for the ported styles (rejected by the user).


### Decision D023

**Title:** Insights filters scope range totals, apps, phrases, pace, impact and learned-fix counters; habits, records, milestones, first activity and persisted usage stay all-time. Lifetime is the full history since first use.

**Date:** 2026-10-22

**Status:** accepted (implemented in `insights::compute_for_window` + `commands/history.rs` `get_insights`)

**Decision:** `compute_for_window(all, window, now)` takes the FULL history plus an optional window and runs two passes. The all-time pass (daily series, heatmap, words today / last 7 days, average, longest take, streaks, records, milestones, first activity) ignores the window. The window pass applies `in_window` to totals, top phrases, top apps, vocabulary, busiest hour, pace/impact and the learned-fix counters (`dictionary_learned`, `personalization_corrections`). `get_insights` must always pass the merged full history, never a pre-filtered snapshot, because the all-time pass reads `all.entries` directly. Lifetime is `window = None`, computed from all retained rows, so it grows with use and never shrinks when another range is selected. This supersedes the "Fixes made counters are lifetime-only" clause of D013; the persisted filler/AutoText counters remain all-time because they carry no timestamps (the rest of D013 stands).

**Context:** Ali reported that lifetime, month and week gave different answers for the same metrics and that lifetime must reflect everything since first use. The root causes were (a) `get_insights` passed a window-filtered history as `all`, so "all-time" views moved with the filter, (b) bounded ranges read the in-memory snapshot (1000-row cap), and (c) the learned-fix counters had been dropped. SQLite now retains every history row (`push_history` no longer trims; `MAX_HISTORY_ENTRIES` caps only the `history_all` snapshot), and `read_history` is read-only and WAL-safe.

**Evidence / source:** `crates/teletype-core/src/insights.rs` (`compute_for_window` and its semantics tests), `crates/teletype-core/src/persist.rs`, `crates/teletype-desktop/src/commands/history.rs`, `ui/src/screens/InsightsScreen.tsx` (labels state the mixed semantics).

### Decision D024

**Title:** A take with no speech is not recorded; a cleanup-only model cannot run a non-Polish preset; the "Show me" preview must say when it fell back or returned the input unchanged.

**Date:** 2026-10-22

**Status:** accepted (implemented)

**Decision:**
- Empty final text (no-speech take) creates no usage count, no history row and no dictionary usage, both in the live `transcribed()` path and in the staged `stage_record` path. `list_dictation_history` and `get_insights` also hide legacy blank rows on read instead of deleting stored rows.
- S1-mini and SpeakoFlow Mini are dictation-cleanup fine-tunes; for any preset other than Polish (`builtin-polish` or `auto_apply`) the engine returns the input with `SkipReason::ModelNotCapable` without calling the model. The gate tests model identity separately from the preset (`is_cleanup_model && !is_polish`), because the card-cleanup flags already include `is_polish`.
- The generic prompt's `<<<` / `>>>` wrapper is stripped from model output before validation, so an echoed delimiter cannot be pasted into the user's text.
- `test_transform` returns `{ text, transformed, skipMessage }`; the preview shows a warning when `transformed` is false (with the skip message) or when the output equals the input. Silent fallbacks must never render as a successful "Polished" result.
- When the selected model is cleanup-only and the preset is not Polish, the Transforms screen shows a status card naming the model, marks unsupported preset cards "Needs a general model", and offers "Open Models"; a failed preview renders the reason in the polished pane instead of blank placeholder text.

**Context:** Ali reported a blank card under TODAY (a no-speech take recorded as a dictation) and that "Show me" on Prompt Engineer returned the input unchanged while displaying it as polished. Replays showed SpeakoFlow treats another preset's instruction as transcript text and returns the input, sometimes with the closing `>>>`.

**Evidence / source:** `crates/teletype-desktop/src/dictation.rs` and `dictation_stages.rs` (blank guard), `transforms/engine.rs` (capability gate), `transforms/validator.rs` (delimiter strip), `commands/transforms.rs` (`TestTransformResult`), `ui/src/screens/TransformsScreen.tsx` (preview notice, blocked card).

### Decision D025

**Title:** Downloads support hard cancel with partial-file cleanup; Models are grouped by type with recommended-first order.

**Date:** 2026-10-22

**Status:** accepted (implemented)

**Decision:**
- `cancel_download(id)` records the id in `AppState.download_cancel_hard` and sets the existing pause flag so the worker stops promptly. The worker removes the hard-cancel entry only after `handle.await`, deletes every `<id>*.part` and `<id>/*.part` partial, emits `model-download-progress` with `status: "cancelled"`, and returns `download cancelled`. Finished shards are never deleted. Pause stays resumable; cancel is terminal.
- Download workers register their pause/cancel flag only after the disk-space preflight and remove it only after the worker joins. Removing a flag before `handle.await` makes a cancel click during a live download unreachable.
- `model-download-progress` carries `status` from `downloading | verifying | paused | cancelled | done | error`; the app-wide store clears `cancelled` exactly like `done`. The UI shows an X + "Cancel" control while progress is active (downloading, verifying, paused).
- The Models rail separates Speech models, Polish models, and Cloud models, each with a one-line hint. Rows sort recommended-first within each group, and each detail pane carries a type chip ("Speech model", "Polish model", "Cloud model"). "Local LLM" is named "Polish Model" throughout the UI.

**Context:** Ali asked for a cancel control in the download UI, a clear split between speech/polish/cloud models, recommended-first ordering, and the "Local LLM" rename. An earlier implementation removed the pause flag before `handle.await`, so a cancel click during a speech download silently did nothing.

**Evidence / source:** `crates/teletype-desktop/src/commands/models.rs` (`cancel_download`, `partial_files_to_delete` and its tests, `download_speech_model`), `lib.rs` (`download_cancel_hard`), `ui/src/lib/downloadStore.ts`, `ui/src/components/DownloadProgress.tsx`, `ui/src/screens/ModelsScreen.tsx`. Verified in the browser preview with a synthetic progress event (bar + Cancel rendered) and a cancelled event (bar cleared); `cargo test -p teletype-desktop --lib -- --skip commands::settings` -> 307 passed.

### Decision D026

**Title:** The catalog row is the single model-to-engine mapping; a model is offered only when the platform ships its engine *and* this build compiled it.

**Date:** 2026-10-22

**Status:** accepted (implemented)

**Decision:**
- Every speech model carries an `Engine` (`crates/teletype-speech/src/catalog.rs:27`: `Whisper | Parakeet | ParakeetCoreml | SherpaParakeet | ParakeetCpp`). There is no per-model engine table, no config file, and no engine name in the UI: `dictation.rs:6041` maps `catalog::Engine` -> `SpeechEngineKind`, and that is the only routing site. Adding an ASR engine means adding one enum variant, one `available_on`/`built_on`/`label`/`platform_label` arm, and rows that use it - nothing else.
- Availability is **two-level on purpose**. `Engine::available_on(platform)` answers "does this platform ship the dependency" and must mirror the target-gated `Cargo.toml` deps (whisper-rs, parakeet-sys, parakeet-cpp-sys are macOS-only; sherpa-onnx unconditional; Core ML needs Apple Silicon + macOS 14+). `Engine::built_on(platform, parakeet_cpp_available)` answers "can *this* binary load it" - only parakeet.cpp needs the extra fact, because its dylib is produced by a build script and a build without it must not offer its models. Conflating the two is exactly the reported bug: the catalog offered Fluid Speech Mini on a machine where the dylib had never been built, and the user got "Speech model load failed: parakeet.cpp engine unavailable" after downloading it.
- `SpeechModel::offered_on_with(platform, fluid_available, parakeet_cpp_available)` (`catalog.rs:183`) is **the single offer predicate**: `available_on && !auxiliary && (engine != ParakeetCoreml || fluid_available) && built_on(...)`. The model list, the default pick (`recommended_speech_model_id`, `catalog.rs:735`), the persisted-selection migration, and the onboarding card all consult it, so a recommendation can never name a row that is absent from the list (that mismatch previously left onboarding with no card to render). `offered_on` is the production wrapper that resolves the runtime fact via `parakeet_cpp::engine_available()`; `offered_on_with` exists so tests and the browser preview can inject it.
- The engine identity is rendered from the engine, not from a UI string: `engine.label()` and `engine.platform_label()` are serialised as `engineLabel`/`platformLabel`, so an unavailable engine explains itself in the row instead of failing at load time.
- Platform-specific model families are expressed through this gate, not through a build-time feature list. On Windows only sherpa-onnx rows are offered; on Apple Silicon the macOS-only engines plus Core ML are offered subject to `fluid_available`. Apple-specific acceleration is therefore already surfaced (whisper.cpp/parakeet ggml/parakeet.cpp on Metal, Core ML on the Neural Engine). **MLX is not an engine we ship** and there are no MLX rows in the catalog; supporting it is a new `Engine` variant plus a sys crate, which is what this decision exists to make cheap.

**Context:** Ali's item 1 was a load failure on every parakeet.cpp model and the ask "every model should have its config and engine on which engine to load for which model... we will be engine agnostic so that we can add more engines as more ASR models come up"; item 2 was "for MLX models or Apple-specific models show it on Apple devices, for Windows show whatever it can support". The existing design already routed engine-per-model from the catalog; what it lacked was the *build* level of availability, which is why a platform-legal row could still be unloadable.

**Evidence / source:** `crates/teletype-speech/src/catalog.rs:27-93` (Engine + the two predicates + labels), `:158-201` (offer gate), `:735` (`recommended_speech_model_id`), `crates/teletype-desktop/src/dictation.rs:6041` (engine -> provider), `crates/teletype-desktop/src/commands.rs:187` (label surface), `crates/teletype-speech/src/parakeet_cpp.rs` (`engine_available()`), `crates/parakeet-cpp-sys/build.rs` + `Cargo.toml` `links = "parakeet_cpp_sys"` (the `DEP_PARAKEET_CPP_SYS_ROOT` / `TELETYPE_SPEECH_PARAKEET_CPP_LIB` path handoff). Tests: `catalog.rs:970-1130` (`recommended_speech_model_id_is_always_an_offered_row`, `..._requires_the_fluid_bridge_for_core_ml`, `offered_on_is_the_list_predicate`). See also L034 (platform-aware selection) and L038 (the parakeet.cpp exit-path follow-on).


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

### Gotcha G022

**`skip_serializing_if` on a struct the UI maps directly is a wire-shape
regression bought for one line of JSON savings.**

**Symptom (production, v0.2.6):** Dictionary screen crashed for users whose
saved words had no sound-alikes:
`undefined is not an object (evaluating 's.aliases.length')`. Intermittent by
looking, deterministic per word.

**Why:** `DictionaryWord.aliases` carried
`#[serde(default, skip_serializing_if = "Vec::is_empty")]`. The skip defaulted
in *files* correctly, but on the *IPC* path it omitted the key for every word
without aliases, so `list_dictionary` returned rows with `aliases: undefined`
and `w.aliases.length` (DictionaryScreen) threw. The roundtrip tests only ever
checked words that *had* aliases, and ipc_contract.rs pinned `learnedFrom`'s
absent-key behaviour but never `aliases` — the exact gap that let this ship.

**How to apply:** for any struct that crosses the Tauri IPC into screens that
index fields directly, never use `skip_serializing_if`; pay the few bytes per
row and always serialise. When you DO rely on absent-key semantics, add an
ipc_contract.rs assertion pinning the key's presence/absence — the contract
test only protects what it explicitly pins. Fixed 2026-10-11: serde attribute
dropped, DictionaryScreen normalizes `aliases/pronunciation/usageCount` on
read, and contract_10 asserts every word row carries an `aliases` array.

**Verification:** `cargo test -p teletype-core` (401+13+11+62+15 pass),
`npm run typecheck` clean.

### Gotcha G023

**A bare-modifier `CGEventTap` silently dies after macOS disables it; the
hotkey stops working with no error in the log.**

**Symptom:** The Mod hotkey (e.g. double-tap Fn) works for a while, then
stops responding entirely. No crash, no error log — the tap just stops
delivering `kCGEventFlagsChanged` events.

**Why:** macOS disables event taps that it considers unresponsive
(`kCGEventTapDisabledByTimeout`) or after certain user-input patterns
(`kCGEventTapDisabledByUserInput`). The tap object still exists, but it no
longer receives events. The callback never fires, so the app has no idea the
tap is dead.

**How to apply:** in `modTapCallback`, handle `kCGEventTapDisabledByTimeout`
and `kCGEventTapDisabledByUserInput` *before* the `kCGEventFlagsChanged`
branch. If `g_tap` is non-NULL, call `CGEventTapEnable(g_tap, TRUE)` and log
`"tap re-enabled after disable"`. This makes the tap self-healing: the next
modifier press works again. Fixed 2026-10-13 in `crates/teletype-desktop/src/mod_tap.m`.

**Verification:** `cargo check -p teletype-desktop` clean (no bundled
llama-server required for the C file). Manual test: hold the Mod key, wait
for the tap to disable (or trigger it via system settings), release, press
again — the hotkey fires and the log shows the re-enable line.

### Gotcha G024

**SIGINT does NOT stop Teletype — the app ignores it outright; use SIGTERM or `app.exit()`. A probe that fires INT and waits will hang, look like a hang, and cost minutes.**

**Symptom:** `kill -INT <pid>` against the running app produces no exit, no log line, no crash report; the process is fully alive (sample it: `teletype-dictation` parked in `run_loop`, ~5 threads, no teardown in flight). Observed live for 45+ s on 2026-10-18.

**Why:** no signal handler is registered anywhere — the clean-exit path is Tauri `core:app` exit (window close / tray Quit / updater restart hooks), not a handler. SIGINT is unhandled → ignored. SIGTERM *is* handled (the runtime/library path) and exits clean in ≈0.35 s. D020 makes this a contract.

**How to apply:** any kill/restart/smoke-test path — dev loop skills, `scripts/e2e/*`, CI, ad-hoc agent work — uses `kill -TERM <pid>` (or `pkill -TERM teletype`) and waits for exit; `-9` is never a substitute (it skips shutdown hooks, `DeadOnExit`, clipboard restoration — the same class of damage as G014). If someone adds a SIGINT handler, update D020 with a superseding ADR first.

### Gotcha G025

**"STALE BINARY / GHOST BUILD" traps across multiple won't-should worktrees sharing one target dir: before diagnosing a UI/state mismatch, prove *which* binary is actually executing -- the tree you're editing and the process can legitimately diverge three different ways.**

**Symptom (2026-10-18, real):** a session deployed the `ahsorathiya7-fuzzy-happiness` worktree build while the *other* checkout's `target/debug/teletype` was newer-looking (timestamp was misleading); launchd was holding a stale pty; a later `kill` with wrong signal (INT, see G024) made it look like the app was a zombified ghost (window visible, AXQueryable,MainUI 0 s, no crash log). Cost: 8 minutes to 2 episodes of the wrong diagnosis.

**How to apply (one-liner audit, run before any "my change isn't showing" debugging):**

```bash
WT=$(git rev-parse --show-toplevel)                      # where am I?
EXE=$(ps -A -o pid,comm -x | awk '$2 ~ /teletype$/ {print $1}' | head -1)   # running pid
lsof -p $EXE 2>/dev/null | awk '$4=="txt"' | head -1     # ACTUAL executable inode path
git -C $(dirname $(dirname $(dirname $(&file -b $EXE 2>/dev/null)))) status --porcelain | head   # that tree's dirtiness
ls -la $WT/target/debug/teletype                         # does the tree I edit even have a binary?
```

The 3 ways the trees diverge: (1) you're editing worktree A, the process runs B; (2) the process runs A via an old binary (A wasn't rebuilt since its edits — `target` older than source = build stale, not binary stale); (3) same binary serve is ELF rewritten under a dataroot (rare, but the G014 inode trap shape). If (1): stop the process (TERM), `cargo build` the right tree, relaunch. If (2): rebuild. If (3): you're in G014 territory, not a binary-freshness issue. Cross-check that the binary and its worktree match via the binary's embedded `git describe` (if present) or the build timestamp vs `git log -1 --format=%ci`.

**Verification (when the fix is claimed):** one full dictation round-trip through the app you believe is running — if the fixed behavior does NOT appear, the diagnosis (and the "fix") is wrong regardless of what the source diff says.

### Gotcha G026

**"CONTROLLER WEDGE: joining the recovery spool writer while capture still holds a `SpoolWriter` clone" — a dictation take ends, `Session::stop()` deadlocks on `Spool::complete() -> JoinHandle::join()`, and the dictation controller thread freezes forever. Every subsequent hotkey, command, and ESC input dies after that. Cancel unresponsive. No crash log (the app is ALIVE, just frozen).**

**Symptom (2026-10-19, real, user-visible):** after one dictation take, the pill is stuck on "Listening — waiting for speech…" and the Cancel button does nothing. All subsequent Fn presses produce no reaction. PID alive, no crash report.

**Root cause (three-thread circular wait, verified by code + thread sample):**

```
Session::stop() [dictation.rs]
  1. RECORDING -> take()            (lines ~2860)
  2. spool.complete()  [recovery.rs]   <-- BLOCKS FOREVER
     -> tx.take()  (drops controller-side mpsc Sender)
     -> writer.join()
        Writer thread: mpsc::Receiver::recv() blocks until channel DISCONNECTS
        Disconnect requires ALL SpoolWriter clones to be dropped — but a clone
        lives inside the capture thread's on_frame closure, which is still
        RUNNING because the capture thread does not stop until recording.finish()
        ... and finish() runs in the worker that the stop() block returns.
```

- `teletype-dictation`: parked at `spool::complete -> JoinHandle::join -> thread::park -> __ulock_wait`
- `teletype-recovery-writer`: parked at `mpmc::list::Receiver::recv`
- `teletype-audio` (capture): alive, holding `SpoolWriter` in its `on_frame` closure

**Why both VAD and Hold are affected:** both move a `spool_cb` clone into `on_frame` (the `start_with_vad_df` non-VAD drop path in `dictation.rs:2270`, and the VAD callback closure from ~line 2375). Whether the first frame arrives before or after the worker spawn is irrelevant to the stop ordering — the wedge occurs at stop time either way.

**Introducing commit:** `9a1969d` (p3.7/p3.9/p2.3: AutoText placeholder + dictionary import/export + recovery spool) wired `spool.complete()` into `stop()` without considering the live capture holder. Never once fixed on feat/beat-wispr; must not be. (The "lost-fix" narrative was a prior-session thesis, unverified.)

**Evidence (2026-10-19):**
- Thread sample of stuck app PID 66446 (see stack above; writer parked in recv, controller parked in join)
- Three residual spool files in `~/Library/Application Support/com.teletype.app/recovery/` from wedged stops (20:03:30, 23:11:38, 23:41:21) — `complete()` never reached `remove_file`
- Regression test `recovery::tests::complete_returns_with_live_writer_clone_held` — RED before fix (threw, `recv_timeout` inside the test harness), GREEN after fix (returned under 500 ms)
- After fix, live app PID 29984: two consecutive takes (01:03:35 and 01:03:59) completed, controller thread sample parked at `Controller::spawn -> mpmc::Receiver::recv` (normal message loop), recovery directory empty after clean stop, startup recovery cleaned up a prior orphan spool (`deleted abandoned tiny spool .../spool-1792355450663.pcm`)

**The fix (verified live):**
1. **Order**: a new non-blocking `Recording::stop_capture()` is called **before** the spool join in `Session::stop()` (sends on `stop_tx`, does not join). Capture stops -> `on_frame` dropped -> the clone is gone -> channel disconnects -> writer drains -> join returns in <10 ms.
2. **Budget (defence in depth)**: `Spool::complete()` and `Spool::abandoned()` join via `join_writer_budgeted(handle, WRITER_JOIN_BUDGET=500ms)` (side thread + `recv_timeout`); on budget expiry the spool file is left on disk (recovery retains it) and a Warn is logged — the controller can never wedge here again.
3. `recording.finish()` inside the worker spawned by `stop()` still joins and collects the final `Captured` audio in a loop — no behavior change beyond the ordering.

**How to apply:** anything that joins threads potentially waiting on channels/resources held by other still-running threads must get the shutdown ordering right — stop the producers of held resources first, then join. In Rust, prefer BOUNDED joins: a free thread + `recv_timeout`, never a raw `JoinHandle::join` on shared-ownership threads. If you ever see `JoinHandle::join -> thread::park -> __ulock_wait` on `teletype-dictation`, don't touch the code — sample every thread first and hunt the circular wait. Symmetric audit: when reading `spawn`ed a thread that holds a captured `Option<Sender>` or closure-owned `Sender<T>` alive until `drop` or explicit `take()`, the shutdown owner must guarantee all senders are dropped before anyone joins the receiver end.

---

## 8. Known constraints

Record constraints that future agents must know.

| Constraint | Why it exists | Evidence / source |
|---|---|---|
| Never link llama.cpp in-process | ggml duplicate symbols vs whisper.cpp | `checkpoint.md`, D002 |
| `llama-server` must be one self-contained binary (static, `LLAMA_OPENSSL=OFF`) | `tauri.conf.json` bundles that single file: a shared build needs ~10 sibling dylibs via an absolute `/tmp` LC_RPATH, and the default link pulls Homebrew `libssl` | G009, L004, `scripts/build-llama-server.sh` |
| Never re-host or bundle publisher model weights | EG-1 Community Model License 1.0 clause 2 (the `eg-1` entry itself was removed in P3.9, 2026-10-15) | `roadmap.md` license analysis; commit `1ebd339` |
| S1-mini attribution: "S1-mini by Superwhisper" | Apache-2.0 naming term | `roadmap.md` |
| ~13 MB release binary, no bundled model assets | Performance/privacy non-negotiable | `roadmap.md` section 0 |
| `teletype-core` stays platform-free | Cross-platform boundary; tested via mocks | `docs/architecture.md` |
| English-first; Parakeet is English-only, `"auto"` resolves to `"en"` | Model capability; prevents mis-transcription | `checkpoint.md` language guard |
| macOS 14+, Rust 1.88+, Node 18+, CMake required | Build/toolchain floor | `README-github.md` |
| Windows platform layer stubbed | UIA/OCR, injection, hotkey, tray not implemented | `README-github.md` Status |
| No telemetry / analytics ever on by default | Product stance + EW contrast | `roadmap.md`, D003 |
| Never log transcripts, AutoText values, or keys | Privacy contract | `docs/architecture.md`, `prompt.md` |
| Never edit `S1_SYSTEM_PROMPT` without retraining | The prompt text and the S1-mini artifact are one contract (transcribed from the published card at the pinned revision); only the generic `CORE_RULES`/`build_prompt` path may absorb new rules | `crates/teletype-core/src/transforms/prompt.rs` header comment |
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
Selecting a local LLM model (S1-mini, Qwen3) errors with
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
| L015 | Tray quit SIGABRT in `ggml_metal_rsets_free`: whisper.cpp parks its Metal GPU device in a C++ function-local static; at `exit()` (called directly by NSApplication's `terminate:` on the tray-Quit path) the static destructor asserts residency-set buffers are empty and aborts. Fix: `RunEvent::Exit` unloads both speech managers **before** `mark_teardown()`, and `mark_teardown()` now calls `exit(0)` so the static destructors run immediately while Metal is still up. Verified: quit via `kAEQuitApplication` AppleEvent with both models warm → clean exit, no crash report. Tried-and-reverted alternatives (NSApplication `setTerminate:`, `NSAppleEventManager` selector) are documented in the learning. **Amended 2026-10-22 by L038:** "unloading here empties the residency sets" is true of whisper.cpp's static device, **not** of parakeet.cpp, which parks its device in a process-global `pk::Backend` that no `unload()` frees - that engine needs an explicit `shutdown_backend()` call in the same hook, and `mark_teardown()` is terminal (`exit(0)`), so nothing may be ordered after it. | lifecycle / macOS / Metal | verified (parakeet.cpp case superseded by L038) | `learnings/2026-10-05-teletype-quit-crash-metal-static-destructors.md` |
| L016 | Dictation "Choose file" button did nothing: the dialog plugin is registered in Rust but `dialog:allow-open` was missing from `capabilities/default.json`, so Tauri v2 denied the `open` IPC command silently (rejected promise swallowed by `.catch(console.error)`). Fix: add `"dialog:allow-open"` to the capability. Verified: native NSOpenPanel now opens with the audio filter. Reusable: in Tauri v2, registering a plugin is not enough — every plugin command used from the webview needs a capability permission; denied IPC is just a rejected promise. | dictation / tauri capabilities | verified | `learnings/2026-10-05-dictation-choose-file-dialog-permission.md` |
| L017 | A "premium UI" revamp means richer use of the **existing** `ui/index.html` design tokens (`--shadow-1/2/3`, `--text-tertiary`, `--border-subtle`, `--accent-soft`, `--tracking-tight`, `--ease`) — multi-layer gradient heroes, elevated cards, icon-chip headers, tabular-nums figures, section eyebrows — **not** a new color palette, and **without** changing the data model, IPC calls, `dictation-state` refresh, or empty-state copy. Proven by the 2026-10-06 Insights rebuild. | frontend / design | verified | `learnings/2026-10-06-insights-premium-revamp-design-tokens.md` |
| L018 | Two coupled lessons from the tab-by-tab battle test. (1) Tauri's global `confirm()` invokes the dialog plugin's `message` command, so a destructive action's native OK/Cancel dialog silently fails to appear (no error) when the capability lacks **`dialog:allow-message`** — the deprecated `dialog:allow-confirm` does NOT cover it. Fixed in `capabilities/default.json`; verified the native NSAlert appears. (2) AX-press / synthetic clicks do **not** reliably trigger React `onClick`: a button that "does nothing" under AX-press is not proof of an app bug — confirm via the Developer log for the expected IPC call, or drive from the DevTools console. | frontend / tauri / testing | verified | `learnings/2026-10-06-ax-press-does-not-trigger-react-synthetic-clicks.md` |
| L019 | A stray installed `/Applications/Teletype.app` can run in parallel with the dev build, each spawning its own EG-1 `llama-server` (violates the "no duplicate copies" goal invariant). The stray's PID changes between kills (manual relaunch, not a watchdog; no launchd/login-item). Fix: kill the installed app **and** its llama-server child; re-check. Before any live-app check, `ps -eo pid,command | grep -E "teletype|llama-server" | grep -v grep` and kill anything that is not `./target/debug/teletype` / vite. | dev environment / process hygiene | verified | `learnings/2026-10-06-stray-installed-app-duplicates-dev-build.md` |
| L020 | Tauri bakes the app version into DMG/EXE filenames and the reported version from `tauri.conf.json` + workspace `Cargo.toml`, **not** from the git tag. A `v0.2.4` tag shipped `Teletype_0.2.2_aarch64.dmg` because the two files had drifted. Fix (2026-10-06): every job in `release.yml` rewrites both files to `${GITHUB_REF_NAME#v}` right after checkout, so artifact names and the app version always match the tag. | release / CI | verified | `8ff97ae` (release.yml) |
| L010 | **A `teletype_desktop_lib` test is flaky.** During the 2026-09-28 ground-truth test-state re-run, `cargo test -p teletype-desktop` showed `58 passed; 2 failed` once and `59 passed; 1 failed` once in early runs, then **never reproduced across 6 consecutive clean runs** (including 3× dedicated `-p teletype-desktop`). The failing test's name was not captured before it turned green. Steady state is green (578 pass / 0 fail / 1 ignored workspace-wide). **Not yet root-caused** — candidate causes: a timing/ordering-dependent desktop test, or an environment race (AX permission, a live llama-server child, or the re-sign state from L006). How to apply: if a desktop test fails in one run and passes on re-run, do not trust either result — re-run the single test in isolation (`cargo test -p teletype-desktop <name>`) 5× to confirm flakiness, capture the name the first time it fails, and check for environment races before blaming the code. | desktop / testing / flaky | candidate (observed 2×, not reproduced since) | — |

| L022 | A chain of `git reset --soft` on the shared `main` branch silently dropped a sibling teammate's unpushed commit (`91c838c`): the reset rewound HEAD past it, so it vanished from the branch while the tree stayed clean and tests passed. Recovered from `git reflog` (`git show <sha>`), content re-applied as `7a399b3`. Rule adopted: no reset/amend/history rewrite on a shared branch without lead approval; when a reported commit is missing, check reflog before assuming it was never made. | git / multi-agent coordination | verified | `learnings/2026-09-25-shared-branch-reset-drops-sibling-commits.md` |
| L023 | Take kind was tagged BEFORE the state machine decided (`input_with_kind` set `self.take_kind` unconditionally), so a Hold-mode release (`HotkeyUp` → Dictation) re-tagged an in-flight command take as dictation and `transcribed()` silently misrouted it; swallowed cross-kind presses did the same. Fix (P2.4–2.8, commit `c601f65`): tag only on `Action::Start` (in `start()`); keep a per-session kind and never pass a fresh default to a stop path. Pinned by `swallowed_press_does_not_retag_take_kind`. Reusable: session mode/kind fields must be written at the single transition INTO the active state, never on the way into `decide`. | dictation / state machine | verified | `learnings/2026-10-15-take-kind-retag-before-decide.md` |
| L024 | The eval harness produced no numbers until it was run on real data: the first real Gate 0 run surfaced three separate panics/aborts (wer backtrace underflowed on the DP's left edge — 384/14,641 exhaustive pairs trigger it; one `NoSpeech` case killed a 500-case run; the polish judge sliced an empty token list and indexed short references past their length). All three are index/length arithmetic that assumed invariants the 17-clip synthetic smoke corpus never violated. Fixed in `9b54c2c` + `34df465` with red-pass-proven regression tests. Reusable: exercise any measurement tool on the real corpus before trusting a number; when one slice/index bug is found, grep the file for the same pattern (they came in threes). | evals / measurement / testing | verified | `learnings/2026-10-15-eval-harness-real-data-bugs.md` |
| L025 | `Earnings22-Cleaned-AA-chunked` ships VAD-segment audio + timings but NO per-chunk transcripts; the old builder apportioned parent words to chunks by duration fraction, which fabricates WER wherever the local speech rate departs from the call average (hypothesis = middle/tail of the window; 0.82 per-chunk). PCM equality proved the file mapping was never the bug; the chunks tile each parent losslessly, so scoring the reassembled whole call against the parent transcript is exact (0.1218, in line with published Parakeet numbers). Fix `429e860`. Reusable: verify a dataset ships the reference *alignment*, not just audio+timings; systematic hypothesis shifts point at the reference, not the model. | evals / datasets | verified | `learnings/2026-10-15-earnings22-chunk-reference-misalignment.md` |
| L026 | bash reads a running script lazily by byte offset; editing the file mid-run (here: committing a change to `fetch_datasets.sh` while two fetch jobs executed it) makes bash re-read shifted bytes and abort with a phantom "unexpected EOF while looking for matching quote" at the script's end. The script was fine (`bash -n` passes; job side effects were complete). Rule: never edit a shell script while a background job runs it — copy it to a temp path for jobs that must survive edits; verify side effects, not exit status. | dev workflow / background jobs | verified | `learnings/2026-10-15-bash-lazy-read-mid-run-edit.md` |
| L027 | P3.7 (§3.7) made `correct_with_dictionary` a thin wrapper over `rescoring.rs`: pass 1 (exact/alias) runs first with confidence 1.0 (phonetic pass provably dormant in the text-only path), then the preserved per-token near-miss tiers (edit-distance fuzzy + pack fuzzy) run over unconsumed tokens. Findings: rescoring pass 1 is a HashMap, NOT Aho-Corasick (dep retained for the future wrapper — do not "fix"); the old near-miss tiers are not subsumed and stay. Gate: `correct_with_dictionary_old` preserved verbatim under `cfg(test)` + `p3_7_wrapper_is_byte_identical_to_the_old_fixer` (27-case corpus incl. the documented apt/its/pie hazard rejections). | dictionary / pipeline | verified | `learnings/2026-10-15-p37-dictionary-wrapper-identity-gate.md` (commit `562c34f`) |
| L028 | P3-F (§3.5): `whistle.cact` (16.9 MB) exists in the app models dir — matches the plan's "tiny engine". `needle3.cact` (35 MB) is the text model loaded by `teletype-inference`'s `NeedleProvider` (NEEDLE_TEXT slot); `whistle.cact` loads into the speech slot (NEEDLE_SPEECH) — they coexist in different slots. `needle-sys` has no `unload`; `reset()` keeps the model resident (known limitation of the process-global C API — document it). `needle_transcribe` FFI has no safe wrapper; call via `unsafe`. `needle_sys::set_audio` safe wrapper handles CString lifecycle. `WhistleProvider` is optional-only (weights license unverified per plan §15): never a default, no catalog `Engine` variant, nothing downloaded. Keywords: top-50 terms (plan §7.5), each ≤ 64 chars, total ≤ 4096 chars, newline-separated. `split_on_pause()` is a pure, tested VAD-pause splitter with 0.3 s overlap + 30 s cap. | speech / whistle | verified | `crates/teletype-speech/src/whistle.rs` (commit `796370e`) |
| L029 | P3.7a (§3.7 pipeline wiring): replaced `use_parakeet` bool with `SpeechEngineKind` enum (Whisper/Parakeet/Fluid/Sherpa/Whistle) derived from catalog `Engine`. Third `SpeechModelManager` ('neural') holds Fluid/Sherpa/Whistle providers. Model path semantics: Fluid → `<models_dir>/fluid` base dir (provider appends repo folder, downloads on load); Sherpa → `<models_dir>/sherpa/<model-id>`; Whisper/Parakeet → `<models_dir>/<file>`. `transcribe()` returns `(String, Option<Vec<Word>>)` — `transcribe_detailed` when `word_timings` capability, else `transcribe` + `None`. Words feed `rescore_with_confidence()` (P3-G) over dictionary + pack terms before the pipeline; engines without timings skip rescoring (byte-identical). `build.rs` emits `-Wl,-rpath,/usr/lib/swift` for the app binary (required by P3-A FluidAudio Swift runtime). Callers updated: commands.rs (transcribe_word, get_runtime_status, transcribe_file), recovery.rs (recover). Whistle catalog wiring deferred (weights license unverified). | dictation / pipeline | verified | `crates/teletype-desktop/src/dictation.rs`, `lib.rs`, `commands.rs`, `recovery.rs`, `build.rs`, `Cargo.toml` (commit `af63e2c`) |
| L030 | pi worker `bw-p43` exited with ZERO edits: 30 read-only commands + 12 ranged reads (mostly the ~4000-line `dictation.rs`), 0 edit calls, 138k input tokens, final message a malformed `<function=todo>` pseudo-call, then settle. Sibling workers edited normally in the same wave — this is an exploration-budget failure, not transport and not task vagueness. Mitigation being validated: put hard budget rules in the kickoff prompt (grep then ranged reads ≤300 lines, ≤10 ops then edit, restate owned files). A pi "success" exit proves nothing: always check the event log's final message AND the worktree diff for the owned files. | dev workflow / pi dispatch | candidate | `tasks/pi-tasks/logs/bw-p43-events.jsonl`; `learnings/2026-10-16-pi-worker-exploration-budget-exhaustion.md` (retry `bw-p43r` in flight) |
| L031 | Every HF Common Voice repo is dead for programmatic fetch: `mozilla-foundation/common_voice_17_0` and all other `common_voice_*` repos 401 (license tightened to Mozilla-Data-Commons-only; verified 2026-10-19). The plan's `cv` set slot is therefore filled by a substitute: Common Spoken Language Surveys EN v5.0 (MDC id `cmu5nqn1h00vwmi07b4dbk085`, CC0, 2026-09-11, synthetic TTS answers to survey prompts, all clips ~10s). Pipeline: download the MDC tarball manually (no verified programmatic MDC loader), then `CV_TARBALL=/path/to/tarball bash scripts/eval/fetch_datasets.sh cv` — extracts, keeps the 357 validated `test`-split clips, drops 52 degenerate quality-tagged rows (`short-audio`/`transcription-length`/`non-allowed-script`), samples 200 with the standard seed via `subset.py`. Verified 2026-10-19: rebuild md5 `9798a84…` bit-identical across runs; `parakeet-ultra-coreml` smoke `cases=200` verbatim micro WER 5.89% / report-normalized 8.37%, 0 case_errors, 64s (synthetic clips → pipeline validation only, per the harness's own warning). The `cv` row of the bake-off baseline awaits the full bake-off re-run. | evals / datasets / Common Voice | verified | `learnings/2026-10-19-cv-slot-mdc-substitute.md` |
| L032 | Product-quality claims come from a cross-tab/layer/theme **QA battery**: every screen × running raw / keypass / Polish × on-demand theme, reading **real DOM values** as the assertion (not rendered values), fresh-read-after-write to defeat async read races. The 2026-10-16/19 batteries surfaced bug classes units miss: stale async setting-read, duplicate spawn, layout contamination, policy leaks. Unit: one screen per layer. DOM reads: fresh-read-after-write is the guard for deferred-write read. | desktop / QA practice | verified (batteries 2026-10-16, 2026-10-19) | sessions 2026-10-16 / 2026-10-19 |
| L033 | Phase 7 personalization is wired into production (2026-10-21, `release/v0.2.7`). **Reachability gotcha:** the store-observation layer (`observe_learning_store`, `poll_learning_observer`) used to `clear()` the `EditWatch` after observing, which silently killed the preference/dictionary hint path for every real paste. Rule: that layer writes the final `ObserverState` back via `set_observer()` and **never** clears the pending; `observe_pending_edit` is the only consumer of `take()`. Reusing a final observer cannot double-record. **Promotion:** `consider_observed_edit` runs only when `learn_from_edits && learn_terminology`, and `created_terms` (only terms the take added) flows into `LearnedOffer.lexicon_terms`; undo checks the row is live before tombstoning so it counts real undos only. Known gap: a lexicon-only promotion (no preference) creates no undo offer, because offers key on preference ids. **Boost (P7.5):** per-take list = lexicon + ephemeral screen terms, one-entry cache keyed by lexicon fingerprint + app category + screen hash; capacities Fluid 100 / Sherpa 50 / Whisper 30 / Whistle 50 / ggml Parakeet 0 (no native boost; post-ASR rescoring covers it). Applied to the streaming provider before the first segment and to the whole-take provider **after `load()`** (Fluid refuses terms until ready). Whisper's `unload()` clears terms but `load()` preserves them; an empty list is still applied so stale terms are cleared. | personalization / Phase 7 / dictation | verified (desktop filtered suites 13 + 3 + 24 green; core 768 green) | `crates/teletype-desktop/src/{dictation.rs,edit_watch.rs,commands/personalization.rs}`, `crates/teletype-core/src/learning/{promote.rs,boost.rs}` |
| L034 | Speech-model selection is platform-aware and honest about what can run (2026-10-21). `pick_engine(pinned, platform, language)` never returns an entry the host cannot load: a pin is honoured only if `available_on(platform)`, then non-Parakeet languages route to Whisper where it ships, macOS 14+ Apple Silicon defaults to `parakeet-ultra-coreml` (`parakeet-v2-coreml` when English-preferred), Windows defaults to sherpa-onnx int8, Intel/older macOS keeps ggml Parakeet v3. `Engine::available_on` is the single gate (Whisper/ggml Parakeet = macOS only; CoreML = macOS 14+; sherpa = all). Downloads verify the catalog `sha256` when pinned and **discard** a mismatched temp file; unpinned entries fall back to the rounded `size_mb` bound (1% slack), because a truncated transfer used to install and report Done. | speech / catalog / downloads | verified (speech 53 green; desktop check clean) | `crates/teletype-speech/src/catalog.rs`, `crates/teletype-desktop/src/commands/models.rs` |
| L035 | All three parallel `pi --mode json` dispatches exited 0 yet made ZERO edits: every model request hit "Request timed out" (auto-retries exhausted before the first tool call); a single cheap `pi --no-session -p` probe reproduced it, so it was endpoint unavailability, not contention. Detection rule: pi exit 0 proves nothing — grep the events JSONL for `auto_retry_end success:false`/`aborted` and confirm work exists via `find <scope> -newermt <dispatch-time>` + diff. Fallback validated: the self-contained task files (`tasks/pi-tasks/*.md`) ran unchanged through in-session general-purpose agents. | dev workflow / pi dispatch | candidate | `learnings/2026-10-21-pi-endpoint-timeout-zero-work.md`; `tasks/pi-tasks/logs/*-events.jsonl` (2026-10-21) |
| L036 | A delegated agent's "failed" notification is a claim about its **output channel**, not its lifetime: two background agents kept writing for 6+ minutes after their notifications (mtimes 21:03/21:04/21:06 vs a ~21:01 failure), and the lead nearly re-implemented one of them before a failing `0 occurrences found` Edit exposed the work already on disk. Rule before reproducing an agent's work: `find <owned paths> -newermt <dispatch time>` + grep its distinctive symbols to prove absence, and attribute a diff to an agent by mtime, never by a whole-tree `git diff --stat` (that produced a false scope-creep accusation here). Mirror image of L035. | agent team / delegation | candidate | `learnings/2026-10-21-agent-wrote-after-failure-signal.md` |
| L037 | Post-dictation learning is gated by four chokepoints that are invisible from the UI: (1) `ObserverEvent::FocusLost`/`EnterPressed`/`Keystroke` are never dispatched in production (only `NextDictationStarted` and `Canceled` from `dictation.rs`, plus a 20s poll), so an edit settled by Enter/Send in a client that clears the field is lost unless the last-good-read fallback has it (fixed 2026-10-22: memory-only `LastGoodRead` + `FLUSH_MIN_QUIET_MS`, passed on both `NextDictationStarted` and the idle poll); (2) promotion re-reads `state.platform.focused_text()` blind at the promote site and silently drops when the field moved; (3) `JudgeContext::default()` leaves `min_conf: None`, which makes `judge.rs` require `proper_noun_like(to)`, so ordinary vocabulary can never promote (fixed 2026-10-22: the production site opts into `allow_lowercase_terms`, capped at 1-2 word corrections); (4) `promote::undo` cleared candidates by term, but candidate rows are keyed by `(alias, term)`, so stale candidates survived an undo (fixed + regression-tested); (5) the boost loop IS wired (`build_boost_terms` → `set_boost_terms`), but `boost_capacity` is 0 for ggml Parakeet and the rescore fallback reads only the Dictionary, so a lexicon-only promotion is inert there (both recommended defaults, Fluid 100 / Sherpa 50, boost fine). Also: Windows focused-field reading **does** work (`uia::focused_control_text()`); only the non-mac/non-Windows fallback is `Unreadable`. | learning / personalization | verified | `learnings/2026-10-21-learn-pipeline-settle-and-gates.md` |
| L038 | **L015's unload-before-teardown rule is engine-specific and does not cover parakeet.cpp.** `pk::Backend` is a process global (`parakeet.cpp/src/ggml_graph.cpp:27`); the C API's `parakeet_capi_free` only deletes the context, so the Metal device and its non-empty residency sets survive every `unload()` and `ggml_metal_rsets_free` asserts at `exit()` (crash report `teletype-2026-10-22-144615.ips`, reproduced deterministically at exit 134 by the ignored real-engine test and by `examples/parakeet_cpp_proof.rs`). Fix: call `pk::shutdown_backend()` after the last provider unload and before `mark_teardown()`; `GGML_METAL_NO_RESIDENCY=1` also silences it (58 s audio: 1.04-1.05 s vs 1.12-1.19 s with residency) and is kept only as a documented fallback. Two adjacent gotchas: (a) **macOS `dlsym` wants ONE leading underscore** on a C++ mangled name - `nm` prints the table form `__ZN2pk16shutdown_backendEv`, which resolves to null and makes a correct teardown hook look inert; the working name is `_ZN2pk16shutdown_backendEv`. (b) `mark_teardown()` calls `exit(0)` and **never returns**, so the llama-server reaper that sat below it was dead code on macOS and every quit orphaned the child - the Exit hook is now ordered persist -> 3× unload -> `shutdown_backend()` -> inference shutdown -> `mark_teardown()` last. Also: `kill -TERM` exits status 0 **without running `RunEvent::Exit`** (verified: pid 30085 alive and its pid file unchanged afterwards); only the tray/AppleEvent quit reaches the hook. Live tray-quit click not verified (tauri-pilot is not wired into this app). | lifecycle / macOS / Metal / speech engines | verified | `learnings/2026-10-22-parakeet-cpp-quit-crash-shutdown-backend.md`; `crates/teletype-speech/src/parakeet_cpp.rs`, `crates/teletype-desktop/src/lib.rs`, `crates/teletype-speech/tests/real_parakeet_cpp.rs` |
| L039 | **`tauri::AppHandle` is not `Send` on macOS** (it wraps tao's `Rc<EventLoopWindowTarget>`), so a SIGTERM handler cannot store it in a `static Mutex<Option<AppHandle>>` or move it into a `std::thread`; both fail with E0277. The t7-sigterm design stores it in an `AtomicPtr<AppHandle>` (raw pointers are `Send+Sync`) and the handler calls `AppHandle::exit(0)`, routing SIGTERM through the same `RunEvent::Exit` teardown the tray Quit uses (persist -> unloads -> `shutdown_backend()` -> provider `shutdown()` -> `mark_teardown()` last), which fixes the L038(b) gap where `kill -TERM` bypassed the hook. **Three things that design got wrong, found by review, not by the compiler:** (1) `install(&app.handle().clone())` stored a pointer to a *temporary* that dies at the semicolon, so every later SIGTERM dereferenced freed memory; the handle must be taken by value and leaked (`Box::leak`) for the pointee to outlive the signal. (2) The claim "macOS delivers signals to the main thread" is false for this process: with a tokio runtime alive, the kernel may run the handler on a worker thread, where touching a non-`Send` `AppHandle` is UB regardless of the pointer being valid. (3) `exit(0)` is not async-signal-safe: `runtime_handle.request_exit` -> tao 0.37.1 `EventLoopProxy::send_event` (`tao-0.37.1/src/platform_impl/macos/event_loop.rs:343-355`) does a channel send that may allocate, then `CFRunLoopSourceSignal`/`CFRunLoopWakeUp`; and its error path (`tauri-2.12.0/src/app.rs:581-587`) calls `cleanup_before_exit()` + `std::process::exit()`. So a SIGTERM landing while the main thread holds the malloc/CF lock can hang instead of exiting; the residual risk is documented in `sigterm.rs` rather than papered over. A strictly correct version blocks SIGTERM with `sigprocmask` and reads it with `sigwait` on a thread holding a `Send` handle. Companion: a startup `reap_orphan(pid_file)` (bounded SIGTERM+5s then SIGKILL+2s, `ps`-gated to only kill a recorded pid whose `ps -o command=` still shows `llama-server` and `-m`) reaps orphans from prior killed runs. Also: **`kill -0` is not a liveness probe for a child you are reaping** - it succeeds for zombies; use `ps -p <pid> -o state=` and treat `Z`/empty as gone (this bit the first `bounded_kill`, which waited the full 5s and called the dead process alive). And the `lib.rs:878` E0277 that blocked the build was **not** pre-existing: it was the new orphan-reap block calling `app.state::<AppState>()` (a `&mut tauri::App`, `!Send`) *inside* the already-spawned warmup thread; using the closure's own `handle` clone is the fix. | lifecycle / macOS / signals / tauri | verified (compiler evidence for the `&mut App` capture; `nm`/source evidence for the tao proxy path; **runtime A/B on the live dev app**: `kill -TERM <app>` -> child llama-server gone and pid file removed in 1.1 s, while the `kill -9` control left llama-server reparented to launchd (ppid 1) with a stale pid file, proving no external reaper does it; the startup `reap_orphan` was isolated by pointing the pid file at a decoy (a `python3` sleep whose `ps` line contains `llama-server -m`, in its own process group) - it died within 5 s of launch and the pid file was replaced by the app's own server) | `learnings/2026-10-22-apphandle-not-send-macos.md`; `crates/teletype-desktop/src/sigterm.rs`, `crates/teletype-desktop/src/lib.rs` (warmup thread + `sigterm::install`), `crates/teletype-inference/src/server.rs` (`reap_orphan` + `bounded_kill`) |
| L040 | **A judge that cannot be satisfied by a correct answer is worse than no judge.** Several `behavior-cases.jsonl` cases listed a `forbidden` signal that sits *inside* the only rendering their own `must_contain` requires, so a perfect model output graded as critical; `contains_phrase`'s boundary lookahead also let `$450` match inside `$450,000`, contradicting its own docstring. Repaired with expected-win suppression, case/separator-sensitive identifier matching, and a load-time satisfiability linter (exit 2 on an unsatisfiable case) that now runs on every corpus load, plus a committed correct-output fixture the `--self-test` uses against the real corpus. | evals / polish gate | verified | `learnings/2026-10-22-polish-gate-selfintercepting-corpus-traps.md` |
| L041 | **Untracked files that tracked code already import break the *next* commit, not HEAD.** 22 Oct 2026 audit: `crates/teletype-desktop/src/sigterm.rs` (imported by tracked `lib.rs:65 mod sigterm;`) and `ui/src/lib/dialogs.ts` (imported by tracked `App.tsx`, `AutoTextScreen.tsx`, `DictionaryScreen.tsx`) plus the polish-gate script/corpus/fixture a tracked `ci.yml` step invokes were all `??`. `git show HEAD:<file>` proved HEAD was clean, so the hazard was prospective - a `cargo build` or `tsc` on a fresh clone would have failed. Recipe: list `git status --porcelain \| awk '$1=="??"'`, keep source extensions, then `git grep -l <basename>` restricted to tracked files; a hit means the file must be staged with its importer. Do not infer breakage from the working tree alone (the file exists locally, so everything compiles). | release integrity / git | verified | `learnings/2026-10-22-untracked-importers-break-next-commit.md` |
| L042 | The polish rail's "Recommended" badge and list order read **only** `CatalogEntry::recommended` / `sort_order` (`commands/models.rs:67,78` -> `ModelsScreen.tsx:632,723`), and nothing validated that pair, so a non-polish entry could wear the badge and lead the list. It did: `needle3` had `recommended: true, sort_order: 0, verdict: NotTested` while its own description says "NOT for polish (use S1-mini)". Fixed (badge off, `sort_order: 10`) and locked by `s1_mini_is_the_recommended_head_of_the_list` + `sort_orders_are_unique`. **Closed 22 Oct 2026, one level deeper than the badge:** `neudecide-q4` (the Command Mode decider) also carried `recommended: true`, so the repair is a **role** flag, not a copy edit. `CatalogEntry::polish_capable` (false for `needle3` and `neudecide-q4` only) now gates three things from one source: the catalog tests (`badge_requires_a_measured_verdict`, `only_polish_engines_are_offered_as_polish_models`), `select_model` (refuses a non-polish id; safe because Command Mode loads NeuDecide straight from `state.models_dir` and never through `select_model`), and the rail (`ModelsScreen` splits on `polishCapable` into a "Command Mode tools" section that offers Download but no Select, and its detail chip reads "Command Mode tool"). The flag is mirrored onto `ModelStatus` rather than re-derived in the UI, so the two cannot drift. `migrate_removed_models` also moves a saved non-polish pick to `speakoflow-mini`: a decider sitting in `selected_llm_model` loads without error and returns every preset's input untouched, which the user reads as "polish is broken", and it was reachable because the badge told people to pick it. Note the speech catalog enforces exactly-one-recommended (`teletype-speech/catalog.rs:916`); the LLM catalog has no such rule **by design**, because three polish rows are legitimately badged. | models UI / catalog invariants | verified | `cargo test -p teletype-inference --lib catalog` (10 passed), `ui/src/test/modelsRailSplit.test.tsx` (4), `crates/teletype-desktop/src/commands.rs` (`migrate_removed_models_moves_a_decider_out_of_the_polish_slot`) |
| L043 | **Most of the project brain is unshippable and the pointers are already dead.** `.gitignore:58` ignores `.agents/*` and re-includes only `state/` and `workflows/`, so `.agents/brain/` is ignored - yet 17 brain files are tracked (added before the rule, and git keeps tracking what is in the index despite a later ignore). Result measured 22 Oct 2026: 56 learning files on disk, 14 in git; `PROJECT_BRAIN.md` references 26 of them and **21 resolve to nothing on a fresh clone**, including every recent one. The tracked index is therefore AGENTS.md precedence level 2-3 pointing at files only this machine has. Candidate fix (not applied - a `.gitignore` change alters what every future commit includes, so it needs Ali's call): add `!.agents/brain/` and commit the 42 untracked learnings, or stop citing `learnings/...` in the tracked index and inline the lesson instead. | knowledge system / repo config | candidate (needs a decision) | `.gitignore:54-60`, `git ls-files .agents/brain \| wc -l` (17) vs `ls .agents/brain/learnings \| wc -l` (56) |
| L044 | **A blanket `*.txt` ignore rule silently drops corpus data that committed code opens.** `.gitignore:103` (`*.txt`, filed under "local scratch, exports, and generated artifacts") matched `evals/human-corpus/scripted/ref/*.verbatim.txt` and `*.clean.txt`, so the 24 reference transcripts that `scripts/eval/human_take.py` and the FORMAT.md scorer open never entered the commit that added the script, and `git add <dir>` reported success without mentioning the skips. Same class as L041 but worse: the importer is committed, so HEAD itself is incomplete. Found by comparing `git show --stat` against the intended file count, then `git check-ignore -v` on one missing path. Fixed with a narrow `!evals/human-corpus/**/*.txt` exception (the `.agents` case in L043 is deliberately left unfixed pending Ali's call). Lesson: after staging a data directory, diff the intended list against the commit's file list and treat a shorter result as an ignore-rule question, not as "nothing else to add". | release integrity / repo config | verified | `git check-ignore -v evals/human-corpus/scripted/ref/case-01.clean.txt` -> `.gitignore:103`, commit `b286c37` |
| L045 | **Split a large dirty tree with `git commit --only -- <paths>`, never with stash or reset.** An earlier `git checkout` in this repo destroyed ~220 lines of uncommitted work, so destructive index and worktree commands are off the table here. `git commit --only -F msg -- path…` commits the content of exactly those paths and leaves the rest of the index staged, so a dirty tree can be partitioned into reviewable commits using `git add` plus pathspec commits alone, and files already staged for another unit survive untouched (verified: 5 staged files stayed staged across 9 commits). Two constraints: a file lands in exactly one commit (`git add -p` is interactive, so split at file granularity on the dominant theme), and a manifest that declares a dependency must land in the same commit as, or before, its first use (the `tar`/`bzip2` deps went with the code that extracts sherpa archives so no intermediate commit fails to build). Prove the split preserved content with `git diff <base>..HEAD --shortstat` plus an empty `git diff HEAD`: together they show the commits equal the tree the tests ran on. | git workflow / repo safety | verified | commits `5098409..f953a3d`, `git diff cee77e6..HEAD --name-only` (144 files) |
| L046 | **Two Models-screen defects survived `tsc`, 170 vitest tests and a green `cargo test`, because both live in rendered state.** (1) The rail div carried `width: 216` *and* `flexBasis: "100%"`; in a row flex container `flex-basis` wins for the main size, so the rail ate the whole row and the sibling `flexWrap: wrap` pushed the "right detail pane" 1100px down the page, below every model. `flexBasis` appears exactly once in `ui/src` (it arrived in `b402100`, a commit whose message is about other screens), and the JSX comment `{/* Left rail */}` plus `<div style={{ flex: 1, minWidth: 0 }}>{/* Right detail pane */}` describe the layout the code no longer produced. (2) The default-selection effect ended in `return "openai"` and, above it, `if (cur === "openai") return cur`. On the first tick both catalogs are still `[]`, so it resolved to `"openai"`; the guard then made that permanent, and the screen opened on the cloud API form for every user regardless of what they had installed. Fix: the fallthrough returns `cur` (the placeholder covers the loading tick), and `refresh()` resolves both catalogs through one `Promise.all` before storing them, because two independent `.then(setState)` chains let whichever IPC lands first decide the pane. Generalise as two checks for any effect that defaults UI state from async data: does a "keep the current value" guard run before the fallback, and does the fallback fire on an empty not-yet-loaded list? And in a row flex container, a `flex-basis` beside a `width` is not decoration. | UI layout / React state | verified | `ui/src/screens/ModelsScreen.tsx` (rail style, `refresh`, default-selection effect), `ui/src/test/modelsRailSplit.test.tsx` (`opens the detail pane on a model, not on the cloud form`), screenshots at 1440/900/620 via `http://localhost:1420/?demo` |
| L047 | **Vocabulary boost: sentence-initial capitals and function words are not vocabulary.** `extract_vocab_terms` only dropped the first word of the file's first sentence (`m.start() == 0`), so every later sentence contributed its opening capital; the lone-word pass added any capitalized token. Earnings22 `vocab.json` carried `The`, `Today`, `Please`, `Thank`, and the CTC/native boost budget was spent on grammar. Fixed with `_at_sentence_start()` (back-scan over whitespace/quotes to the previous `.?!:;\n`), `STOPWORDS` trimming at phrase edges, sentence-start/stopword skips in the single-word pass, and a doctest. Re-ran all six earnings22 cells at `981900e` with pre-fix reports preserved: Ultra 6.88 -> 10.61 verbatim (was 14.42), TDT 12.18 -> 12.18 (was the 100.00 harness bug, now confirmed inert), Whisper 69.62 -> 68.94 (was 65.98; the old gain was common-word overlap). Entity accuracy drops under boost in every engine; no boost default changes. Lesson: a heuristic term list is a label, so re-run boosted cells whenever the extractor changes, and never compare recall across two different term lists. | evals / vocabulary boost | verified | `python3 -m doctest scripts/eval/subset.py`; `learnings/2026-10-22-vocab-boost-sentence-initial-trap.md`; `.agents/brain/asrs.md` |

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
| `https://models.enviouslabs.co/eg1/EG-1-MODEL-LICENSE.txt` | EG-1 usage/redistribution terms (entry removed from the catalog 2026-10-15, P3.9) | 2026-09-22 (HTTP 200 verified per roadmap) | Drove D001 |
| S1-mini download URL + SHA-256 (the EG-1 catalog entry and mirror were removed 2026-10-15, P3.9) | Catalog entry and checksum — now the Superwhisper Hugging Face repo | 2026-09-22 (verified live per roadmap); HF-primary re-verified 2026-10-15 | In `roadmap.md` and `catalog.rs` |
| `prompt.md` (original spec) | Product requirements, quality bar, privacy rules | 2026-09-22 | In-repo primary spec |
| LiveKit Agents UI (`github.com/livekit/components-js`, `packages/shadcn`, Apache-2.0) | Dot-matrix grid visualizer ported as the `dotGrid` pill style. Component is plain React+CSS (no WebGL); the LiveKit coupling is only `useMultibandTrackVolume`/`useAgent` wrappers, and the official `volumeBands` prop is the clean injection seam. Aura/Wave visualizers ARE WebGL shaders and are not cheap ports | 2026-09-26 | Vendored with attribution in `ui/src/pill.tsx` |
| `github.com/jaredpalmer/kev` (Apache-2.0) + `logicrw/awesome-jev-projects` + pinggy.io JEV-alternatives benchmark (2026-09-23) | Jev = TypeSafe's closed "System One" decision model: state + typed questions (Choice/Score/Noul) → probability per option in one parallel pass, no token generation, 50-100ms hosted. Open clones (Laya 421M BERT, Kev 0.8-27B Qwen+LoRA, SemIf frozen-logit wrapper, NanoJev 0.6B, jevlike trainer) reproduce the interface, NOT zero-shot accuracy: independent jabr 49-task benchmark = Jev 0.966 vs best-open 0.704. Kev-0.8B MLX on Apple Silicon: 47ms warm / 77ms fresh per multi-question request. Teletype fit: NOT a drop-in for polish (text generation, wrong job); plausible use = fast intent classifier (polish vs raw insertion), context-app detection, transform routing. Blocked on: Python sidecar vs Rust-only + llama-server D002 precedent, 13MB binary + no-bundled-weights constraint, zero-shot accuracy gap | 2026-09-26 | Research only, no code written; user asked "useful or not" before any implementation |
| `github.com/yijunyu/jev-rs` (Apache-2.0/MIT) + `jev-sdk` crate (docs.rs) | Rust-native Jev-class engine: reads next-token logprobs from any llama-server GGUF (Qwen3-4B: 75ms p50 warm, zero output tokens), wire-compatible `POST /v1/systemone`, `jev ask`/`serve`/`mcp`/`eval`/`calibrate`. Accuracy on dev_tasks: 0.71 overall (choice 0.92, noul 0.64, score 0.56) — same 0.70 open-class ceiling. Reuses the llama-server Teletype ALREADY bundles (D002 subprocess, no new runtime, no Python). `jev-sdk` = hosted-TypeSafe-only client (network, violates D003). Fit: strictly better than Python Kev for a future agent layer (same accuracy, Rust, no new sidecar), but still not useful for current deterministic pipeline | 2026-09-26 | Follow-up to Kev research; no code written |
| Root planning docs: `Teletype Agent Plan Beat Wispr Flow.md` + `Teletype Agent Plan NeuDecide Addendum.md` (desk-checked 2026-11-01) + `PROGRESS.md` on `feat/beat-wispr` | Governing plan: gates 0–7, P0.1–P7.1, baseline-ways rule, never-break-delivery rule, NeuDecide hybrid decider shape (tool from audio, arguments from transcript, Apache-2.0) | 2026-11-01 | Plan of record (Decision D020); executes on `feat/beat-wispr`; state = that branch's `PROGRESS.md` |
| `huggingface.co/AuroraAI-Research/AuroraASR-1M` (published 2026-10-18, Apache-2.0; 0 downloads at check) | Sub-1M (949,073-param) weight-shared Conformer-CTC dictation: 4 unique Conformer-lite blocks ×2 passes, d_model=128, 256-piece BPE, greedy CTC, no LM; trained on LibriSpeech-960h only, English-only, 16 kHz; **no punctuation/casing in output**. Self-reported WER (greedy): test-clean 11.43 / test-other 26.50 — within ~0.5 of Femto-ASR (2.39M, 10.95/25.36), far above Citrinet-256 (10M; 3.8/9.8), Moonshine Tiny (27M; 4.58 clean), Whisper tiny.en (39M; 5.6/14.6). Speed claims (unverified by us): RTF 0.0023 = 436× real-time on 1 CPU core; 3.81 MB FP32 / 1.06 MB INT8, claims near-lossless to INT8. Verdict 2026-10-21: admirable extreme-minimal model, but NOT a replacement for Parakeet TDT 0.6B v3 as our primary ASR — higher WER (parakeet class beats it by roughly 1/3 on clean and ~5× on test-other), no punctuation, read-speech-only training with no noise augmentation, no production track record. Plausible only as a cheap pre-screen/embedded path or an `evals/` baseline for tiny models | 2026-10-21 (HF API + model card; numbers self-reported) | Research only, no code written; user asked "is this a good ASR" before any implementation |

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
| 2026-10-21 | Platform-aware speech defaults: `catalog::pick_engine` returns `parakeet-ultra-coreml` on Apple Silicon macOS >=14 (with the Fluid bridge built), `sherpa-parakeet-v3-int8` on Windows/Linux, `parakeet-tdt-v3` on Intel/older macOS; the UI "Recommended" badge and first-launch default must come from the same function AND only ever name a model the host can actually load and list | User: "run by default Parakeet Ultra... recommend Ultra for Mac and whichever is best for Windows"; audits found badge/list divergence (`commands/models.rs:703` vs filter `:709`) |
| 2026-10-22 | Keep a standing ASR/audio eval register at `.agents/brain/asrs.md`; keep Needle 3 in it for evaluation even though its catalog row was removed (keep NeuDecide, remove needle3). Run evaluations on a regular cadence; after each round the measured top 3 per host class become the recommended models and Teletype's ranking follows the results. A recommendation requires a measured run, a loadable catalog row, and a confirmed weights license | User: "keep needle3 in brain for evaluations, we will keep evaluating a lot of asrs and audio models on a regular basis... once we do the runs, we will use that to improve the model ranking on teletype and accordingly the top 3 will be recommended"; `.agents/brain/asrs.md` |

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

| 2026-11-01 | `ali-agent-health` audit (6 checks) + `to-tickets` for the Beat-Wispr plan: registry 8/8 ACTIVE; all learning files status-tailed; fixed duplicate section number (`## 10. QA audit` → unnumbered); fixed stale `typing_windows.rs` path (now `typing/` since `8f86c5c`); added current-work topology (3-branch + plan authority), Decisions **D019** (EG-1 removal, `1ebd339`) and **D020** (commit-while-you-go streaming, `fcae04b`, plan of record), learning **L032** (QA battery practice), planning-docs source row; added ADR-013/ADR-014 files (local-only — see drift notes). Left for Ali: (a) brain is mostly gitignored (`.gitignore: .agents/*`) and the two tracked lineages (main `caa90f4`, refactor `ae98488`) will conflict on merge — prefer the refactor lineage and salvage from there; (b) ADR-013/014 and most learnings are untracked locally — track or resign if they must survive between checkouts; (c) an older, uncommitted brain revision (v0.2.5-era: "State here?" 31-item list, QA-battery bug list) existed in a prior working tree and was never committed anywhere in git — durable items from it have been re-verified and re-recorded above (L023); the rest is treated as lost. |

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

## QA audit (2026-09-27; historical section — see learning L023 for the current battery practice)

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
