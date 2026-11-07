# Data-driven updatable model catalog - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three hardcoded Rust model catalogs (speech, LLM, command deciders) with three JSON files in a new `crates/teletype-catalog` crate, loaded at runtime from a data-dir working copy with a shipped-default fallback and a silent GitHub-release update, so adding a model is a data edit that can ship without an app build.

**Architecture:** One new crate owns the entry schema, the engine registry (string name to existing Rust loader), a validator that re-runs the invariants the existing catalog tests assert, and a loader. The speech, LLM, and command code paths read parsed entries instead of `const CATALOG` arrays. A small manifest-driven update path fetches, sha256-verifies, validates, and swaps the working copy only after a full pass. The command-mode gate is a pure function over capabilities. Intent routing already runs through the existing TinyDecide `CombinedDecisionProvider`; the routing work in this plan is a confidence signal plus instrumentation, not a new router.

**Tech Stack:** Rust (workspace, edition 2021, MSRV 1.90), serde/serde_json (already workspace deps), reqwest 0.12 (already in teletype-desktop and teletype-inference), sha2, Tauri v2 data dirs, GitHub releases via the existing `release.yml` pipeline.

**Spec:** `docs/superpowers/specs/2026-11-07-data-driven-model-catalog-design.md`

## Plan corrections log (2026-11-08, lead)

This plan was reconstructed byte-faithful from the session transcript after the original worktree was lost in a repo re-clone, then corrected against the current source (`79524a3`) and the recovered spec. Each correction is a ruling the implementer must honor; cite the ruling id next to any edit it drives.

- **R1 (router already exists):** TinyDecide is already integrated via `CombinedDecisionProvider` (`crates/teletype-inference/src/combined.rs`): `classify_intent` (TinyDecide) then `extract_command` (Needle 3 / LLM fallback), called by the desktop pipeline. The engine lives at `crates/tinydecide` (path dep `../tinydecide` already in teletype-inference), NOT `TheREZOR/TinyDecide/rust/`. Tasks 14-15 are therefore gap-fill: expose a confidence signal and instrument decisions. Do NOT create a `teletype-router` crate and do NOT add a second routing path.
- **R2 (empty domains are valid):** `validate_*` and `Catalog::bundled()` must accept an empty domain (no rows = nothing to violate). The "at least one recommended" rule applies only to a NON-empty speech/llm domain; command is exempt. The "all three domains populated" guarantee is a separate completeness test added in Task 6, not a load-time panic. This unblocks Task 5 going green before Task 6 fills llm/command.
- **R3 (CommandEntry full field set):** Spec line 109-111 requires command entries to carry the display fields the UI reads. `CommandEntry` includes `size_label`, `specialty`, `sha256`, `backup_url`, `attribution`, `license_name`, `license_url`, `requires_license_accept`, `recommended`, `verdict`, `verdict_note`, `sort_order`, `shards` in addition to the base fields. `crates/teletype-desktop/src/commands/models.rs` consumes these for the decider row; dropping them is data loss.
- **R4 (recommended count):** Source has 1 recommended speech row (`parakeet-tdt-v3`) and 3 recommended LLM rows (`s1-mini`, `speakoflow-mini`, `qwen2.5-3b`). The invariant is "at least one recommended per non-empty speech/llm domain, no upper bound", NOT "exactly one". Command has zero recommended and is exempt.
- **R5 (Ivy is the first pure-data add):** The current source has 22 speech rows and NO `ivy` (it was an in-flight addition lost in the re-clone). Spec line 199-202 names Ivy as the first model added under the new system. So Task 5 migrates the 22 rows actually in source; Task 13 adds `ivy` as pure data using the verified artifacts preserved below, and is the acceptance test. Do not migrate ivy in Task 5.
- **R6 (Task 7 consumer ripple):** Re-pointing the speech catalog ripples beyond `catalog.rs`: `teletype-speech/src/lib.rs` + `Cargo.toml`, `teletype-inference/src/qwen3_asr.rs` + `Cargo.toml`, `teletype-desktop/src/dictation.rs` + `commands.rs` + `commands/models.rs`, and `teletype-eval/src/bakeoff.rs` + `driver.rs`. `SpeechModel` becomes a thin wrapper so field reads become method calls (`.file` -> `.file()`, `.english_only` -> `.english_only()`, `.id` -> `.id()`, `.engine` -> `.engine()`), `find()` returns an owned `Option<SpeechModel>`, and the new `Engine::Onnx` variant needs exhaustiveness arms (`unreachable!("Onnx decider is not a dictation engine")`) in dictation.rs and bakeoff.rs. Task 7 enumerates these.
- **R7 (size_mb units):** `size_mb` is decimal MB (1 MB = 1_000_000 bytes) and must satisfy `size_mb * 1_000_000 >= sum(artifact bytes)` (tolerance +1024 bytes). The old Rust rows rounded to MiB, so two values must be re-derived from artifact bytes during migration: `qwen3-asr` 2404 -> 2521 (artifacts sum 2_520_744_288), `granite-speech-470m-q4` 279 -> 280 (sum 279_114_080). Every other row's `size_mb` migrates as-is only if it already satisfies the inequality; otherwise re-derive from bytes.
- **R8 (line numbers refreshed):** Existing-facts line numbers below are verified against `79524a3`, not the lost worktree.
- **R9 (working tree):** Execution happens in `/Users/a.sorathiya/Documents/Ali/teletype-wt/catalog` on branch `feat/data-driven-catalog` (base `79524a3`). The old `teletype-wt/integration` path is gone.

## Global Constraints

These apply to every task. Copy them verbatim; they come from the spec's locked decisions and the project operating rules.

- **Working tree:** `/Users/a.sorathiya/Documents/Ali/teletype-wt/catalog`, branch `feat/data-driven-catalog`. Build/test from here. Use a private `CARGO_TARGET_DIR` per concurrently building task (see the batch PLAN.md); never share one target dir between worktrees that build at the same time.
- **No em/en dashes ever.** This is absolute: UI strings, code comments, docs, and test names use commas or periods instead.
- **English only** everywhere: code, comments, test names, JSON field docs, task notes.
- **Never write secrets into any file.** API keys live in the OS keychain (`com.teletype.app`, `llm:<provider>`). The catalog and the manifest carry only model links, sizes, and sha256s, never credentials.
- **Git safety:** no `git checkout`, `git restore`, `git reset`, `git stash`, `git clean`, or `git commit` with `-A`. Commit by explicit pathspec only. Workers do not commit, push, stash, reset, or checkout unless their task file says so; the lead commits after verifying. Never touch another worker's uncommitted files.
- **Closed engine set.** Engines stay Rust. Data routes a model to an already-implemented engine by name. An unknown engine name is a validation error at load, never a runtime surprise. A genuinely new engine is a separate, evaluated code change and is out of scope for this plan.
- **sha256-only for v1.** The manifest's per-file sha256s stop a corrupted download. A minisign/ed25519 signature over the manifest is a recorded follow-up, not a v1 gate.
- **Update cadence:** check on every launch, throttled to at most once per hour. Launch is the trigger; the throttle avoids hammering the release API on rapid relaunches.
- **Separate command file.** Command deciders live in their own `command.json`, not folded into `llm.json`.
- **Preserve the existing test coverage.** The catalog tests (43 in `teletype-speech/src/catalog.rs`, 18 in `teletype-inference/src/catalog.rs`) are re-pointed at parsed data, not deleted. A bad entry must fail the build, not the user.
- **No data loss in migration.** Every current model row moves to JSON field for field, including the multi-file/shard shapes, the Core ML repo-shape, and the OS-managed Apple-speech shape.
- **The app always has a working catalog.** A failed or absent update keeps the last good working copy; a missing working copy falls back to the bundled shipped defaults. The app never reads a catalog that failed validation. An empty domain is valid (R2); completeness is asserted by a dedicated test.
- **size_mb is decimal MB (R7).** `size_mb * 1_000_000 >= sum(artifact bytes)` within +1024 bytes tolerance.

## Existing facts the plan builds on (verified against 79524a3, do not re-derive)

