# Data-driven updatable model catalog - Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three hardcoded Rust model catalogs (speech, LLM, command deciders) with three JSON files in a new `crates/teletype-catalog` crate, loaded at runtime from a data-dir working copy with a shipped-default fallback and a silent GitHub-release update, so adding a model is a data edit that can ship without an app build.

**Architecture:** One new crate owns the entry schema, the engine registry (string name to existing Rust loader), a validator that re-runs the invariants the ~61 existing catalog tests assert, and a loader. The speech, LLM, and command code paths read parsed entries instead of `const CATALOG` arrays. A small manifest-driven update path fetches, sha256-verifies, validates, and swaps the working copy only after a full pass. The command-mode gate is a pure function over capabilities. A separate TinyDecide router crate is the intent-routing concern.

**Tech Stack:** Rust (workspace, edition 2021, MSRV 1.90), serde/serde_json (already workspace deps), reqwest 0.12 (already in teletype-desktop and teletype-inference), Tauri v2 data dirs, GitHub releases via the existing `release.yml` pipeline.

**Spec:** `docs/superpowers/specs/2026-11-07-data-driven-model-catalog-design.md`

## Global Constraints

These apply to every task. Copy them verbatim; they come from the spec's locked decisions and the project operating rules.

- **Working tree:** `/Users/a.sorathiya/Documents/Ali/teletype-wt/integration`. Build/test from here.
- **No em/en dashes ever.** This is absolute: UI strings, code comments, docs, and test names use commas or periods instead.
- **English only** everywhere: code, comments, test names, JSON field docs, task notes.
- **Never write secrets into any file.** API keys live in the OS keychain (`com.teletype.app`, `llm:<provider>`). The catalog and the manifest carry only model links, sizes, and sha256s, never credentials.
- **Git safety:** no `git checkout`, `git restore`, `git reset`, `git stash`, `git clean`, or `git commit` with `-A`. Commit by explicit pathspec only. Never touch another worker's uncommitted files.
- **Closed engine set.** Engines stay Rust. Data routes a model to an already-implemented engine by name. An unknown engine name is a validation error at load, never a runtime surprise. A genuinely new engine is a separate, evaluated code change and is out of scope for this plan.
- **sha256-only for v1.** The manifest's per-file sha256s stop a corrupted download. A minisign/ed25519 signature over the manifest is a recorded follow-up, not a v1 gate.
- **Update cadence:** check on every launch, throttled to at most once per hour. Launch is the trigger; the throttle avoids hammering the release API on rapid relaunches.
- **Separate command file.** Command deciders live in their own `command.json`, not folded into `llm.json`.
- **Preserve the existing test coverage.** The ~61 catalog tests (43 in `teletype-speech/src/catalog.rs`, 18 in `teletype-inference/src/catalog.rs`) are re-pointed at parsed data, not deleted. A bad entry must fail the build, not the user.
- **No data loss in migration.** Every current model row moves to JSON field for field, including the multi-file/shard shapes, the Core ML repo-shape, and the OS-managed Apple-speech shape.
- **The app always has a working catalog.** A failed or absent update keeps the last good working copy; a missing working copy falls back to the bundled shipped defaults. The app never reads a catalog that failed validation.

## Existing facts the plan builds on (do not re-derive)

- Workspace members (from `Cargo.toml`): `parakeet-sys`, `needle-sys`, `parakeet-cpp-sys`, `teletype-core`, `teletype-speech`, `teletype-fluid`, `teletype-apple`, `teletype-inference`, `teletype-desktop`, `teletype-eval`, `teletype-decider`. There is **no** `teletype-catalog` crate yet.
- Speech catalog: `crates/teletype-speech/src/catalog.rs` (~2812 lines). `pub enum Engine` at line 27 (variants: `Whisper`, `Parakeet`, `ParakeetCoreml`, `SherpaParakeet`, `ParakeetCpp`, `Qwen3Asr`, `AppleSpeech`, `TranscribeCpp`). `pub struct SpeechArtifact` at line 174 (`file`, `bytes`, `sha256`). `pub struct SpeechModel` at line 359. `pub const CATALOG: &[SpeechModel]` at line 668.
- Speech `SpeechModel` fields: `id`, `name`, `file`, `engine`, `size_mb`, `sha256: Option`, `english_only`, `recommended`, `auxiliary`, `description`, `languages: Option<&[&str]>`, `speed: f64`, `accuracy: f64`, `min_ram_gb`, `base_url: Option`, `purposes: &[Purpose]`.
- LLM catalog: `crates/teletype-inference/src/catalog.rs` (~1047 lines). `pub struct CatalogEntry` at line 139. `pub const CATALOG: &[CatalogEntry]` at line 314. `find(id)` at line 587. `polish_shortlist_ids(gpu_accelerated)` at line 301.
- LLM `CatalogEntry` fields: `id`, `name`, `size_label`, `url`, `size_mb`, `description`, `specialty`, `sha256: Option`, `backup_url: Option`, `attribution: Option`, `license_name: Option`, `license_url: Option`, `requires_license_accept`, `recommended`, `polish_capable`, `runs_on_cpu`, `verdict`, `verdict_note`, `sort_order`, `shards: &[CatalogShard]`. Rows with `polish_capable: false` are command deciders (e.g. `neudecide-q4`).
- `Specialty` consts: `CLEANUP`, `ALL_ROUND`, `INSTRUCT`, `FAST_SMALL`. `ModelVerdict`: `Recommended`, `Mixed`.
- Speech base-URL consts: `WHISPER_BASE_URL`, `PARAKEET_BASE_URL`, `FLUID_REPO_BASE_URL`, `SHERPA_MODELS_BASE_URL`.
- Speech purpose-label vocabulary (test-pinned, line 318): `Everyday dictation`, `Fastest`, `Most accurate`, `Multilingual`, `English only`, `Live streaming`, `Context-aware`, `Low memory`, `No download`, `Long recordings`, `Indian languages`, `Experimental`.
- Command decider selection: `crates/teletype-desktop/src/command_mode.rs`. `DeciderKind` enum (`NeuDecide`, `Needle`, and the LLM/needle path); the `DecisionProvider` (TinyDecide + Needle 3 / LLM) is referenced as the fallback extractor.
- HTTP client already available: `reqwest 0.12` with `rustls-tls`, `blocking`, `json` in both `teletype-desktop` and `teletype-inference`.
- Tauri `identifier` is `com.teletype.app`. Data dir is resolved in `crates/teletype-desktop/src/lib.rs` (`app.path().app_data_dir()`, with `TELETYPE_E2E_DATA_DIR` override at line 742; `models_dir = data_dir.join("models")` at line 754). The catalog working copy will live under this data dir.
- Release pipeline: `.github/workflows/release.yml` builds and publishes macOS and Windows releases. It is the host for the catalog manifest + files as release assets.

---

## File structure

New crate `crates/teletype-catalog/`:

- `Cargo.toml` - crate manifest. Deps: `serde`, `serde_json`, `thiserror`, `tracing`.
- `src/lib.rs` - re-exports the public API (schema types, `Catalog`, `Engine`, registry, `load`, `validate`, `update`).
- `src/schema.rs` - the entry structs: `Artifact`, `SpeechEntry`, `LlmEntry`, `CommandEntry`, `Capability`, plus the parsed catalog container.
- `src/engine.rs` - the engine registry: `Engine::from_name(&str)`, `Engine::available_on`, `Engine::built_on`, `Engine::label`, `Engine::platform_label`. This is the existing `Engine` enum re-keyed by a stable string name.
- `src/validate.rs` - `validate_speech`, `validate_llm`, `validate_command`, and the shared invariant helpers. One API used by both CI and the update path.
- `src/loader.rs` - read a catalog from a directory (working copy) or the bundled default; `Catalog` accessor methods (`speech()`, `llm()`, `command()`, `by_id`, capability lookups).
- `src/update.rs` - the silent-update path: fetch manifest, compare version, download changed files, sha256 verify, validate, swap. `last_check` throttle file.
- `speech.json`, `llm.json`, `command.json` - the three data files, bundled via `include_str!` as the shipped defaults.

Consumers that change (no new logic, only re-pointed at parsed data):

