# Teletype: Refactoring and Feature Roadmap

Repository: [alihusains/teletype](https://github.com/alihusains/teletype), analyzed at commit [`916f666`](https://github.com/alihusains/teletype/commit/916f66672de0231f9a66213ddba4ae293742ee4b) (v0.2.6, 9 Oct 2026).
Method: static review of the full tree (Rust workspace + React UI + CI). The test suite was **not executed** (the Rust toolchain could not be installed in the analysis sandbox), so the test counts below come from static analysis, not a coverage run.

Line links point to the pinned commit so they stay accurate as `main` moves.

---

## 1. Snapshot

| Metric | Value |
|---|---|
| Rust source | ~47,200 lines across 6 crates |
| TypeScript/React UI | ~13,300 lines |
| Rust tests | ~705 `#[test]` functions; 8 integration-test files |
| UI tests | **0** (no test runner in `ui/package.json`) |
| Tauri commands | ~111 in a single file |
| Largest files | `commands.rs` 4,755 · `dictation.rs` 3,119 · `pipeline.rs` 2,039 · `vocab.rs` 1,622 · `itn.rs` 1,456 · `pill.tsx` 1,346 · `server.rs` 1,345 |
| CI | Linux/macOS `cargo test` per crate, Windows `cargo check` only, no coverage |

**What's already strong.** Crate boundaries are mostly clear (`core` / `speech` / `inference` / `desktop` / `eval`). There's a real offline eval harness. Many regressions are pinned by tests that name the bug ID (BUG-018, BUG-06…09). There's an IPC contract test, a corrupt-file quarantine in storage, and a "brain" decision log. This roadmap builds on that discipline. It doesn't replace it.

---

## 2. Performance bottlenecks

Ranked by expected user-visible impact on the dictation hot path: hotkey release → text pasted.

### P1. Regexes recompiled on every dictation (ITN)
- [`itn.rs#L236-L241`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-core/src/itn.rs#L236-L241): `fn re(pattern) -> Regex` calls `Regex::new` on **every call**. Its comment says "the regex crate caches internally". That's wrong: `regex` has no global compile cache.
- `normalize()` runs ~20 passes, and there are ~30 `re_sub` call sites. Several patterns are built with `format!` from large number-word alternations. So each dictation compiles dozens of regexes, which costs several milliseconds to tens of milliseconds on a cold path.
- **Fix:** move every static pattern to `static X: LazyLock<Regex>`. Dynamic patterns get a `LazyLock<HashMap<&'static str, Regex>>` or a `RegexSet`. Add a criterion bench for `itn::normalize` over the eval corpus. Expected win: 10–50× on ITN.

### P2. Inference mutex held for the whole LLM call
- [`dictation.rs#L1546`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/src/dictation.rs#L1546) takes `state.inference.lock()` and holds it until [`#L1792`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/src/dictation.rs#L1792). That's the entire transform, up to the 20 s cap.
- The comment calls this intentional ("no model swap mid-inference"). The side effect is that every other command needing `inference` blocks for the full LLM latency: model list, status, Models screen, and tray.
- **Fix:** store the engine as `Arc<dyn Engine>` behind an `ArcSwap` (or `RwLock<Arc<…>>`). The dictation clones the `Arc` and drops the lock right away. A model swap installs a new `Arc`, and the old engine lives until in-flight runs finish. That gives the same safety with no contention.

### P3. Quadratic token streaming to the pill
- [`dictation.rs#L1581-L1585`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/src/dictation.rs#L1581-L1585): every token appends to `stream` and then emits `stream.clone()`, the whole running text. Over N tokens that's O(N²) bytes serialized across IPC, plus a React re-render per token.
- **Fix:** emit only the delta (`{seq, tok}`) and let the pill concatenate. Or coalesce emits to one per animation frame (~16 ms). Either cuts IPC traffic by about 100× on long rewrites.

### P4. Full deep-clone of every store per dictation
- Before processing, [`dictation.rs#L1480-L1545`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/src/dictation.rs#L1480) clones AutoText, transforms, profile, styles, dictionary, and user packs, each under its own `Mutex`. Then it filters system AutoText entries on every run.
- `vocab::terms_for` ([`vocab.rs#L1567`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-core/src/vocab.rs#L1567)) rebuilds and lowercases every pack alias each dictation.
- **Fix:** give each store an immutable snapshot (`ArcSwap<Arc<Store>>`), published on write and read in O(1). Precompile a `CompiledLexicon` (lowercased aliases, length-bucketed or BK-tree index, Aho-Corasick for exact multi-word matches) only when the dictionary or packs change.

### P5. Dictionary fuzzy matching is O(tokens × entries × windows) with allocations
- `correct_with_dictionary` ([`pipeline.rs#L540`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-core/src/pipeline.rs#L540)) runs `edit_distance` against every entry for each 1–4-token window. It allocates lowercased strings inside the loop.
- That's fine for 50 words. It degrades badly with large user dictionaries plus enabled vocab packs (thousands of aliases).
- **Fix:** use the precompiled lexicon from P4. Pre-filter by length (|len(a) − len(b)| ≤ k) and first-letter or phonetic key (Double Metaphone), then compute bounded Levenshtein (stop early once over k). Benchmark with 5k entries.

### P6. Persistence: double serialization, full rewrite, on the hot path
- [`storage.rs#L61-L64`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-core/src/storage.rs#L61-L64): `save()` calls `to_vec_pretty(doc)` **twice**. The first result (`_bytes`) is discarded. That's a free 2× win.
- Each dictation saves usage, history, and the dictionary ([`dictation.rs#L1787-L1836`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/src/dictation.rs#L1787)). History holds up to 1,000 entries with context and is rewritten in full, pretty-printed, every time. `history.add` inserts at the front of a `Vec`, which is O(n).
- Writes don't `fsync` before rename, so a crash can leave a zero-length file on some filesystems.
- **Fix:** (a) delete the duplicate serialization; (b) use compact JSON for large stores; (c) move history and usage to SQLite (`rusqlite`, WAL) or an append-only JSONL with periodic compaction; (d) debounce saves through a single background writer thread; (e) `File::sync_all` before rename.

### P7. Audio resampling without anti-aliasing
- [`audio/mod.rs#L494`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-core/src/audio/mod.rs#L494) uses linear interpolation for 48 kHz → 16 kHz with no low-pass filter. Content above 8 kHz folds back into the speech band, which can quietly hurt WER.
- **Fix:** use `rubato` (sinc or FFT resampler), or a simple polyphase FIR for the integer 3:1 case. Check the WER delta with `teletype-eval`.

### P8. Whisper tuning
- [`whisper/macos.rs#L71`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-speech/src/whisper/macos.rs#L71) sets `n_threads` to all logical cores. On Apple Silicon, efficiency cores slow whisper.cpp down, so cap it at the performance-core count (`sysctl hw.perflevel0.logicalcpu`).
- `no_timestamps` and `single_segment` aren't set for short utterances, and no `initial_prompt` is used (see feature F2).

### P9. Hand-rolled HTTP/SSE client to `llama-server`
- [`server.rs#L600-L660`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-inference/src/server.rs#L600): raw `TcpStream` with HTTP/1.1. It doesn't decode `Transfer-Encoding: chunked`; it only works because SSE frames happen to line up with chunk boundaries.
- [`#L616`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-inference/src/server.rs#L616) calls `String::from_utf8_lossy(chunk)` on each 8 KB read. Any multi-byte UTF-8 character split across reads (accents, CJK, Arabic, emoji) turns into `U+FFFD`. **This is a correctness bug for non-English users.**
- **Fix:** buffer bytes and decode only complete UTF-8 prefixes (or split on `\n\n` at the byte level first). Longer term, replace the client with `reqwest` + `eventsource-stream`, or `ureq` if you want to stay sync.

---

## 3. Correctness bugs found during review

| # | Issue | Location | Severity |
|---|---|---|---|
| B1 | UTF-8 split across socket reads corrupts streamed LLM text | `server.rs#L616` | High (non-English) |
| B2 | No chunked-transfer decoding; works by coincidence | `server.rs` SSE reader | Medium |
| B3 | Duplicate `to_vec_pretty` in every save | `storage.rs#L61` | Low (perf) |
| B4 | No `fsync` before atomic rename | `storage.rs` | Medium (data loss on power loss) |
| B5 | Corrupt-file quarantine overwrites the previous `.corrupt` | `storage.rs` | Low (forensics lost) |
| B6 | Two integration tests share `temp_dir()/teletype_p317_test`, so they race under parallel `cargo test` | [`tests/file_transcribe.rs#L38,L58`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/tests/file_transcribe.rs#L38) | Medium (flaky CI) |
| B7 | Storage tests use fixed temp-dir names; parallel runs or reruns collide | `storage.rs#L92-L122` | Low |
| B8 | `save_settings` takes the whole `Settings` document. The `save_lock` serializes writes, but a window holding a stale snapshot still overwrites another window's newer fields (lost update) | [`commands.rs#L641`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/src/commands.rs#L641) | Medium |

---

## 4. Code smells

### S1. God modules
- `commands.rs` (4,755 lines, ~111 commands) mixes settings, AutoText, transforms, dictionary, models, licensing, icons, scratchpad, and diagnostics.
- `dictation.rs` has one function, `transcribed()`, that runs ~500 lines (≈ L1434–L1934). It handles store snapshots, app context, LLM routing, streaming, paste, history, usage, and learning.
- `pill.tsx` is 1,346 lines with 15 `useState` hooks and 8 event listeners.
- **Refactor:** split commands into `commands/{settings,autotext,transforms,dictionary,models,history,license,system}.rs`. Break `transcribed()` into explicit stages (see A2).

### S2. God-object `AppState`
- [`lib.rs#L51`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-desktop/src/lib.rs#L51): 36 public fields. Each domain is duplicated as a `Mutex<T>` plus a `JsonStore<T>`, and every caller has to remember to `save()` after mutating, often while still holding the lock. Lock order isn't documented, and `dictation.rs` takes six or more locks in sequence.
- **Refactor:** one `Repository<T>` type per domain that owns the data, snapshot, and persistence (`get() -> Arc<T>`, `update(|t| …) -> Result<Arc<T>>`, auto-persist on update, emit change events). `AppState` becomes a handful of repositories plus services.

### S3. Stringly-typed settings and errors
- `Settings` has many enum-like `String` fields (`recording_mode`, `pill_position`, `list_style`, `match_strictness`, `app_icon`, …) parsed with `from_str_lenient`. Typos become silent defaults.
- About 300+ `Result<_, String>` signatures. Errors lose structure and get matched by text.
- **Refactor:** use `#[serde(rename_all = "kebab-case")] enum`s with `#[serde(other)] Unknown` for forward compatibility. Add a `thiserror` error type per crate and a single `CommandError { code, message }` at the IPC edge so the UI can branch on `code`.

### S4. Hand-maintained IPC types
- TS interfaces are declared by hand. [`tests/ipc_contract.rs`](https://github.com/alihusains/teletype/blob/916f66672de0231f9a66213ddba4ae293742ee4b/crates/teletype-core/tests/ipc_contract.rs) exists because this caused four shipped bugs (BUG-06…09). It parses source text, which is fragile.
- About 18 commands appear in the Rust handler list but are never `invoke`d from the UI (e.g. `get_status`, `get_model_status`, `download_model`, `reset_usage_stats`, `transform_selection`, `update_style_profile`). They're either dead code or reached only from Rust/tray, so audit them.
- **Refactor:** generate bindings with [`tauri-specta`](https://github.com/specta-rs/tauri-specta) (typed `commands.ts` + event types). Then delete the text-parsing contract test and the hand-written interfaces.

### S5. Layering leaks
- `teletype-core` is documented as platform-independent but depends on `cpal`, `arboard`, and `enigo`, which are OS I/O.
- `teletype-desktop` depends on `teletype-core` with `features = ["test-support"]` in **production** dependencies, so test fixtures ship in the release binary.
- `docs/architecture.md` is stale. It says llama isn't linked, yet the app now runs a `llama-server` sidecar and Parakeet.
- **Refactor:** move audio capture, clipboard, and typing into a `teletype-platform` crate (or into `desktop`). Make `test-support` a `[dev-dependencies]` feature only. Regenerate the architecture doc and check it in CI (e.g. a test asserting that the crate graph matches the doc).

### S6. Logging inconsistency
`eprintln!` in library code (`storage.rs`), `tracing::*` elsewhere, and a custom `crate::log_entry` ring buffer. Standardize on `tracing` with a custom `Layer` that feeds the in-app Developer log.

### S7. Manual side-effect diffing in `save_settings`
Each field change is hand-checked (`if settings.x != previous.x { apply_x() }`), and new settings regularly ship without their runtime-apply code (BUG-009, BUG-010 are exactly this). **Refactor:** register a `SettingsObserver` per subsystem (`hotkey`, `tray`, `pill`, `typing`, `icon`). Each declares the fields it watches and receives `(old, new)`.

### S8. Repository hygiene
`ui/dist/` (built bundle), conversation exports (`*.html`, `*.txt`), and `prompt.md` are committed at the root. Remove them and add them to `.gitignore`, so reviews and `git blame` stay clean.

### S9. Unfinished migration
`use_parakeet: bool` still lives next to the newer engine-selection model (the repo's own roadmap item, "Phase 0: replace `use_parakeet` with `speech_engine`"). Finish it with a one-time settings migration and delete the bool.

---

## 5. Test coverage gaps

Files with substantial logic and **zero** tests:

| File | Lines | Risk |
|---|---|---|
| `teletype-desktop/src/lib.rs` (startup, state wiring, migrations) | 815 | Startup regressions |
| `teletype-eval/src/bin/eval.rs` | 513 | Eval harness itself unverified |
| `teletype-speech/src/parakeet.rs` (FFI) | 307 | Memory safety, error paths |
| `platform/macos_impl.rs` / `windows_impl.rs` | 243 / 143 | Permission and focus logic |
| `tray.rs` | 213 | Menu state drift |
| `input_engine/mod.rs`, `mod_tap.rs`, `fn_tap.rs` | 140 / 99 / 78 | Hotkey edge cases |
| `typing/typing_windows.rs` | 116 | Windows-only, never run in CI |

Structural gaps:

1. **No UI tests at all.** Add Vitest + React Testing Library with a mocked `@tauri-apps/api` `invoke`/`listen`. Priorities: `pill.tsx` state machine (idle → recording → transcribing → streaming → done/error), `SettingsScreen` save round-trip, and `TransformsScreen` create/delete (BUG-06/09 territory).
2. **Thin coverage of the hot path.** `commands.rs` has 15 tests for ~111 commands, and `dictation.rs` has 17 tests for 3,119 lines. Once `transcribed()` is split into pure stages (A2), each stage can be unit-tested without Tauri.
3. **Windows never runs tests.** CI does only `cargo check` on Windows. Add `cargo test -p teletype-core -p teletype-inference` on `windows-latest` (both are platform-light).
4. **No coverage measurement.** Add `cargo llvm-cov` (Linux job) with an artifact plus a ratcheting floor (start at the measured baseline, never go down).
5. **No property, fuzz, or concurrency tests.**
   - `proptest` for ITN (idempotence: `normalize(normalize(x)) == normalize(x)`; no word loss outside numbers), dictionary correction (never corrupts exact matches), and the SSE parser (arbitrary chunk splits, including mid-UTF-8, give identical output; this would have caught B1).
   - `cargo fuzz` targets for `itn::normalize`, the SSE parser, and `JsonStore::load` on arbitrary bytes.
   - `loom` or a stress test for the inference engine swap once A3 lands.
6. **No performance regression tests.** Add `criterion` benches for `itn::normalize`, `Pipeline::process` (dictionary of 5k entries), and `resample_to_target`, run on CI in a non-blocking job with `critcmp` output.
7. **Flaky fixtures.** Replace every `temp_dir().join("fixed-name")` with `tempfile::TempDir` (B6, B7).
8. **Eval in CI.** Run `teletype-eval` on a small fixed corpus per PR and fail if WER regresses by more than X% against `main`. This guards P7 and P8 tuning and Whisper upgrades.

---

## 6. Proposed architectural improvements

### A1. Repository + snapshot state model
```rust
pub struct Repo<T> { cur: ArcSwap<T>, store: JsonStore<T>, tx: broadcast::Sender<Arc<T>> }
impl<T: Clone + Serialize> Repo<T> {
    pub fn get(&self) -> Arc<T>;                          // lock-free read
    pub fn update(&self, f: impl FnOnce(&mut T)) -> Result<Arc<T>>; // clone-on-write, persist via writer thread, broadcast
}
```
This removes the `Mutex` + `JsonStore` pairs, the forgotten saves, lock-ordering risk, and per-dictation deep clones (P4, S2, B8 with patch-style updates).

### A2. Dictation as an explicit stage pipeline
Split `transcribed()` into pure, testable stages with one `DictationContext`:

`Capture → Resample → VAD/Trim → ASR → ITN → Dictionary/Packs → AutoText → Transform (LLM, optional) → Format → Deliver (paste/type) → Record (history/usage/learning)`

Each stage is `fn(&Ctx, Input) -> Result<Output, StageError>` and emits timings into a `DictationTrace`. The Developer screen shows the trace as a waterfall. The trace becomes the main perf-debugging tool and the basis for P1–P6 regression checks.

### A3. Engine registry with hot-swappable `Arc` handles
`SpeechEngine` and `LlmEngine` traits live behind `ArcSwap<Arc<dyn …>>` (P2). Model downloads, warm-up, and swaps run in a background `ModelManager` actor that talks over a channel. The UI reads its status, which never blocks.

### A4. Background persistence actor
A single writer thread receives `Persist(domain, Arc<T>)` messages, coalesces bursts (e.g. 250 ms), writes compact JSON with fsync, and moves history/usage to SQLite (P6). The dictation hot path does no disk I/O.

### A5. Typed IPC and event bus
Use `tauri-specta` for commands and events (S4) plus a domain-event enum (`SettingsChanged{diff}`, `DictationStage{…}`, `TransformToken{seq, delta}`). The UI subscribes through one typed hook instead of 20 stringly `listen()` calls.

### A6. Crate re-layering
```
teletype-core        pure: pipeline, ITN, dictionary, vocab, personalization, storage traits
teletype-platform    NEW: audio capture (cpal), clipboard, typing, focus/app context, permissions
teletype-speech      ASR engines (whisper, parakeet)
teletype-inference   LLM engines (llama-server sidecar, OpenAI-compat), HTTP via reqwest
teletype-desktop     Tauri shell only: commands (split), tray, overlay, wiring
teletype-eval        offline harness
```
Enforce the boundaries with `cargo deny` bans, or a test asserting that `teletype-core` has no `cpal`, `enigo`, or `arboard` dependency.

### A7. UI structure
Break `pill.tsx` into a `usePillMachine()` reducer (explicit states) plus presentational components. Introduce a light store (Zustand or a `useSyncExternalStore` wrapper over settings events) so screens stop re-fetching settings on every `settings-changed` event.

---

## 7. Feature roadmap

Features that fit the existing architecture and become cheap after the refactors:

| ID | Feature | Depends on | Notes |
|---|---|---|---|
| F1 | **Live partial transcripts** in the pill while speaking | A2, A3 | Chunked Whisper/Parakeet decoding on a sliding window; final pass on release |
| F2 | **Dictionary-biased ASR** via Whisper `initial_prompt` (top-N user terms + active app context) | P4 lexicon | Currently unused; low effort, likely WER win on proper nouns. Validate with eval |
| F3 | **Per-stage latency HUD** in the Developer screen | A2 | Waterfall from `DictationTrace`; export as JSON for bug reports |
| F4 | **Undo last dictation** (global hotkey) | A2 Deliver stage | Records what was pasted and where; sends the matching backspaces or restores the clipboard |
| F5 | **History search + export** (FTS5) | A4 SQLite | Full-text search, date filters, CSV/Markdown export |
| F6 | **Settings sync/backup** (export/import bundle) | A1 | One file with settings, dictionary, AutoText, transforms, styles; versioned schema |
| F7 | **Streaming LLM with cancel** | A3, P3 | Esc cancels the transform and pastes the raw transcript |
| F8 | **Multilingual robustness** | B1 fix, P7 | Per-app language lock already exists; add an auto-detect confidence threshold and fallback |
| F9 | **Plugin-style transforms** (user JS/regex/LLM chains) | A2 | Transform = ordered list of steps; ship built-ins as presets |
| F10 | **Windows parity** | CI tests on Windows | Matches the repo's `docs/roadmap-windows.md`; blocked mostly by test coverage |
| F11 | **Crash/telemetry opt-in** (local-first) | S6 tracing | Panic hook writes a redacted report the user can attach to issues |

---

## 8. Phased execution plan

Effort: S ≤ 1 day, M ≤ 1 week, L > 1 week.

### Phase 0: Quick wins (1 week)
| Task | Ref | Effort | Impact |
|---|---|---|---|
| Delete duplicate serialization in `JsonStore::save`; add `sync_all` | P6, B3, B4 | S | Med |
| `LazyLock` all ITN regexes | P1 | S | High |
| Byte-level SSE buffering (fix UTF-8 split) + regression test | B1, P9 | S | High |
| Delta token events to the pill | P3 | S | Med |
| `tempfile::TempDir` in all tests | B6, B7 | S | Med (CI stability) |
| Move `test-support` feature to dev-deps | S5 | S | Low |
| Remove committed `ui/dist`, transcripts, `prompt.md` | S8 | S | Low |
| Cap Whisper threads at performance cores; `no_timestamps` | P8 | S | Med |

### Phase 1: Measurement and safety net (1–2 weeks)
- criterion benches (ITN, pipeline, resample) + `cargo llvm-cov` baseline in CI.
- Vitest + RTL scaffold; first tests for the pill state machine and the settings round-trip.
- `cargo test` for `core` and `inference` on `windows-latest`.
- proptest for ITN, SSE, and dictionary; fuzz targets for storage load and SSE.
- `teletype-eval` WER gate on PRs.

### Phase 2: State and concurrency (2–3 weeks)
- A1 `Repo<T>` with `ArcSwap` snapshots; migrate settings → dictionary → AutoText → transforms → profile/styles.
- A3 engine `Arc` swap; drop the inference lock before generation (P2).
- A4 persistence actor; history/usage → SQLite with migration from JSON.
- Patch-based `update_settings(patch)` and `SettingsObserver`s (B8, S7).
- Finish `use_parakeet` → `speech_engine` migration (S9).

### Phase 3: Structure (2–3 weeks)
- Split `commands.rs` by domain; split `transcribed()` into A2 stages with `DictationTrace`.
- `tauri-specta` typed bindings; delete hand-written TS interfaces and the text-parsing contract test (S4).
- Typed enums for settings and `thiserror` errors (S3).
- `teletype-platform` crate extraction and boundary enforcement (A6).
- Unify logging on `tracing` (S6).
- Refresh `docs/architecture.md` from the new layout.

### Phase 4: Performance depth (1–2 weeks)
- Compiled lexicon: length/phonetic index + bounded Levenshtein + Aho-Corasick (P4, P5).
- Proper resampler (`rubato`) validated by eval WER (P7).
- Replace the hand-rolled HTTP with `reqwest` + SSE crate (P9, B2).

### Phase 5: Features (rolling)
Order by value and dependency readiness: F2 → F3 → F7 → F4 → F1 → F5 → F6 → F9 → F8 → F10 → F11.

---

## 9. Success metrics

| Metric | How to measure | Target |
|---|---|---|
| Release → paste latency (no LLM), p50/p95 | `DictationTrace` | −30% vs. Phase 0 baseline |
| ITN time per utterance | criterion | < 1 ms |
| UI command latency during an LLM transform | trace / manual probe | < 50 ms (currently up to full LLM time) |
| Disk writes per dictation | trace counter | 0 on the hot path; batched afterwards |
| Rust line coverage | llvm-cov | Baseline → +15 pts by end of Phase 3 |
| UI test count | Vitest | ≥ 40 tests covering pill + 3 critical screens |
| WER on eval corpus | teletype-eval | No regression; −5% relative after F2 + P7 |
| Flaky CI runs | CI history | 0 known temp-dir races |

---

## 10. Caveats

- Findings come from static reading at a single commit. Performance estimates are directional; Phase 1 benches should confirm them before Phase 2 work starts.
- "Unused command" detection matched `invoke("…")` strings in `ui/src`. Commands called from Rust (tray, hotkeys) or built dynamically may be false positives.
- The chunked-encoding observation (B2) depends on `llama-server`'s response framing for the bundled version. Verify it with a captured response before rewriting the client.