- Workspace members (from `Cargo.toml`): `parakeet-sys`, `needle-sys`, `parakeet-cpp-sys`, `teletype-core`, `teletype-speech`, `teletype-fluid`, `teletype-apple`, `teletype-inference`, `teletype-desktop`, `teletype-eval`, `teletype-decider`. `crates/tinydecide` and `crates/conformance` exist as path-dep crates but are not workspace members. There is **no** `teletype-catalog` crate yet.
- Speech catalog: `crates/teletype-speech/src/catalog.rs` (2660 lines). `pub enum Engine` at line 27 (variants: `Whisper`, `Parakeet`, `ParakeetCoreml`, `SherpaParakeet`, `ParakeetCpp`, `Qwen3Asr`, `AppleSpeech`, `TranscribeCpp`). `Engine::available_on` at line 60, `Engine::built_on` at line 105. `pub struct SpeechArtifact` at line 169 (`file`, `bytes`, `sha256`). `pub struct SpeechModel` at line 301; its `provider_path` at 414, `install_path` at 433, `url` at 441. `pub const CATALOG: &[SpeechModel]` at line 609 (22 rows). `find(id)` at line 1052 returns `Option<&'static SpeechModel>`.
- The 22 current speech ids: `parakeet-tdt-v3`, `fluid-speech-mini`, `parakeet-tdt-v3-q4`, `parakeet-tdt-v3-f16`, `parakeet-ultra-coreml`, `parakeet-v2-coreml`, `parakeet-v3-coreml`, `parakeet-ctc-110m-coreml`, `sherpa-parakeet-v3-int8`, `sherpa-parakeet-v2-int8`, `large-v3-turbo-q5`, `large-v3-turbo`, `small-en`, `base-en`, `base`, `tiny`, `qwen3-asr`, `apple-speech-transcriber`, `apple-dictation-transcriber`, `nemotron-3.5-asr-streaming-q4`, `moonshine-streaming-medium-q8`, `granite-speech-470m-q4`. Exactly one recommended: `parakeet-tdt-v3`. Exactly one auxiliary: `parakeet-ctc-110m-coreml`. **`ivy` is NOT in source (R5).**
- Speech `SpeechModel` fields: `id`, `name`, `file`, `engine`, `size_mb`, `sha256: Option`, `english_only`, `recommended`, `auxiliary`, `description`, `languages: Option<&[&str]>`, `speed: f64`, `accuracy: f64`, `min_ram_gb`, `base_url: Option`, `purposes: &[Purpose]`.
- LLM catalog: `crates/teletype-inference/src/catalog.rs` (1047 lines). `pub struct CatalogEntry` at line 139. `polish_shortlist_ids(gpu_accelerated)` at line 301. `pub const CATALOG: &[CatalogEntry]` at line 314 (10 rows). `find(id)` at line 587. Three recommended rows: `s1-mini`, `speakoflow-mini`, `qwen2.5-3b` (R4). One decider: `neudecide-q4` (`polish_capable: false` at line 539).
- LLM `CatalogEntry` fields: `id`, `name`, `size_label`, `url`, `size_mb`, `description`, `specialty`, `sha256: Option`, `backup_url: Option`, `attribution: Option`, `license_name: Option`, `license_url: Option`, `requires_license_accept`, `recommended`, `polish_capable`, `runs_on_cpu`, `verdict`, `verdict_note`, `sort_order`, `shards: &[CatalogShard]`.
- `Specialty` consts: `CLEANUP`, `ALL_ROUND`, `INSTRUCT`, `FAST_SMALL`. `ModelVerdict`: `Recommended`, `Mixed` (only `gemma2-2b` is `Mixed`).
- Speech base-URL consts: `WHISPER_BASE_URL` (`https://huggingface.co/ggerganov/whisper.cpp/resolve/main`), `PARAKEET_BASE_URL` (`https://huggingface.co/ggml-org/parakeet-GGUF/resolve/main`), `FLUID_REPO_BASE_URL` (`https://huggingface.co`), `SHERPA_MODELS_BASE_URL` (`https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models`).
- Speech purpose-label vocabulary (12 labels): `Everyday dictation`, `Fastest`, `Most accurate`, `Multilingual`, `English only`, `Live streaming`, `Context-aware`, `Low memory`, `No download`, `Long recordings`, `Indian languages`, `Experimental`.
- Intent routing already integrated (R1): `crates/teletype-inference/src/combined.rs` `CombinedDecisionProvider`; `crates/teletype-inference/src/tinydecide.rs` `TinyDecideProvider` (`classify_intent` at line 427); engine at `crates/tinydecide`. `DictationTrace` is at `crates/teletype-desktop/src/dictation_stages.rs:127`; the trace already carries `decider`, `decider_ms`, `tool`, `args_source` (see `dictation.rs:2309`, `:3227`).
- Command decider selection: `crates/teletype-desktop/src/command_mode.rs`. The `DecisionProvider` (TinyDecide + Needle 3 / LLM) is the fallback extractor; decider availability is gated by model presence on disk.
- HTTP client already available: `reqwest 0.12` with `rustls-tls`, `blocking`, `json` in both `teletype-desktop` and `teletype-inference`.
- Tauri `identifier` is `com.teletype.app`. Data dir is resolved in `crates/teletype-desktop/src/lib.rs` (`app.path().app_data_dir()`, with `TELETYPE_E2E_DATA_DIR` override; `models_dir = data_dir.join("models")`). The catalog working copy lives under this data dir at `catalogs/`.
- Release pipeline: `.github/workflows/release.yml` builds and publishes macOS and Windows releases. It is the host for the catalog manifest + files as release assets.
- UI tooling: `ui/node_modules` is absent in the checkout; installing in a fresh worktree is banned. Task 10's component-test gate is reported as not-runnable unless node_modules is present; live verification is deferred to a human-reviewed morning check.

## Ivy verified artifacts (preserved for Task 13, R5)

- main: `ivy-Q8_0.gguf`, 1_834_422_208 bytes, sha256 `da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed`
- mmproj: `mmproj-ivy-f16.gguf`, 641_774_016 bytes, sha256 `07ed1cc9c96c19aba84354b9135c69332747a98d50363a321bf1418aececfc00`
- engine `llama-server`, base_url `https://huggingface.co/raj7676/ivy-dictation/resolve/main`, english_only true, languages `["en"]`, capabilities `["asr"]`, purposes `["English only" (inferred), "Experimental" (inferred)]`, size_mb 2477 (sum 2_476_196_224 bytes -> ceil to MB), runs_on `["macos"]`, recommended false, auxiliary false.

---

## File structure

New crate `crates/teletype-catalog/`:

- `Cargo.toml` - crate manifest. Deps: `serde`, `serde_json`, `thiserror`, `tracing`, `sha2`.
- `src/lib.rs` - re-exports the public API (schema types, `Catalog`, `Engine`, registry, `load`, `validate`, `update`, `gate`) and `CatalogError`.
- `src/schema.rs` - the entry structs: `Artifact`, `Purpose`, `SpeechEntry`, `LlmEntry`, `CommandEntry`, `Capability`, plus the `Catalog` container.
- `src/engine.rs` - the engine registry: `Engine::from_name`, `name`, `available_on`, `built_on`, `label`, `platform_label`. The existing `Engine` enum re-keyed by a stable string name, plus `Onnx`. `Platform` moves here (re-exported from teletype-speech) so the catalog crate has no speech dependency.
- `src/validate.rs` - `validate_speech`, `validate_llm`, `validate_command`, `validate_catalog`, and shared invariant helpers. One API used by both CI and the update path. Empty domains are valid (R2).
- `src/loader.rs` - read a catalog from a directory (working copy) or the bundled default; `Catalog` accessor methods.
- `src/gate.rs` - the command-mode gate pure function (Task 9).
- `src/update.rs` - the silent-update path: fetch manifest, compare version, download changed files, sha256 verify, validate, swap. `last_check` throttle file, `seed_from_bundled`.
- `speech.json`, `llm.json`, `command.json` - the three data files, bundled via `include_str!` as the shipped defaults.

Consumers that change (no new logic, only re-pointed at parsed data):

- `crates/teletype-speech/src/catalog.rs` + `src/lib.rs` + `Cargo.toml` - delete the `const CATALOG` array and per-id artifact helpers; `SpeechModel` becomes a thin wrapper over a parsed `SpeechEntry`; read from `teletype_catalog` (R6).
- `crates/teletype-inference/src/catalog.rs` + `Cargo.toml` - delete the `const CATALOG` array; read from `teletype_catalog`.
- `crates/teletype-inference/src/qwen3_asr.rs` + `Cargo.toml` - artifacts read from the entry (R6).
- `crates/teletype-desktop/src/dictation.rs`, `src/commands.rs`, `src/commands/models.rs` - field-to-method ripple, `Engine::Onnx` arms, decider display fields from `command.json` (R6, R3).
- `crates/teletype-eval/src/bakeoff.rs`, `src/driver.rs` - owned `SpeechModel`, method calls, `Engine::Onnx` arm (R6).
- `crates/teletype-desktop/src/command_mode.rs` - reads the command-mode gate from capabilities; routing confidence + instrumentation (R1).

No new router crate (R1). The TinyDecide engine stays at `crates/tinydecide`.

---

### Task 1: Scaffold `crates/teletype-catalog` + entry schema

**Files:**
- Create: `crates/teletype-catalog/Cargo.toml`
- Create: `crates/teletype-catalog/src/lib.rs`
- Create: `crates/teletype-catalog/src/schema.rs`
- Modify: `Cargo.toml` (add `"crates/teletype-catalog"` to `members`)
- Test: `crates/teletype-catalog/tests/schema_roundtrip.rs`

**Interfaces:**
- Consumes: nothing (foundational).
- Produces: `Capability` enum (`Asr`, `Polish`, `ToolCalling`, `Omni`); `Artifact { file: String, bytes: u64, sha256: Option<String> }`; `Purpose { label: String, basis: String, evidence: Option<String> }`; `SpeechEntry`, `LlmEntry`, `CommandEntry` structs with serde derive; `Catalog { speech: Vec<SpeechEntry>, llm: Vec<LlmEntry>, command: Vec<CommandEntry> }`.

The schema is the union of the two existing structs plus the capability set, field for field so nothing is lost.

`SpeechEntry` (mirrors `SpeechModel`):
- `id: String`, `name: String`, `engine: String` (registry name), `capabilities: Vec<Capability>`, `languages: Option<Vec<String>>`, `english_only: bool`, `artifacts: Vec<Artifact>`, `base_url: Option<String>`, `size_mb: u32`, `min_ram_gb: u32`, `speed: f64`, `accuracy: f64`, `recommended: bool`, `auxiliary: bool`, `runs_on: Vec<String>`, `purposes: Vec<Purpose>`, `description: String`.
- The single-file / two-file / shard shapes all collapse into `artifacts: Vec<Artifact>`. `sha256` is `None` only for the Core ML repo shape and the OS-managed Apple-speech shape (whose `artifacts` is empty).

`LlmEntry` (mirrors `CatalogEntry`):
- `id`, `name`, `size_label`, `capabilities: Vec<Capability>` (replaces `polish_capable`; a polish row carries `["polish"]`), `size_mb`, `description`, `specialty`, `sha256: Option<String>`, `backup_url: Option<String>`, `attribution: Option<String>`, `license_name: Option<String>`, `license_url: Option<String>`, `requires_license_accept: bool`, `recommended: bool`, `runs_on_cpu: bool`, `verdict`, `verdict_note`, `sort_order`, `shards: Vec<Artifact>`, `runs_on: Vec<String>`.