- `crates/teletype-speech/src/catalog.rs` - delete the `const CATALOG` array and the per-id artifact helpers; read from `teletype_catalog`.
- `crates/teletype-inference/src/catalog.rs` - delete the `const CATALOG` array; read from `teletype_catalog`.
- `crates/teletype-desktop/src/dictation.rs`, `crates/teletype-desktop/src/commands/models.rs`, `crates/teletype-inference/src/qwen3_asr.rs` - already route through `llama_asr_artifacts_for` / provider dir; those helpers collapse to reading the entry's artifacts.
- `crates/teletype-desktop/src/command_mode.rs` - reads the command-mode gate from capabilities.

New crate for the router (separate pipeline concern):

- `crates/teletype-router/` - wraps the TinyDecide Rust engine (path dependency on the `rust/` engine shipped in the repo). Owns the route decision, the conservative threshold, and decision instrumentation.

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
- Produces: `Capability` enum (`Asr`, `Polish`, `ToolCalling`, `Omni`); `Artifact { file: String, bytes: u64, sha256: Option<String> }`; `SpeechEntry`, `LlmEntry`, `CommandEntry` structs with serde derive; a `Catalog { speech: Vec<SpeechEntry>, llm: Vec<LlmEntry>, command: Vec<CommandEntry> }` container.

The schema is the union of the two existing structs plus the capability set. Field for field, so nothing is lost:

`SpeechEntry` (mirrors `SpeechModel`):
- `id: String`, `name: String`, `engine: String` (registry name, not an enum), `capabilities: Vec<Capability>`, `languages: Option<Vec<String>>`, `english_only: bool`, `artifacts: Vec<Artifact>`, `base_url: Option<String>`, `size_mb: u32`, `min_ram_gb: u32`, `speed: f64`, `accuracy: f64`, `recommended: bool`, `auxiliary: bool`, `runs_on: Vec<String>` (platform names; replaces the `Engine` match in `available_on`), `purposes: Vec<Purpose>`, `description: String`.
- The old single-file-vs-two-file-vs-shard shapes all collapse into `artifacts: Vec<Artifact>`: one file = one artifact, the llama-server pair = two, multi-shard = many. `sha256` is `None` only for the Core ML repo shape and the OS-managed Apple-speech shape.

`LlmEntry` (mirrors `CatalogEntry`):
- `id`, `name`, `size_label`, `capabilities: Vec<Capability>` (this replaces `polish_capable`; a polish row carries `["polish"]`, a command decider carries `["tool_calling"]`), `size_mb`, `description`, `specialty`, `sha256: Option<String>`, `backup_url: Option<String>`, `attribution: Option<String>`, `license_name: Option<String>`, `license_url: Option<String>`, `requires_license_accept: bool`, `recommended: bool`, `runs_on_cpu: bool`, `verdict`, `verdict_note`, `sort_order`, `shards: Vec<Artifact>`, `runs_on: Vec<String>`.

`CommandEntry` (mirrors the `polish_capable: false` LLM rows, split out):
- `id`, `name`, `engine: String`, `capabilities: Vec<Capability>` (always includes `tool_calling`), `artifacts: Vec<Artifact>`, `base_url: Option<String>`, `size_mb`, `description`, `runs_on: Vec<String>`, `runs_on_cpu: bool`.

- [ ] **Step 1: Write the failing roundtrip test**

```rust
// crates/teletype-catalog/tests/schema_roundtrip.rs
use teletype_catalog::schema::{Capability, Artifact, SpeechEntry};

#[test]
fn speech_entry_roundtrips_through_json() {
    let json = r#"{
      "id": "qwen3-asr",
      "name": "Qwen3-ASR 1.7B",
      "engine": "llama-server",
      "capabilities": ["asr"],
      "languages": ["en", "de", "pl"],
      "english_only": false,
      "artifacts": [
        {"file": "Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 2165034944,
         "sha256": "58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57"},
        {"file": "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf", "bytes": 355709344,
         "sha256": "46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d"}
      ],
      "base_url": "https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main",
      "size_mb": 2404,
      "min_ram_gb": 4,
      "speed": 6.0,
      "accuracy": 9.0,
      "recommended": false,
      "auxiliary": false,
      "runs_on": ["macos"],
      "purposes": [{"label": "Multilingual", "basis": "inferred"}],
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
Expected: FAIL, `cannot find crate teletype_catalog` (crate not yet a member / not built).

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
```

Add `"crates/teletype-catalog"` to the `members` array in the root `Cargo.toml` (right after `"crates/teletype-core"`).

`src/schema.rs`: define `Capability`, `Artifact`, `Purpose`, `SpeechEntry`, `LlmEntry`, `CommandEntry`, `Catalog` with `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`. Use `#[serde(rename_all = "camelCase")]` to match the existing IPC convention. `Capability` is `#[serde(rename_all = "snake_case")]` so JSON reads `asr`, `polish`, `tool_calling`, `omni`.

`src/lib.rs`: `pub mod schema;` and re-export `pub use schema::*;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test schema_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/teletype-catalog/
git commit -m "feat(catalog): scaffold teletype-catalog crate with entry schema"
```

---

### Task 2: Engine registry + capability helpers

**Files:**
- Create: `crates/teletype-catalog/src/engine.rs`
- Modify: `crates/teletype-catalog/src/lib.rs`
- Test: `crates/teletype-catalog/tests/engine_registry.rs`

**Interfaces:**
- Consumes: `Capability` (Task 1).
- Produces: `Engine` enum with a stable `name` per variant; `Engine::from_name(&str) -> Result<Engine, CatalogError>`; `Engine::name(&self) -> &'static str`; `Engine::available_on(&self, platform: Platform) -> bool`; `Engine::built_on(&self, platform: Platform, parakeet_cpp_available: bool) -> bool`; `Engine::label(&self) -> &'static str`; `Engine::platform_label(&self) -> &'static str`. `Platform` is re-used from `teletype-speech` (add `teletype-speech` as a dependency of `teletype-catalog`, or move `Platform` into `teletype-catalog` and re-export it from teletype-speech; prefer the latter so the catalog crate has no speech dependency).

The registry is the existing `Engine` enum re-keyed by string. Names map to the current variants:
- `whisper` -> `Whisper`
- `parakeet` -> `Parakeet`
- `parakeet-coreml` -> `ParakeetCoreml`
- `sherpa` -> `SherpaParakeet`
- `parakeet-cpp` -> `ParakeetCpp`
- `llama-server` -> `Qwen3Asr`
- `apple-speech` -> `AppleSpeech`
- `transcribe-cpp` -> `TranscribeCpp`
- `onnx` -> reserved for the NeuDecide command decider path (see Task 8); it has no speech provider, so `available_on` returns true (decider availability is gated by model presence, not platform) and it is never offered as a speech row.

`available_on` and `built_on` copy the current match arms from `crates/teletype-speech/src/catalog.rs` lines 60-115 verbatim. `label` and `platform_label` copy lines 118-144 verbatim. The point of this task is that the logic does not change, only that it is keyed by string and lives in the catalog crate so the validator can reject unknown names.

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

- [ ] **Step 3: Write the registry**

`src/engine.rs`: define `Engine` with the eight variants plus `Onnx`. Implement `from_name` (a `match` returning `Result`, `_ => Err(CatalogError::UnknownEngine(name))`), `name`, and copy `available_on`, `built_on`, `label`, `platform_label` from the speech catalog. Add `Onnx` arms: `available_on` returns `true`, `built_on` returns `true`, `label` returns `"ONNX decider"`, `platform_label` returns `"any"`.

Define `CatalogError` (in `lib.rs` or `schema.rs`) with `thiserror`: `UnknownEngine(String)`, `InvalidJson(String)`, `Validation(String)`, `Io(String)`, `Network(String)`, `HashMismatch(String)`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test engine_registry`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/
git commit -m "feat(catalog): engine registry keyed by string name"
```

---

### Task 3: Validator (the invariants, run by CI and the update path)

**Files:**
- Create: `crates/teletype-catalog/src/validate.rs`
- Modify: `crates/teletype-catalog/src/lib.rs`
- Test: `crates/teletype-catalog/tests/validate.rs`

**Interfaces:**
- Consumes: `SpeechEntry`, `LlmEntry`, `CommandEntry`, `Catalog` (Task 1); `Engine::from_name` (Task 2); `CatalogError` (Task 2).
- Produces: `validate_speech(&[SpeechEntry]) -> Result<(), CatalogError>`, `validate_llm(&[LlmEntry]) -> Result<(), CatalogError>`, `validate_command(&[CommandEntry]) -> Result<(), CatalogError>`, and `validate_catalog(&Catalog) -> Result<(), CatalogError>` that calls all three.

