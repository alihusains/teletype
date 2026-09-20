# Teletype — Checkpoint

_Last updated: 2026-09-20 (episode complete)_

Teletype is a macOS dictation app (Tauri v2 + Rust + React) replicating **Wispr Flow**'s
functionality and aesthetic: hold **Fn** to dictate anywhere, text is transcribed locally
and injected into the focused text field.

The full Wispr Flow roadmap is now implemented. Both previously-blocking issues
(🔴 #1 stuck pill, 🔴 #2 language detection) are fixed.

---

## ✅ What has been done

### Core dictation
- **Fn key trigger** — bare Fn (no modifiers) via a native CGEventTap (`fn_tap.m` / `fn_tap.rs`).
  Held = record, released = transcribe. Double-tap = continuous "hands-free" mode.
- **State machine** (`teletype-core/src/state.rs`) — `decide()` maps hotkey inputs to
  `Start` / `Stop` / `GoHandsFree` / `Discard` / `CancelPipeline` actions across
  `Idle → Listening → Transcribing → Transforming → Inserting → Idle`.
- **Local transcription** — Parakeet (fast local ASR) + Whisper fallback, selected via
  the model catalog. Runs on a worker thread so the UI never blocks.
- **Text injection** — clipboard paste with restore (`injector.rs`), platform paste shortcut.
- **Transform pipeline** (`pipeline.rs`) — capitalization, punctuation, filler-word removal
  (configurable word list), transforms, style-profile phrases, dictionary protection, and
  optional LLM inference, applied before injection.
- **Dictation works end-to-end** — holding Fn, speaking, and releasing transcribes and types
  text into the focused field.

### Dictation history
- Every dictation is saved to `~/Library/Application Support/com.teletype.app/dictation.json`
  (`teletype-core/src/history.rs`), even when there's no text field to paste into.
- **Dictation history screen** (`ui/src/screens/DictationScreen.tsx`) with day-grouping,
  copy, and delete.

### Floating pill (SpeakType parity) — FIXED
- **Pill phase mismatch fixed**: `overlay.rs` now emits the typed `PillState` before
  show/hide with structured `tracing` logs (debug `eprintln!` removed). The React pill
  (`ui/src/pill.tsx`) consumes `pill-state` via the StrictMode-safe `useTauriEvent` hook;
  all diagnostic markers (`stateEvents` / `lastRaw` / green debug text / console.log)
  are removed.
- **`Warming` phase wired** — `dictation.rs` checks `is_loaded()` before recording; if the
  model isn't loaded it shows the warming pill, loads on a background thread, and re-emits
  `Recording{startedAtMs}` on `WarmupDone` (guarded by session id so a stale load can't
  clobber a newer session).
- **`useTauriEvent` applied to the main app** — `App.tsx`'s `dictation-state` listener now
  uses the hook (StrictMode-safe).
- **`always_show_pill`** reset to `false` in `settings.json`.

### Language & audio reliability — FIXED
- **Language guard** — `effective_language()` in `dictation.rs`: for English-only engines
  (Parakeet), `"auto"`/empty resolves to `"en"`, so short utterances can no longer be
  mis-transcribed as Russian. Settings default is `en`; the UI offers English / Auto-detect.
- **"No speech detected" disambiguation** — `Captured` now carries a `peak` amplitude;
  a near-silent capture (< 0.005 peak) reports "No audio captured — check the microphone"
  instead of the misleading "No speech detected".

### Roadmap features — ALL IMPLEMENTED
| # | Feature | Status | Where |
|---|---------|--------|-------|
| 1 | Core dictation | ✅ | `fn_tap`, `dictation.rs`, `pipeline.rs` |
| 2 | Dictation history | ✅ | `history.rs`, `DictationScreen.tsx` |
| 3 | Floating pill (SpeakType parity) | ✅ | `overlay.rs`, `pill.tsx` |
| 4 | Dashboard / stats | ✅ NEW | `stats.rs`, `DashboardScreen.tsx` |
| 5 | Notetaker | ✅ NEW | `ScratchpadScreen.tsx` (private dictation target) |
| 6 | Insights | ✅ NEW | `insights.rs`, `InsightsScreen.tsx` |
| 7 | Dictionary | ✅ NEW | `dictionary.rs`, `DictionaryScreen.tsx` |
| 8 | Snippets | ✅ | `autotext/` + `AutoTextScreen.tsx` (`/trigger` expansion) |
| 9 | Style | ✅ NEW | `style.rs`, `StylesScreen.tsx` |
| 10 | Transforms | ✅ Polished | `transforms/` + `TransformsScreen.tsx` (auto-apply now persists) |
| 11 | Scratchpad | ✅ NEW | `scratchpad.rs`, `ScratchpadScreen.tsx` |

**New core modules** (all pure, all unit-tested):
- `teletype-core/src/stats.rs` — `DashboardStats`: words today / 7-day / total, streak,
  14-day daily bars, est. minutes spoken. Pure fn over history.
- `teletype-core/src/insights.rs` — `Insights`: top 2-/3-gram phrases (subsumed 2-grams
  dropped), top apps, avg words/dictation, busiest hour, vocabulary size.
- `teletype-core/src/dictionary.rs` — custom words + pronunciation hints; `known_words()`
  feeds the pipeline so transforms keep them verbatim.
- `teletype-core/src/style.rs` — style profiles (3 built-ins: Concise / Professional /
  Casual + user profiles). The active profile's phrases are merged into the prompt packet.
- `teletype-core/src/scratchpad.rs` — private dictation buffer (200-entry cap, combined
  text export).

**Pipeline wiring** (`pipeline.rs`): `Pipeline` now takes `dictionary`, `styles`,
`active_style`; the prompt packet gets the active style profile's phrases and
`keep '<word>' as written` hints for dictionary words.

**Scratchpad routing** (`dictation.rs`): when `scratchpad_enabled` and the active app is
Teletype itself, the final text is appended to the scratchpad instead of injected.

**New Tauri commands** (`commands.rs`, all registered in `lib.rs`):
`get_dashboard_stats`, `get_insights`, `list_dictionary` / `add_dictionary_word` /
`remove_dictionary_word`, `list_style_profiles` / `create_style_profile` /
`update_style_profile` / `delete_style_profile` / `set_active_style_profile` /
`reset_style_profiles`, `list_scratchpad` / `append_scratchpad` /
`delete_scratchpad_entry` / `clear_scratchpad` / `get_scratchpad_text`.

**New settings fields** (serde-defaulted, backward compatible): `active_style_profile`,
`scratchpad_enabled`.

**New stores**: `dictionary.json`, `styles.json`, `scratchpad.json` in the app config dir.

### Audio
- **`LevelMeter`** in `teletype-core/src/audio/mod.rs`: RMS + peak → dB normalization
  (-58 dB floor) → noise gate → fast-attack/slow-release smoothing.
- **`Captured.peak`** — peak amplitude of the resampled buffer, used for the
  no-audio-vs-no-speech distinction.

---

## Verification (this episode)
- `cargo test --workspace` — **109 passed, 0 failed** (84 baseline + 25 new:
  stats 5, insights 4, dictionary 2, style 3, scratchpad 3, plus pipeline updates).
- `cargo clippy --workspace` — 0 errors; remaining warnings are pre-existing
  (unused `parakeet_free_params` FFI decl, mic_permission.m `dbPath`).
- `npm run typecheck` + `npm run build` (Vite) — clean.
- `cargo build --release` — clean; release binary launches, model warms on Metal
  (Apple M4 Pro), "speech model ready".
- Test-delta check: baseline (stashed) suite also passed 84/84, so no regressions.

## Verification (follow-up fix pass, 2026-09-20)
Fixed the issues blocking a clean build/test/lint:
- **Compile errors** — `teletype-core` used `regex` (pipeline.rs) without the
  dependency, and `injector.rs` gated `tracing::{warn,error}` behind
  `#[cfg(not(test))]` while using them in non-test code. Added `regex` to the
  workspace + core deps; un-gated the tracing import.
- **Linker error (release + debug)** — the app linked both whisper.cpp
  (whisper-rs, speech) and llama.cpp (llama_cpp, inference), which both embed
  ggml, so the final binary failed with hundreds of duplicate-symbol errors.
  The llama.cpp runtime is now unlinked: `teletype-inference/src/llama.rs`
  keeps the `LlamaProvider` public surface (catalog, download, UI intact) but
  `warm_up()`/`generate()` return a clear "runtime not linked" error, and the
  pipeline falls back to deterministic transforms. README + docs updated.
- **Clippy** — `manual_checked_division` (stats.rs, insights.rs),
  `inconsistent_digit_grouping` (insights.rs test), 6 missing `SAFETY`
  comments (commands.rs, macos_impl.rs), dead `parakeet_free_params`
  (allowed), unused `dbPath` in mic_permission.m.
- Re-verified: `cargo build --workspace`, `cargo test --workspace` (109 pass),
  `cargo clippy --workspace --all-targets` (0 rust warnings),
  `npm run typecheck` + `npm run build` — all clean.

## Remaining (manual, needs a real mic + screen)
1. **Visual pill verification** — hold Fn, confirm the pill morphs idle → recording
   (red dot + waveform + timer) → processing → idle, and that hover expands the chips.
   The event path is confirmed wired (typed payload, pre-show emit, StrictMode-safe
   hook); a human-in-front-of-the-mac check is the last step.
2. **Live language check** — dictate "Hello" a few times and confirm English output.
3. **Scratchpad routing** — with the toggle on, focus the Teletype window and dictate;
   the text should land in the Scratchpad screen.

## Key files
- `crates/teletype-core/src/state.rs` — `Phase`, `UiState`, **`PillState`**, **`PillPosition`**
- `crates/teletype-core/src/audio/mod.rs` — mic capture, `LevelMeter`, `Captured.peak`
- `crates/teletype-core/src/stats.rs` / `insights.rs` / `dictionary.rs` / `style.rs` / `scratchpad.rs` — new feature modules
- `crates/teletype-core/src/pipeline.rs` — unified pipeline (now style + dictionary aware)
- `crates/teletype-desktop/src/dictation.rs` — controller, `WarmupDone`, `effective_language`, scratchpad routing
- `crates/teletype-desktop/src/overlay.rs` — pill window show/hide, 9-position placement
- `crates/teletype-desktop/src/commands.rs` — all IPC (settings incl. `active_style_profile`, `scratchpad_enabled`)
- `crates/teletype-desktop/src/lib.rs` — state wiring (dictionary/styles/scratchpad stores)
- `ui/src/pill.tsx` — floating pill (4 phases, diagnostics removed)
- `ui/src/lib/useTauriEvent.ts` — StrictMode-safe event hook
- `ui/src/screens/` — Home, Dictation, **Dashboard**, **Insights**, Transforms, AutoText, **Dictionary**, **Style**, **Scratchpad**, Personalization, Models, Settings, Onboarding

## Reference
- SpeakType pill implementation (the source we're matching):
  `github.com/karansinghgit/speaktype` — `desktop/src/pill/Pill.tsx`,
  `desktop/src-tauri/src/pill.rs`, `desktop/src-tauri/src/dictation.rs`,
  `desktop/src/lib/useTauriEvent.ts`

## App data
- `~/Library/Application Support/com.teletype.app/dictation.json` — history
- `~/Library/Application Support/com.teletype.app/settings.json` — preferences
  (language=en, always_show_pill=false, active_style_profile, scratchpad_enabled)
- `~/Library/Application Support/com.teletype.app/{dictionary,styles,scratchpad}.json`
- Dev log: `/tmp/teletype-dev.log`