`CommandEntry` (R3, mirrors the decider row with its FULL display field set):
- `id`, `name`, `engine: String`, `capabilities: Vec<Capability>` (always includes `tool_calling`), `artifacts: Vec<Artifact>`, `base_url: Option<String>`, `size_mb`, `description`, `runs_on: Vec<String>`, `runs_on_cpu: bool`, `size_label`, `specialty`, `sha256: Option<String>`, `backup_url: Option<String>`, `attribution: Option<String>`, `license_name: Option<String>`, `license_url: Option<String>`, `requires_license_accept: bool`, `recommended: bool`, `verdict`, `verdict_note`, `sort_order`, `shards: Vec<Artifact>`.

- [ ] **Step 1: Write the failing roundtrip test**

```rust
// crates/teletype-catalog/tests/schema_roundtrip.rs
use teletype_catalog::schema::{Capability, SpeechEntry};

#[test]
fn speech_entry_roundtrips_through_json() {
    let json = r#"{
      "id": "qwen3-asr", "name": "Qwen3-ASR 1.7B", "engine": "llama-server",
      "capabilities": ["asr"], "languages": ["en", "de", "pl"], "english_only": false,
      "artifacts": [
        {"file": "Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 2165034944,
         "sha256": "58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57"},
        {"file": "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 355709344,
         "sha256": "46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d"}
      ],
      "base_url": "https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main",
      "size_mb": 2521, "min_ram_gb": 4, "speed": 6.0, "accuracy": 9.0,
      "recommended": false, "auxiliary": false, "runs_on": ["macos"],
      "purposes": [{"label": "Multilingual", "basis": "inferred", "evidence": null}],
      "description": "..."
    }"#;
    let e: SpeechEntry = serde_json::from_str(json).unwrap();
    assert_eq!(e.engine, "llama-server");
    assert!(e.capabilities.contains(&Capability::Asr));
    assert_eq!(e.artifacts.len(), 2);
    assert_eq!(e.artifacts[0].bytes, 2_165_034_944);
    let back = serde_json::to_string(&e).unwrap();
    let again: SpeechEntry = serde_json::from_str(&back).unwrap();
    assert_eq!(again.id, e.id);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test schema_roundtrip`
Expected: FAIL, `cannot find crate teletype_catalog`.

- [ ] **Step 3: Write the crate manifest + schema**

`crates/teletype-catalog/Cargo.toml`:
```toml
[package]
name = "teletype-catalog"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
sha2 = "0.10"
```

Add `"crates/teletype-catalog"` to the `members` array in the root `Cargo.toml` (right after `"crates/teletype-core"`).

`src/schema.rs`: define `Capability`, `Artifact`, `Purpose`, `SpeechEntry`, `LlmEntry`, `CommandEntry`, `Catalog` with `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`. Use `#[serde(rename_all = "camelCase")]` on the entry structs to match the IPC convention. `Capability` is `#[serde(rename_all = "snake_case")]` so JSON reads `asr`, `polish`, `tool_calling`, `omni`. `Catalog` uses field names `speech`, `llm`, `command` (the JSON files are arrays; the loader assembles the container, so `Catalog` itself need not derive a single-file shape).

`src/lib.rs`: `pub mod schema;` and `pub use schema::*;`. Define `CatalogError` here with `thiserror`: `UnknownEngine(String)`, `InvalidJson(String)`, `Validation(String)`, `Io(String)`, `Network(String)`, `HashMismatch(String)`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test schema_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit (lead commits after verification; worker leaves changes staged-by-pathspec)**

```bash
git add Cargo.toml crates/teletype-catalog/
git commit -m "feat(catalog): scaffold teletype-catalog crate with entry schema"
```

---

### Task 2: Engine registry + Platform move

**Files:**
- Create: `crates/teletype-catalog/src/engine.rs`
- Modify: `crates/teletype-catalog/src/lib.rs`
- Modify: `crates/teletype-speech/src/catalog.rs` (re-export `Platform` from teletype-catalog; keep `Engine` for now until Task 7)
- Modify: `crates/teletype-speech/Cargo.toml` (add `teletype-catalog` dep)
- Test: `crates/teletype-catalog/tests/engine_registry.rs`

**Interfaces:**
- Consumes: `Capability`, `CatalogError` (Task 1).
- Produces: `Engine` enum with a stable `name` per variant; `Engine::from_name(&str) -> Result<Engine, CatalogError>`; `name`, `available_on(platform)`, `built_on(platform, parakeet_cpp_available, transcribe_cpp_available)`, `label`, `platform_label`. `Platform` is DEFINED in `teletype-catalog::engine` and re-exported at `teletype_speech::catalog::Platform` (move it out of teletype-speech so the catalog crate has no speech dependency).

The registry is the existing `Engine` enum re-keyed by string. Names map to variants:
- `whisper` -> `Whisper`, `parakeet` -> `Parakeet`, `parakeet-coreml` -> `ParakeetCoreml`, `sherpa` -> `SherpaParakeet`, `parakeet-cpp` -> `ParakeetCpp`, `llama-server` -> `Qwen3Asr`, `apple-speech` -> `AppleSpeech`, `transcribe-cpp` -> `TranscribeCpp`, `onnx` -> `Onnx` (the NeuDecide command decider; no speech provider, never offered as a speech row).

`available_on` and `built_on` copy the current match arms from `crates/teletype-speech/src/catalog.rs` lines 60-104 verbatim. `label`/`platform_label` copy the current label logic. Add `Onnx` arms: `available_on` returns `true`, `built_on` returns `true`, `label` returns `"ONNX decider"`, `platform_label` returns `"any"`. The logic does not change; it is keyed by string and lives in the catalog crate so the validator can reject unknown names.

- [ ] **Step 1: Write the failing test**

```rust
// crates/teletype-catalog/tests/engine_registry.rs
use teletype_catalog::engine::Engine;

#[test]
fn known_names_resolve_to_engines() {
    assert_eq!(Engine::from_name("llama-server").unwrap(), Engine::Qwen3Asr);
    assert_eq!(Engine::from_name("whisper").unwrap(), Engine::Whisper);
    assert_eq!(Engine::from_name("parakeet-coreml").unwrap(), Engine::ParakeetCoreml);
    assert_eq!(Engine::from_name("onnx").unwrap(), Engine::Onnx);
}

#[test]
fn unknown_name_is_an_error_not_a_variant() {
    assert!(Engine::from_name("no-such-engine").is_err());
    assert!(Engine::from_name("").is_err());
}

#[test]
fn name_roundtrips() {
    for (name, e) in [("whisper", Engine::Whisper), ("llama-server", Engine::Qwen3Asr)] {
        assert_eq!(Engine::from_name(name).unwrap().name(), name);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test engine_registry`
Expected: FAIL, `could not find engine in crate teletype_catalog`.

- [ ] **Step 3: Write the registry + move Platform**

`src/engine.rs`: define `Platform` (`Macos`, `Windows`, `Linux`) and `Engine` with the eight variants plus `Onnx`. Implement `from_name` (`match` returning `Result`, `_ => Err(CatalogError::UnknownEngine(name))`), `name`, and copy `available_on`, `built_on`, `label`, `platform_label` from the speech catalog. In `teletype-speech`, replace the local `Platform` definition with `pub use teletype_catalog::engine::Platform;` and add the `teletype-catalog` path dep. Keep the speech `Engine` enum until Task 7 removes it.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test engine_registry && cargo test -p teletype-speech catalog`
Expected: PASS (speech still compiles with the re-exported Platform).

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/ crates/teletype-speech/
git commit -m "feat(catalog): engine registry keyed by string; move Platform into catalog"
```

---

### Task 3: Validator (the invariants, run by CI and the update path)

**Files:**
- Create: `crates/teletype-catalog/src/validate.rs`
- Modify: `crates/teletype-catalog/src/lib.rs`
- Test: `crates/teletype-catalog/tests/validate.rs`

**Interfaces:**
- Consumes: schema (Task 1), `Engine::from_name` + `CatalogError` (Task 2).
- Produces: `validate_speech(&[SpeechEntry]) -> Result<(), CatalogError>`, `validate_llm(&[LlmEntry])`, `validate_command(&[CommandEntry])`, `validate_catalog(&Catalog)` calling all three.

Invariants (each pins an existing test, now against data). **R2: an empty slice is VALID for every domain.** **R4: the recommended rule is "at least one", not "exactly one".**

1. **Unique ids per domain.** Duplicate id = error.
2. **At least one `recommended` per NON-empty speech/llm domain (R4).** Zero recommended in a non-empty speech or llm set = error. No upper bound (LLM has 3). `validate_command` does NOT enforce a recommended rule (decider set has none). An empty domain skips this rule (R2).
3. **Engine name is known.** Every `engine` string resolves via `Engine::from_name`; unknown = error (closed-engine-set guarantee).
4. **Purpose labels from the fixed 12-label vocabulary.** A label outside it = error.
5. **sha256 is 64-hex or absent.** Each artifact with a `sha256` must be exactly 64 lowercase hex chars. Absent is allowed only for the Core ML repo shape (`parakeet-coreml`) and the OS-managed Apple-speech shape (empty artifacts); any other single/multi-file row must pin every artifact.
6. **Languages agree with `english_only`.** If `english_only` is true, `languages` must be `["en"]` alone. Contradiction = error.
7. **`size_mb` covers artifact bytes (R7).** `size_mb * 1_000_000 + 1024 >= sum(artifacts.bytes)`. Below = error.
8. **`runs_on` non-empty and known platforms** (`macos`, `windows`, `linux`) for every row.