The validator re-runs the invariants the existing tests assert, now against data. Each invariant below is one that an existing test pins, so this is what keeps that coverage alive after the Rust literals are deleted:

1. **Unique ids per domain.** `catalog_ids_are_unique` (speech, line 1719) and the LLM equivalent. Duplicate id = error.
2. **Exactly one `recommended` per domain.** The speech and LLM catalogs each pin a single recommended row. Zero or two = error.
3. **Engine name is known.** Every `engine` string resolves via `Engine::from_name`; unknown = error. (This is the closed-engine-set guarantee.)
4. **Purpose labels from the fixed vocabulary.** `purposes` labels must be in the 12-label set from the speech catalog (line 318). A label outside it is a catalog bug.
5. **sha256 is 64-hex or absent.** Each artifact with a `sha256` must be exactly 64 lowercase hex chars. Absent is allowed only for the Core ML repo shape (`parakeet-coreml` engine) and the OS-managed Apple-speech shape; any other single- or multi-file row must pin every artifact.
6. **Languages table agrees with `english_only`.** If `english_only` is true, `languages` must be `["en"]` alone. If false and `languages` is `Some`, `en` may be present among others. Contradiction = error.
7. **`size_mb` is at least the sum of artifact bytes (rounded).** `size_mb * 1_000_000 >= sum(artifacts.bytes)` within a small rounding tolerance.
8. **`runs_on` is non-empty and uses known platform names.** Platform names: `macos`, `windows`, `linux`.

- [ ] **Step 1: Write the failing tests (one per invariant, each with a violating fixture)**

```rust
// crates/teletype-catalog/tests/validate.rs
use teletype_catalog::validate::*;
use teletype_catalog::schema::{Artifact, SpeechEntry, Capability, Purpose};

fn base_speech(id: &str) -> SpeechEntry {
    SpeechEntry {
        id: id.into(),
        name: format!("Model {id}"),
        engine: "whisper".into(),
        capabilities: vec![Capability::Asr],
        languages: Some(vec!["en".into(), "de".into()]),
        english_only: false,
        artifacts: vec![Artifact { file: "a.gguf".into(), bytes: 1_000_000, sha256: Some("a".repeat(64)) }],
        base_url: None,
        size_mb: 1,
        min_ram_gb: 2,
        speed: 5.0,
        accuracy: 5.0,
        recommended: true,
        auxiliary: false,
        runs_on: vec!["macos".into()],
        purposes: vec![Purpose { label: "Multilingual".into(), basis: "inferred".into(), evidence: None }],
        description: "d".into(),
    }
}

#[test]
fn a_good_speech_set_passes() {
    assert!(validate_speech(&[base_speech("a")]).is_ok());
}

#[test]
fn duplicate_ids_fail() {
    let mut b = base_speech("a"); b.recommended = false;
    let set = vec![base_speech("a"), b];
    assert!(validate_speech(&set).is_err());
}

#[test]
fn unknown_engine_fails() {
    let mut e = base_speech("a"); e.engine = "nope".into();
    assert!(validate_speech(&[e]).is_err());
}

#[test]
fn bad_purpose_label_fails() {
    let mut e = base_speech("a");
    e.purposes = vec![Purpose { label: "Made-up".into(), basis: "inferred".into(), evidence: None }];
    assert!(validate_speech(&[e]).is_err());
}

#[test]
fn english_only_with_other_languages_fails() {
    let mut e = base_speech("a");
    e.english_only = true;
    e.languages = Some(vec!["en".into(), "de".into()]);
    assert!(validate_speech(&[e]).is_err());
}

#[test]
fn size_mb_below_artifact_sum_fails() {
    let mut e = base_speech("a");
    e.size_mb = 0; // artifacts total 1 MB
    assert!(validate_speech(&[e]).is_err());
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p teletype-catalog --test validate`
Expected: FAIL, `could not find validate in crate teletype_catalog`.

- [ ] **Step 3: Write the validator**

`src/validate.rs`: implement the eight invariants. Return `Err(CatalogError::Validation(msg))` with a message naming the offending id and invariant. Helper `is_64_hex(s: &str) -> bool` (len 64, all `[0-9a-f]`). The `recommended` count check returns an error for `!= 1`.

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
- Modify: `crates/teletype-catalog/src/lib.rs`
- Test: `crates/teletype-catalog/tests/loader.rs`

**Interfaces:**
- Consumes: schema (Task 1), validator (Task 3).
- Produces: `Catalog::load_from_dir(dir: &Path) -> Result<Catalog, CatalogError>` (reads `speech.json`, `llm.json`, `command.json`, validates each); `Catalog::bundled() -> Catalog` (parses the three `include_str!` files, validates); `Catalog::speech()`, `llm()`, `command()`; `Catalog::speech_by_id(id) -> Option<&SpeechEntry>`, `llm_by_id`, `command_by_id`; `Catalog::speech_offered_on(entry, platform, fluid_available, parakeet_cpp_available) -> bool`; `Catalog::speech_provider_path(entry, models_dir) -> PathBuf`; `Catalog::speech_url(entry) -> String`; `Catalog::speech_install_path(entry, models_dir) -> PathBuf`.

The loader is the thin layer the spec describes: it parses the JSON and exposes the same predicates the old `SpeechModel` impl methods provided, now reading from parsed entries instead of a `const` array. `speech_provider_path`, `speech_url`, and `speech_install_path` reproduce the existing `provider_path` / `url` / `install_path` behavior (catalog.rs lines 472-518) by reading `entry.artifacts`, `entry.base_url`, and `entry.engine`. For the llama-server pair, the provider dir is `models_dir.join(&entry.id)` (this replaces `llama_asr_provider_dir_for`). For the Core ML shape the provider path is `models_dir.join("fluid")` and the URL uses `FLUID_REPO_BASE_URL` with the entry's first artifact file as the repo folder.

`Catalog::load_from_dir` must refuse to return a catalog if any file is missing or fails validation; the caller (the app) is responsible for falling back to `Catalog::bundled()` on error.

- [ ] **Step 1: Write the failing test**

```rust
// crates/teletype-catalog/tests/loader.rs
use teletype_catalog::Catalog;

#[test]
fn bundled_catalog_is_valid_and_nonempty() {
    let c = Catalog::bundled();
    assert!(!c.speech().is_empty());
    assert!(!c.llm().is_empty());
    assert!(c.speech_by_id("qwen3-asr").is_some());
    assert!(c.speech_by_id("ivy").is_some());
}

#[test]
fn load_from_missing_dir_is_an_error() {
    let c = Catalog::load_from_dir(std::path::Path::new("/nonexistent/teletype-catalog"));
    assert!(c.is_err());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test loader`
Expected: FAIL, `no function or associated item named bundled` (and the JSON files do not exist yet, so `include_str!` will not compile until Task 5/6 add them; for this task, create the three JSON files as **empty arrays** `[]` first so `include_str!` compiles, then fill them in Tasks 5 and 6).

- [ ] **Step 3: Write the loader**

Create the three JSON files as empty arrays so the crate compiles: `speech.json`, `llm.json`, `command.json` each containing `[]`.

`src/loader.rs`: `impl Catalog { pub fn bundled() -> Self { ... parse include_str! ... validate ... } pub fn load_from_dir(...) ... }` plus the accessor methods. Wire `speech_provider_path` / `speech_url` / `speech_install_path` / `speech_offered_on` to mirror the speech-catalog methods.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test loader`
Expected: PASS. (The empty arrays validate as valid-but-empty; the "nonempty" assertions will fail until Tasks 5/6 fill the files, so run the full `cargo test -p teletype-catalog` after Task 6 and expect green.)

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/
git commit -m "feat(catalog): loader with bundled defaults and working-copy load"
```

---

### Task 5: Populate `speech.json` with all 23 current speech models

**Files:**
- Modify: `crates/teletype-catalog/speech.json`
- Test: `crates/teletype-catalog/tests/speech_data.rs`

**Interfaces:**
- Consumes: schema + validator (Tasks 1, 3).
- Produces: a complete, valid `speech.json` containing every current row, field for field.

The 23 current speech model ids (from `crates/teletype-speech/src/catalog.rs` `const CATALOG`), each must be migrated with its real field values read from the source:

