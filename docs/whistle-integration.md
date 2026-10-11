# Whistle / Command-Mode Integration Plan

This branch (`whistle-integration`) implements the roadmap in
`teletype_complete_roadmap.md`, starting with the P0 foundation: the speech
engine abstraction and the Whistle provider.

## What is done (this commit)

### Command Mode (C1.1 text commands + C1.2 local actions)
- `crates/teletype-desktop/src/command_mode.rs` implements the intent parser
  (`Draft`, `Transform`, `OpenApp`) and the side-effect handler. `OpenApp` is
  gated by a strict macOS bundle-ID allowlist (Safari, Finder, Terminal,
  Calculator, Notes, Calendar, VS Code, Mail). Drafts and transforms run on
  the existing `InferenceProvider` (llama-server GGUF or OpenAI-compatible)
  and insert via the same `RecordTarget` / `insert_text` path as dictation,
  with the same cancelled/stale-target guards as `PipelineReady`.
- The Tauri command `run_command` is registered in `lib.rs` and the IPC
  contract test (`contract_02`) accepts module-qualified registrations.
- `ui/src/settings/CommandModeSection.tsx` is the discoverable, keyboard-
  operable entry point: type or tap an example, press Run, see the result.
- ADR-011 records why Command Mode runs on the InferenceProvider rather than
  on Needle tool-calling for text generation. Needle tool-calling is now
  wired for the unclassified-speech path: `run_needle_command` calls
  `needle_init` / `needle_complete_text` / `parse_tool_call` and validates
  the result through `validate_tool_call` before any side effect. When the
  `cactus` feature is absent the user sees a visible refusal, never a
  silent success.

**Security model (production-hardened):**
- `OpenApp`: any installed Mac app can be launched by display name via
  `open -a <name>`. The name is validated against a runtime-discovered list
  of installed `.app` bundles (`/Applications/`, `/System/Applications/`,
  `~/Applications/`). Shell metacharacters (`/`, `~`, backtick, `;`, `|`,
  `$`) are rejected before the call. `launch_app_by_name` re-checks as
  defense in depth. No shell is ever invoked with user text.
- `OpenPath`: files, folders, and URLs can be opened via `open <target>`.
  The target must be a path (starting with `/`, `~`, or `.`) or a valid
  http/https URL. Shell metacharacters (backtick, `;`, `|`, `&&`, `||`)
  are rejected. `~` is expanded to `$HOME` before the call.
- `RunTransform`: the Needle model can pick any of the 6 built-in
  transforms (shorten, professional, casual, translate, bullets, rewrite).
  The transform id is validated against a fixed list.
- `Draft`: the model output is inserted into the focused field and is
  **never sent**. There is no send/delete/remove tool in the tool schema.
  Dangerous verbs (send, delete, remove, erase, trash, cancel, undo, quit,
  exit) are refused at the parser level before reaching any model.
- `Transform`: reuses the transform engine's validator; oversized or empty
  output is refused.
- Needle tool calls are untrusted: `validate_tool_call` checks the tool name
  against the advertised set, validates every argument against the schema,
  and rejects anything that does not fit. The allowlist and the tool schema
  are kept in sync by the test suite.
- Delivery gate: the insert is refused if the target app is no longer in
  front (`command_delivery_refusal` → `resolve_delivery`).

### Phase 0 — Speech engine foundation
- `Engine` enum now has a third variant, `Whistle`
  (`crates/teletype-speech/src/catalog.rs`), alongside `Whisper`/`Parakeet`.
- New `ModelCapabilities` struct (streaming, word timestamps, language
  detection, keyword biasing, embeddings, `max_audio_seconds`) with a
  `SpeechModel::capabilities()` method, so the app can reason about what an
  engine can do without branching on the engine name.
- A `Whistle` catalog entry exists (`id: "whistle"`, file `whistle.cactus`)
  and is gated `available: false` until the Cactus runtime + a pinned bundle
  ship. The UI can show it as "coming soon" without offering a broken download.

### Phase 1 — Whistle provider (adapter)
- `crates/teletype-speech/src/whistle.rs` implements `SpeechProvider` for
  Whistle: 16 kHz f32 in, transcript out — the same contract as Parakeet and
  Whisper, so the rest of the app needs no engine-specific branching.
- The documented ~30 s per-inference window is enforced: audio longer than 30 s
  is split into 30 s windows, each transcribed, and the transcripts are joined.
- The FFI is isolated behind a `CactusTransport` trait. The real binding
  (`libcactus_engine.a`) is compiled only under the `cactus` Cargo feature;
  without it the provider degrades to "runtime not bundled". This keeps the
  default build and test suite green on any machine.
- 14 unit tests cover load/transcribe/unload, the 30 s chunking boundary
  (exactly 30 s = 1 call; 61 s = 3 calls), empty audio → NoSpeech,
  transcribe-before-load → NoModel, error mapping, language forwarding, and
  the f32→PCM16 clamp/scale.

## The Cactus FFI (ground truth)

Cactus (`github.com/cactus-compute/cactus`) ships a **static C library**
(`libcactus_engine.a`) with a small, clean FFI. One library backs both STT and
tool-calling, which is why the whole command/agent roadmap rests on one native
boundary:

- STT: `cactus_init`, `cactus_transcribe` (raw 16 kHz PCM16), and
  `cactus_stream_transcribe_{start,process,stop}` for partials.
- Tool calling (Phase 5, Needle): `cactus_complete(..., tools_json, ...)` —
  the same model handle takes tool schemas and returns structured tool calls.
- `cactus_get_last_error()` for diagnostics.

The production transport in `whistle.rs` already calls these; it is feature-gated
so it compiles only once the library is present.

## What is needed to make Whistle downloadable & usable

1. **Vendor the native runtime.** Build `libcactus_engine.a` for macOS arm64
   (and later x86_64 + Windows) and ship it. Options:
   - `brew install cactus-compute/cactus/cactus` for dev, or
   - `cactus build` from source, then point `build.rs` at the build dir via
     `cargo:rustc-link-search=native=...`.
2. **Pin a Whistle bundle.** Confirm the exact Hugging Face repo + `.cactus`
   file Cactus-Compute publishes for Whistle, set `CACTUS_BASE_URL`/`file` and
   the SHA-256 in the catalog, and flip `available: true`.
3. **Enable the feature.** Add `cactus` to the desktop crate's feature list
   (and the release build) so the real transport links in.
4. **Wire the manager.** Add a `whistle: SpeechModelManager<...WhistleProvider>`
   to `AppState`, and route `resolve_speech_model` / `transcribe` by `Engine`
   (the `use_parakeet: bool` is the next thing to replace with the engine).

## Roadmap status

| Phase | Status |
|---|---|
| 0 Architecture (engine enum + capabilities) | Done |
| 1 Whistle provider (adapter + chunking) | Done (adapter); runtime pending |
| 2 Whistle optimization / benchmark | Not started |
| 3 Whistle advanced (biasing, timestamps, embeddings) | Not started |
| 4 Command mode foundation | Not started |
| 5 Needle 3 (tool calling via same FFI) | Not started |
| 6+ MCP, tools, permissions, agent | Not started |

The `cactus_complete` tool-calling path is the natural next milestone after the
runtime is vendored, since it reuses the same model handle and FFI module.