- [ ] **Step 1: Write the failing tests (one per invariant)**

```rust
// crates/teletype-catalog/tests/validate.rs
use teletype_catalog::validate::*;
use teletype_catalog::schema::{Artifact, SpeechEntry, Capability, Purpose};

fn base_speech(id: &str) -> SpeechEntry {
    SpeechEntry {
        id: id.into(), name: format!("Model {id}"), engine: "whisper".into(),
        capabilities: vec![Capability::Asr], languages: Some(vec!["en".into(), "de".into()]),
        english_only: false,
        artifacts: vec![Artifact { file: "a.gguf".into(), bytes: 1_000_000, sha256: Some("a".repeat(64)) }],
        base_url: None, size_mb: 1, min_ram_gb: 2, speed: 5.0, accuracy: 5.0,
        recommended: true, auxiliary: false, runs_on: vec!["macos".into()],
        purposes: vec![Purpose { label: "Multilingual".into(), basis: "inferred".into(), evidence: None }],
        description: "d".into(),
    }
}

#[test] fn a_good_speech_set_passes() { assert!(validate_speech(&[base_speech("a")]).is_ok()); }
#[test] fn empty_domain_is_valid() { assert!(validate_speech(&[]).is_ok()); }
#[test] fn duplicate_ids_fail() {
    let mut b = base_speech("a"); b.recommended = false;
    assert!(validate_speech(&[base_speech("a"), b]).is_err());
}
#[test] fn zero_recommended_in_nonempty_speech_fails() {
    let mut e = base_speech("a"); e.recommended = false;
    assert!(validate_speech(&[e]).is_err());
}
#[test] fn multiple_recommended_is_allowed() {
    assert!(validate_speech(&[base_speech("a"), base_speech("b")]).is_ok());
}
#[test] fn unknown_engine_fails() {
    let mut e = base_speech("a"); e.engine = "nope".into();
    assert!(validate_speech(&[e]).is_err());
}
#[test] fn bad_purpose_label_fails() {
    let mut e = base_speech("a");
    e.purposes = vec![Purpose { label: "Made-up".into(), basis: "inferred".into(), evidence: None }];
    assert!(validate_speech(&[e]).is_err());
}
#[test] fn english_only_with_other_languages_fails() {
    let mut e = base_speech("a"); e.english_only = true; e.languages = Some(vec!["en".into(), "de".into()]);
    assert!(validate_speech(&[e]).is_err());
}
#[test] fn size_mb_below_artifact_sum_fails() {
    let mut e = base_speech("a"); e.size_mb = 0;
    assert!(validate_speech(&[e]).is_err());
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p teletype-catalog --test validate`
Expected: FAIL, `could not find validate in crate teletype_catalog`.

- [ ] **Step 3: Write the validator**

`src/validate.rs`: implement the eight invariants. Return `Err(CatalogError::Validation(msg))` naming the offending id and invariant. Helper `is_64_hex(s) -> bool` (len 64, all `[0-9a-f]`). Recommended check: for non-empty speech/llm, error if count == 0; no upper bound; command exempt; empty domain skips (R2). Wire `pub mod validate;` in lib.rs.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p teletype-catalog --test validate`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/
git commit -m "feat(catalog): validator re-running the catalog invariants"
```

---

### Task 4: Loader (bundled defaults + working copy) + `Catalog` accessors

**Files:**
- Create: `crates/teletype-catalog/src/loader.rs`
- Create: `crates/teletype-catalog/speech.json`, `llm.json`, `command.json` (empty arrays `[]` for now)
- Modify: `crates/teletype-catalog/src/lib.rs`
- Test: `crates/teletype-catalog/tests/loader.rs`

**Interfaces:**
- Consumes: schema (Task 1), validator (Task 3).
- Produces: `Catalog::load_from_dir(dir: &Path) -> Result<Catalog, CatalogError>`; `Catalog::bundled() -> Catalog` (parses three `include_str!` files, validates; empty domains are valid per R2 so this does not panic before Tasks 5/6); `speech()`, `llm()`, `command()`; `speech_by_id`, `llm_by_id`, `command_by_id`; `speech_offered_on(entry, platform, fluid_available, parakeet_cpp_available) -> bool`; `speech_provider_path(entry, models_dir) -> PathBuf`; `speech_url(entry) -> String`; `speech_install_path(entry, models_dir) -> PathBuf`.

The loader parses JSON and exposes the predicates the old `SpeechModel` impl methods provided. `speech_provider_path`/`speech_url`/`speech_install_path` reproduce the existing `provider_path`/`url`/`install_path` behavior (catalog.rs lines 414-470) by reading `entry.artifacts`, `entry.base_url`, `entry.engine`. For the llama-server pair the provider dir is `models_dir.join(&entry.id)`. For the Core ML shape the provider path is `models_dir.join("fluid")` and the URL uses `FLUID_REPO_BASE_URL`. Base-URL defaults by engine: `WHISPER_BASE_URL`, `PARAKEET_BASE_URL`, `SHERPA_MODELS_BASE_URL`, `FLUID_REPO_BASE_URL` (copy the four consts from the speech catalog into loader.rs).

`load_from_dir` refuses to return a catalog if any file is missing or fails validation; the caller falls back to `bundled()` on error.

- [ ] **Step 1: Write the failing test**

```rust
// crates/teletype-catalog/tests/loader.rs
use teletype_catalog::Catalog;

#[test]
fn bundled_catalog_loads_with_empty_domains() {
    let c = Catalog::bundled();
    assert!(c.speech_by_id("qwen3-asr").is_none()); // still empty until Task 5
}

#[test]
fn load_from_missing_dir_is_an_error() {
    assert!(Catalog::load_from_dir(std::path::Path::new("/nonexistent/teletype-catalog")).is_err());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test loader`
Expected: FAIL, `no function or associated item named bundled`.

- [ ] **Step 3: Write the loader + empty JSON files**

Create `speech.json`, `llm.json`, `command.json` each containing `[]` so `include_str!` compiles. `src/loader.rs`: `impl Catalog { pub fn bundled() -> Self { parse include_str!; validate_catalog; } pub fn load_from_dir(...) }` plus accessors. Wire the four path/url/offered helpers to mirror the speech-catalog methods.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test loader`
Expected: PASS (empty domains validate per R2).

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/
git commit -m "feat(catalog): loader with bundled defaults and working-copy load"
```

---

### Task 5: Populate `speech.json` with the 22 current speech models (R5)

**Files:**
- Modify: `crates/teletype-catalog/speech.json`
- Test: `crates/teletype-catalog/tests/speech_data.rs`

**Interfaces:**
- Consumes: schema + validator + loader (Tasks 1, 3, 4).
- Produces: a complete, valid `speech.json` containing every current source row, field for field. **Ivy is NOT migrated here (R5); it is added in Task 13.**

The 22 current speech ids are listed in Existing facts. Migrate each with its real field values read from `crates/teletype-speech/src/catalog.rs` `const CATALOG` (line 609).

Migration rules (no data loss):
- `engine` is the registry string for the row's `Engine` variant (`Qwen3Asr` -> `llama-server`, `ParakeetCoreml` -> `parakeet-coreml`, `SherpaParakeet` -> `sherpa`, `TranscribeCpp` -> `transcribe-cpp`, `AppleSpeech` -> `apple-speech`, `Whisper` -> `whisper`, `Parakeet` -> `parakeet`, `ParakeetCpp` -> `parakeet-cpp`).
- `capabilities` is `["asr"]` for every speech row.
- `artifacts` from the row's file + sha256. The two-file llama-server row `qwen3-asr` uses both artifacts from `QWEN3_ASR_ARTIFACTS`. The pinned streaming rows use `PINNED_SPEECH_ARTIFACTS`. Core ML rows: `artifacts: []` (repo shape, no sha256). Apple-speech rows: `artifacts: []` (OS-managed).
- `base_url` is the row's override, or `null` to use the engine default.
- `runs_on` from the engine's `available_on`: `["macos"]` for macOS-only engines, `["macos","windows","linux"]` for `sherpa`.
- `purposes` migrate with `label`, `basis` (`measured`/`inferred`), `evidence`.
- **size_mb (R7):** re-derive from artifact bytes where the source MiB value fails the inequality: `qwen3-asr` -> 2521, `granite-speech-470m-q4` -> 280. Verify every row satisfies `size_mb * 1_000_000 + 1024 >= sum(bytes)`.

- [ ] **Step 1: Write the failing data test**

```rust
// crates/teletype-catalog/tests/speech_data.rs
use teletype_catalog::Catalog;

#[test]
fn speech_json_has_all_22_models() {
    let c = Catalog::bundled();
    let expected = [
        "parakeet-tdt-v3", "fluid-speech-mini", "parakeet-tdt-v3-q4", "parakeet-tdt-v3-f16",
        "parakeet-ultra-coreml", "parakeet-v2-coreml", "parakeet-v3-coreml", "parakeet-ctc-110m-coreml",
        "sherpa-parakeet-v3-int8", "sherpa-parakeet-v2-int8", "large-v3-turbo-q5", "large-v3-turbo",
        "small-en", "base-en", "base", "tiny", "qwen3-asr", "apple-speech-transcriber",
        "apple-dictation-transcriber", "nemotron-3.5-asr-streaming-q4", "moonshine-streaming-medium-q8",
        "granite-speech-470m-q4",
    ];
    assert_eq!(c.speech().len(), expected.len());
    for id in expected { assert!(c.speech_by_id(id).is_some(), "missing speech row {id}"); }
    assert!(c.speech_by_id("ivy").is_none(), "ivy is added in Task 13, not migrated");
}

#[test]
fn every_speech_row_has_asr_capability_and_a_known_engine() {
    let c = Catalog::bundled();
    for e in c.speech() {
        assert!(e.capabilities.contains(&teletype_catalog::Capability::Asr), "{}", e.id);
        let _ = teletype_catalog::engine::Engine::from_name(&e.engine).unwrap();
    }
}

#[test]
fn qwen3_asr_row_carries_its_two_artifacts_and_corrected_size() {
    let c = Catalog::bundled();
    let q = c.speech_by_id("qwen3-asr").unwrap();
    assert_eq!(q.artifacts.len(), 2);
    assert_eq!(q.size_mb, 2521);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test speech_data`