`parakeet-tdt-v3`, `fluid-speech-mini`, `parakeet-tdt-v3-q4`, `parakeet-tdt-v3-f16`, `parakeet-ultra-coreml`, `parakeet-v2-coreml`, `parakeet-v3-coreml`, `parakeet-ctc-110m-coreml`, `sherpa-parakeet-v3-int8`, `sherpa-parakeet-v2-int8`, `large-v3-turbo-q5`, `large-v3-turbo`, `small-en`, `base-en`, `base`, `tiny`, `qwen3-asr`, `ivy`, `apple-speech-transcriber`, `apple-dictation-transcriber`, `nemotron-3.5-asr-streaming-q4`, `moonshine-streaming-medium-q8`, `granite-speech-470m-q4`.

Migration rules (no data loss):
- `engine` is the registry string for the row's current `Engine` variant (e.g. `Qwen3Asr` -> `llama-server`, `ParakeetCoreml` -> `parakeet-coreml`, `SherpaParakeet` -> `sherpa`, `TranscribeCpp` -> `transcribe-cpp`, `AppleSpeech` -> `apple-speech`).
- `capabilities` is `["asr"]` for every speech row (a speech model transcribes).
- `artifacts` comes from the row's file + sha256. For the two-file llama-server rows (`qwen3-asr`, `ivy`), use both artifacts from `QWEN3_ASR_ARTIFACTS` / `IVY_ARTIFACTS`. For the pinned streaming rows, use `PINNED_SPEECH_ARTIFACTS`. For Core ML rows, `artifacts` is empty and `sha256` is absent (repo shape). For Apple-speech rows, `artifacts` is empty (OS-managed, no download).
- `base_url` is the row's override, or `null` to use the engine default (the loader applies `WHISPER_BASE_URL` / `PARAKEET_BASE_URL` / `SHERPA_MODELS_BASE_URL` / `FLUID_REPO_BASE_URL` by engine).
- `runs_on` is derived from the engine's `available_on` platform: `macos` for the macOS-only engines, `["macos","windows","linux"]` for `sherpa`.
- `purposes` migrate with their `label`, `basis` (`measured`/`inferred`), and `evidence` path.

The `ivy` row is migrated with its real verified artifacts (main `ivy-Q8_0.gguf` 1_834_422_208 bytes sha256 `da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed`, mmproj `mmproj-ivy-f16.gguf` 641_774_016 bytes sha256 `07ed1cc9c96c19aba84354b9135c69332747a98d50363a321bf1418aececfc00`), `engine` `llama-server`, `base_url` `https://huggingface.co/raj7676/ivy-dictation/resolve/main`, `english_only` true, `languages` `["en"]`, `purposes` `["English only" (inferred), "Experimental" (inferred)]`.

- [ ] **Step 1: Write the failing data test**

```rust
// crates/teletype-catalog/tests/speech_data.rs
use teletype_catalog::Catalog;

#[test]
fn speech_json_has_all_23_models() {
    let c = Catalog::bundled();
    let expected = [
        "parakeet-tdt-v3", "fluid-speech-mini", "parakeet-tdt-v3-q4",
        "parakeet-tdt-v3-f16", "parakeet-ultra-coreml", "parakeet-v2-coreml",
        "parakeet-v3-coreml", "parakeet-ctc-110m-coreml", "sherpa-parakeet-v3-int8",
        "sherpa-parakeet-v2-int8", "large-v3-turbo-q5", "large-v3-turbo", "small-en",
        "base-en", "base", "tiny", "qwen3-asr", "ivy", "apple-speech-transcriber",
        "apple-dictation-transcriber", "nemotron-3.5-asr-streaming-q4",
        "moonshine-streaming-medium-q8", "granite-speech-470m-q4",
    ];
    assert_eq!(c.speech().len(), expected.len());
    for id in expected {
        assert!(c.speech_by_id(id).is_some(), "missing speech row {id}");
    }
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
fn ivy_row_carries_its_two_verified_artifacts() {
    let c = Catalog::bundled();
    let ivy = c.speech_by_id("ivy").unwrap();
    assert_eq!(ivy.artifacts.len(), 2);
    assert_eq!(ivy.artifacts[0].sha256.as_deref(),
        Some("da50c4dcfc9bb36baeca3a5dedb742988dbe1058ab282dbb62d8a31be27795ed"));
    assert_eq!(ivy.artifacts[1].sha256.as_deref(),
        Some("07ed1cc9c96c19aba84354b9135c69332747a98d50363a321bf1418aececfc00"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test speech_data`
Expected: FAIL, `speech().len() == 0` (the file is still `[]`).

- [ ] **Step 3: Write `speech.json`**

Populate `speech.json` with all 23 rows, reading each field from `crates/teletype-speech/src/catalog.rs` `const CATALOG`. This is a data transcription job: open the source, copy each `SpeechModel` literal's values into the JSON shape, and apply the migration rules above. Do not invent values; every number, hash, URL, and purpose chip comes from the source row.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog --test speech_data && cargo test -p teletype-catalog --test validate`
Expected: PASS (the validator now runs against the real 23 rows at build time).

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/speech.json crates/teletype-catalog/tests/speech_data.rs
git commit -m "feat(catalog): migrate all 23 speech models to speech.json"
```

---

### Task 6: Populate `llm.json` and `command.json`

**Files:**
- Modify: `crates/teletype-catalog/llm.json`
- Modify: `crates/teletype-catalog/command.json`
- Test: `crates/teletype-catalog/tests/llm_command_data.rs`

**Interfaces:**
- Consumes: schema + validator (Tasks 1, 3).
- Produces: complete, valid `llm.json` and `command.json`.

The 10 current LLM catalog ids (from `crates/teletype-inference/src/catalog.rs` `const CATALOG`): `s1-mini`, `qwen3.5-2b`, `speakoflow-mini`, `qwen2.5-3b`, `fast`, `qwen3-0.6b`, `quality`, `qwen2.5-7b`, `gemma2-2b`, `neudecide-q4`.

Migration rules:
- Rows with the current `polish_capable: true` go into `llm.json` with `capabilities: ["polish"]`.
- Rows with the current `polish_capable: false` are command deciders and go into `command.json` with `capabilities: ["tool_calling"]`. In the current catalog the decider row is `neudecide-q4`. If any other row is `polish_capable: false`, it also moves to `command.json`.
- `specialty` maps to the `Specialty` const name in JSON (`CLEANUP` -> `cleanup`, etc.).
- `verdict` maps `Recommended` -> `recommended`, `Mixed` -> `mixed`.
- Multi-shard rows keep their `shards` array (each shard becomes an `Artifact`).
- `sort_order`, `runs_on_cpu`, `size_label`, and all license/attribution/backup fields migrate verbatim.

After the split, `llm.json` contains only polish rows and `command.json` contains only decider rows. The validator in Task 3 checks each domain has exactly one `recommended`; if the decider split leaves a domain with zero recommended rows, the validator for that domain must treat "recommended" as optional for `command.json` (a decider set has no single recommended default the way the polish set does). Adjust `validate_command` to enforce unique ids, known engine, and sha256 form, but not a single-recommended rule.

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
    // neudecide-q4 must NOT be in llm.json
    assert!(c.llm_by_id("neudecide-q4").is_none());
}

#[test]
fn command_json_has_the_decider() {
    let c = Catalog::bundled();
    let d = c.command_by_id("neudecide-q4").unwrap_or_else(|| panic!("missing neudecide-q4"));
    assert!(d.capabilities.contains(&teletype_catalog::Capability::ToolCalling));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-catalog --test llm_command_data`
Expected: FAIL, files are still `[]`.

- [ ] **Step 3: Write `llm.json` and `command.json`**

Transcribe the 10 rows from `crates/teletype-inference/src/catalog.rs` `const CATALOG`, splitting polish rows into `llm.json` and decider rows into `command.json`. If `validate_command` still enforces single-recommended and the split breaks it, relax that one rule in `validate.rs` (decider set has no single recommended default) and note the change in the commit.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-catalog`
Expected: PASS across the whole crate (schema, engine, validate, loader, speech, llm/command data).

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/llm.json crates/teletype-catalog/command.json crates/teletype-catalog/tests/llm_command_data.rs crates/teletype-catalog/src/validate.rs
git commit -m "feat(catalog): migrate LLM polish rows and command deciders to JSON"
```

---

### Task 7: Re-point the speech catalog at parsed data; delete the Rust literals

