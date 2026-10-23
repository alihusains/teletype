# TELETYPE: Beat Wispr Flow (Agent Execution Plan)

> **Audience:** an autonomous AI coding agent working inside the `alihusains/teletype` repository.
> **Author context:** produced 14 Oct 2026 from a code review of branch `whistle-integration` @ `486ba44` (the requested `needle3-final-test` branch was not on GitHub; apply this plan to whichever branch has the newest Needle/Command Mode work, after rebasing it on `main`).
> **Goal:** Teletype becomes **more accurate and faster than Wispr Flow**, keeps improving the more a user uses it, and ships with proof (benchmarks + tests).

---

## 0. How to use this document (read first, agent)

### 0.1 Execution rules

1. **Work phase by phase in the order given.** Every phase ends with a **Gate**. Do not start the next phase until the Gate passes and its checklist is ticked in `PROGRESS.md`.
2. **One branch, small commits.**
   - Create `feat/beat-wispr` from the newest branch. Run `git fetch && git rebase origin/main` first.
   - Use one commit per task ID (e.g. `P1.2: remove fixed 250 ms paste sleep`).
   - Every commit must pass `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` and `npm --prefix ui run typecheck`.
3. **Measure before and after.** Phase 0 builds the measuring tools. Every later change that claims a speed or accuracy gain must show the before/after numbers in its commit message and in `evals/RESULTS.md`.
4. **Never break delivery.** Whatever fails (AI polish, ITN, dictionary, learning, noise filter), the user still gets the raw transcript pasted. Add a test for every new fallback.
5. **Legal rules (hard):**
   - **Do NOT copy code from EnviousWispr** (GPLv3). Re-implement the behaviour only.
   - **Do NOT download, call or bundle EG-1** (EnviousWispr's model). Its license only allows use inside Envious Labs apps and bans use in competing products (see `EG-1-MODEL-LICENSE.txt` §1a, §2b in their repo). Remove it from the catalog (task P3.9).
   - **Do NOT download models from `models.enviouslabs.co`.** Use the original publisher (Hugging Face / GitHub releases).
   - Only use models and libraries with licenses listed in §15 (Apache-2.0, MIT, BSD, CC-BY-4.0 with attribution, plus Superwhisper's Apache-2.0 + naming clause for S1-mini).
   - Add a root `LICENSE` file (MIT, matching `Cargo.toml`) in P0.1.
6. **Privacy rules:** all audio, transcripts and learning data stay on the device. New data stores must be inside the app data dir, be deletable from Settings, and have a retention limit.
7. **When blocked** (e.g. an API differs from this document), write the finding in `PROGRESS.md` under "Deviations", pick the closest safe alternative, and continue. Never silently skip a Gate.

### 0.2 Target numbers (the definition of "beat Wispr Flow")

| Metric | Wispr Flow reference | Teletype target | How measured |
|---|---|---|---|
| End-to-end wait after you stop speaking, p50 | 1.28 s measured ([Superwhisper benchmark](https://superwhisper.com/benchmarks/wispr-flow-raw)); 700 ms internal p99 target ([Wispr Flow](https://wisprflow.ai/research/technical-challenges)) | **≤ 350 ms** (polish off), **≤ 600 ms** (polish on) | `DictationTrace` (P0.2), eval harness |
| End-to-end p95 | n/a | **≤ 700 ms** with polish on | same |
| Raw WER, 8-dataset blend | 8.7% ([Superwhisper](https://superwhisper.com/benchmarks/wispr-flow-raw)) | **≤ 8.0%** | `teletype-eval` public sets (P0.4) |
| Vocabulary-term recall (Earnings-22 with vocab list) | 63.6% ([Superwhisper](https://superwhisper.com/benchmarks/wispr-flow-raw)) | **≥ 90%** | `teletype-eval vocab` (P0.4) |
| Edit rate (words the user fixes / 100 dictated words) | Wispr goal: "zero edits" | **≤ 2.0 after 1 week of use**, trending down | Learning metrics (P7.9) |
| Works offline | No | Yes | e2e test with network disabled |

### 0.3 What already exists (do not rebuild)

- **Speech engines:** Whisper (whisper-rs, macOS only), Parakeet TDT v3/v2 via whisper.cpp ggml (`crates/parakeet-sys`, macOS only), Whistle via Cactus FFI (`crates/teletype-speech/src/whistle.rs`, feature `cactus`, local-only).
- **Audio and VAD:** Silero VAD (`crates/teletype-core/src/vad.rs`, `silero-vad-pure`), audio capture (`crates/teletype-core/src/audio/mod.rs`, cpal), crash recovery spool (`teletype-desktop/src/recovery.rs`), live-preview loop (`dictation.rs` `INTERIM_INTERVAL`).
- **Text processing and polish:** ITN (`teletype-core/src/itn.rs`), dictionary + vocab packs (`dictionary.rs`, `vocab.rs`), polish via llama-server sidecar (`teletype-inference/src/server.rs`), S1-mini special prompt path (`transforms/prompt.rs` `S1_SYSTEM_PROMPT`).
- **Learning:** edit watching (`teletype-desktop/src/edit_watch.rs`, macOS AX), learning rules (`teletype-core/src/personalization/learn.rs`, `candidates.rs`).
- **Command Mode:** `teletype-desktop/src/command_mode.rs` + Needle (`whistle.rs` / vendor `tinydecide`).
- **Evals:** `crates/teletype-eval`, `evals/` corpus (17 synthesized clips, empty `evals/human-corpus/`).

---

## 1. Architecture after this plan (target picture)

```
Hotkey ─► AudioFrontEnd ─► Segmenter (Silero VAD) ─► SpeechEngine (per OS) ─► Committer
             │  pre-roll ring buffer         │              │ boosted by UserLexicon   │
             │  DC/high-pass/AGC             │              ▼                          ▼
             │  (optional) DeepFilter → VAD only     word timings + confidence   committed text
             ▼                                                                     │
        TraceRecorder ◄──────────────────────────────────────────────────────────────┤
                                                                                   ▼
                    TextPipeline: ITN ► Dictionary/Alias ► ProtectedTerms ► Polish (LLM, gated) ► Guard
                                                                                   ▼
                                    Delivery (paste cascade)  ──►  Bookkeeping (async: history, usage, learning)
                                                                                   ▼
                     LearningLoop: EditObserver ► EditJudge ► UserLexicon / StyleMemory / FewShot ► Replay-verify
```

**Engines per platform:**

| Platform | Default engine | English high-accuracy option | Fallback | Vocabulary boosting |
|---|---|---|---|---|
| macOS 14+ (Apple Silicon) | **Parakeet Ultra Core ML** via FluidAudio (Neural Engine) | Parakeet TDT 0.6B **v2** Core ML | Existing ggml Parakeet; Whisper large-v3-turbo (whisper.cpp) for languages Parakeet lacks | FluidAudio `CustomVocabularyContext` (CTC rescoring) |
| macOS 13 / Intel | Existing ggml Parakeet v3 (Metal/CPU) | ggml Parakeet v2 | Whisper | Post-ASR phonetic rescoring (P3.7) |
| Windows 10/11 x64/arm64 | **sherpa-onnx** Parakeet TDT 0.6B v3 int8 | sherpa-onnx Parakeet v2 int8 | Whisper (whisper-rs CPU) | sherpa-onnx hotwords if supported for NeMo TDT, else P3.7 |

---

## 2. Repository map (paths the agent will touch)

```
Cargo.toml                                   workspace, license = "MIT"
crates/teletype-core/src/
  audio/mod.rs                               capture (cpal), resample_to_target (linear; replace)
  vad.rs                                     Silero VAD wrapper
  itn.rs                                     number/format normalization (regex recompiled per call: fix)
  pipeline.rs                                text pipeline (dictionary fuzzy fix at ~L540)
  dictionary.rs, vocab.rs                    user dictionary + vocab packs
  injector.rs                                paste (BEFORE_PASTE=250ms sleep L46: fix)
  storage.rs                                 JsonStore (double serialize L61-64: fix)
  personalization/{learn.rs,candidates.rs,…} learning rules
  transforms/{prompt.rs,engine.rs}           LLM prompt build (dynamic content too early: fix)
  state.rs                                   hotkey state machine (decide)
crates/teletype-speech/src/
  lib.rs                                     trait SpeechProvider
  catalog.rs                                 enum Engine {Whisper, Parakeet, Whistle}; model list
  parakeet.rs, whisper/, whistle.rs
crates/teletype-inference/src/
  catalog.rs                                 LLM models (contains EG-1: remove)
  server.rs                                  llama-server sidecar + hand-rolled SSE (UTF-8 bug L616)
crates/teletype-desktop/src/
  dictation.rs                               controller; transcribed(); interim loop; stop()
  command_mode.rs                            Command Mode
  commands.rs                                Tauri commands + Settings struct
  edit_watch.rs, ax_text.rs                  edit observation (macOS)
  lib.rs                                     AppState, handler registration
crates/teletype-eval/                        eval harness
evals/                                       corpus + reports
ui/src/                                      React UI (pill.tsx, screens/*)
.github/workflows/{ci.yml,release.yml}
```

---

## PHASE 0: Measurement, safety net, legal (must be first)

**Why:** you cannot prove you beat Wispr Flow without a trace and a real corpus. These tools are also what the learning loop later uses to verify itself.

### P0.1 Legal hygiene
- Add `LICENSE` (MIT, copyright "Ali Husain Sorathiya and contributors").
- Add `THIRD_PARTY_MODELS.md` listing every model, URL, license and attribution text (start from §15).
- `git rm` committed build/junk files: `ui/dist/` (add to `.gitignore`), `*.html`/`*.txt` chat exports, `prompt.md` at the repo root. Keep docs.
- Remove the duplicate vendor copy: keep `vendor/tinydecide/`, delete `vendor/tinydecide-model/` source duplicates (identical `engine.rs`/`tokenizer.rs`/`weights.rs`). Load the model file through the download manager rather than committing `model.bin`.

### P0.2 `DictationTrace` (per-stage timing)
Create `crates/teletype-core/src/trace.rs`:

```rust
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DictationTrace {
    pub session: u64,
    pub engine: String,          // "parakeet-ultra-coreml" etc.
    pub audio_ms: u32,           // speech duration
    pub t_hotkey_down: Option<u64>,     // monotonic µs
    pub t_first_audio: Option<u64>,
    pub t_hotkey_up: Option<u64>,
    pub t_asr_start: Option<u64>,
    pub t_asr_end: Option<u64>,
    pub t_text_pipeline_end: Option<u64>,
    pub t_polish_start: Option<u64>,
    pub t_polish_first_token: Option<u64>,
    pub t_polish_end: Option<u64>,
    pub t_paste_sent: Option<u64>,
    pub polish_skipped_reason: Option<String>,
    pub committed_segments: u32,
    pub tail_audio_ms: u32,      // audio decoded after release
    pub errors: Vec<String>,
}
impl DictationTrace { pub fn mark(&mut self, f: impl FnOnce(&mut Self, u64)) { f(self, now_us()) } }
```

- Use one monotonic clock (`std::time::Instant` relative to process start).
- Wire marks into `dictation.rs` at each stage. The trace travels with the session id.
- Append each finished trace as one JSON line to `<app_data>/traces/YYYY-MM-DD.jsonl`. Rotate after 7 days or 50 MB.
- Add a Developer-screen panel: a table of the last 50 traces, p50/p95/p99 of `release→paste` and per-stage medians, and an "Export traces" button.
- **Test:** `trace_marks_are_monotonic`, plus `trace_written_after_paste_not_before` (an assertion through a mock writer).

### P0.3 Bench harness (`criterion`)
Create `crates/teletype-core/benches/`:
- `itn_bench.rs`: `itn::normalize` over all corpus sentences.
- `pipeline_bench.rs`: `Pipeline::process` with dictionaries of 0, 500 and 5,000 entries.
- `resample_bench.rs`: 10 s of 48 kHz audio converted to 16 kHz.
- CI job `bench` (non-blocking) runs `cargo bench -- --save-baseline pr` and posts `critcmp main pr` output as an artifact.

### P0.4 Accuracy eval corpus and metrics
Extend `crates/teletype-eval`. All scoring code lives in `crates/teletype-eval/src/metrics.rs`.

1. **Datasets**, downloaded by `scripts/eval/fetch_datasets.sh` into `~/.cache/teletype-eval/`, never committed:
   - LibriSpeech test-clean and test-other: 200 utterances each, a random subset with a fixed seed.
   - Earnings-22: 100 segments **plus a per-file vocabulary list** of company names, tickers and people, extracted from the reference.
   - AMI IHM: 100 segments (meetings, disfluent).
   - Common Voice 17 English: 200 segments (accents).
   - FLEURS: 50 per language for en, de, fr, es, it, pt, nl, pl, ru, uk.
   - MUSAN noise mixes at 10, 5 and 0 dB SNR over 100 LibriSpeech test-other clips (script `scripts/eval/mix_noise.py`).
2. **Human corpus** in `evals/human-corpus/`, committed with consent and stored with Git LFS:
   - Record **≥ 120 real dictations**: at least 4 voices, at least 3 accents, MacBook mic, AirPods (Bluetooth HFP), USB mic, quiet room and café noise.
   - Content mix: 25% email/chat, 20% code and technical terms, 15% names and proper nouns, 15% numbers/dates/money, 15% self-corrections ("no wait, make that Thursday"), 10% lists.
   - Each clip gets `id.wav` (16-bit mono, any rate), `id.verbatim.txt` (what was said, fillers included), `id.clean.txt` (what the user wants pasted) and `id.meta.json` (`{speaker, mic, noise, app_context, vocab:[...] }`).
   - Add `scripts/eval/record_clip.sh` to make recording easy.
3. **Metrics:**
   - `wer_verbatim(raw_asr, verbatim)`, using the Open ASR Leaderboard Whisper English normalizer (port the normalization rules; don't copy code under incompatible licenses).
   - `wer_clean(final_text, clean)`, which scores what is pasted. **Fillers removed by polish must not count as errors here.**
   - `vocab_recall` = correctly produced vocabulary terms ÷ vocabulary terms present in the reference. `vocab_false_alarm` = vocabulary terms produced but not in the reference.
   - `entity_accuracy` for numbers, dates and money (exact match after ITN normalization).
   - `semantic_ok`: local judge using Qwen3-4B with a fixed rubric; score 0/1 for "meaning preserved, nothing invented". Report it, don't gate on it.
   - **Latency:** p50/p95/p99 of `release→paste` (from `DictationTrace`) in **live mode**, where the harness plays the WAV into the real pipeline in real time and emits synthetic hotkey up/down events. The existing replay mode doesn't count.
4. **CLI:**
   ```
   teletype-eval run --engine parakeet-ultra --polish s1-mini --set human,librispeech,earnings22-vocab,ami,cv,noise --live --out evals/runs/<git-sha>/
   teletype-eval compare evals/runs/<base> evals/runs/<head>   # prints a markdown diff table
   ```
5. **Thresholds file** `evals/thresholds.json`, enforced in CI (P9):
   ```json
   {"wer_verbatim_max_regression_pct": 3, "vocab_recall_min": 0.90,
    "latency_release_to_paste_p95_ms_max": 700, "wer_clean_human_max": 0.06}
   ```
6. Write `evals/RESULTS.md` (an auto-generated table). Every later phase appends a row.

### P0.5 Test infrastructure
- Replace every `std::env::temp_dir().join("fixed-name")` in tests with `tempfile::TempDir`. Known spots: `teletype-core/src/storage.rs` tests and `teletype-desktop/tests/file_transcribe.rs` (two tests share `teletype_p317_test`).
- Add `proptest`, `insta` (snapshot) and `cargo-fuzz` scaffolding.
- UI: add `vitest`, `@testing-library/react` and `jsdom` to `ui/package.json`, plus `ui/src/test/tauriMock.ts` that mocks `invoke`/`listen`.
- CI: add `cargo llvm-cov` on the Linux job; publish lcov plus a coverage floor = current − 0 (ratchet).
- CI: run `cargo test -p teletype-core -p teletype-inference` on `windows-latest`, not just `cargo check`.

**Gate 0**
- [ ] `teletype-eval run` produces a baseline for the current default (ggml Parakeet v3 + S1-mini) on all sets, saved as `evals/runs/baseline/`.
- [ ] Baseline numbers are copied into `evals/RESULTS.md`, including live p50/p95/p99.
- [ ] CI is green with the new jobs; coverage is reported.

---

## PHASE 1: Latency quick wins (no model change)

| ID | Task | Location | Expected gain |
|---|---|---|---|
| P1.1 | **Remove the fixed 250 ms paste sleep.** Replace `thread::sleep(BEFORE_PASTE)` with `wait_for_modifiers_released(max = 150ms, poll = 5ms)`: macOS uses `CGEventSourceFlagsState(kCGEventSourceStateHIDSystemState)` and checks the Cmd/Opt/Ctrl/Shift/Fn bits; Windows uses `GetAsyncKeyState` for VK_CONTROL/VK_MENU/VK_SHIFT/VK_LWIN/VK_RWIN. Return immediately if none are held. | `teletype-core/src/injector.rs` L46, L371 | ~200–250 ms |
| P1.2 | **Move clipboard restore off the critical path.** `BEFORE_RESTORE` (350 ms) runs on a detached thread after paste. Guard it with a per-session token so a new dictation cancels a pending restore of an older one. | `injector.rs` L398 | frees the pipeline for the next dictation |
| P1.3 | **Paste before bookkeeping.** In `transcribed()`, send `PipelineReady` (inject) right after the final text is ready. Move usage save (≈L1910), history save (≈L1944) and dictionary usage save to a `Bookkeeper` background thread (an mpsc channel; it coalesces writes for 250 ms). | `teletype-desktop/src/dictation.rs` | 5–30 ms + stable p99 |
| P1.4 | **Don't join the interim thread on release.** In `stop()`, set the interim cancel flag and **don't** wait for an in-flight interim decode. The final decode must not be blocked by the speech-model lock held by the interim decode. Give the final decode its own engine handle, or make the interim decode check a cancel flag between chunks. | `dictation.rs` ≈L1387 | 0–530 ms |
| P1.5 | **Compile ITN regexes once.** Replace `fn re(pattern) -> Regex` with `static` `LazyLock<Regex>` per pattern. For `format!`-built patterns, build them once inside a `LazyLock`. | `teletype-core/src/itn.rs` L236–241 | 10–30 ms |
| P1.6 | **Fix the double serialization** in `JsonStore::save` (delete the unused `_bytes`). Add `file.sync_all()` before rename. Keep a rotating `.corrupt.N` (N ≤ 3). | `teletype-core/src/storage.rs` L61–64 | I/O |
| P1.7 | **Cache-friendly polish prompt.** Reorder `build_prompt` so the **static** block comes first: role, CORE_RULES, output contract, list style. The **dynamic** block (app context, preferences, terms, language, text) comes last. Send a 1-token warm-up request with the static prefix at app start and after a model switch, keeping llama-server's `cache_prompt: true`. | `teletype-core/src/transforms/prompt.rs` L221+ | 100–300 ms on non-S1 models |
| P1.8 | **Hold the inference engine through an `Arc` snapshot**, not a mutex held across generation. Use `ArcSwap<Arc<dyn Engine>>`; the dictation clones the `Arc` and drops the lock. | `dictation.rs` ≈L1546–1792 | UI no longer freezes |
| P1.9 | **Delta streaming to the pill.** Emit `{seq, delta}` tokens, not the whole string each time. | `dictation.rs` ≈L1581 | IPC O(n) |
| P1.10 | **Fix the UTF-8 split** in the SSE reader. Keep a `Vec<u8>` buffer, split frames on `b"\n\n"`, and decode a frame only once it's complete. Add a proptest: random chunk boundaries (including mid-codepoint) give identical output. | `teletype-inference/src/server.rs` L616 | correctness (non-English) |
| P1.11 | **Trim silence before the final decode.** Use Silero VAD to drop leading/trailing silence beyond 150 ms, keeping 150 ms of padding. | `dictation.rs` before final ASR | faster decode, fewer hallucinations |
| P1.12 | **Pre-warm the speech model on hotkey-down** (LLM pre-warm already happens). If the model is unloaded, start loading on key-down, not on release. | `dictation.rs` `start()` | removes cold-start spikes |

**Tests for Phase 1**
- Unit: `wait_for_modifiers_released_returns_immediately_when_none_held` (inject a fake modifier source through a trait).
- Unit: `bookkeeping_happens_after_paste` (mock controller records the event order: `PipelineReady` must come before `HistorySaved`).
- Unit: `stop_does_not_block_on_interim_decode` (a mock provider sleeps 500 ms in interim; assert final starts in < 20 ms).
- Proptest: SSE UTF-8 split (P1.10); ITN output identical before and after the `LazyLock` change (snapshot the entire corpus with `insta`).
- Bench: ITN at least 10× faster than baseline.

**Gate 1**
- [ ] Live p50 `release→paste` improves by ≥ 400 ms vs. baseline, with WER unchanged (±0.2 pt).
- [ ] All new tests pass; ITN snapshot is unchanged.

---

## PHASE 2: Fix Command Mode (it doesn't trigger today)

### 2.1 Root causes found in code (verify with the failing tests first)

1. **The arm flag is cleared before the take is classified.**
   - `hotkey_down_event()` checks `if self.command_mode_active { self.command_mode_active = false; self.start(false); return; }`.
   - Later, `transcribed()` branches on `if self.command_mode_active { … run command … }` (≈L1558).
   - The flag is already `false` by then, so **every spoken command is treated as normal dictation.**
2. **Nothing ever arms Command Mode.**
   - The Tauri command `arm_command_mode` (`commands.rs` ≈L898) is registered in `lib.rs` but called nowhere in `ui/src`, the tray or any hotkey.
   - There's no Command Mode hotkey. The only working path is the **typed** "Run" box in `ui/src/settings/CommandModeSection.tsx` (`run_command`).
3. **In Hold mode, release doesn't stop a command take.**
   - The command path calls `self.start(false)` directly and skips `state::decide`, so `self.hotkey_down` is never set to `true`.
   - On release, `decide(Input::HotkeyUp)` sees `was_down == false` and returns `Action::Nothing` (`teletype-core/src/state.rs` ≈L104–106). Recording keeps going.
4. `Settings.command_mode_active` (`commands.rs` L230) is persisted but never read: dead state.
5. Unclassified commands go to Needle (`run_needle_command`). Without the `cactus` feature (the default in CI and releases), Needle is unavailable, so anything outside the deterministic intents fails with a refusal.

### 2.2 Tasks

| ID | Task |
|---|---|
| P2.1 | Write **failing tests first** in `dictation.rs` tests (headless controller): (a) `armed_command_take_reaches_command_router`, (b) `hold_mode_release_stops_command_take`, (c) `command_flag_does_not_leak_into_next_dictation`. |
| P2.2 | Introduce `enum TakeKind { Dictation, Command, Transform }` stored **on the session**: `self.sessions[id].kind`. Set it at `start()` and read it in `transcribed()`. Delete the `command_mode_active` bool from the controller and from `Settings`, with a serde-compatible migration (ignore the old field). |
| P2.3 | Route every start through the state machine: `self.input_with_kind(Input::HotkeyDown, TakeKind::Command)`, so `hotkey_down` and hands-free are tracked the same way for every take kind. |
| P2.4 | **Dedicated Command hotkey.** Add `Settings.command_hotkey: Option<String>` (default macOS `"Ctrl+Option+Space"`, Windows `"Ctrl+Alt+Space"`). Register it with the global-shortcut plugin next to the dictation hotkey. Hold or toggle follows `recording_mode`. Add it to `KeybindsScreen.tsx`, with conflict detection through the existing `hotkey_conflict` command. |
| P2.5 | **Spoken trigger (optional, on by default):** if a normal dictation starts with the wake phrase `"teletype"` / `"hey teletype"` (fuzzy, edit distance ≤ 1, first 1.2 s), strip it and re-route the take as `TakeKind::Command`. Make it configurable in Settings. Test with a mock transcript. |
| P2.6 | **Selection-aware commands**, like Wispr Flow's Command Mode. If text is selected when the command hotkey is pressed, the command becomes a transform on the selection ("make this shorter", "translate to Arabic"). This uses the existing transform engine. macOS reads the selection via `text_selection.rs`; Windows support comes in P8.5. |
| P2.7 | **No-model fallback:** when Needle is unavailable, run unclassified commands through the local polish LLM (Qwen3-1.7B or the configured model) using the **same JSON tool schema** and grammar-constrained output. llama-server supports a `json_schema` field. Keep all safety checks (allowlist, no shell metacharacters, no send/delete tools). |
| P2.8 | **UI feedback:** the pill shows a distinct "Command" state (icon + color). On refusal, show the reason for 3 s. Add a Tray menu item "Run voice command". |
| P2.9 | **Logging:** every command take writes `command_mode: armed / started / classified=<intent> / executed / refused(<reason>)` into the trace. |

**Gate 2**
- [ ] The P2.1 tests pass.
- [ ] Manual check on macOS. Hold the Command hotkey and say each of the following; every one does the right thing:
  - "open Safari"
  - "search the web for rust async"
  - "write an email to Sara saying I'll be late" (inserts a draft)
  - "make this shorter", with text selected
- [ ] With `cactus` off, an unclassified command still works through the P2.7 fallback.

---

## PHASE 3: Speech engine upgrade (Neural Engine on Mac, ONNX on Windows, vocabulary boosting)

### 3.0 What EnviousWispr uses (verified in their repo, re-implemented here without their code)

| Component | EnviousWispr choice | What Teletype will use |
|---|---|---|
| Parakeet runtime | **FluidAudio** (Apache-2.0, Swift; their fork `saurabhav88/FluidAudio` pinned in `Package.resolved`) | **Upstream** [FluidInference/FluidAudio](https://github.com/FluidInference/FluidAudio), pinned to a release tag |
| Parakeet models | `FluidInference/parakeet-ultra-coreml` (new default), `FluidInference/parakeet-tdt-0.6b-v3-coreml` | Same public Hugging Face repos (CC-BY-4.0) |
| Whisper runtime | WhisperKit (`argmaxinc/argmax-oss-swift`), models from `argmaxinc/whisperkit-coreml` (`openai_whisper-large-v3-v20240930_turbo`) | Keep whisper.cpp (already in Rust). Enable its Core ML encoder (`WHISPER_COREML=1`) for the large-v3-turbo option |
| VAD | Silero VAD Core ML (`FluidInference/silero-vad-coreml`, `silero-vad-unified-256ms-v6.0.0`) | Keep `silero-vad-pure` (works on every OS); optionally FluidAudio `VadManager` on macOS |
| Learning | Edit-judge classifier (mmBERT-small fine-tune) + "kev" word-check model, served from their own servers | Our own rule + phonetic + optional local-LLM judge (Phase 7). **Don't use their hosted models.** |
| Polish LLM | EG-1 (license forbids other apps), S1-mini | S1-mini (English), Qwen3 instruct models (multilingual). **Remove EG-1.** |

**Why Parakeet Ultra**
- Same architecture, tokenizer, 25 languages and speed as v3, with lower error rates ([moondream/parakeet-ultra](https://huggingface.co/moondream/parakeet-ultra)):

  | Benchmark | Parakeet v3 WER | Parakeet Ultra WER |
  |---|---|---|
  | Open ASR English average | 6.26 | **5.80** |
  | FLEURS (25 languages) | 11.62 | **9.55** |
  | Background noise | 6.72 | **5.82** |
  | Long-form | 2.71 | **1.94** |

- The Core ML build runs at ~127× real time on the Neural Engine, about the same as v3 ([FluidInference/parakeet-ultra-coreml](https://huggingface.co/FluidInference/parakeet-ultra-coreml)).

### 3.1 New crate `crates/teletype-fluid` (macOS only)

**Approach:** in-process Swift static library called from Rust through a C ABI. Use the [`swift-rs`](https://github.com/Brendonovich/swift-rs) crate (MIT/Apache, common in Tauri apps) **or** plain `@_cdecl` + `cc` linking. Don't use a CLI sidecar on the hot path.

```
crates/teletype-fluid/
  Cargo.toml                 [target.'cfg(target_os="macos")'.dependencies] swift-rs; build-deps swift-rs
  build.rs                   SwiftLinker::new("14.0").with_package("TeletypeFluid", "swift/").link()
  swift/Package.swift        swift-tools 6.0; platforms [.macOS(.v14)]; dependency FluidAudio exact: "<pinned tag>"
  swift/Sources/TeletypeFluid/Bridge.swift
  src/lib.rs                 safe Rust wrapper implementing teletype_speech::SpeechProvider
```

**C ABI (Swift `@_cdecl`, all calls synchronous, thread-safe through an internal serial actor):**

```swift
// Returns 0 on success; on error writes UTF-8 message into err (NUL-terminated).
@_cdecl("ttf_init")       func ttf_init(_ version: UnsafePointer<CChar>,   // "ultra" | "v3" | "v2"
                                        _ modelsDir: UnsafePointer<CChar>,
                                        _ err: UnsafeMutablePointer<CChar>, _ errCap: Int) -> Int32
@_cdecl("ttf_is_ready")   func ttf_is_ready() -> Int32
@_cdecl("ttf_set_vocab")  func ttf_set_vocab(_ json: UnsafePointer<CChar>) -> Int32   // [{"text":"Siobhan","aliases":["shivon","chevon"],"weight":1.0}]
@_cdecl("ttf_transcribe") func ttf_transcribe(_ pcm: UnsafePointer<Float>, _ n: Int,
                                              _ lang: UnsafePointer<CChar>,          // "en" | "auto" | BCP-47
                                              _ out: UnsafeMutablePointer<CChar>, _ outCap: Int) -> Int32
// out = JSON: {"text": "...", "confidence": 0.93,
//              "tokens":[{"t":"Hel","start":0.12,"end":0.20,"conf":0.98}, ...],
//              "words":[{"w":"Hello","start":0.12,"end":0.31,"conf":0.97}], "duration_ms": 61}
@_cdecl("ttf_unload")     func ttf_unload()
```

**Swift implementation notes**
- Load models with `AsrModels.downloadAndLoad(version: .ultra | .v3 | .v2)`, **or** a local directory if the user pre-downloaded. Then `AsrManager(config: .default)` + `loadModels(models)` (API shown in the [FluidAudio README](https://github.com/FluidInference/FluidAudio)).
- Bridge async to sync with a `DispatchSemaphore` inside the actor. Never block the main thread.
- **Vocabulary boosting:**
  - Build `CustomVocabularyContext(terms: [CustomVocabularyTerm(text:aliases:)])`.
  - Load the CTC spotter once with `CtcModels.downloadAndLoad()` + `CtcKeywordSpotter(models:)`.
  - Call `asrManager.transcribe(samples, customVocabulary: vocabulary)`.
  - With a Parakeet 0.6B model this is "Approach 2" (separate CTC 110M encoder, ~130 MB, ~26× real time, 99.4% dictionary recall on their earnings benchmark) ([FluidAudio Custom Vocabulary](https://github.com/FluidInference/FluidAudio/blob/main/Documentation/ASR/CustomVocabulary.md)).
  - Start from the documented `VocabularyRescorer.Config` defaults. Tune `cbw` and the similarity thresholds only through the eval harness (P3.8).
- Fill the `tokens`/`words` arrays from the result's token timings (FluidAudio exposes `tokenTimings`; verify the field names in the pinned version). Word confidence = mean of its tokens' confidences.
- Model storage: `<app_data>/models/fluid/<repo-name>/`. Respect `HF_ENDPOINT` for mirrors. Verify downloaded files (size + SHA-256, recorded in `catalog.rs`).

**Rust side (`src/lib.rs`)**
- `pub struct FluidParakeet { version: FluidVersion, vocab_hash: u64 }` implements `SpeechProvider`. `transcribe()` returns `text`; a new method `transcribe_detailed()` returns `Transcript { text, words: Vec<Word{ text, start_ms, end_ms, conf }>, confidence }`.
- **Extend the trait (backwards compatible):**
  ```rust
  pub struct Word { pub text: String, pub start_ms: u32, pub end_ms: u32, pub conf: f32 }
  pub struct Transcript { pub text: String, pub words: Vec<Word>, pub confidence: f32, pub language: Option<String> }
  pub struct BoostTerm { pub text: String, pub aliases: Vec<String>, pub weight: f32 }
  pub trait SpeechProvider: Send + 'static {
      // existing methods …
      fn transcribe_detailed(&mut self, samples: &[f32], language: &str) -> Result<Transcript, SpeechError> {
          self.transcribe(samples, language).map(|text| Transcript { text, words: vec![], confidence: 1.0, language: None })
      }
      fn set_boost_terms(&mut self, _terms: &[BoostTerm]) -> Result<(), SpeechError> { Ok(()) } // no-op default
      fn capabilities(&self) -> ModelCapabilities { ModelCapabilities::default() }
  }
  ```
- Runtime check: on macOS < 14 or Intel, FluidAudio is unavailable and the app falls back to the ggml Parakeet engine. Show "Neural Engine acceleration needs macOS 14 on Apple Silicon" in the Models screen.

### 3.2 Catalog and engine selection (`crates/teletype-speech/src/catalog.rs`)

Add `Engine::ParakeetCoreml` and `Engine::SherpaParakeet`. New model entries:

| id | engine | repo / URL | size | languages | default? |
|---|---|---|---|---|---|
| `parakeet-ultra-coreml` | ParakeetCoreml | `hf://FluidInference/parakeet-ultra-coreml` | ~600 MB | 25 EU | **Default on macOS 14+ Apple Silicon** |
| `parakeet-v2-coreml` | ParakeetCoreml | `hf://FluidInference/parakeet-tdt-0.6b-v2-coreml` | ~600 MB | en | English option ("highest recall") |
| `parakeet-v3-coreml` | ParakeetCoreml | `hf://FluidInference/parakeet-tdt-0.6b-v3-coreml` | ~600 MB | 25 EU | fallback |
| `parakeet-ctc-110m-coreml` | ParakeetCoreml (aux) | downloaded by `CtcModels.downloadAndLoad()` | ~100 MB | n/a | auto-installed when the user has ≥ 1 dictionary term |
| `sherpa-parakeet-v3-int8` | SherpaParakeet | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8.tar.bz2` | 640 MB | 25 EU | **Default on Windows** |
| `sherpa-parakeet-v2-int8` | SherpaParakeet | `…/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8.tar.bz2` | ~630 MB | en | Windows English option |
| `large-v3-turbo-q5` (existing) | Whisper | existing | ~550 MB | 99 | **Auto for languages Parakeet doesn't cover** (ar, hi, ur, fa, tr, zh, ja, ko, …) |

The sherpa-onnx URLs come from the [sherpa-onnx NeMo transducer models page](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html).

**Selection logic** (`fn pick_engine(settings, os, lang) -> ModelId`):
1. If the user pinned a model, use it.
2. If the language is set and Parakeet doesn't support it, use Whisper large-v3-turbo.
3. macOS 14+ arm64: Ultra. If `lang == "en"` and the eval in P3.8 shows v2 better on the human corpus, use v2 for English.
4. Windows: sherpa v3 int8 (v2 for English if the eval says so).
5. Otherwise use the existing ggml Parakeet v3.

### 3.3 Windows engine: `crates/teletype-speech/src/sherpa.rs`
- Dependency: `sherpa-onnx = "1.13"` ([docs.rs](https://docs.rs/sherpa-onnx/latest/sherpa_onnx/)). It links statically and downloads matching prebuilt libs for Windows x64/arm64 and macOS. In CI, set `SHERPA_ONNX_ARCHIVE_DIR` to a cached directory.
- Build `OfflineRecognizer` with `OfflineTransducerModelConfig { encoder, decoder, joiner }`, `tokens`, `model_type = "nemo_transducer"`, `num_threads = min(4, physical_cores)` and `decoding_method = "greedy_search"`.
- **Hotwords spike (P3.6):**
  - sherpa-onnx hotwords require `modified_beam_search` and are documented for transducer models ([sherpa hotwords docs](https://github.com/k2-fsa/sherpa/blob/master/docs/source/onnx/hotwords/index.rst)).
  - Support for NeMo TDT offline isn't explicitly documented. Test it: build a hotwords file from the lexicon (BPE units; export the vocabulary from the model's tokenizer) and run Earnings-22-vocab.
  - If recall improves by ≥ 10 pts with WER not worse, enable it with `hotwords_score` tuned (start at 1.5). Otherwise use P3.7.
- Also use sherpa's `VoiceActivityDetector` only if it beats `silero-vad-pure` in CPU cost; otherwise keep the existing VAD.

### 3.4 Whisper improvements (all OSes)
- Pass `initial_prompt` = the top 30 lexicon terms joined by ", ", plus the previous committed sentence (≤ 200 tokens total).
- Set `no_context = true` between dictations, `single_segment = true` for audio under 30 s, and `no_timestamps = true` unless word timings are needed.
- On Apple Silicon, `n_threads` = the performance-core count (`sysctlbyname("hw.perflevel0.physicalcpu")`).
- Build whisper.cpp with Core ML encoder support (`WHISPER_COREML=1`) and download `ggml-large-v3-turbo-encoder.mlmodelc` next to the ggml file.
- **Windows:** enable `whisper-rs` for `cfg(windows)` (CPU, AVX2), so non-Parakeet languages work. This removes the current stub in `whisper/stub.rs`.

### 3.5 Whistle (Cactus)
- Pass lexicon terms as `keywords` (currently `null`, `whistle.rs` ≈L790–796).
- Split at VAD pauses with 0.3 s overlap instead of hard 30 s cuts (≈L754).
- Keep it as an **optional "Tiny" engine** (16.9 MB) for low-RAM machines. Don't make it the default unless P3.8 shows it wins.
- Ship it only after the Cactus engine and weights licenses are confirmed. The repo is labelled Apache-2.0; the weights' license isn't stated ([cactus-compute/needle](https://github.com/cactus-compute/needle)).

### 3.6 Post-ASR phonetic rescoring (fallback booster for engines without native boosting)
Create `crates/teletype-core/src/rescoring.rs`:
- **Inputs:** `Transcript` with word confidences, plus `Vec<BoostTerm>`.
- **Algorithm:**
  1. Find candidate spans of 1–4 words where any word has `conf < 0.85` **or** the span is phonetically close to a term.
  2. Compute `sim = 1 - lev(dmetaphone(span), dmetaphone(term)) / max_len`, combined with a character-level Jaro-Winkler score.
  3. Replace when `sim ≥ 0.82` and (`conf_span < 0.85` or term weight ≥ 2). Keep the term's original casing.
  4. Never replace when the span is in the protected skip list (numbers, URLs, code).
- **Performance:** precompute phonetic keys for terms (an index from the first phonetic char to terms). Must handle 5,000 terms in < 2 ms per dictation (bench).
- **Crates:** `rphonetic` (Double Metaphone, MIT) or an in-house implementation; `strsim` (MIT).
- **Tests:** a proptest that exact matches are never altered; unit tests for "shivon → Siobhan", "kubernetes → Kubernetes" and "pie torch → PyTorch".

### 3.7 Replace the old dictionary fuzzy fixer
`pipeline.rs::correct_with_dictionary` (≈L540) becomes a thin wrapper over `rescoring.rs` with word confidences. Old behaviour stays under `cfg(test)` for regression comparison. Keep alias exact replacement first, using Aho-Corasick (`aho-corasick` crate) over lowercased aliases.

### 3.8 Engine bake-off (decides the defaults)
Run `teletype-eval` on every engine × set (human, LibriSpeech clean/other, Earnings-22-vocab, AMI, CV, noise 10/5/0 dB, FLEURS subset) with vocabulary boosting off and on. Pick:
- macOS default: best `wer_clean_human` within latency budget (`asr_ms` p95 ≤ 120 ms for a 10 s clip on an M1).
- Windows default: best `wer_clean_human` with `asr_ms` p95 ≤ 600 ms for a 10 s clip on a 4-core x64 laptop.
- Boosting parameters: maximize `vocab_recall` subject to `vocab_false_alarm ≤ 1%` and verbatim WER regression ≤ 0.3 pt.

Write the results to `evals/RESULTS.md` and the decision to `docs/decisions/D-engine-defaults.md`.

### 3.9 LLM catalog clean-up (`crates/teletype-inference/src/catalog.rs`)
- **Delete the EG-1 entries** (≈L183–225, ≈L372–381) and all references in prompts and tests (`prompt.rs` ≈L430 loops over `["eg-1","s1-mini"]`). Add a migration: if the user's selected model is `eg-1`, switch to `s1-mini` and show a one-time notice.
- **S1-mini download order:** make Superwhisper's Hugging Face repo [`superwhisper/s1-mini-GGUF`](https://huggingface.co/superwhisper/s1-mini-GGUF) the **primary** URL and remove the `models.enviouslabs.co` mirror. Keep the naming clause: always display "S1-mini by Superwhisper".
- Keep the existing correct S1 handling: exact system prompt, control line `[Styling: …] [Structure: …] [Context: …]`, `--temp 0`, `enable_thinking:false`.

**Gate 3**
- [ ] macOS: Ultra Core ML runs through the bridge. A 10 s clip is transcribed in ≤ 120 ms p95 on an M1 or newer (trace `asr_ms`).
- [ ] Windows: sherpa Parakeet transcribes. `cargo test -p teletype-speech --features sherpa` passes on `windows-latest` with a fixture WAV.
- [ ] `vocab_recall` ≥ 90% on Earnings-22-vocab with false alarms ≤ 1%, on both OSes (native boosting or rescoring).
- [ ] `wer_verbatim` on the 8-set blend ≤ baseline − 1.0 pt.
- [ ] EG-1 is gone from code, catalog, tests and UI (`rg -i "eg-1|enviouslabs" crates ui` returns nothing except the migration and its test).

---

## PHASE 4: Commit-as-you-go streaming (latency independent of length)

**Idea:** decode finished speech **while the user is still talking**. On release, only the last few seconds remain.

### 4.1 `Segmenter` (`crates/teletype-core/src/segmenter.rs`)
- Input: 16 kHz frames from the capture thread (32 ms hops) plus Silero probabilities.
- Emit `SegmentReady { start_sample, end_sample }` when:
  - speech is followed by ≥ 350 ms of silence (configurable `commit_pause_ms`, default 350), **or**
  - the segment length reaches 12 s. In that case, cut at the lowest-energy 20 ms frame within the last 2 s, with 0.4 s overlap into the next segment. (Ultra's Core ML encoder has a fixed 15 s window, so stay below it.)
- Unit-test with synthetic speech/silence patterns.

### 4.2 `Committer` (`crates/teletype-desktop/src/committer.rs`)
- A worker thread owns a **second** engine handle or a queue to the single engine. Priority: the final tail always comes first; segments are processed FIFO.
- For each segment, call `transcribe_detailed`. For overlapped segments, de-duplicate words by timestamp (drop words in the overlap region from the later segment when their start time is less than the previous segment's last word end).
- Keep `committed: Vec<Transcript>` plus a volatile preview.
- **Live preview:** emit `pill-preview { committed_text, volatile_text }`. The volatile part comes from a cheap interim decode of the uncommitted tail, run at most every 700 ms and only when idle. Committed text never changes.
- **On release:**
  - cancel the interim decode
  - decode only `tail = audio[last_commit_end - 0.4s ..]`
  - merge the results
  - send them to the text pipeline

  Trace fields: `committed_segments`, `tail_audio_ms`.

### 4.3 Polish for long dictations
- If the total audio is ≥ 20 s, run polish per committed segment group (≈ 2–3 sentences) **during** recording. On release, polish only the tail, then run a fast "join" pass (rules only: capitalization at the boundaries, list continuation).
- Keep a setting "Polish whole text at the end (slower, more consistent)" for users who prefer it.

### 4.4 Tests
- Integration test with a mock engine that takes 10 ms per second of audio: a 60 s take produces `tail_audio_ms ≤ 3,000` and release-to-text ≤ 80 ms (mock).
- WER parity: committed-segments WER ≤ whole-utterance WER + 0.2 pt on the eval sets (proves the cuts don't hurt).
- Overlap de-dupe: property test that no word is duplicated or dropped at segment joins.

**Gate 4**
- [ ] Live p95 release-to-paste for 60 s dictations is within 100 ms of the p95 for 5 s dictations.
- [ ] WER parity holds.

---

## PHASE 5: Audio front end and noise handling ("best noise filter", done right)

### 5.0 Policy (evidence-based)
**Don't feed denoised audio to the speech model by default.** A systematic study found that speech enhancement made ASR worse in **all 40** model × noise configurations tested (Whisper, Parakeet, Gemini Flash and others). Absolute semantic-WER rose by 1.1–46.6 points, because modern models are already noise-robust ([arXiv 2512.17562](https://arxiv.org/abs/2512.17562)). Parakeet Ultra is specifically better in noise (5.82 vs 6.72 WER on noisy sets) ([moondream/parakeet-ultra](https://huggingface.co/moondream/parakeet-ultra)).

So the "noise filter" strategy is:
1. A **robust model** (Ultra).
2. **Clean capture:** pre-roll, the right mic, ducking other audio.
3. **Denoise only for decisions:** VAD, endpointing, level meter.
4. **Optional full denoise**, enabled only when the eval proves it helps in a condition.

### 5.1 Pre-roll ring buffer (don't lose the first word)
- **Problem:** the mic stream is built and started on hotkey press (`audio/mod.rs` ≈L339), so the first 100–400 ms (more on Bluetooth) can be lost.
- **Implement `MicPrewarm`:** keep the cpal input stream open while "armed" and write into a 1.0 s lock-free ring buffer (`ringbuf` crate). On hotkey-down, the take starts with the last **300 ms** of pre-roll.
- **Arming modes** (Settings → Audio → "Instant start"):
  - `off`: current behaviour.
  - `while_app_active`: the stream is open while Teletype's window or pill is focused.
  - `always` (default **off**): the stream is always open. macOS shows the orange mic indicator, so explain this in the UI. Audio is held in memory only, never written, and the buffer is zeroed on disarm.
- **Default:** `off`, plus the low-cost improvement: build the cpal stream once at startup and call `pause()`/`play()` instead of rebuilding per take. Measure `t_first_audio - t_hotkey_down` in the trace; target ≤ 60 ms on the built-in mic.
- **Tests:**
  - Unit test the ring buffer (wrap-around, concurrent write/read).
  - Integration test with a fake device: a hotkey press at t = 1.000 s delivers samples from t = 0.700 s.

### 5.2 Smart mic selection
- Detect Bluetooth HFP input (macOS: `kAudioDevicePropertyTransportType == kAudioDeviceTransportTypeBluetooth`; Windows: MMDevice property `PKEY_Device_EnumeratorName == "BTHENUM"`).
- Setting "When using Bluetooth headphones, record from the built-in mic" (default **on** when a built-in mic exists). This avoids narrowband HFP audio and keeps music in high quality.
- Show the active input device and its sample rate in the pill tooltip and in Developer → Audio.

### 5.3 High-quality resampling
- Replace the linear `resample_to_target` (`audio/mod.rs` ≈L494) with `rubato` (`SincFixedIn`, `sinc_len = 128`, `f_cutoff = 0.95`, `oversampling_factor = 128`, `BlackmanHarris2`).
- For the 48 kHz → 16 kHz integer case, use `FftFixedIn` for lower CPU.
- **Test:** sine sweep 0–24 kHz → 16 kHz output has ≥ 60 dB attenuation above 8 kHz (aliasing check).

### 5.4 Light conditioning (keep only if the eval says so)
In `crates/teletype-core/src/audio/condition.rs`:
- DC removal (one-pole high-pass at 20 Hz).
- 80 Hz 2nd-order Butterworth high-pass (rumble, desk bumps).
- **Slow AGC** to a −20 dBFS RMS speech target, max +18 dB gain, attack 50 ms / release 500 ms, applied only during VAD speech. Then a peak limiter at −1 dBFS.
- **Clipping detector:** if more than 0.1% of samples are ≥ 0.99, record `clipped=true` in the trace and show the tip "Your mic is too loud".
- **Eval gate:** run with and without conditioning on all sets. Keep each stage only if `wer_verbatim` doesn't get worse on any set by more than 0.1 pt and improves on at least one (expected: quiet speakers and the CV accents set).

### 5.5 DeepFilterNet3: for VAD and endpointing, not for ASR input
- Use [`deepfilter-rt`](https://github.com/shimondoodkin/deepfilter-rt) (Rust + ONNX Runtime). Use the `dfn3_ll` model: 0-frame lookahead, ~10 ms latency, 48 kHz frames of 480 samples, real time on CPU.
  - Verify the crate license and the DeepFilterNet model license (the upstream DeepFilterNet project is MIT/Apache-2.0 dual) before shipping, and record both in `THIRD_PARTY_MODELS.md`.
- Pipeline: capture at 48 kHz splits into **(a)** the raw path (resample → ASR) and **(b)** DFN3-LL → resample → Silero VAD + level meter + endpointing.
- **Benefit:** fewer false "speech" detections from fans, keyboards and cafés, so hands-free and auto-stop end on time and segments cut at real pauses.
- **Settings → Audio → Noise handling:**
  - `Smart` (default): denoise for detection only.
  - `Off`.
  - `Aggressive (experimental)`: also feeds denoised audio to the speech model. Enable automatically for a take **only** if the measured pre-speech SNR is < 0 dB **and** P5.8 eval shows Aggressive beats raw at 0 dB.
- CPU budget: ≤ 5% of one core on an M1 or a 4-core x64 laptop (measure in the soak test).

### 5.6 Echo and other audio playing
- **Duck other audio while recording** (Settings → "Lower other audio while dictating", default on, level 20%):
  - macOS: lower the default output device volume via CoreAudio (`kAudioDevicePropertyVolumeScalar`), restore on stop, and restore on crash through the recovery spool.
  - Windows: `IAudioSessionManager2` → set each other session's `ISimpleAudioVolume` to 20%, restore on stop.
- **Optional "Pause media"** (macOS: send the system play/pause media key through `NSEvent` `NX_KEYTYPE_PLAY`; Windows: `VK_MEDIA_PLAY_PAUSE`). There's no public API to check whether something is playing, so this is opt-in only and sends a second toggle on stop. Default off.
- **Experimental echo cancellation** for users dictating during calls with speakers: FluidAudio's **LocalVQE** (Apache-2.0; AEC + noise suppression + dereverb at 16 kHz; beta) ([FluidAudio](https://github.com/FluidInference/FluidAudio)), or Apple voice processing (`AVAudioInputNode.setVoiceProcessingEnabled(true)`) through the Swift bridge. Behind a setting, default off; eval it on a recorded "speaker playing music" set.

### 5.7 Per-take audio quality signals
For each take, compute `speech_rms_db`, `noise_rms_db` (non-speech frames), `snr_db`, `clipped` and the `bt_hfp` flag, and store them in the trace. The Insights screen shows "Mic quality this week" plus tips.

### 5.8 Noise eval set
`scripts/eval/mix_noise.py` mixes MUSAN noise (music/babble/noise) and recorded café/keyboard/fan noise into the human corpus and LibriSpeech test-other at 10, 5, 0 and −5 dB. Report WER for each `Noise handling` mode.

**Gate 5**
- [ ] `t_first_audio - t_hotkey_down` p95 ≤ 60 ms on the built-in mic (≤ 30 ms with Instant start).
- [ ] The first word is never lost on the human corpus "first word" check (clips that start speaking immediately): 0 dropped first words over 50 trials.
- [ ] Noise sets: WER (Smart mode) ≤ WER (Off) on every SNR. Auto-stop false-trigger rate in noise is reduced by ≥ 50% vs. baseline.

---

## PHASE 6: AI polish: faster, safer, context-aware

### 6.1 Model choices (all local through the existing llama-server sidecar)

| Use | Model | File | License | Notes |
|---|---|---|---|---|
| **Default, English** | **S1-mini by Superwhisper** | `superwhisper/s1-mini-GGUF` → `s1-mini-q4_k_m.gguf` (462 MB) | Apache-2.0 + naming clause | 0.6B text normalizer trained for ASR cleanup; 94.8% token accuracy on 7,519 cases; **must** use its fixed system prompt, control line, `temp 0` and thinking off ([model card](https://huggingface.co/superwhisper/s1-mini-GGUF)) |
| **Default, non-English** | Qwen3-1.7B (instruct) Q4_K_M | existing catalog entry "Fast (Qwen3 1.7B)" | Apache-2.0 | Generic prompt (`build_prompt`); thinking off |
| **Quality** (user opt-in) | Qwen3-4B-Instruct-2507 Q4_K_M | existing "Quality (Qwen3 4B)", updated to the 2507 instruct build if the current one isn't | Apache-2.0 | Use for long, formal and email dictations |
| **Commands fallback** (P2.7) | Same as Quality or Fast | n/a | Apache-2.0 | JSON-schema-constrained |
| Cloud (BYO key, optional) | OpenAI / Anthropic / Gemini through the existing OpenAI-compatible client | n/a | n/a | Off by default; never required |

### 6.2 Skip the LLM when it isn't needed (the biggest polish latency win)
Extend the polish gate (`pipeline.rs`, `polish_gate_threshold_words = 8`) into `PolishGate::decide(&Transcript, &Context) -> Decision`. **Skip** polish when all of these are true:
- no filler tokens (`um, uh, er, like (as filler), you know, I mean`; language-specific lists)
- no self-correction markers (`no wait`, `actually`, `scratch that`, `I mean`, `sorry`, a repeated word pair)
- no list cue (`first/second/third`, `number one`, `bullet`, `new line`, `next item`)
- no email/letter context detected
- average word confidence ≥ 0.9
- the user hasn't disabled skipping

When skipping, still apply the rules-based formatting: ITN, sentence capitalization, end punctuation from the ASR output, and smart spacing. Record `polish_skipped_reason` in the trace.

**Target:** ≥ 50% of short dictations skip the LLM, with no `wer_clean` regression on the human corpus.

### 6.3 Protected-terms guard (stops the polish model "fixing" names)
- **Before polish:** collect the protected set = lexicon terms present in the text + words boosted by the speech engine + numbers/URLs/emails/code identifiers.
- **After polish:** align the polished output to the input (word-level diff). For every protected token that was changed or removed, **restore** it in place. If more than 3 protected tokens changed, discard the polish and use the rules-only output.
- **Hallucination guard:** if `len(polished_words) > 1.3 × len(input_words) + 5`, or the output contains a phrase that's absent from the input and longer than 6 words (except list markers and greetings in email mode), fall back.
- **Tests:** golden cases (`insta`) for names, product names, code identifiers, numbers and mixed-language text.

### 6.4 Context (what Wispr Flow does, done locally)
- **At hotkey-down, capture `ScreenContext`:**
  - app bundle id / exe name and window title
  - the focused field's text up to 400 chars before the cursor and 100 after (macOS AX through the existing `ax_text.rs`; Windows UIA TextPattern in P8.5)
  - the selection, if any
- **Derive:**
  - `app_category` ∈ {email, chat, code, docs, terminal, browser, notes, other} from a mapping table (`crates/teletype-core/src/app_category.rs`)
  - `ephemeral_terms` = capitalized tokens, `@handles`, CamelCase/snake_case identifiers and email names found in the context
- **Feed `ephemeral_terms` to:**
  - the speech engine boost list for **this take only** (P3), weight 0.5
  - the protected-terms guard
  - the polish prompt (non-S1 models) as "Names and terms visible on screen: …"
- **S1-mini:** set `Context: email` when `app_category == email`, otherwise `general`. Map `Styling` from per-app style memory (P7.7).
- **Privacy:** context never leaves the device. Never log full context text, only lengths and counts, in traces.

### 6.5 Output length and speed
- `max_tokens = ceil(1.4 × input_tokens) + 24` (not a flat 300).
- Stop sequences for non-S1 models: `["\n\n\n", "</output>"]`.
- llama-server flags: keep `-fa on -ctk q8_0 -ctv q8_0 --jinja`. Add `--cache-reuse 256` and `-np 1`. Keep `-c 4096`. Warm the static prefix (P1.7).
- On Apple Silicon, make sure all layers are offloaded to Metal (`-ngl 999`). On Windows CPU, `-t` = physical cores.

### 6.6 Optional later: Teletype's own polish model (P6.6, after Gate 7)
- Fine-tune Qwen3-1.7B (Apache-2.0) with LoRA on:
  - (a) public data: AMI/Switchboard-style disfluent → clean pairs created with an **open-license** teacher (e.g. Qwen3-32B-Instruct)
  - (b) **only with explicit opt-in**, the user's own accepted (raw → final) pairs
- **Never** train on EG-1 outputs (its license §2c bans it) or on Wispr Flow outputs.
- Export to GGUF Q4_K_M and add it to the catalog.

**Gate 6**
- [ ] Polish-on p50 release-to-paste ≤ 450 ms, p95 ≤ 700 ms on the human corpus (M1 or newer).
- [ ] `wer_clean_human` with polish ≤ without polish − 1.0 pt.
- [ ] Protected-terms golden tests: 100% pass.

---

## PHASE 7: The self-improving loop (more use, more accurate)

### 7.0 What exists today and its limits
- **Today:**
  - `edit_watch.rs` re-reads the field only on the next dictation or a timer (macOS AX only).
  - `personalization/learn.rs` extracts greeting/sign-off and **one** terminology swap per edit, which needs **2** observations (`candidates.rs`).
  - Learned items feed the text fixer and the polish prompt, **not the speech model**. Nothing verifies that a learned item actually helped.
- **Target:** a closed loop.
  1. Observe the user's edits.
  2. Judge whether each edit fixes a mishearing, is a style change or is a content rewrite.
  3. Learn into the right store.
  4. Apply it to the speech model, the text pipeline and polish.
  5. Verify by replay.
  6. Show progress to the user.

### 7.1 Learning store (SQLite)
`crates/teletype-core/src/learning/store.rs` using `rusqlite` (bundled, WAL). File: `<app_data>/learning/learning.sqlite`.

```sql
CREATE TABLE dictation (
  id INTEGER PRIMARY KEY, ts INTEGER, app_id TEXT, app_category TEXT, engine TEXT, lang TEXT,
  audio_path TEXT NULL,                -- only if "Keep audio for self-tests" is on
  raw_asr TEXT, words_json TEXT,       -- Transcript.words with conf
  pasted TEXT, observed TEXT NULL,     -- what the field contained after the user finished editing
  edit_ops_json TEXT NULL, polish_model TEXT NULL, polish_skipped INTEGER
);
CREATE TABLE lexicon (
  term TEXT PRIMARY KEY, aliases_json TEXT, source TEXT CHECK(source IN ('manual','learned','contacts','import','pack')),
  weight REAL DEFAULT 1.0, hits INTEGER DEFAULT 0, false_alarms INTEGER DEFAULT 0,
  created INTEGER, last_used INTEGER, tombstoned INTEGER DEFAULT 0, app_scope TEXT NULL
);
CREATE TABLE style_rule (app_category TEXT, rule TEXT, value TEXT, evidence INTEGER, updated INTEGER,
  PRIMARY KEY(app_category, rule));
CREATE TABLE example (id INTEGER PRIMARY KEY, app_category TEXT, raw TEXT, final TEXT, accepted_unedited INTEGER, ts INTEGER);
CREATE TABLE metric_daily (day TEXT PRIMARY KEY, words INTEGER, edited_words INTEGER, dictations INTEGER, learned_terms INTEGER);
```

- **Migration:** import the existing dictionary JSON as `source='manual'` and existing learned candidates as `source='learned'`. Keep the JSON dictionary as the UI's source of truth for one release, synced both ways, then switch the UI to SQLite.
- **Retention:** dictation rows 90 days; audio 7 days / 200 clips max (opt-in); deletable via Settings → Privacy → "Delete learning data".

### 7.2 EditObserver (capture what the user really wanted)
Rewrite `edit_watch.rs` as `learning/observer.rs` + platform readers.
- **At paste:** store `anchor_before` (≤ 40 chars before the insertion point) and `anchor_after`, the field's AX element id (macOS) or UIA RuntimeId (Windows), and `pasted`.
- **Observe the field again when** any of these happen first:
  - (a) focus leaves the field
  - (b) Enter / Cmd+Enter / Ctrl+Enter is pressed (send)
  - (c) 20 s after the last keystroke in the field
  - (d) the next dictation starts
  - (e) 3 min hard cap
- **Locate the dictated span** in the new field text by anchors (fuzzy, Levenshtein ≤ 3 on the anchors), then extract `observed`. If it can't be located with confidence, drop the observation (never learn from guesses).
- **Readers:**
  - macOS: AX (existing `ax_text.rs`).
  - Windows: UIA `ValuePattern`/`TextPattern` (P8.5).
  - Apps with no access (some Electron/Java apps): skip.
- **Password/secure fields:** never read (`AXSecureTextField`, UIA `IsPassword`).

### 7.3 Diff and classify edits
`learning/diff.rs`: word-level Myers diff (`similar` crate, Apache-2.0/MIT) between `pasted` and `observed` gives ops `Equal / Replace(a→b) / Insert / Delete`.

`learning/judge.rs` classifies each `Replace` op (1–4 words):
- **Skip list** (never learn), modelled on Wispr Flow's published auto-learn exclusions ([Wispr Flow docs](https://docs.wisprflow.ai/articles/4052411709-teach-flow-your-words-with-the-dictionary)):
  - file paths, URLs/domains, partial emails, code symbols, keys/hashes/IDs
  - numbers/dates, Markdown/emoji
  - everyday words (a top-20k frequency list from a permissively licensed source, e.g. SCOWL), very common first names, everyday acronyms (PDF, CEO, ASAP)
- **Mishearing (learn into the lexicon)** when all of these hold:
  - `phon_sim(a, b) ≥ 0.6` (Double Metaphone + Jaro-Winkler)
  - `b` isn't in the skip list
  - **and** either `min_conf(a) < 0.9` or `b` looks like a proper noun or term (capitalized mid-sentence, CamelCase, contains digits or letters like "PyTorch")
- **Style (learn into `style_rule`)** includes:
  - casing changes (`email` → `Email`)
  - punctuation changes (Oxford comma, em dash)
  - contraction changes (`do not` → `don't`)
  - greeting/sign-off (existing logic)
  - list formatting
- **Rewrite (don't learn the lexicon; store as an `example`)**: low phonetic similarity, or more than 4 words changed.
- **Ambiguous cases** (score between thresholds) **with** the local Fast LLM installed: ask the judge prompt with JSON schema `{"kind":"mishearing|style|rewrite","term":string|null}`. Cap at 1 call per dictation, run in the background, never on the hot path.

### 7.4 Promote, apply and notify
- **Mishearing:**
  - Promote **immediately** if it's proper-noun-like and `phon_sim ≥ 0.75`; otherwise after 2 observations (keep the existing `candidates.rs` counting).
  - Add `a` (what the ASR wrote) as an **alias** of `b`. Aliases feed the engines' boosting (FluidAudio supports aliases) and the exact alias replacer.
- **Notify** with a non-blocking toast: "Learned: **Siobhan**" with Undo. Undo tombstones the term, so it's never re-learned automatically (same behaviour as Wispr Flow's deletions).
- **Style:** update `style_rule` with evidence counting and apply it when evidence ≥ 3 and ≥ 70% consistency. For S1-mini, map it to the control line (e.g. formal vs casual for `app_category`).
- **Examples:** keep ≤ 200 per `app_category`. For non-S1 models, inject the 2 most similar examples (BM25 over raw text via `tantivy` or a small in-house BM25) as few-shot pairs into the dynamic prompt section.

### 7.5 Build the boost list for each take
Function `lexicon::boost_list(ctx) -> Vec<BoostTerm>`:
- **Candidates:** manual + learned + contacts + pack + `ephemeral_terms` from screen context, excluding tombstoned terms.
- **Score:**
  ```
  score = 2.0·[manual] + 1.5·[learned] + 1.0·[screen] + 0.7·[contacts] + 0.5·[pack]
        + 0.8·log1p(hits) − 2.0·false_alarm_rate + 0.5·[app_scope matches] + recency_decay(last_used, 30d)
  ```
- **Take the top N.** N is tuned in P3.8. Start with N = 100 for FluidAudio CTC rescoring, N = 50 for sherpa hotwords, 30 for the Whisper prompt and 50 for Whistle keywords.
- **Cache** keyed by `(lexicon_version, app_category, screen_terms_hash)`. Rebuilding must take < 1 ms.

### 7.6 Contacts and imports
- **macOS Contacts** (opt-in button "Import names from Contacts", `CNContactStore`): import given/family/nickname/organization as `source='contacts'`, weight 0.7, via the Swift bridge.
- **Import files:** CSV/TSV (`term,aliases`), plain text (one term per line) and the Wispr Flow dictionary export (CSV) when available. Show a preview before import.

### 7.7 Replay verification ("prove it got better")
- When the machine is idle ≥ 2 min **and** on AC power **and** "Keep audio for self-tests" is on, run `learning/replay.rs`.
- **Inputs:** the last ≤ 200 stored clips that have `observed` text.
- **For each clip,** run ASR with the **current** boost list and with the **previous** version's list (stored by `lexicon_version`), then compute WER against `observed` (verbatim-normalized).
- **Per term:**
  - hit = the term now appears correctly where `observed` has it
  - false alarm = the term appears where `observed` doesn't
  - Update `hits` and `false_alarms`. If `false_alarm_rate > 0.3` with ≥ 3 false alarms, reduce the weight by half. If it stays bad, auto-tombstone and notify ("Stopped boosting 'Kate' because it caused mistakes").
- **Write a "personal accuracy" row:** `personal_wer_before`, `personal_wer_after`.
- **Budget:** at most 60 s CPU per run, cancel immediately on user activity.

### 7.8 Metrics and UI ("you can see it improving")
- `metric_daily`: words dictated, words edited afterwards, learned terms.
- The Insights screen shows:
  - **edit rate per 100 words** (7-day rolling)
  - **personal accuracy trend** (from replay)
  - terms learned this week (with Undo)
  - "Top fixes" (the most frequent mishearings fixed)
- Settings → Learning:
  - Learn from my edits (on)
  - Learn names from screen (on)
  - Keep audio for self-tests, 7 days (off)
  - Import from Contacts…
  - Delete all learning data

### 7.9 Optional advanced (after Gate 7)
- **Personal n-gram LM:** build a 3-gram ARPA model from the user's `observed` texts (a small Rust writer for counts with modified Kneser-Ney smoothing, producing a KenLM-compatible ARPA file). Use it for N-best rescoring where an engine offers N-best (sherpa `modified_beam_search` returns hypotheses; FluidAudio has a CTC decoder with ARPA LM support, per its docs). Gate on the eval.
- **Personal acoustic adaptation** (future research): LoRA fine-tune of Parakeet on the user's retained clips. Out of scope for this plan; note it in `docs/research/`.

### 7.10 Tests for the learning loop
- **Unit:** the skip list (100 positive / 100 negative examples), the phonetic similarity table, diff → ops, judge classification on 60 labelled edit pairs in `crates/teletype-core/tests/fixtures/edits.jsonl` (target ≥ 90% accuracy).
- **Property:** learning never adds a skip-list item; Undo/tombstone is permanent; the boost list is deterministic for the same inputs.
- **Simulation test** (`tests/learning_sim.rs`), the most important:
  - Simulated user: 30 days, 40 dictations/day, built from the eval human corpus + 50 synthetic names.
  - The mock ASR mishears names with 40% probability unless the name is in the boost list (then 5%).
  - The user corrects every mishearing.
  - **Assert:** edit rate decreases monotonically week over week; day-30 edit rate ≤ 25% of day-1; false alarms ≤ 1%.
- **Integration (real engine, macOS CI with a self-hosted Apple Silicon runner, or a manual job):**
  - Take 20 human clips with rare names.
  - Run 1 has no lexicon. Feed the corrections, then run 2.
  - **Assert:** recall of those names goes from baseline to ≥ 90%.
- **Privacy:**
  - A secure field is never read (AX mock returns `AXSecureTextField`).
  - Delete-all removes the DB and audio files.

**Gate 7**
- [ ] The simulation test passes.
- [ ] The real-engine name-recall test reaches ≥ 90%.
- [ ] Replay demotes a deliberately bad term within 2 runs.
- [ ] Insights shows the edit-rate trend.

---

## PHASE 8: Delivery reliability, Windows parity, everyday features

| ID | Task | Detail |
|---|---|---|
| P8.1 | **Paste cascade** | `injector.rs` → `Delivery::deliver(text, target)`: (1) clipboard + Cmd/Ctrl+V; (2) **verify**: macOS reads AX `kAXValueAttribute` / selected range and checks the text arrived, within 150 ms; Windows reads UIA ValuePattern; (3) if not, type via `enigo` (≤ 2,000 chars, 2 ms/char cap) or AX `kAXSelectedTextAttribute` set (macOS) / UIA `ValuePattern.SetValue` at the selection (Windows); (4) if everything fails, keep the text in "Paste last" and show a toast. Per-app policy table `delivery_policy.toml` (e.g. terminals → type, Office → paste + verify, secure fields → refuse). |
| P8.2 | **Return to origin** | Record the frontmost app pid + window id at hotkey-down. Before paste, if focus changed, re-activate the origin (macOS `NSRunningApplication.activate(options:)`, Windows `AllowSetForegroundWindow` + `SetForegroundWindow`). If that fails, hold the text for "Paste last". |
| P8.3 | **Full clipboard restore** | macOS: snapshot all `NSPasteboardItem` types/data and restore after paste (skip > 50 MB). Windows: enumerate `EnumClipboardFormats` and restore all formats. Test with an image, a file URL and RTF. |
| P8.4 | **Paste Last Dictation** hotkey | Default `Ctrl+Cmd+V` on macOS, `Ctrl+Alt+V` on Windows. Pastes the last final text. |
| P8.5 | **Windows parity** | (a) Hotkey recorder: capture in the webview (`keydown`/`keyup`, `event.code`) for Windows instead of the macOS-only native panel (`commands.rs` returns "Hotkey capture is only available on macOS"). (b) Text selection and field reading via UIA (`windows` crate: `IUIAutomation`, `TextPattern`, `ValuePattern`). (c) Smart spacing using UIA text before the cursor. (d) Start/stop sounds via `PlaySoundW`. (e) Edit observation (P7.2) via UIA. |
| P8.6 | **Escape recovery UI** | Recovery spool exists; add a tray item "Recover last recording" and a pill action when a take is cancelled (keep 24 h). |
| P8.7 | **Settings search** | A fuzzy search box over all settings labels (UI only). |

**Tests:** an e2e "delivery matrix" (P9.6) with each target app and each fallback forced.

**Gate 8**
- [ ] The delivery matrix is 100% on the listed apps, macOS and Windows.
- [ ] Clipboard restore tests pass.

---

## PHASE 9: Testing (all kinds) and CI

### 9.1 Test pyramid and where each test lives

| Kind | Tool | Location | Runs |
|---|---|---|---|
| Unit | `cargo test` | inline `#[cfg(test)]` | every PR, all OSes |
| Property | `proptest` | `crates/*/tests/prop_*.rs` | every PR |
| Snapshot | `insta` | ITN, polish golden, prompt builder | every PR |
| Fuzz | `cargo fuzz` | `fuzz/fuzz_targets/{sse_parser,itn_normalize,storage_load,diff_ops,hotwords_file}.rs` | nightly, 10 min/target |
| Integration (mock engines) | `cargo test` | `crates/teletype-desktop/tests/pipeline_*.rs` | every PR |
| **Engine conformance** | `cargo test --features <engine>` | `crates/teletype-speech/tests/conformance.rs`: one suite every `SpeechProvider` must pass (loads, transcribes `fixtures/hello.wav` → contains "hello"; empty audio → `NoSpeech`; 60 s → no panic; `set_boost_terms` round trip; unload frees memory) | macOS + Windows CI |
| FFI safety | ASan build of the Swift bridge + Rust (`RUSTFLAGS=-Zsanitizer=address` nightly) | `teletype-fluid` tests | nightly macOS |
| Concurrency | `loom` (engine `ArcSwap` swap, Bookkeeper channel) + a stress test of 1,000 back-to-back mock dictations | `tests/concurrency_*.rs` | nightly |
| UI unit | Vitest + Testing Library | `ui/src/**/*.test.tsx`: pill state machine (idle → listening → transcribing → polishing → done/error/command), Settings round trip, Keybinds (incl. Command hotkey), Learning toasts + Undo, Insights charts render | every PR |
| UI accessibility | `vitest-axe` | each screen | every PR |
| UI visual regression | Playwright against `vite preview` with the mocked Tauri API; screenshots per screen in light/dark | `ui/e2e/` | every PR (diff threshold 0.1%) |
| **End-to-end, real apps** | macOS: AppleScript + AX checks; Windows: `pywinauto` | `scripts/e2e/` (see 9.6) | nightly on self-hosted runners; required before release |
| Accuracy regression | `teletype-eval` vs `evals/thresholds.json` | CI `eval` job | every PR (small set: 60 clips), nightly (full) |
| Latency regression | `teletype-eval --live` + criterion | CI `perf` job | nightly; PR comment |
| Soak / memory | `scripts/soak.sh`: 8 h, one dictation every 20 s through the live harness | self-hosted | weekly + pre-release. Pass: RSS growth < 50 MB, no FD/handle leak, p95 latency drift < 10% |
| Chaos | `tests/chaos_*.rs` + scripts | n/a | nightly. Kill llama-server mid-polish → raw text pasted; corrupt model file → error toast + fallback engine; disk full (tmpfs) → paste still works; mic unplugged mid-take → take ends gracefully and audio is kept in recovery |
| Privacy / offline | Run the full e2e suite with the network blocked (macOS `pfctl` rule, Windows firewall rule) | `scripts/e2e/offline.sh` | nightly. Also grep logs and traces for any transcript text (must be none) |
| Migration | Load settings/dictionary/history fixtures from v0.1.0, v0.2.0 and v0.2.6 | `crates/teletype-desktop/tests/migration.rs` | every PR |
| Installer smoke | Install the DMG/NSIS silently, launch with `--self-test` (new flag: loads models, runs `fixtures/hello.wav` through the full pipeline in a hidden window, prints a JSON trace, exits 0) | `release.yml` pre-publish step | every release |
| Localization | UTF-8 round trip for ar, hi, zh, ru, de through ASR mock → polish mock → paste | `tests/i18n.rs` | every PR |

### 9.2 Required new fixtures
`crates/teletype-speech/tests/fixtures/`:
- `hello.wav` (1 s)
- `names.wav` (rare names)
- `numbers.wav`
- `long_60s.wav`
- `noise_0db.wav`
- `silence.wav`
- `first_word_immediate.wav`

Generate them from the human corpus with consent, or with TTS for CI-only files. Store with LFS.

### 9.3 CI workflow changes (`.github/workflows/ci.yml`)
- Matrix: `ubuntu-latest` (core, inference, eval, coverage), `macos-14` (arm64: speech + teletype-fluid build + conformance with downloaded models cached via `actions/cache`), `windows-latest` (core, inference, speech `--features sherpa`, desktop build, conformance).
- Jobs: `fmt`, `clippy`, `test`, `ui` (typecheck + vitest + playwright), `coverage` (llvm-cov ratchet), `eval-small` (gate), `bench` (non-blocking), `license-check` (`cargo deny check licenses bans` with GPL denied; `npx license-checker --onlyAllow "MIT;Apache-2.0;BSD-2-Clause;BSD-3-Clause;ISC;0BSD;CC0-1.0"`), and `models-check` (verifies every catalog URL resolves and its SHA-256 matches; weekly).
- Nightly workflow: full eval, fuzz, loom/stress, chaos, e2e (self-hosted), offline suite.

### 9.4 `cargo deny` config (`deny.toml`)
- `licenses.allow = ["MIT","Apache-2.0","BSD-2-Clause","BSD-3-Clause","ISC","Zlib","Unicode-3.0","CC0-1.0","MPL-2.0"]`
- `bans.deny = [{ name = "*gpl*" }]`
- `sources.allow-git` = only the pinned forks you control.

### 9.5 Coverage targets
- teletype-core ≥ 80% lines
- teletype-speech ≥ 70% (excluding FFI glue)
- teletype-desktop ≥ 55%
- UI ≥ 60% statements

### 9.6 End-to-end delivery matrix (`scripts/e2e/`)
- **macOS targets:** TextEdit, Notes, Safari `<textarea>` (local test page), Chrome `<textarea>` and `contenteditable`, VS Code, Terminal, Slack (if installed on the runner), Microsoft Word (if licensed).
- **Windows targets:** Notepad, WordPad (or Word), Chrome `<textarea>`, VS Code, Windows Terminal.
- **Each case:**
  1. Focus the target.
  2. Trigger a dictation through the `--e2e` IPC hook. It plays `fixtures/hello.wav` into a virtual input device: BlackHole on macOS, VB-CABLE on Windows. Or use the harness's file-input mode.
  3. Wait.
  4. Read the field via AX/UIA.
  5. Assert the expected text and that the clipboard was restored.
  6. Repeat with each delivery fallback forced.
- **Command Mode cases:**
  - "open TextEdit" → TextEdit becomes frontmost
  - select text + "make this shorter" → the text changed and is shorter

---

## PHASE 10: Head-to-head benchmark vs Wispr Flow (publishable)

### 10.1 Protocol
- **Hardware:** one Apple Silicon Mac (state the model) and one Windows laptop (state the CPU). Normal home network for Wispr Flow (it needs the cloud).
- **Apps:** latest Wispr Flow (state the version and plan) and Teletype release candidate. Same hotkey mode (hold), same target app (TextEdit / Notepad), same microphone route (a virtual mic playing the WAV in real time), AI polish **on** in both (Wispr's default cleanup vs Teletype's default polish).
- **Corpus:** the 120-clip human corpus + 100 Earnings-22-vocab clips. Add the vocabulary terms to **both** apps' dictionaries before the run.
- **Automation** (`scripts/bench/h2h.py`), for each clip and each app:
  1. Focus TextEdit and clear it.
  2. Press and hold the app's hotkey through a CGEvent/SendInput key-down.
  3. Play the WAV.
  4. Release the hotkey right after the audio ends and stamp `t_release`.
  5. An AX/UIA observer stamps `t_text` when the field value first changes.
  6. Read the final text after 3 s.

  Do 3 full passes on different days and times.
- **Metrics:**
  - `release_to_text` p50/p95/p99
  - `wer_clean` vs `clean.txt`
  - `vocab_recall`, `vocab_false_alarm`
  - `entity_accuracy`
  - failure rate (no text within 5 s)
- **Publish** `evals/WISPR_COMPARISON.md`: methodology, versions, a raw CSV, and charts (latency CDF, WER bars). Include all failures. Don't cherry-pick.

### 10.2 Pass criteria ("we beat Wispr Flow")
Teletype must be better on **all four**:
- median `release_to_text`
- p95 `release_to_text`
- `wer_clean`
- `vocab_recall`

Equal or better is required on `entity_accuracy` and failure rate. If any one fails, open issues listing the gap and loop back to the relevant phase.

---

## 11. Settings added or changed (summary for the Settings struct and UI)

```jsonc
{
  "speech_model": "auto",                    // "auto" | catalog id; auto uses pick_engine()
  "english_model_preference": "auto",        // "auto" | "ultra" | "v2"
  "commit_pause_ms": 350,
  "instant_start": "off",                    // "off" | "while_app_active" | "always"
  "prefer_builtin_mic_with_bt": true,
  "noise_handling": "smart",                 // "off" | "smart" | "aggressive"
  "audio_conditioning": true,                // kept only if eval passes (P5.4)
  "duck_other_audio": true, "duck_level": 0.2, "pause_media": false,
  "echo_cancellation": false,                // experimental
  "polish_skip_when_clean": true,
  "polish_whole_text_at_end": false,
  "command_hotkey": "Ctrl+Option+Space",     // Windows: "Ctrl+Alt+Space"
  "command_wake_phrase": true,
  "paste_last_hotkey": "Ctrl+Cmd+V",
  "learning": { "learn_from_edits": true, "learn_from_screen": true,
                "keep_audio_for_selftest": false, "selftest_retention_days": 7 }
}
```

Removed: `command_mode_active` (P2.2). EG-1 model ids migrate to `s1-mini` (P3.9).

---

## 12. Order of work and rough effort (one agent, sequential)

| Phase | Effort | Depends on |
|---|---|---|
| 0 Measurement, legal, test infra | 3–4 days | n/a |
| 1 Latency quick wins | 2–3 days | 0 |
| 2 Command Mode fixes | 2–3 days | 0 |
| 3 Engines (FluidAudio bridge, sherpa, boosting, catalog) | 7–10 days | 0, 1 |
| 4 Commit-as-you-go streaming | 4–5 days | 3 |
| 5 Audio front end and noise | 4–6 days | 0, 3 |
| 6 Polish | 3–4 days | 1, 3 |
| 7 Learning loop | 8–10 days | 3, 6 |
| 8 Delivery + Windows parity | 6–8 days | 1 |
| 9 Testing/CI hardening (continuous; final pass) | 3 days | all |
| 10 Head-to-head benchmark | 2 days | all |

---

## 13. Final Definition of Done

**Phases and evidence**
- [ ] Gates 0–8 ticked in `PROGRESS.md`, each with linked eval runs.
- [ ] `evals/RESULTS.md` shows the baseline → final progression for WER (verbatim and clean), vocabulary recall, entity accuracy and latency p50/p95/p99, on macOS and Windows.

**Benchmark and coverage**
- [ ] `evals/WISPR_COMPARISON.md` published and meeting §10.2.
- [ ] All test kinds in §9.1 are implemented and green. Coverage targets in §9.5 are met.

**Legal and privacy**
- [ ] `cargo deny` and the UI license check pass.
- [ ] `THIRD_PARTY_MODELS.md` is complete.
- [ ] No EG-1, no `enviouslabs.co` URLs, no GPL code.
- [ ] Offline e2e suite passes. Privacy checks (no transcript text in logs, secure fields never read, delete-all works) pass.

**Product and release**
- [ ] Command Mode works through the hotkey, the wake phrase and the tray. With no Needle, the fallback works.
- [ ] Learning: the simulation test passes, the real-engine name-recall test is ≥ 90%, and Insights shows the trend.
- [ ] Release builds (signed macOS DMG, Windows NSIS) pass `--self-test` in CI.

---

## 14. Known risks and what to do

| Risk | Mitigation |
|---|---|
| FluidAudio API differs from this document (names change between releases) | Pin a release tag. Write the bridge against that tag's README/Docs. Record differences in `PROGRESS.md` "Deviations". |
| Swift runtime linking issues inside Tauri | Use `swift-rs` (proven with Tauri). Set the macOS minimum to 14.0 for the bridge target only, and runtime-gate the feature. As a last resort, use a helper executable (`teletype-asr-helper`) bundled as a Tauri `externalBin` and talking framed binary over stdin/stdout (adds ~1–3 ms). |
| sherpa-onnx hotwords not supported for NeMo TDT offline | Use `rescoring.rs` (P3.6) on Windows; revisit when sherpa adds support. |
| Vocabulary boosting causes false alarms with large lists | Context-ranked top-N (P7.5), false-alarm tracking and replay demotion (P7.7), and the eval gate `vocab_false_alarm ≤ 1%`. |
| Denoising hurts accuracy | Default "Smart" mode never feeds denoised audio to ASR (P5.0, P5.5). |
| Mic always-on privacy concerns | "Instant start" is off by default, with clear UI, memory-only buffering and the OS indicator explained. |
| Parakeet doesn't cover the user's language (e.g. Arabic, Hindi, Urdu) | Auto-route to Whisper large-v3-turbo (Core ML encoder on macOS). Evaluate Qwen3-ASR (Apache-2.0, 30 languages) in a later spike. |

---

## 15. Models, libraries and licenses (allowed list)

| Item | Source | License | Attribution / notes |
|---|---|---|---|
| FluidAudio (Swift SDK) | https://github.com/FluidInference/FluidAudio | Apache-2.0 | NOTICE in the About screen |
| Parakeet Ultra Core ML | https://huggingface.co/FluidInference/parakeet-ultra-coreml (base: https://huggingface.co/moondream/parakeet-ultra) | CC-BY-4.0 | "Parakeet Ultra by Moondream, based on NVIDIA Parakeet TDT 0.6B v3" |
| Parakeet TDT 0.6B v3 / v2 Core ML | https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml, https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v2-coreml | CC-BY-4.0 | NVIDIA attribution |
| Parakeet CTC 110M (vocab boosting) | downloaded via FluidAudio `CtcModels` | CC-BY-4.0 (verify on download page) | NVIDIA attribution |
| sherpa-onnx (Rust crate) | https://docs.rs/sherpa-onnx | Apache-2.0 | n/a |
| sherpa Parakeet v3/v2 int8 ONNX | https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models | CC-BY-4.0 (from NVIDIA) | NVIDIA attribution |
| whisper.cpp / whisper-rs | existing | MIT | n/a |
| Whisper large-v3-turbo | existing ggml download | MIT | OpenAI |
| Silero VAD (`silero-vad-pure`) | existing | MIT | n/a |
| DeepFilterNet3 via deepfilter-rt | https://github.com/shimondoodkin/deepfilter-rt | verify (DeepFilterNet upstream MIT/Apache-2.0) | n/a |
| LocalVQE (optional) | FluidAudio | Apache-2.0 (beta) | n/a |
| S1-mini | https://huggingface.co/superwhisper/s1-mini-GGUF | Apache-2.0 + naming clause | Always display "S1-mini by Superwhisper"; keep LICENSE + NOTICE |
| Qwen3 1.7B / 4B-Instruct-2507 GGUF | Hugging Face (Qwen official or bartowski/unsloth quantizations) | Apache-2.0 | n/a |
| Whistle / Needle (Cactus) | https://github.com/cactus-compute/needle | repo Apache-2.0; **weights license unverified** | Ship only after confirmation |
| rubato, ringbuf, aho-corasick, strsim, similar, rusqlite, arc-swap, swift-rs, proptest, insta, criterion | crates.io | MIT/Apache-2.0 | n/a |
| **Forbidden** | EG-1 (Envious Labs), any `models.enviouslabs.co` download, EnviousWispr source code (GPLv3), Wispr Flow outputs for training | n/a | n/a |

---

## 16. Sources used to write this plan

**Wispr Flow**
- Latency targets and architecture: [Wispr Flow technical challenges](https://wisprflow.ai/research/technical-challenges), [Baseten case study](https://www.baseten.co/resources/customers/wispr-flow/).
- Measured accuracy and latency: [Superwhisper benchmark: Wispr Flow raw](https://superwhisper.com/benchmarks/wispr-flow-raw).
- Dictionary auto-learn rules: [Wispr Flow dictionary docs](https://docs.wisprflow.ai/articles/4052411709-teach-flow-your-words-with-the-dictionary).

**Speech models**
- Parakeet on the Neural Engine: [Superwhisper benchmark: Parakeet V3](https://superwhisper.com/benchmarks/parakeet-v3).
- Parakeet Ultra accuracy: [moondream/parakeet-ultra](https://huggingface.co/moondream/parakeet-ultra), [FluidInference/parakeet-ultra-coreml](https://huggingface.co/FluidInference/parakeet-ultra-coreml).
- FluidAudio models and vocabulary boosting: [FluidAudio](https://github.com/FluidInference/FluidAudio), [FluidAudio Custom Vocabulary](https://github.com/FluidInference/FluidAudio/blob/main/Documentation/ASR/CustomVocabulary.md), [FluidAudio streaming docs](https://docs.fluidinference.com/asr/streaming).
- Windows (sherpa-onnx): [sherpa-onnx NeMo models](https://k2-fsa.github.io/sherpa/onnx/pretrained_models/offline-transducer/nemo-transducer-models.html), [sherpa-onnx hotwords](https://github.com/k2-fsa/sherpa/blob/master/docs/source/onnx/hotwords/index.rst), [sherpa-onnx Rust crate](https://docs.rs/sherpa-onnx/latest/sherpa_onnx/).

**Noise handling**
- Denoising vs ASR accuracy: [arXiv 2512.17562](https://arxiv.org/abs/2512.17562).
- DeepFilterNet in Rust: [deepfilter-rt](https://github.com/shimondoodkin/deepfilter-rt).

**Polish model**
- S1-mini usage: [superwhisper/s1-mini-GGUF](https://huggingface.co/superwhisper/s1-mini-GGUF).

**EnviousWispr internals** (FluidAudio fork, Parakeet Ultra default, WhisperKit, Silero Core ML, edit-judge/word-check models, EG-1 license) were read directly from [saurabhav88/EnviousWispr](https://github.com/saurabhav88/EnviousWispr) (`Package.resolved`, `Sources/EnviousWisprCore/ParakeetModelSelection.swift`, `Sources/EnviousWispr/Resources/*-delivery-manifest.json`, `EG-1-MODEL-LICENSE.txt`).