Expected: FAIL, `speech().len() == 0`.

- [ ] **Step 3: Write `speech.json`**

Populate with all 22 rows, reading each field from the source `const CATALOG`. Data transcription: copy each `SpeechModel` literal's values into the JSON shape and apply the migration rules. Do not invent values; every number, hash, URL, and purpose chip comes from the source row (except the R7 size_mb re-derivations, which come from artifact bytes).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test speech_data && cargo test -p teletype-catalog --test validate`
Expected: PASS. (R2 means bundled() validates even though llm/command are still empty, so this task goes green on its own.)

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/speech.json crates/teletype-catalog/tests/speech_data.rs
git commit -m "feat(catalog): migrate the 22 speech models to speech.json"
```

---

### Task 6: Populate `llm.json` and `command.json` + completeness test

**Files:**
- Modify: `crates/teletype-catalog/llm.json`, `command.json`
- Test: `crates/teletype-catalog/tests/llm_command_data.rs`

**Interfaces:**
- Consumes: schema + validator + loader (Tasks 1, 3, 4).
- Produces: complete, valid `llm.json` (9 polish rows) and `command.json` (1 decider row, full display fields per R3); a completeness test asserting all three domains populated.

The 10 current LLM ids: `s1-mini`, `qwen3.5-2b`, `speakoflow-mini`, `qwen2.5-3b`, `fast`, `qwen3-0.6b`, `quality`, `qwen2.5-7b`, `gemma2-2b`, `neudecide-q4`.

Migration rules:
- `polish_capable: true` rows -> `llm.json` with `capabilities: ["polish"]` (9 rows).
- `polish_capable: false` rows -> `command.json` with `capabilities: ["tool_calling"]` (the decider `neudecide-q4`), carrying the FULL display field set (R3): `size_label`, `specialty`, `sha256`, `backup_url`, `attribution`, `license_name`, `license_url`, `requires_license_accept`, `recommended`, `verdict`, `verdict_note`, `sort_order`, `shards`. The `neudecide-q4` shards (6 files) migrate from the source `CatalogShard` array with their bytes + sha256; base_url `https://github.com/alihusains/teletype-models/releases/download/neudecide-v1-q4/`; engine `onnx`; size_mb 41.
- `specialty` maps to the const name lowercased (`CLEANUP` -> `cleanup`, `ALL_ROUND` -> `all_round`, `INSTRUCT` -> `instruct`, `FAST_SMALL` -> `fast_small`).
- `verdict` maps `Recommended` -> `recommended`, `Mixed` -> `mixed` (only `gemma2-2b` is `mixed`).
- `sort_order`, `runs_on_cpu`, `size_label`, license/attribution/backup fields migrate verbatim. `runs_on` for polish rows is `["macos","windows"]` per source `runs_on_host`.
- Three recommended LLM rows migrate with `recommended: true` (R4); the validator already allows this.

- [ ] **Step 1: Write the failing data test**

```rust
// crates/teletype-catalog/tests/llm_command_data.rs
use teletype_catalog::Catalog;

#[test]
fn llm_json_has_the_polish_rows() {
    let c = Catalog::bundled();
    for id in ["s1-mini", "qwen3.5-2b", "speakoflow-mini", "qwen2.5-3b", "fast",
               "qwen3-0.6b", "quality", "qwen2.5-7b", "gemma2-2b"] {
        let e = c.llm_by_id(id).unwrap_or_else(|| panic!("missing llm row {id}"));
        assert!(e.capabilities.contains(&teletype_catalog::Capability::Polish), "{id}");
    }
    assert!(c.llm_by_id("neudecide-q4").is_none());
}

#[test]
fn command_json_has_the_decider_with_display_fields() {
    let c = Catalog::bundled();
    let d = c.command_by_id("neudecide-q4").unwrap_or_else(|| panic!("missing neudecide-q4"));
    assert!(d.capabilities.contains(&teletype_catalog::Capability::ToolCalling));
    assert_eq!(d.engine, "onnx");
    assert_eq!(d.shards.len(), 6);
    assert!(!d.specialty.is_empty(), "decider display fields must survive (R3)");
}

#[test]
fn bundled_catalog_is_complete() {
    let c = Catalog::bundled();
    assert_eq!(c.speech().len(), 22);
    assert_eq!(c.llm().len(), 9);
    assert_eq!(c.command().len(), 1);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test llm_command_data`
Expected: FAIL, files still `[]`.

- [ ] **Step 3: Write `llm.json` and `command.json`**

Transcribe the 10 rows from `crates/teletype-inference/src/catalog.rs` `const CATALOG` (line 314), splitting polish rows into `llm.json` and the decider into `command.json` with all R3 fields.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog`
Expected: PASS across the whole crate (schema, engine, validate, loader, speech, llm/command, completeness).

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/llm.json crates/teletype-catalog/command.json crates/teletype-catalog/tests/llm_command_data.rs
git commit -m "feat(catalog): migrate LLM polish rows and command decider to JSON"
```

---

### Task 7: Re-point the speech catalog at parsed data; delete the Rust literals (R6)

**Files:**
- Modify: `crates/teletype-speech/src/catalog.rs`, `src/lib.rs`, `Cargo.toml`
- Modify: `crates/teletype-inference/src/qwen3_asr.rs`, `Cargo.toml`
- Modify: `crates/teletype-desktop/src/dictation.rs`, `src/commands.rs`, `src/commands/models.rs`
- Modify: `crates/teletype-eval/src/bakeoff.rs`, `src/driver.rs`
- Test: the 43 existing `#[test]` fns in `crates/teletype-speech/src/catalog.rs`

**Interfaces:**
- Consumes: `teletype_catalog::Catalog` + accessors (Tasks 1, 4).
- Produces: `speech_model_list`, `recommended_speech_model_id`, and `SpeechModel`-shaped access backed by a parsed `Catalog`. `SpeechModel` is a thin wrapper `{ entry: teletype_catalog::SpeechEntry }` with `from_entry` and accessor METHODS (`engine()`, `file()`, `id()`, `name()`, `size_mb()`, `english_only()`, `auxiliary()`, `artifacts()`); `find()` returns owned `Option<SpeechModel>`; `catalog()` fn replaces `const CATALOG`.

Approach:
1. Add `teletype-catalog = { path = "../teletype-catalog" }` to teletype-speech (already added in Task 2 for Platform; confirm).
2. Make `SpeechModel` a wrapper over a parsed `SpeechEntry`. Replace `const CATALOG` with a `catalog()` fn building `Vec<SpeechModel>` from `teletype_catalog::Catalog::bundled().speech()`. Every method reads the parsed entry.
3. Delete the per-id artifact helpers (`llama_asr_artifacts_for`, `llama_asr_provider_dir_for`, `qwen3_asr_main_file`, `qwen3_asr_mmproj_file`, `QWEN3_ASR_ARTIFACTS`, `PINNED_SPEECH_ARTIFACTS`) once data lives in speech.json. Consumers switch to `catalog.speech_by_id(id).artifacts`. (Ivy helpers do not exist in source; Task 13 adds ivy as data only.)
4. **Fix the consumer ripple (R6):** field reads become method calls across dictation.rs, commands.rs, commands/models.rs, qwen3_asr.rs, bakeoff.rs, driver.rs; `find()` now returns owned so `.map` sites use `.as_ref()` and `spawn_blocking` sites clone the entry; add `Engine::Onnx` exhaustiveness arms (`unreachable!("Onnx decider is not a dictation engine")`) in dictation.rs and bakeoff.rs; test call sites that passed `&SpeechModel` now pass owned or `.as_ref()` as the new signature requires.
5. **Re-point the 43 tests**, not delete them. Each invariant test reads the parsed catalog; value-specific tests keep asserting the same values against the parsed entry.

The behavioral contract is unchanged: same rows, same predicates, same recommended pick per host. The existing tests are the safety net.

- [ ] **Step 1: Add a smoke test that the parsed catalog drives the list**

```rust
// added to the test module in crates/teletype-speech/src/catalog.rs
#[test]
fn speech_model_list_is_backed_by_the_parsed_catalog() {
    let parsed = teletype_catalog::Catalog::bundled();
    assert_eq!(parsed.speech().len(), catalog().len());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-speech catalog`
Expected: FAIL (list still hardcoded; `catalog()` not yet defined).

- [ ] **Step 3: Re-point the catalog + fix all consumers (R6)**

Change `const CATALOG` to the parsed-backed `catalog()`. Update `speech_model_list`, `recommended_speech_model_id`, and `SpeechModel` methods. Delete per-id helpers/consts. Update every consumer listed in Files. Run `cargo clippy -p teletype-speech -p teletype-inference -p teletype-desktop -p teletype-eval --all-targets` to catch the field-to-method and exhaustiveness ripple (lib-only tests miss integration-test and bench compile errors).

- [ ] **Step 4: Re-point the 43 tests and run the suites**