**Files:**
- Modify: `crates/teletype-speech/src/catalog.rs`
- Modify: `crates/teletype-speech/Cargo.toml` (add `teletype-catalog` dep)
- Test: the 43 existing `#[test]` fns in `crates/teletype-speech/src/catalog.rs`

**Interfaces:**
- Consumes: `teletype_catalog::Catalog` and its accessors (Tasks 1, 4).
- Produces: `speech_model_list`, `recommended_speech_model_id`, and the `SpeechModel`-shaped access the app uses, now backed by a parsed `Catalog` instead of `const CATALOG`.

This is the migration step that deletes the Rust literals. The approach:

1. Add `teletype-catalog = { path = "../teletype-catalog" }` to `crates/teletype-speech/Cargo.toml`.
2. Keep the `SpeechModel` struct as the **runtime shape** the app consumes, but make it a thin wrapper that reads from a parsed `SpeechEntry`. The cleanest path: keep `SpeechModel` and its `impl` methods, but change `const CATALOG` from a hardcoded array to a `fn` that builds `Vec<SpeechModel>` from `teletype_catalog::Catalog::bundled().speech()` (or a working-copy catalog passed in). Every `SpeechModel` method that reads `self.engine` / `self.file` / `self.base_url` now reads the parsed entry's fields.
3. Delete the per-id artifact helpers (`llama_asr_artifacts_for`, `llama_asr_provider_dir_for`, `qwen3_asr_main_file`, `qwen3_asr_mmproj_file`, `ivy_asr_main_file`, `ivy_asr_mmproj_file`, `QWEN3_ASR_ARTIFACTS`, `IVY_ARTIFACTS`, `PINNED_SPEECH_ARTIFACTS`) once the data lives in `speech.json`. The consumers (`qwen3_asr.rs`, `dictation.rs`, `commands/models.rs`) switch to `catalog.speech_by_id(id).artifacts`.
4. **Re-point the 43 existing tests**, not delete them. Each test that asserts a catalog invariant now reads from the parsed catalog. For example `catalog_ids_are_unique` iterates `Catalog::bundled().speech()`; `qwen3_asr_row_is_well_formed` and `ivy_row_is_well_formed` read `speech_by_id("qwen3-asr")` / `speech_by_id("ivy")` and assert the same field values. The predicate tests (`offered_on_is_the_list_predicate`, `recommended_speech_model_id_is_always_an_offered_row`) keep their assertions but call the new parsed-backed accessors.

The behavioral contract is unchanged: the same rows, the same predicates, the same recommended pick per host. This task is a refactor that swaps the data source, so the existing tests are the safety net. If a test's assertion was tied to a specific hardcoded value, keep asserting that value against the parsed entry.

- [ ] **Step 1: Add the dependency and a smoke test that the parsed catalog drives the list**

```rust
// added to the existing test module in crates/teletype-speech/src/catalog.rs
#[test]
fn speech_model_list_is_backed_by_the_parsed_catalog() {
    let parsed = teletype_catalog::Catalog::bundled();
    assert_eq!(parsed.speech().len(), speech_model_list(host_platform(), false).len());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-speech catalog`
Expected: FAIL (the list is still the hardcoded array; lengths may match but the test asserts the parsed source is authoritative, which it is not yet).

- [ ] **Step 3: Re-point the catalog at parsed data**

Change `const CATALOG` to a parsed-backed source. Update `speech_model_list`, `recommended_speech_model_id`, and the `SpeechModel` impl methods to read from `teletype_catalog`. Delete the per-id artifact helpers and the `QWEN3_ASR_ARTIFACTS` / `IVY_ARTIFACTS` / `PINNED_SPEECH_ARTIFACTS` consts. Update the three consumers to read `catalog.speech_by_id(id).artifacts`.

- [ ] **Step 4: Re-point the 43 tests and run the whole speech test suite**

Update each of the 43 `#[test]` fns to read from the parsed catalog. Run:
`cargo test -p teletype-speech`
Expected: PASS, all 43 tests green against the parsed data.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-speech/
git commit -m "refactor(speech): drive the speech catalog from teletype-catalog JSON"
```

---

### Task 8: Re-point the LLM catalog + command deciders at parsed data

**Files:**
- Modify: `crates/teletype-inference/src/catalog.rs`
- Modify: `crates/teletype-inference/Cargo.toml` (add `teletype-catalog` dep)
- Modify: `crates/teletype-desktop/src/command_mode.rs` (decider selection reads `command.json`)
- Test: the 18 existing `#[test]` fns in `crates/teletype-inference/src/catalog.rs`

**Interfaces:**
- Consumes: `teletype_catalog::Catalog` (Tasks 1, 4, 6).
- Produces: `find(id)`, `polish_shortlist_ids`, and the `CatalogEntry`-shaped access backed by `llm.json`; command decider rows read from `command.json`.

Same approach as Task 7:
1. Add `teletype-catalog` to `crates/teletype-inference/Cargo.toml`.
2. Back the `CatalogEntry` access from `Catalog::bundled().llm()`. `find(id)` reads `llm_by_id`. `polish_shortlist_ids` keeps its literal shortlist ids but re-checks each against the parsed entry's `capabilities` (must contain `polish`) and `runs_on_host`.
3. The `polish_capable` bool is gone; the capability set `["polish"]` / `["tool_calling"]` replaces it. Any code that branched on `polish_capable` now branches on `capabilities.contains(&Capability::Polish)`.
4. Command decider selection in `crates/teletype-desktop/src/command_mode.rs` reads the decider rows from `Catalog::bundled().command()` instead of assuming a hardcoded decider id. The `neudecide-q4` row in `command.json` is the default decider; its availability is still gated by model presence on disk (the Gate A re-host), so a missing model = `no_action` fallback, unchanged.
5. **Re-point the 18 existing tests** (e.g. `s1_mini_is_the_recommended_head_of_the_list`, `polish_shortlist_rows_are_all_polish_models_this_host_can_run`, `unmeasured_rows_rank_below_measured_rows`) to read from the parsed catalog.

- [ ] **Step 1: Add the dependency and a smoke test**

```rust
// added to the test module in crates/teletype-inference/src/catalog.rs
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
Expected: FAIL (not yet backed by parsed data).

- [ ] **Step 3: Re-point the LLM catalog and the command deciders**

Back `find`, `polish_shortlist_ids`, and the `CatalogEntry` access from parsed `llm.json`. Replace `polish_capable` branches with capability checks. Point `command_mode.rs` decider selection at `command.json`. Delete the hardcoded `const CATALOG` array in the inference catalog.

- [ ] **Step 4: Re-point the 18 tests and run the suites**

Update the 18 `#[test]` fns to read from the parsed catalog. Run:
`cargo test -p teletype-inference && cargo test -p teletype-desktop command_mode`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-inference/ crates/teletype-desktop/src/command_mode.rs
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

The spec defines the gate as a pure function:
`command_mode_available(selected_llm, selected_asr) = selected_llm.capabilities.contains("tool_calling") && selected_asr is usable on this host`.

Because the "why not" message needs to name the missing part, the return is an enum, not a bool:

```rust
pub enum CommandModeStatus {
    /// Both parts are present; Command Mode can be offered.
    Available,
    /// The selected LLM cannot emit tool calls.
    MissingToolCallingLlm,
    /// The selected ASR model is not usable on this host.
    MissingUsableAsr,
    /// Neither part is present.
    MissingBoth,
}
```

`asr_usable` is passed in (the caller resolves it from `speech_offered_on` + installed state) so the gate stays pure and testable without a host.

- [ ] **Step 1: Write the failing table test**