Run: `cargo test -p teletype-speech && cargo test -p teletype-eval && cargo test -p teletype-desktop`
Expected: PASS. Report honest before/after test counts; grep touched test files for `fn debug_`, `dbg!`, stray `eprintln!`/`println!` and confirm every new test has an assertion.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-speech/ crates/teletype-inference/ crates/teletype-desktop/ crates/teletype-eval/
git commit -m "refactor(speech): drive the speech catalog from teletype-catalog JSON"
```

---

### Task 8: Re-point the LLM catalog + command deciders at parsed data

**Files:**
- Modify: `crates/teletype-inference/src/catalog.rs`, `Cargo.toml`
- Modify: `crates/teletype-desktop/src/command_mode.rs`, `src/commands/models.rs`
- Test: the 18 existing `#[test]` fns in `crates/teletype-inference/src/catalog.rs`

**Interfaces:**
- Consumes: `teletype_catalog::Catalog` (Tasks 1, 4, 6).
- Produces: `find(id)`, `polish_shortlist_ids`, `CatalogEntry`-shaped access backed by `llm.json`; decider rows read from `command.json` with full display fields (R3).

Approach:
1. Add `teletype-catalog` to teletype-inference Cargo.toml.
2. Back `CatalogEntry` access from `Catalog::bundled().llm()`. `find(id)` reads `llm_by_id`. `polish_shortlist_ids` keeps its literal shortlist ids but re-checks each against the parsed entry's `capabilities` (must contain `polish`) and `runs_on_host`.
3. `polish_capable` bool is gone; `capabilities.contains(&Capability::Polish)` replaces it.
4. Decider selection in command_mode.rs reads `Catalog::bundled().command()`. `neudecide-q4` is the default decider; availability still gated by model presence on disk (missing model = `no_action` fallback, unchanged). `commands/models.rs` reads the decider's display fields (specialty, verdict, sort_order, license, attribution) from the `CommandEntry` (R3).
5. **Re-point the 18 tests** to read the parsed catalog.

- [ ] **Step 1: Add a smoke test**

```rust
#[test]
fn llm_find_is_backed_by_the_parsed_catalog() {
    let parsed = teletype_catalog::Catalog::bundled();
    assert!(find("s1-mini").is_some());
    assert!(parsed.llm_by_id("s1-mini").is_some());
    assert!(parsed.llm_by_id("neudecide-q4").is_none());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-inference catalog`
Expected: FAIL.

- [ ] **Step 3: Re-point the LLM catalog + deciders**

Back `find`, `polish_shortlist_ids`, `CatalogEntry` access from parsed `llm.json`. Replace `polish_capable` branches with capability checks. Point command_mode.rs + models.rs at `command.json`. Delete the hardcoded `const CATALOG`. Run `cargo clippy -p teletype-inference -p teletype-desktop --all-targets`.

- [ ] **Step 4: Re-point the 18 tests and run the suites**

Run: `cargo test -p teletype-inference && cargo test -p teletype-desktop command_mode`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-inference/ crates/teletype-desktop/src/command_mode.rs crates/teletype-desktop/src/commands/models.rs
git commit -m "refactor(inference): drive LLM and command catalogs from teletype-catalog JSON"
```

---

### Task 9: Command-mode gate function

**Files:**
- Create: `crates/teletype-catalog/src/gate.rs`
- Modify: `crates/teletype-catalog/src/lib.rs`
- Test: `crates/teletype-catalog/tests/gate.rs`

**Interfaces:**
- Consumes: `LlmEntry`, `SpeechEntry`, `Capability` (Task 1).
- Produces: `CommandModeStatus` enum and `command_mode_available(selected_llm: &LlmEntry, selected_asr: &SpeechEntry, asr_usable: bool) -> CommandModeStatus`.

The gate is a pure function: available iff the selected LLM has `tool_calling` AND the selected ASR is usable on this host. The return is an enum so the UI can name the missing part:

```rust
pub enum CommandModeStatus {
    Available,
    MissingToolCallingLlm,
    MissingUsableAsr,
    MissingBoth,
}
```

`asr_usable` is passed in (caller resolves from `speech_offered_on` + installed state) so the gate stays pure and host-free.

- [ ] **Step 1: Write the failing table test**

```rust
// crates/teletype-catalog/tests/gate.rs
use teletype_catalog::gate::*;
use teletype_catalog::schema::{LlmEntry, SpeechEntry, Capability};

// Build minimal fixtures from the schema (fill every required field; no todo!()).
fn llm(tool_calling: bool) -> LlmEntry {
    let caps = if tool_calling { vec![Capability::ToolCalling] } else { vec![Capability::Polish] };
    LlmEntry {
        id: "l".into(), name: "L".into(), size_label: "1 GB".into(), capabilities: caps,
        size_mb: 1, description: "d".into(), specialty: "cleanup".into(), sha256: None,
        backup_url: None, attribution: None, license_name: None, license_url: None,
        requires_license_accept: false, recommended: true, runs_on_cpu: true,
        verdict: "recommended".into(), verdict_note: String::new(), sort_order: 1,
        shards: vec![], runs_on: vec!["macos".into()],
    }
}
fn asr() -> SpeechEntry {
    SpeechEntry {
        id: "a".into(), name: "A".into(), engine: "whisper".into(), capabilities: vec![Capability::Asr],
        languages: Some(vec!["en".into()]), english_only: true, artifacts: vec![], base_url: None,
        size_mb: 1, min_ram_gb: 1, speed: 5.0, accuracy: 5.0, recommended: true, auxiliary: false,
        runs_on: vec!["macos".into()], purposes: vec![], description: "d".into(),
    }
}