```rust
// crates/teletype-catalog/tests/gate.rs
use teletype_catalog::gate::*;
use teletype_catalog::schema::{LlmEntry, SpeechEntry, Capability};

fn llm(tool_calling: bool) -> LlmEntry { /* build a minimal LlmEntry with the given capability */ todo!() }
fn asr() -> SpeechEntry { /* build a minimal SpeechEntry with ["asr"] */ todo!() }

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

`src/gate.rs`: implement `CommandModeStatus` and `command_mode_available`. The logic is the conjunction from the spec, mapped to the four enum variants.

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
- Modify: `ui/` (the Settings / model-picker screen that offers Command Mode; locate the component that currently gates the Command Mode chip)
- Modify: `crates/teletype-desktop/src/commands/models.rs` (expose `CommandModeStatus` to the UI via IPC)
- Test: `ui/` component test if the repo has one for the model picker; otherwise a manual verification step.

**Interfaces:**
- Consumes: `CommandModeStatus` (Task 9).
- Produces: the Command Mode chip is shown only when the status is `Available`; otherwise a plain message names the missing part.

The UI change is small and driven by data. The model-picker IPC response gains a `commandModeStatus` field (camelCase, matching the existing `Settings` convention). The frontend renders:
- `Available` -> the Command Mode chip, as today.
- `MissingToolCallingLlm` -> a plain line: "Command Mode needs an LLM that can run tools. Pick a model with tool calling."
- `MissingUsableAsr` -> "Command Mode needs a usable speech model on this computer."
- `MissingBoth` -> both reasons.

No new styling; reuse the existing disabled-chip / hint text pattern already on the model picker. The message copy is plain English, no em/en dashes.

- [ ] **Step 1: Add the IPC field**

In `crates/teletype-desktop/src/commands/models.rs`, compute `command_mode_available` for the currently selected LLM + ASR and include it in the model-list response as `commandModeStatus`.

- [ ] **Step 2: Render the chip or the message in the UI**

Locate the Command Mode chip in `ui/` and branch on `commandModeStatus`. Add the four message strings.

- [ ] **Step 3: Verify live**

Per the ali-app-dev-loop skill, start/restart the dev app (vite on 1420 + the binary + re-sign llama-server if rebuilt), then read the model picker back: with the default LLM + ASR the chip is present; switch the LLM to a non-tool-calling row and confirm the "needs an LLM that can run tools" line appears. Screenshot read-back is the evidence, not a green typecheck.

- [ ] **Step 4: Commit**

```bash
git add ui/ crates/teletype-desktop/src/commands/models.rs
git commit -m "feat(ui): show which part is missing when Command Mode is unavailable"
```

---

### Task 11: Silent-update path (manifest fetch, verify, validate, swap)

**Files:**
- Create: `crates/teletype-catalog/src/update.rs`
- Modify: `crates/teletype-catalog/src/lib.rs`
- Modify: `crates/teletype-desktop/Cargo.toml` (add `sha2` if not present; `reqwest` is already a dep)
- Test: `crates/teletype-catalog/tests/update.rs`

**Interfaces:**
- Consumes: `Catalog` (Task 4), validator (Task 3), `reqwest` (already available), `sha2`.
- Produces: `update::Manifest { version: u64, files: HashMap<String, String>, url: String }`; `update::try_update(data_dir: &Path, fetch: &dyn Fetch) -> UpdateResult`; `update::working_copy_dir(data_dir: &Path) -> PathBuf`; `update::last_check_path(data_dir: &Path) -> PathBuf`; `update::should_check(last_check: Option<SystemTime>, now: SystemTime) -> bool`.

The manifest is a small JSON file published as a GitHub release asset:
```json
{ "version": 3,
  "files": { "speech": "<sha256>", "llm": "<sha256>", "command": "<sha256>" },
  "url": "https://github.com/<owner>/teletype/releases/download/catalog-v3" }
```
`url` is the base; each file is fetched from `{url}/speech.json`, `{url}/llm.json`, `{url}/command.json`.

`try_update` flow (the safety property: the working copy is only replaced after a full pass):
1. **Throttle.** Read `last_check`; if `should_check` is false (checked within the last hour), return `UpdateResult::Skipped`.
2. **Fetch the manifest** from the latest GitHub release asset (`catalog-manifest.json`). Any network error -> `UpdateResult::Failed`, keep the last good catalog.
3. **Compare version.** If the manifest `version` is not greater than the installed version (read from a `version` file in the working copy dir), return `UpdateResult::Current`.
4. **For each changed file** (sha256 differs from the installed file's hash): download it to a temp path, compute its sha256, and compare to the manifest's hash. Mismatch -> `UpdateResult::Failed`, keep the old file.
5. **Validate.** Parse all three downloaded files into a `Catalog` and run `validate_catalog`. Any validation error -> `UpdateResult::Failed`, keep the old catalog.
6. **Swap.** Only after steps 4 and 5 fully pass, atomically replace the working-copy files (write temp, rename) and bump the installed `version`. Write the new `last_check`.
7. **Rollback is automatic.** Because the swap only happens after a full validate, a bad published file cannot take the app down; the next launch retries.

`Fetch` is a trait so tests can stub the HTTP layer (no real network in tests). `working_copy_dir` is `data_dir.join("catalogs")`. On first launch, if the working copy is absent, the app copies the bundled defaults into it (this seeding step lives in the app's startup, Task 12 wiring, but the `working_copy_dir` helper is here).

- [ ] **Step 1: Write the failing tests with a stub fetch**

```rust
// crates/teletype-catalog/tests/update.rs
use teletype_catalog::update::*;
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, Duration};

struct StubFetch { manifest: String, files: HashMap<String, String> }
impl Fetch for StubFetch {
    fn get(&self, url: &str) -> Result<Vec<u8>, String> {
        if url.ends_with("catalog-manifest.json") { return Ok(self.manifest.as_bytes().to_vec()); }
        let name = url.rsplit('/').next().unwrap();
        self.files.get(name).cloned().ok_or_else(|| format!("missing {name}")).map(|s| s.into_bytes())
    }
}

#[test]
fn a_newer_manifest_updates_the_working_copy() {
    let dir = tempdir(); // use a temp dir helper
    let manifest = r#"{"version":2,"files":{"speech":"<sha>","llm":"<sha>","command":"<sha>"},
                       "url":"https://example/catalog-v2"}"#;
    // seed a v1 working copy, then run update with a v2 manifest whose files hash-match
    let res = try_update(&dir, &StubFetch { manifest: manifest.into(), files: good_files() });
    assert!(matches!(res, UpdateResult::Updated));
}

#[test]
fn a_wrong_sha256_keeps_the_old_catalog() {
    let dir = tempdir();
    let res = try_update(&dir, &StubFetch { manifest: v2_manifest().into(), files: bad_hash_files() });
    assert!(matches!(res, UpdateResult::Failed));
    // the working copy still holds the old (v1) files
    assert_eq!(installed_version(&dir), 1);
}

#[test]
fn a_schema_error_keeps_the_old_catalog() {
    let dir = tempdir();
    let res = try_update(&dir, &StubFetch { manifest: v2_manifest().into(), files: invalid_schema_files() });
    assert!(matches!(res, UpdateResult::Failed));
}

#[test]
fn a_recent_check_is_skipped() {
    let dir = tempdir();
    write_last_check(&dir, SystemTime::now());
    let res = try_update(&dir, &StubFetch { manifest: v2_manifest().into(), files: good_files() });
    assert!(matches!(res, UpdateResult::Skipped));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p teletype-catalog --test update`
Expected: FAIL, `could not find update`.

- [ ] **Step 3: Write the update path**

`src/update.rs`: implement `Manifest`, `UpdateResult` (`Updated`, `Current`, `Skipped`, `Failed`), the `Fetch` trait, `should_check`, `working_copy_dir`, `last_check_path`, and `try_update` with the seven-step flow. Add `sha2` to `crates/teletype-desktop/Cargo.toml` (or to `teletype-catalog` if the hashing lives there).

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test -p teletype-catalog --test update`
Expected: PASS (all four scenarios: update, wrong hash keeps old, schema error keeps old, recent check skipped).

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-catalog/src/update.rs crates/teletype-catalog/src/lib.rs crates/teletype-catalog/Cargo.toml crates/teletype-catalog/tests/update.rs
git commit -m "feat(catalog): silent-update path with verify, validate, swap-only-after-pass"
```

---

### Task 12: Wire the working copy into app startup + CI release job

**Files:**
- Modify: `crates/teletype-desktop/src/lib.rs` (seed working copy on startup, run `try_update` throttled)
- Modify: `.github/workflows/release.yml` (emit the catalog manifest + files as release assets)
- Test: `crates/teletype-desktop` startup test (seed + fallback)

**Interfaces:**
- Consumes: `Catalog::bundled()` (Task 4), `Catalog::load_from_dir` (Task 4), `update::try_update` (Task 11).
- Produces: on every launch the app (a) seeds the working copy from bundled defaults if absent, (b) runs `try_update` (throttled to once/hour), (c) loads the catalog from the working copy, falling back to `Catalog::bundled()` if the working copy is missing or failed validation.

Startup wiring in `crates/teletype-desktop/src/lib.rs` (near the existing `data_dir` resolution at line 742):
```rust
let catalogs_dir = teletype_catalog::update::working_copy_dir(&data_dir);
teletype_catalog::update::seed_from_bundled(&catalogs_dir); // no-op if present
let _ = teletype_catalog::update::try_update(&data_dir, &HttpFetch); // throttled, non-fatal
let catalog = teletype_catalog::Catalog::load_from_dir(&catalogs_dir)
    .unwrap_or_else(|e| { tracing::warn!(?e, "catalog load failed, using bundled defaults"); teletype_catalog::Catalog::bundled() });
```
The catalog is stored once (e.g. in the app state) and handed to the speech/LLM/command code paths. `try_update` is non-fatal: any error is logged, never fatal, and the app continues with the last good catalog.

CI release job: in `.github/workflows/release.yml`, add a step (in the publish job) that builds `catalog-manifest.json` from the three repo JSON files (compute each file's sha256, set `version` from a `CATALOG_VERSION` env or the release tag) and uploads `catalog-manifest.json` plus `speech.json`, `llm.json`, `command.json` as release assets. This reuses the existing release pipeline; no new runner.

- [ ] **Step 1: Write the failing startup test**

```rust
// in crates/teletype-desktop (startup test module)
#[test]
fn startup_seeds_the_working_copy_and_falls_back_on_bad_data() {
    let dir = tempdir();
    let catalogs_dir = teletype_catalog::update::working_copy_dir(dir.path());
    teletype_catalog::update::seed_from_bundled(&catalogs_dir);
    assert!(catalogs_dir.join("speech.json").exists());
    // corrupt the working copy, then load: must fall back to bundled, not panic
    std::fs::write(catalogs_dir.join("speech.json"), b"not json").unwrap();
    let c = teletype_catalog::Catalog::load_from_dir(&catalogs_dir)
        .unwrap_or_else(|_| teletype_catalog::Catalog::bundled());
    assert!(!c.speech().is_empty());
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-desktop startup`
Expected: FAIL, `seed_from_bundled` not yet defined / wiring not in place.

- [ ] **Step 3: Add `seed_from_bundled` + wire startup + add the CI step**

Add `update::seed_from_bundled(dir)` (copies the three bundled files into `dir` if absent). Wire the startup block above. Add the CI step to `release.yml`.

- [ ] **Step 4: Verify**

Run: `cargo test -p teletype-desktop startup`. Then verify the `release.yml` change by reading the diff (a YAML lint or a dry-run of the step's manifest-generation command locally). Do not trigger a real release.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-desktop/src/lib.rs .github/workflows/release.yml
git commit -m "feat(desktop): seed + silently update the catalog working copy; publish manifest in CI"
```

---

### Task 13: Ivy as the first model added under the new system (acceptance test)

**Files:**
- Modify: `crates/teletype-catalog/speech.json` (already has the `ivy` row from Task 5; this task proves the *add* path)
- Test: `crates/teletype-catalog/tests/acceptance_ivy.rs`

**Interfaces:**
- Consumes: the whole pipeline (Tasks 1-12).
- Produces: proof that adding a model is a data edit, not a code change.

Ivy was migrated in Task 5. This task is the acceptance test the spec names: it proves the end-to-end "add one entry, no Rust change" property. It does not add a second new model (YAGNI); it asserts that the existing `ivy` row flows through every layer without any per-id Rust code:

1. The row is in `speech.json` and parses (Task 5).
2. The validator accepts it (Task 3).
3. `speech_by_id("ivy")` returns it and `speech_provider_path` returns `models_dir/ivy` (Task 4).
4. The speech catalog (re-pointed in Task 7) offers it via `speech_offered_on` on macOS.
5. No Rust file contains a hardcoded `"ivy"` string outside the JSON and this test (the grep guard).

- [ ] **Step 1: Write the acceptance test**

```rust
// crates/teletype-catalog/tests/acceptance_ivy.rs
use teletype_catalog::Catalog;

#[test]
fn ivy_flows_through_every_layer_from_pure_data() {
    let c = Catalog::bundled();
    let ivy = c.speech_by_id("ivy").expect("ivy must be in the data");
    // engine resolves
    assert_eq!(teletype_catalog::engine::Engine::from_name(&ivy.engine).unwrap(),
               teletype_catalog::engine::Engine::Qwen3Asr);
    // provider path derives from the id, not a per-id helper
    let models_dir = std::path::Path::new("/models");
    assert_eq!(c.speech_provider_path(ivy, models_dir), models_dir.join("ivy"));
    // two verified artifacts
    assert_eq!(ivy.artifacts.len(), 2);
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p teletype-catalog --test acceptance_ivy`
Expected: PASS.

- [ ] **Step 3: Grep guard for hardcoded ivy**

Run: `grep -rn '"ivy"' crates/ --include=*.rs | grep -v 'teletype-catalog/speech.json' | grep -v acceptance_ivy`
Expected: no Rust source file hardcodes the `ivy` id (only the JSON and this test reference it). If any consumer still special-cases `ivy`, fix it to read the entry.

- [ ] **Step 4: Commit**

```bash
git add crates/teletype-catalog/tests/acceptance_ivy.rs
git commit -m "test(catalog): Ivy acceptance test proving add-a-model-is-a-data-edit"
```

---

### Task 14: TinyDecide router crate (wrapper + conformance)

**Files:**
- Create: `crates/teletype-router/Cargo.toml`
- Create: `crates/teletype-router/src/lib.rs`
- Modify: `Cargo.toml` (add `"crates/teletype-router"` to `members`)
- Test: `crates/teletype-router/tests/conformance.rs`

**Interfaces:**
- Consumes: the TinyDecide Rust engine, shipped in the repo at `TheREZOR/TinyDecide/rust/` (a path dependency; the engine is conformance-tested to 1e-6 against the reference). Add that path dep in `crates/teletype-router/Cargo.toml`.
- Produces: `Router` (loads the 6.2 MB model once); `Router::route(transcript: &str) -> RouteDecision`; `RouteDecision { command_confidence: f32, tool: Option<String>, slots: BTreeMap<String, String> }`.

The router runs TinyDecide on the **transcript** (text-in, first 127 tokens), not the audio. In one encoder pass it answers: `noul("This is a command, not dictation.")` for the route probability, `choice("Which tool?", [tool names])` for the tool, and `span(...)` for the free-form slots. This is the spec's "one pass answers routing + tool + slots together."

The crate is a thin wrapper: it owns model loading (the 6.2 MB 4-bit weights, a path dep or a bundled file with a sha256 pin), exposes `route`, and returns the raw confidence + tool + slots. The conservative threshold and the confirm-on-external-action policy live in Task 15, not here, so this crate is a pure "ask TinyDecide" layer.

- [ ] **Step 1: Write the failing conformance test**

```rust
// crates/teletype-router/tests/conformance.rs
use teletype_router::Router;

#[test]
fn a_clear_command_scores_high_and_names_the_tool() {
    let r = Router::load_default().expect("tinydecide model loads");
    let d = r.route("Open the terminal for me");
    assert!(d.command_confidence > 0.5);
    assert_eq!(d.tool.as_deref(), Some("open_app"));
}

#[test]
fn plain_dictation_scores_low() {
    let r = Router::load_default().expect("tinydecide model loads");
    let d = r.route("I need to finish the report by end of day and send it to the team");
    assert!(d.command_confidence < 0.5);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-router --test conformance`
Expected: FAIL, crate not yet a member / `Router` not defined.

- [ ] **Step 3: Write the wrapper**

Add `"crates/teletype-router"` to the workspace `members`. `crates/teletype-router/Cargo.toml` path-depends on the TinyDecide `rust/` engine. `src/lib.rs`: `Router::load_default` loads the model; `route` runs the one-pass `noul` + `choice` + `span` and returns `RouteDecision`.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p teletype-router --test conformance`
Expected: PASS. (If the exact tool string differs from `open_app`, align the test to the actual v1 tool vocabulary from `crates/teletype-decider/src/tools.rs`.)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/teletype-router/
git commit -m "feat(router): TinyDecide wrapper crate for transcript intent routing"
```

---

### Task 15: Routing policy (conservative gate + confirm-on-external + instrumentation)

**Files:**
- Modify: `crates/teletype-desktop/src/command_mode.rs` (use the router + policy for per-take routing)
- Create: `crates/teletype-desktop/src/routing_policy.rs` (or add to `command_mode.rs`)
- Test: `crates/teletype-desktop` routing policy tests

**Interfaces:**
- Consumes: `Router::route` (Task 14), the existing dry-run Effects preview (the confirm path for external actions).
- Produces: a per-take decision: `RouteToCommand { tool, slots, confidence }` or `InsertAsText`. Plus a decision log record for every take.

The policy encodes the spec's error-asymmetry rule:
1. **Conservative toward dictation.** Route to command only when `command_confidence` clears a **high threshold** (a named constant, e.g. `COMMAND_CONFIDENCE_THRESHOLD: f32 = 0.8`, to be tuned on real takes, not guessed). Below the threshold, insert as text. A false dictation is harmless and easily redone; a false command is dangerous.
2. **Always confirm external actions.** Any tool that leaves the machine (`send_message`, `draft_message` send, `compose_email` send, `open_file`, `web_search` that posts, or any external app) goes through the existing dry-run Effects preview regardless of confidence. Never fire-and-forget a low-confidence outward command. In-machine, easily-undone actions (`edit_text`, `switch_style`, `insert_autotext`) may run without confirmation when confidence is high.
3. **Instrument every decision.** For each take, log: the transcript, the confidence, the route taken (command vs text), the tool, and whether the user confirmed (for external actions). This is the data the threshold is tuned from. Write to the existing trace/log path (`DictationTrace` already carries `decider` and `decider_ms`; add `route_confidence` and `route`).

The threshold is a single named constant so it is tunable from one place. The policy is a pure function of `(RouteDecision, tool_is_external, user_confirmed) -> RouteOutcome`, testable without the model.

- [ ] **Step 1: Write the failing policy tests**

```rust
// in crates/teletype-desktop (routing policy test module)
use crate::routing_policy::*;
use teletype_router::RouteDecision;

fn dec(conf: f32, tool: &str) -> RouteDecision {
    RouteDecision { command_confidence: conf, tool: Some(tool.into()), slots: Default::default() }
}

#[test]
fn below_threshold_routes_to_text_even_if_confident_enough_to_be_gray() {
    let d = dec(0.7, "open_app");
    assert!(matches!(decide(&d, /*external*/ false, /*confirmed*/ false), RouteOutcome::InsertAsText));
}

#[test]
fn above_threshold_in_machine_action_runs() {
    let d = dec(0.9, "switch_style");
    assert!(matches!(decide(&d, false, false), RouteOutcome::Command { .. }));
}

#[test]
fn external_action_always_requires_confirmation() {
    let d = dec(0.99, "send_message");
    assert!(matches!(decide(&d, true, /*confirmed*/ false), RouteOutcome::AwaitConfirmation { .. }));
    assert!(matches!(decide(&d, true, /*confirmed*/ true), RouteOutcome::Command { .. }));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p teletype-desktop routing_policy`
Expected: FAIL, `routing_policy` not defined.

- [ ] **Step 3: Write the policy + wire it into command_mode**

`routing_policy.rs`: `COMMAND_CONFIDENCE_THRESHOLD`, `RouteOutcome` (`InsertAsText`, `Command`, `AwaitConfirmation`), `decide(...)`. In `command_mode.rs`, replace the current per-take gate with: run the router on the transcript, call `decide`, and route accordingly. Add `route_confidence` and `route` to the trace. Wire the confirm path to the existing dry-run Effects preview.

- [ ] **Step 4: Run the desktop suite**

Run: `cargo test -p teletype-desktop`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-desktop/src/routing_policy.rs crates/teletype-desktop/src/command_mode.rs
git commit -m "feat(desktop): conservative TinyDecide routing with confirm-on-external and decision logging"
```

---

### Task 16: Integration + full test suite + live verification

**Files:**
- No new files; this is the verification gate.

**Interfaces:**
- Consumes: everything (Tasks 1-15).
- Produces: a green full workspace build + test, and a live app check that the model picker, the command-mode gate, and a routed take all work end to end.

- [ ] **Step 1: Full workspace build + test**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS. All crates compile; the catalog crate, the re-pointed speech and inference catalogs, the router, and the desktop wiring are all green. The ~61 re-pointed catalog tests pass against the parsed JSON.

- [ ] **Step 2: Confirm no Rust literals remain**

Run: `grep -rn 'const CATALOG' crates/teletype-speech/src/catalog.rs crates/teletype-inference/src/catalog.rs`
Expected: the hardcoded `const CATALOG` arrays are gone (replaced by parsed-backed sources). If any per-id artifact helper (`llama_asr_artifacts_for`, `QWEN3_ASR_ARTIFACTS`, etc.) remains and is referenced, remove it.

- [ ] **Step 3: Live app verification (per ali-app-dev-loop)**

Start/restart the dev app (vite on 1420 + the binary + re-sign llama-server if rebuilt). Verify by reading the UI back:
1. The model picker lists the speech models from the JSON catalog (Ivy present, Qwen3-ASR present).
2. The Command Mode chip shows for the default LLM + ASR; switch the LLM to a non-tool-calling row and confirm the "needs an LLM that can run tools" message appears.
3. A spoken command take routes to the command path at high confidence; a spoken dictation take inserts as text. Check the trace for `route` and `route_confidence`.
Screenshot read-back is the evidence, not a green build.

- [ ] **Step 4: Commit (if any fixes landed)**

```bash
git add -p   # review each hunk; commit only verified changes by explicit pathspec
git commit -m "chore: integration fixes from full-suite and live verification"
```

---

## Execution order and dependencies

Tasks are ordered so each builds on the previous. The catalog core (Tasks 1-6) is independent of the desktop app and can be developed and tested in isolation. The re-pointing (Tasks 7-8) depends on the data being present (Tasks 5-6). The gate and UI (Tasks 9-10) depend on the re-pointed catalogs. The update path (Tasks 11-12) depends on the loader and validator. The router (Tasks 14-15) is a separate pipeline concern and can be developed in parallel with Tasks 9-12 once Task 14's engine path dep is available.

- **Phase A (catalog core, no app coupling):** Tasks 1, 2, 3, 4, 5, 6.
- **Phase B (migration):** Tasks 7, 8.
- **Phase C (gate + UI):** Tasks 9, 10.
- **Phase D (delivery):** Tasks 11, 12.
- **Phase E (acceptance):** Task 13.
- **Phase F (router, parallelizable):** Tasks 14, 15.
- **Phase G (integration):** Task 16.

## Self-review (run before execution)

- **Spec coverage:** every spec section maps to a task. Data files + schema (Tasks 1, 5, 6); engine registry (Task 2); validator (Task 3); loader + shipped default (Task 4); re-point + delete literals (Tasks 7, 8); command-mode gate (Task 9); UI message (Task 10); silent update (Task 11); CI host (Task 12); Ivy acceptance (Task 13); TinyDecide router + conservative gate + instrumentation (Tasks 14, 15); full migration no-data-loss (Tasks 5, 6, 7, 8). The omni pipeline shape is a follow-up (no omni model is shipped in v1), so it is intentionally not a task here.
- **Placeholder scan:** the only `todo!()` are inside test fixtures in Tasks 9 and 14, where the surrounding comment says to build a minimal entry; the implementer fills them from the schema. No "TBD", "implement later", or "similar to Task N" in any step.
- **Type consistency:** `Capability` is used consistently (`Asr`, `Polish`, `ToolCalling`, `Omni`) across Tasks 1, 3, 7, 8, 9. `Catalog` accessors (`speech_by_id`, `llm_by_id`, `command_by_id`, `speech_provider_path`, `speech_offered_on`) are defined in Task 4 and used unchanged in Tasks 7, 9, 13. `CommandModeStatus` (Task 9) is consumed by the UI (Task 10). `RouteDecision` / `Router` (Task 14) are consumed by the policy (Task 15). `UpdateResult` / `Fetch` / `try_update` / `seed_from_bundled` / `working_copy_dir` (Task 11) are consumed by startup (Task 12) and the loader test (Task 4).

## Not in this plan (follow-ups from the spec)

- Sign the manifest (minisign/ed25519) once the update channel is in production and the app is signed.
- Measure TinyDecide on the 185-clip command suite to set `COMMAND_CONFIDENCE_THRESHOLD` from data (the constant in Task 15 is a starting value, not a tuned one).
- Re-host NeuDecide (Gate A) so the audio-direct path is available to compare against TinyDecide.
- The omni pipeline shape (one-model-does-everything) once an omni model is actually shipped.