#[test]
fn gate_is_the_conjunction_of_the_two_parts() {
    assert!(matches!(command_mode_available(&llm(true), &asr(), true), CommandModeStatus::Available));
    assert!(matches!(command_mode_available(&llm(false), &asr(), true), CommandModeStatus::MissingToolCallingLlm));
    assert!(matches!(command_mode_available(&llm(true), &asr(), false), CommandModeStatus::MissingUsableAsr));
    assert!(matches!(command_mode_available(&llm(false), &asr(), false), CommandModeStatus::MissingBoth));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test gate`
Expected: FAIL, `could not find gate`.

- [ ] **Step 3: Write the gate**

`src/gate.rs`: implement `CommandModeStatus` and `command_mode_available` (the conjunction mapped to four variants). Wire `pub mod gate;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test gate`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/src/gate.rs crates/teletype-catalog/src/lib.rs crates/teletype-catalog/tests/gate.rs
git commit -m "feat(catalog): command-mode gate as a pure capability function"
```

---

### Task 10: Command-mode UI message (which part is missing)

**Files:**
- Modify: `ui/` (the Settings / model-picker component that gates the Command Mode chip)
- Modify: `crates/teletype-desktop/src/commands/models.rs` (expose `CommandModeStatus` via IPC)
- Test: component test if node_modules is present; otherwise reported not-runnable + morning live check.

**Interfaces:**
- Consumes: `CommandModeStatus` (Task 9).
- Produces: the Command Mode chip shows only when `Available`; otherwise a plain message names the missing part.

The model-picker IPC response gains a `commandModeStatus` field (camelCase). The frontend renders:
- `Available` -> the Command Mode chip, as today.
- `MissingToolCallingLlm` -> "Command Mode needs an LLM that can run tools. Pick a model with tool calling."
- `MissingUsableAsr` -> "Command Mode needs a usable speech model on this computer."
- `MissingBoth` -> both reasons.

Reuse the existing disabled-chip / hint text pattern. Copy is plain English, no em/en dashes.

- [ ] **Step 1: Add the IPC field**

In `commands/models.rs`, compute `command_mode_available` for the selected LLM + ASR and include `commandModeStatus` in the model-list response.

- [ ] **Step 2: Render the chip or message in the UI**

Locate the Command Mode chip in `ui/`, branch on `commandModeStatus`, add the four message strings.

- [ ] **Step 3: Verify**

If `ui/node_modules` exists, run the component test / `npx tsc --noEmit`. If absent, report the gate as not-runnable (installing is banned) and defer live verification to a human-reviewed morning check via ali-app-dev-loop (start dev app, read the model picker back, switch LLM to a non-tool-calling row, confirm the message). Screenshot read-back is the evidence, not a green typecheck. Do NOT claim a pass that did not run.

- [ ] **Step 4: Commit**

```bash
git add ui/ crates/teletype-desktop/src/commands/models.rs
git commit -m "feat(ui): show which part is missing when Command Mode is unavailable"
```

---

### Task 11: Silent-update path (manifest fetch, verify, validate, swap)

**Files:**
- Create: `crates/teletype-catalog/src/update.rs`
- Modify: `crates/teletype-catalog/src/lib.rs`, `Cargo.toml` (sha2 already added in Task 1; add reqwest if hashing/fetch lives here)
- Test: `crates/teletype-catalog/tests/update.rs`

**Interfaces:**
- Consumes: `Catalog` (Task 4), validator (Task 3), `sha2`, a `Fetch` trait.
- Produces: `update::Manifest { version: u64, files: HashMap<String, String>, url: String }`; `update::try_update(data_dir: &Path, fetch: &dyn Fetch) -> UpdateResult`; `update::working_copy_dir(data_dir) -> PathBuf` (`data_dir.join("catalogs")`); `update::last_check_path`; `update::should_check(last_check, now) -> bool`; `update::seed_from_bundled(dir)`; `UpdateResult { Updated, Current, Skipped, Failed }`.

Manifest (GitHub release asset `catalog-manifest.json`):
```json
{ "version": 3, "files": { "speech": "<sha256>", "llm": "<sha256>", "command": "<sha256>" },
  "url": "https://github.com/<owner>/teletype/releases/download/catalog-v3" }
```
Each file fetched from `{url}/speech.json`, etc.

`try_update` flow (working copy replaced only after a full pass):
1. **Throttle.** Read `last_check`; if `should_check` false (within last hour), return `Skipped`.
2. **Fetch manifest.** Network error -> `Failed`, keep last good catalog.
3. **Compare version.** Not greater than installed `version` file -> `Current`.
4. **Each changed file** (sha256 differs): download to temp, compute sha256, compare to manifest. Mismatch -> `Failed`, keep old.
5. **Validate.** Parse all three into a `Catalog`, run `validate_catalog`. Error -> `Failed`, keep old.
6. **Swap.** Only after 4+5 pass, atomically replace working-copy files (write temp, rename), bump `version`, write `last_check`.
7. **Rollback automatic.** Bad published file cannot take the app down; next launch retries.

`Fetch` is a trait so tests stub HTTP (no real network). `seed_from_bundled` copies the three bundled files into `dir` if absent (called by startup, Task 12).

- [ ] **Step 1: Write the failing tests with a stub fetch**

```rust
// crates/teletype-catalog/tests/update.rs
use teletype_catalog::update::*;
use std::collections::HashMap;
use std::time::{SystemTime, Duration};

struct StubFetch { manifest: String, files: HashMap<String, String> }
impl Fetch for StubFetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        if url.ends_with("catalog-manifest.json") { return Ok(self.manifest.as_bytes().to_vec()); }
        let name = url.rsplit('/').next().unwrap();
        self.files.get(name).cloned().ok_or_else(|| format!("missing {name}")).map(|s| s.into_bytes())
    }
}
// Helpers (tempdir, good_files, bad_hash_files, invalid_schema_files, v1/v2 manifests,
// installed_version, write_last_check) are built from the schema + sha2; fill them concretely.

#[test]
fn a_newer_manifest_updates_the_working_copy() {
    let dir = tempfile::tempdir().unwrap();
    seed_from_bundled(&working_copy_dir(dir.path()));
    let res = try_update(dir.path(), &StubFetch { manifest: v2_manifest(), files: good_files() });
    assert!(matches!(res, UpdateResult::Updated));
}
#[test]
fn a_wrong_sha256_keeps_the_old_catalog() {
    let dir = tempfile::tempdir().unwrap();
    seed_from_bundled(&working_copy_dir(dir.path()));
    let res = try_update(dir.path(), &StubFetch { manifest: v2_manifest(), files: bad_hash_files() });
    assert!(matches!(res, UpdateResult::Failed));
}
#[test]
fn a_schema_error_keeps_the_old_catalog() {
    let dir = tempfile::tempdir().unwrap();
    seed_from_bundled(&working_copy_dir(dir.path()));
    let res = try_update(dir.path(), &StubFetch { manifest: v2_manifest(), files: invalid_schema_files() });
    assert!(matches!(res, UpdateResult::Failed));
}
#[test]
fn a_recent_check_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    write_last_check(dir.path(), SystemTime::now());
    let res = try_update(dir.path(), &StubFetch { manifest: v2_manifest(), files: good_files() });
    assert!(matches!(res, UpdateResult::Skipped));
}
```
Add `tempfile` as a dev-dependency of teletype-catalog.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p teletype-catalog --test update`
Expected: FAIL, `could not find update`.

- [ ] **Step 3: Write the update path**

`src/update.rs`: implement `Manifest`, `UpdateResult`, `Fetch`, `should_check`, `working_copy_dir`, `last_check_path`, `seed_from_bundled`, `try_update` (seven-step flow). Use `sha2` for hashing. Wire `pub mod update;`.

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p teletype-catalog --test update`
Expected: PASS (all four scenarios).

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/
git commit -m "feat(catalog): silent-update path with verify, validate, swap-only-after-pass"
```

---

### Task 12: Wire the working copy into app startup + CI release job

**Files:**
- Modify: `crates/teletype-desktop/src/lib.rs` (seed + throttled try_update + fallback load)
- Modify: `crates/teletype-desktop/Cargo.toml` (add `teletype-catalog` dep)
- Modify: `.github/workflows/release.yml` (emit catalog manifest + files as release assets)
- Test: `crates/teletype-desktop` startup test (seed + fallback)

**Interfaces:**
- Consumes: `Catalog::bundled()`, `Catalog::load_from_dir` (Task 4), `update::try_update`/`seed_from_bundled`/`working_copy_dir` (Task 11).
- Produces: on every launch the app (a) seeds the working copy from bundled defaults if absent, (b) runs `try_update` (throttled once/hour, non-fatal), (c) loads from the working copy, falling back to `bundled()` on missing/invalid.

Startup wiring in `lib.rs` near the existing `data_dir` resolution:
```rust
let catalogs_dir = teletype_catalog::update::working_copy_dir(&data_dir);
teletype_catalog::update::seed_from_bundled(&catalogs_dir);
let _ = teletype_catalog::update::try_update(&data_dir, &HttpFetch); // throttled, non-fatal
let catalog = teletype_catalog::Catalog::load_from_dir(&catalogs_dir)
    .unwrap_or_else(|e| { tracing::warn!(?e, "catalog load failed, using bundled defaults"); teletype_catalog::Catalog::bundled() });
```
`HttpFetch` is the real reqwest-backed `Fetch` impl. The catalog is stored once in app state and handed to the speech/LLM/command paths. `try_update` errors are logged, never fatal.

CI: in `release.yml` publish job, add a step building `catalog-manifest.json` from the three repo JSON files (compute each sha256, set `version` from `CATALOG_VERSION` env or release tag) and upload it plus the three files as release assets. Reuse the existing pipeline; no new runner. **Do not trigger a real release** (R2 from the lost run: the step was committed but no release fired, so the app no-ops until assets exist; that is expected and safe).

- [ ] **Step 1: Write the failing startup test**

```rust
#[test]
fn startup_seeds_the_working_copy_and_falls_back_on_bad_data() {
    let dir = tempfile::tempdir().unwrap();
    let catalogs_dir = teletype_catalog::update::working_copy_dir(dir.path());
    teletype_catalog::update::seed_from_bundled(&catalogs_dir);
    assert!(catalogs_dir.join("speech.json").exists());
    std::fs::write(catalogs_dir.join("speech.json"), b"not json").unwrap();
    let c = teletype_catalog::Catalog::load_from_dir(&catalogs_dir)
        .unwrap_or_else(|_| teletype_catalog::Catalog::bundled());
    assert!(!c.speech().is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-desktop startup`
Expected: FAIL (wiring not in place).

- [ ] **Step 3: Wire startup + add the CI step**

Wire the startup block, add `HttpFetch`, add the release.yml manifest step.

- [ ] **Step 4: Verify**

Run: `cargo test -p teletype-desktop startup`. Verify the release.yml change by reading the diff and dry-running the manifest-generation command locally (compute sha256 of the three files). Do not trigger a real release.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-desktop/src/lib.rs crates/teletype-desktop/Cargo.toml .github/workflows/release.yml
git commit -m "feat(desktop): seed + silently update the catalog working copy; publish manifest in CI"
```

---

### Task 13: Ivy as the first pure-data model (acceptance test, R5)

**Files:**
- Modify: `crates/teletype-catalog/speech.json` (ADD the `ivy` row)
- Test: `crates/teletype-catalog/tests/acceptance_ivy.rs`

**Interfaces:**
- Consumes: the whole pipeline (Tasks 1-12).
- Produces: proof that adding a model is a data edit, not a code change. Ivy is absent from the Rust source (R5); this task adds it purely as data using the verified artifacts in the "Ivy verified artifacts" section above.

This is the acceptance test the spec names (line 199-202). It asserts the new `ivy` row flows through every layer without any per-id Rust code:
1. The row is added to `speech.json` and parses.
2. The validator accepts it.
3. `speech_by_id("ivy")` returns it and `speech_provider_path` returns `models_dir/ivy`.
4. The re-pointed speech catalog offers it via `speech_offered_on` on macOS.
5. No Rust file hardcodes `"ivy"` outside this test (grep guard).

- [ ] **Step 1: Add the ivy row to speech.json**

Add the `ivy` entry using the preserved verified artifacts (main `ivy-Q8_0.gguf` 1_834_422_208 bytes sha256 `da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed`; mmproj `mmproj-ivy-f16.gguf` 641_774_016 bytes sha256 `07ed1cc9c96c19aba84354b9135c69332747a98d50363a321bf1418aececfc00`), engine `llama-server`, base_url `https://huggingface.co/raj7676/ivy-dictation/resolve/main`, english_only true, languages `["en"]`, capabilities `["asr"]`, purposes `["English only" (inferred), "Experimental" (inferred)]`, size_mb 2477, runs_on `["macos"]`, recommended false, auxiliary false. Update the Task 6 completeness test's speech count from 22 to 23 (this is the one intentional data addition).

- [ ] **Step 2: Write the acceptance test**

```rust
// crates/teletype-catalog/tests/acceptance_ivy.rs
use teletype_catalog::Catalog;

#[test]
fn ivy_flows_through_every_layer_from_pure_data() {
    let c = Catalog::bundled();
    let ivy = c.speech_by_id("ivy").expect("ivy must be in the data");
    assert_eq!(teletype_catalog::engine::Engine::from_name(&ivy.engine).unwrap(),
               teletype_catalog::engine::Engine::Qwen3Asr);
    let models_dir = std::path::Path::new("/models");
    assert_eq!(c.speech_provider_path(ivy, models_dir), models_dir.join("ivy"));
    assert_eq!(ivy.artifacts.len(), 2);
    assert_eq!(ivy.artifacts[0].sha256.as_deref(),
        Some("da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed"));
}
```

- [ ] **Step 3: Run it + grep guard**

Run: `cargo test -p teletype-catalog --test acceptance_ivy`
Expected: PASS.
Run: `grep -rn '"ivy"' crates/ --include=*.rs | grep -v acceptance_ivy`
Expected: no Rust source hardcodes the ivy id. If any consumer special-cases ivy, fix it to read the entry.

- [ ] **Step 4: Commit**

```bash
git add crates/teletype-catalog/speech.json crates/teletype-catalog/tests/acceptance_ivy.rs crates/teletype-catalog/tests/llm_command_data.rs
git commit -m "test(catalog): add Ivy as pure data, proving add-a-model-is-a-data-edit"
```

---

### Task 14: Routing confidence signal (gap-fill, R1)

**Files:**
- Modify: `crates/teletype-inference/src/tinydecide.rs` (expose confidence)
- Modify: `crates/teletype-inference/src/combined.rs` (thread confidence through `classify_intent`)
- Modify: `crates/teletype-core/src/llm.rs` (the `DecisionProvider` trait / `Intent` if a confidence accessor is needed)
- Test: `crates/teletype-inference` confidence test

**Interfaces:**
- Consumes: the existing `TinyDecideProvider` + `CombinedDecisionProvider` (R1). NO new crate.
- Produces: a confidence signal on the route decision. Either `classify_intent_with_confidence(&self, text: &str) -> (Intent, f32)` on the provider, or a `RouteDecision { intent: Intent, confidence: f32 }` return. The existing `classify_intent` stays (callers that only need the intent keep working); the confidence variant is additive.

TinyDecide already returns a probability for `noul("This is a command, not dictation.")`. Surface that probability instead of discarding it when collapsing to `Intent`. Read `crates/tinydecide/src/engine.rs` for the probability API and `combined.rs` for where the intent is currently collapsed. Do not add a second routing path or a new threshold here (the conservative gate already exists; threshold tuning is a follow-up).

- [ ] **Step 1: Write the failing test**

```rust
// in crates/teletype-inference (tinydecide test module, guarded by model presence like the existing tests)
#[test]
fn classify_intent_exposes_a_confidence_in_unit_range() {
    let provider = match TinyDecideProvider::load(&model_dir()) { Ok(p) => p, Err(_) => return };
    let (_intent, conf) = provider.classify_intent_with_confidence("Open the terminal for me");
    assert!((0.0..=1.0).contains(&conf));
}
```
(Use the same model-presence guard the existing tinydecide tests use; if the model is absent the test returns early, matching repo convention.)

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-inference classify_intent_exposes`
Expected: FAIL, method not defined.

- [ ] **Step 3: Expose the confidence**

Add `classify_intent_with_confidence` to `TinyDecideProvider` returning the raw probability alongside the intent; thread it through `CombinedDecisionProvider`. Keep `classify_intent` as a thin wrapper that drops the confidence.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-inference && cargo clippy -p teletype-inference --all-targets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-inference/ crates/teletype-core/
git commit -m "feat(inference): expose TinyDecide route confidence alongside intent"
```

---

### Task 15: Route instrumentation (gap-fill, R1)

**Files:**
- Modify: `crates/teletype-desktop/src/dictation_stages.rs` (`DictationTrace` fields)
- Modify: `crates/teletype-desktop/src/dictation.rs` (populate the fields where the decision is made)
- Modify: `crates/teletype-desktop/src/command_mode.rs` (pass confidence into the trace)
- Test: `crates/teletype-desktop` trace test

**Interfaces:**
- Consumes: the confidence signal (Task 14), the existing `DictationTrace` (dictation_stages.rs:127) which already carries `decider`, `decider_ms`, `tool`, `args_source`.
- Produces: `DictationTrace` gains `route_confidence: Option<f32>` and `route: Option<String>` (`"command"` / `"dictation"`), populated for every take. The existing conservative gate and confirm-on-external (dry-run Effects) are reused unchanged; this task only records the decision data the threshold is tuned from.

- [ ] **Step 1: Write the failing test**

```rust
// in crates/teletype-desktop (trace test module)
#[test]
fn dictation_trace_carries_route_and_confidence() {
    let mut t = DictationTrace::default();
    t.route = Some("command".into());
    t.route_confidence = Some(0.92);
    assert_eq!(t.route.as_deref(), Some("command"));
    assert_eq!(t.route_confidence, Some(0.92));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-desktop dictation_trace_carries`
Expected: FAIL, fields not defined.

- [ ] **Step 3: Add the fields + populate them**

Add `route_confidence` and `route` to `DictationTrace` (serde camelCase like the existing fields). Populate them where the pipeline makes the route decision (dictation.rs near the existing `decider_ms` population at :6407-:7006), reading the confidence from Task 14. Do not change the gate logic.

- [ ] **Step 4: Run the desktop suite**

Run: `cargo test -p teletype-desktop && cargo clippy -p teletype-desktop --all-targets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-desktop/src/dictation_stages.rs crates/teletype-desktop/src/dictation.rs crates/teletype-desktop/src/command_mode.rs
git commit -m "feat(desktop): instrument route decision with confidence and route label"
```

---

### Task 16: Integration + full test suite + live verification

**Files:**
- No new files; the verification gate.

**Interfaces:**
- Consumes: everything (Tasks 1-15).
- Produces: a green full workspace build + test, and (morning, human-reviewed) live app check.

- [ ] **Step 1: Full workspace build + test**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS. The catalog crate, re-pointed speech/inference catalogs, gate, update path, and desktop wiring all green. The re-pointed catalog tests pass against parsed JSON.

- [ ] **Step 2: Confirm no Rust literals remain**

Run: `grep -rn 'const CATALOG' crates/teletype-speech/src/catalog.rs crates/teletype-inference/src/catalog.rs`
Expected: the hardcoded arrays are gone. If any per-id artifact helper remains and is referenced, remove it.

- [ ] **Step 3: Live app verification (morning, human-reviewed, per ali-app-dev-loop)**

Start/restart the dev app (vite 1420 + binary + re-sign llama-server if rebuilt). Read the UI back:
1. Model picker lists speech models from JSON (Ivy present, Qwen3-ASR present).
2. Command Mode chip shows for default LLM + ASR; switch LLM to a non-tool-calling row, confirm the "needs an LLM that can run tools" message.
3. A spoken command take routes to command at high confidence; a dictation take inserts as text; trace shows `route` and `route_confidence`.
Screenshot read-back is the evidence. If this cannot run overnight (no node_modules / no live audio), report it as deferred to morning, not passed.

- [ ] **Step 4: Commit (if any fixes landed), by explicit pathspec**

```bash
git add <explicit verified paths>
git commit -m "chore: integration fixes from full-suite verification"
```

---

## Execution order and dependencies

- **Phase A (catalog core, no app coupling):** Tasks 1, 2, 3, 4, 5, 6. These share `crates/teletype-catalog/src/lib.rs` and the JSON files, so they run as ONE sequential chain (exclusive ownership), not in parallel.
- **Phase B (migration):** Tasks 7, 8. Task 7 touches speech + inference + desktop + eval; Task 8 touches inference + desktop. They share `teletype-inference` and `teletype-desktop`, so run sequentially (7 then 8).
- **Phase C (gate + UI):** Tasks 9, 10. Task 9 (catalog gate) is independent of Phase B and can run in parallel with it (disjoint files: gate.rs vs speech/inference). Task 10 depends on 9 and touches models.rs (shared with Task 8) so runs after 8.
- **Phase D (delivery):** Tasks 11, 12. Task 11 (update.rs) is independent of B/C and can run in parallel (disjoint: update.rs). Task 12 depends on 11 and touches lib.rs + models.rs-adjacent startup; run after 8/10.
- **Phase E (acceptance):** Task 13, after 7 (needs the re-pointed speech catalog) and 6.
- **Phase F (routing gap-fill, R1):** Tasks 14, 15. Task 14 touches inference (shared with 8) so after 8; Task 15 touches desktop dictation (shared with 7) so after 7/14.
- **Phase G (integration):** Task 16, after all.

Parallelism is limited by exclusive file ownership: the catalog core is a single chain; later phases parallelize only where files are disjoint (e.g. Task 9 gate.rs and Task 11 update.rs alongside Phase B). The batch PLAN.md assigns concrete owners and private target dirs.

## Self-review (run before execution)

- **Spec coverage:** data files + schema (1, 5, 6); engine registry (2); validator (3); loader + shipped default (4); re-point + delete literals (7, 8); command-mode gate (9); UI message (10); silent update (11); CI host (12); Ivy acceptance as first pure-data add (13, R5); routing confidence + instrumentation (14, 15, R1); full migration no-data-loss (5, 6, 7, 8). Omni pipeline shape is a follow-up (no omni model shipped in v1).
- **Placeholder scan:** Task 9 fixtures are concrete (no todo!()). Task 11 test helpers are described concretely (built from schema + sha2 + tempfile). No "TBD" or "similar to Task N".
- **Type consistency:** `Capability` (`Asr`, `Polish`, `ToolCalling`, `Omni`) consistent across 1, 3, 7, 8, 9. `Catalog` accessors defined in 4, used unchanged in 7, 9, 13. `CommandModeStatus` (9) consumed by UI (10). `UpdateResult`/`Fetch`/`try_update`/`seed_from_bundled`/`working_copy_dir` (11) consumed by startup (12). Confidence signal (14) consumed by instrumentation (15). `CommandEntry` carries the full R3 field set used by models.rs (8).

## Not in this plan (follow-ups from the spec)

- Sign the manifest (minisign/ed25519) once the update channel is in production and the app is signed.
- Measure TinyDecide on the 185-clip command suite to tune the routing confidence threshold from data.
- Re-host NeuDecide (Gate A) so the audio-direct path is available to compare against TinyDecide.
- The omni pipeline shape (one-model-does-everything) once an omni model is shipped.
- Push the feature branch and open a PR (a shared action; deferred to a human-reviewed morning decision).