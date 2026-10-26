# Plan: Model deletion + complete state coverage

## Context

The user wants two things on the AI Models screen: (1) the ability to DELETE
downloaded models (they occupy several GB and there is currently no way to
reclaim the space), and (2) every state on that screen handled correctly.

Research findings (verified against source):
- **No delete exists anywhere.** No `delete_model`/`delete_speech_model` IPC
  command, no file-removal function in `teletype-inference`, no Delete button or
  handler in the UI.
- **Model files on disk** live under `AppState.models_dir`
  (`<data_dir>/models`). LLM layout: single `<id>.gguf` or multi-shard
  `<id>/<shard>` (see `CatalogEntry::is_downloaded`/`entrypoint`,
  `crates/teletype-inference/src/catalog.rs:234,247`). Speech layout per engine:
  `ProviderPath`/`install_path` (`crates/teletype-speech/src/catalog.rs:213,224`).
- **In-use guard does not exist.** The active LLM is `state.inference`
  (`Arc<dyn InferenceProvider>`), stop pattern in `select_model`
  (`crates/teletype-desktop/src/commands/models.rs:207-222`). The active speech
  model is `settings.selected_speech_model`, loaded into one of the
  `SpeechModelManager` slots (`lib.rs:142-163`), unloadable via `.unload()`.
  Nothing currently prevents deleting a loaded model's files.
- **Missing UI states** (the "every state" half):
  - *Catalog-fetch error*: `refresh()` (ModelsScreen.tsx:282-288) swallows
    `list_models`/`list_speech_models` errors to `[]`, so a failed read is
    indistinguishable from an empty catalog.
  - *Delete-in-progress* and *delete-failed*: don't exist yet (no delete).
  - *In-use / cannot-delete* guard: none.
  - Already handled: in-use chip, downloaded, downloading %, verifying,
    license gate, download-failed, empty-catalog, Ollama states.

Decisions (from the user):
- **Scope:** delete BOTH polish/LLM models and speech/dictation models.
- **In-use model:** STOP (unload) it, THEN delete. After, the app shows
  "no active model" until the user picks another.
- **Confirmation:** TWO-TAP delete (first tap arms "Delete? tap to confirm",
  second tap deletes). No modal.

## Design

### Backend - LLM delete (`crates/teletype-desktop/src/commands/models.rs`)

Add `delete_model(state, id) -> CommandResult<()>`:
1. `catalog::find(id)` -> error if unknown.
2. `!entry.is_downloaded(&state.models_dir)` -> error "not downloaded".
3. **In-use guard:** read `settings.selected_llm_model` + provider. If this id
   is the active `local-server` model, clone `state.inference` and call
   `.stop()` (same spawn_blocking pattern as select_model:207-222), and clear
   `settings.selected_llm_model` / set provider so the readiness panel reflects
   "no model". (Stop is safe: nothing is mid-transcribe in the settings flow,
   but the stop still releases the llama-server child holding the file.)
4. Remove the files. Single: `models_dir/<id>.gguf`. Multi-shard:
   `models_dir/<id>/` (remove_dir_all). Use `std::fs::remove_file` /
   `remove_dir_all`.
5. `emit_runtime_status(&app, &state)` so the readiness panel updates now.

Add `delete_speech_model(state, id) -> CommandResult<()>`:
1. `teletype_speech::catalog::find(id)` -> error if unknown.
2. Compute the on-disk path via `install_path(&state.models_dir)` (already
   handles Core ML's `fluid/<repo>` vs file vs sherpa dir).
3. **In-use guard:** if `id == settings.selected_speech_model`, unload the
   owning `SpeechModelManager` slot (route by engine: Core ML/Sherpa ->
   `state.neural`, ParakeetCpp -> `state.parakeet_cpp`, Parakeet ->
   `state.parakeet`, Whisper -> `state.speech`), then **reset the selection to
   the platform default** (`default_speech_model_id()` in commands.rs) so the
   app still has a valid dictation model. If the default model is not itself
   downloaded, leave settings pointing at it (the existing "not downloaded,
   re-download" path already surfaces that).
4. Remove the path (file or dir by `metadata()`).
   - Core ML: only delete `models_dir/fluid/<repo>` if it exists (the bridge
     may own nothing yet); refuse nothing.
5. `emit_runtime_status` if there is an LLM effect (none here, but keep
   consistent).

Register both in `lib.rs` (~line 1100-1107, next to `download_model`).

### Frontend (`ui/src/screens/ModelsScreen.tsx`)

- Add a **Delete** affordance on rows where `isDownloaded(m)`. Given the row
  currently renders ONE control (`ActionButton`: Use/Download), add a small
  secondary "trash" text button beside it (right-aligned, muted) so the
  primary action stays the row's purpose.
- **Two-tap:** new state `confirmingDelete: string | null`. First tap sets it
  to the row id; the button turns red and reads "Delete? confirm". A second tap
  within ~4s (auto-reset on a timer) fires `deleteModel(id)` /
  `deleteSpeechModel(id)`. Any other interaction (switching rows, refresh)
  cancels the armed state.
- **Delete-in-progress:** state `deletingId: string | null`. While set, that
  row's Delete control shows "Deleting…", other Delete controls are disabled
  (one at a time, like select).
- **Delete-failed:** state `deleteError: {id, message} | null`, rendered under
  the row (reuse the `rowBody` pattern / the existing danger text style),
  cleared on next successful action or refresh.
- On success: `refresh()`, clear the armed/deleting/error states.
- **Close the catalog-fetch error state:** change `refresh()` so a rejected
  `list_models`/`list_speech_models` sets a `catalogError` string instead of
  silently `[]`. Render a banner (not the empty state) with a "Retry" button
  when `catalogError` is set. Keep the true-empty `EmptyState` for the
  no-error, zero-rows case.
- Disable Delete while a download for that row is in flight (can't delete a
  file that isn't fully here), and hide Delete when not downloaded.

### Types / IPC typing

`ModelStatus`/`SpeechModelStatus` already carry `downloaded` and `selected`,
so the UI has everything it needs to decide when to show/enable Delete. No
struct changes required.

## Files to change

- `crates/teletype-desktop/src/commands/models.rs` - two new commands + helpers
  (LLM file removal, speech file removal, in-use stop/reset).
- `crates/teletype-desktop/src/lib.rs` - register `delete_model`,
  `delete_speech_model` in `generate_handler!`.
- `ui/src/screens/ModelsScreen.tsx` - Delete button (two-tap), deleting/
  deleteError/catalogError state, retry banner, refresh() error surfacing.
- Tests:
  - Rust (inline in models.rs `mod tests`): LLM single + multi-shard removal;
    in-use stop-then-delete (assert engine stopped + settings cleared); unknown
    id / not-downloaded errors; speech file + sherpa-dir removal; deleting the
    selected speech model resets selection to default; Core ML no-op when
    absent.
  - Frontend (`ui/src/test/modelsScreen.test.tsx`): Delete visible only when
    downloaded; two-tap arms then deletes; deleting state disables siblings;
    delete-failed shows the message; catalog-fetch error shows retry banner
    (not empty state); Delete hidden/disabled while downloading.

## Verification

- `cargo test -p teletype-core -p teletype-desktop` green.
- `cargo clippy -p teletype-desktop --all-targets` clean (no new warnings).
- `ui/`: `npm run typecheck` (or `tsc --noEmit`) + `npm test` green.
- Manual (`/ali-app-dev-loop`): download a small model, delete it (two-tap),
  confirm the file is gone and the row returns to "Download"; delete the
  in-use polish model and confirm the readiness panel drops to "no model" and
  dictation still works after re-selecting.

## Out of scope

- Ollama model deletion (Ollama owns its models; we only reference them).
- Cloud/OpenAI endpoints (no local files).
- Bulk delete / "delete all" (one row at a time).

---

# Part 2: Apple Intelligence polish/LLM provider (roadmap P1.4)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the macOS 26+ Apple Intelligence system model as a third, zero-download polish/LLM provider, selectable alongside `local-server` and `openai-compat`, without touching either of those existing paths.

**Architecture:** A new `InferenceProvider` implementation wrapping the `foundation-models` crate's `LanguageModelSession`. It is selected by a new `"apple-intelligence"` arm in the existing provider-string dispatch (`rehydrate_provider` at startup, `select_model` on user choice, `ensure_local_provider`-style lazy load). The Models screen gains one row for the system model; that row's Download and Delete controls are both hidden (Apple owns the weights; there is nothing to fetch or delete). Because the model lives inside the OS, the provider is only offered when the OS actually has it (macOS 26+, Apple Silicon, `SystemLanguageModel::is_available()` true).

**Tech Stack:** `foundation_models` crate (safe Rust bindings for Apple Foundation Models), `InferenceProvider` trait in `teletype-core::llm`, existing Tauri IPC + React `ModelsScreen`.

**Spec:** `.agents/brain/PROJECT_BRAIN.md` (roadmap P1.4 "Apple Intelligence provider, dep removed, never built"; open question Q003 "provider priority when Apple Intelligence and a local server are both present") and `docs/roadmap-macos.md` (P1.4 "the zero-download system model on macOS 26+"; the open priority-chain decision).

## Global Constraints

- **macOS 26.0+ and Apple Silicon only.** The `foundation-models` crate requires macOS 26.0 to build and run. The dependency must be **target-gated** to macOS so the Windows `windows-latest` CI job (`cargo check --workspace` under MSVC, decision D007) still compiles. This is the exact failure L048 documents: commit 9163631 added `tauri-apple-intelligence = "0.2.1"` as an *un-gated* dependency, which broke the Windows build and was removed in 3c9783a. Do not repeat it.
- **Purely additive.** No existing provider arm may change behavior. The `local-server`, `openai-compat`, and `""` arms of `rehydrate_provider` and the `openai-compat`/local logic of `select_model` are untouched; we add a new arm beside them.
- **No download, no delete.** The system model has no on-disk files under `state.models_dir`. It must never appear as `downloaded: true` with a Delete control. Its "installed" state is availability, not a file.
- **No em/en dashes in user-facing copy.** Use a hyphen, comma, or colon (project-wide output rule).
- **Do not commit Ali's unrelated uncommitted work.** This branch has a large set of unrelated in-progress edits (autotext, insights, icons, benches, learnings, etc.). Commit only this feature's files by explicit pathspec; never `git add -A`, never `git stash`/`reset`/`checkout` to move uncommitted work.
- **In-process, not a child process.** `foundation-models` runs in-process, so the provider's `stop()`/`shutdown()` are no-ops and `is_local()` is `true` (it is a local model, so the polish-skip gate applies). It holds no external process, so `is_alive()` stays the default `true`.
- **Availability is a runtime fact, not a build fact.** `SystemLanguageModel::is_available()` can return false even on macOS 26+ (e.g. Apple Intelligence not enabled in System Settings). Every code path that offers or loads the model must check it at runtime and degrade to "unavailable" rather than error.

## Research findings (verified against source)

- **Groundwork that survived 9163631:** the `InferenceProvider::context_tokens()` trait method (llm.rs:165) and `scaled_timeout()` (llm.rs:127). **Everything else was removed in 3c9783a** (the un-gated Cargo dep and the "rehydration path" the commit message claimed). `grep -rn apple_intelligence crates/` currently returns nothing.
- **The three provider-selection seams** (all in `crates/teletype-desktop/src/commands/models.rs`):
  - `select_model` (line 170) - stops the old `state.inference` provider, spawns the new one, sets `settings.selected_llm_provider = "local-server"` (line 259) and `selected_llm_model = id` (line 258), then `emit_runtime_status`.
  - `rehydrate_provider` (line 835) - `match settings.selected_llm_provider.as_str()` with arms `"local-server"` (839), `"openai-compat"` (884), `""` (918), `other` (921).
  - `ensure_local_provider` (line 937) - lazy first-use load; no-ops unless `selected_llm_provider == "local-server"`.
- **The real `foundation-models` API** (docs.rs/foundation-models): `SystemLanguageModel::is_available() -> bool`; `LanguageModelSession::new() -> Result<...>`; `session.respond("...") -> Result<String>`. Requires macOS 26.0+ and Apple Silicon.
- **`ModelStatus` struct** (models.rs:29-56) already has `downloaded`, `selected`, `recommended`, `runs_on_host`, `specialty` fields. The UI reads `downloaded` to decide when to show the primary Download/Use action and the Delete control. For the system model we synthesize `downloaded: true` (it is "installed" = available) so the row reads as ready, but gate Delete on a separate condition (Task 6).
- **Q003 (provider priority):** still open in the brain. This plan resolves it as option (c) - the user picks explicitly in Settings > Models; on macOS 26+ the Apple Intelligence row carries a "Recommended" badge (zero download, zero disk, in-OS) but is not forced. No code auto-switches the provider.

## File map

| File | Change | Responsibility |
|---|---|---|
| `crates/teletype-desktop/Cargo.toml` | Modify | Target-gated `foundation-models` dependency (macOS only). |
| `crates/teletype-desktop/src/apple_intelligence.rs` | Create | `AppleIntelligenceProvider` (an `InferenceProvider`), `apple_intelligence_available()` runtime gate. macOS-only module. |
| `crates/teletype-desktop/src/lib.rs` | Modify | Register the module; (no handler change - reuses `select_model`/`list_models`). |
| `crates/teletype-desktop/src/commands/models.rs` | Modify | `"apple-intelligence"` arms in `rehydrate_provider` and a new `select` branch; `ensure_apple_intelligence_provider`; `apple-intelligence` entry in `model_list`. |
| `ui/src/screens/ModelsScreen.tsx` | Modify | Render the system-model row; hide Download and Delete for it; show a "Recommended" badge on macOS 26+. |
| `crates/teletype-desktop/src/apple_intelligence.rs` (`#[cfg(test)]`) | Create | Unit tests for the availability gate and provider trait conformance (mocked session). |
| `ui/src/test/modelsScreen.test.tsx` | Modify | Row renders; Download/Delete hidden for system model; select works; badge shown. |

## Interfaces

- **Produces (Task 2):**
  - `fn apple_intelligence_available() -> bool` in `teletype_desktop::apple_intelligence` - `true` only on macOS 26+ Apple Silicon with `SystemLanguageModel::is_available()`.
  - `struct AppleIntelligenceProvider` implementing `teletype_core::llm::InferenceProvider` with `model_id() -> "apple-intelligence"`, `model_name() -> &str`, `generate(...)`, `context_tokens() -> None`, `is_local() -> true`.
- **Consumes (Task 3 onward):** those two symbols; the `ModelStatus` fields `id`, `downloaded`, `selected`, `recommended`, `runs_on_host`; and the existing IPC `select_model(id)` / `list_models`.

---

### Task 1: Target-gated `foundation-models` dependency

**Files:**
- Modify: `crates/teletype-desktop/Cargo.toml:83-103` (inside `[target.'cfg(target_os = "macos")'.dependencies]`)

**Interfaces:**
- Produces: a `foundation_models` crate usable from a macOS-gated module.
- Consumes: nothing.

- [ ] **Step 1: Confirm the exact published crate name/version before editing**

The `foundation-models` crate is newer than the (removed) `tauri-apple-intelligence` wrapper. Confirm the current name and version so we do not pin a wrong one:

Run: `cargo search foundation-models`
Expected: a `foundation-models` (or similarly named) crate is listed. If the real crate is a different name (e.g. `apple-foundation-models`), use that name in Step 2 instead. Note the version.

If the crate cannot be confirmed, STOP and report back rather than guessing a name: the plan must not invent a dependency.

- [ ] **Step 2: Add the target-gated dependency**

Edit `crates/teletype-desktop/Cargo.toml` inside the existing `[target.'cfg(target_os = "macos")'.dependencies]` block (lines 83-103). Add a line (substitute the confirmed name/version from Step 1):

```toml
# Apple Intelligence system model (P1.4). macOS 26+ only; target-gated so the
# windows-latest CI job (D007) still compiles. An un-gated copy of this dep
# broke the Windows build and was reverted (L048) - keep it gated.
foundation-models = "0.x"
```

- [ ] **Step 3: Verify the Windows build still compiles**

This is the regression L048 was about. Run both:

Run: `cargo check -p teletype-desktop --target x86_64-pc-windows-msvc` (if the MSVC target is installed) OR, at minimum,
Run: `cargo check -p teletype-desktop --target x86_64-apple-darwin`
Expected: compiles on macOS. On a Windows/MSVC CI run the gated dep is absent, so no FoundationModels symbols are referenced. (If the Windows target is not installed locally, note it and rely on the `windows-latest` CI job in the verification phase.)

- [ ] **Step 4: Verify the lockfile only added the dep under macOS**

Run: `git diff --stat Cargo.lock`
Expected: `foundation-models` appears; it is a transitive addition, not a change to any existing crate version.

- [ ] **Step 5: Commit**

```bash
git add crates/teletype-desktop/Cargo.toml Cargo.lock
git commit -m "build(macos): add target-gated foundation-models dep for P1.4"
```

---

### Task 2: `AppleIntelligenceProvider` + availability gate (macOS-only module)

**Files:**
- Create: `crates/teletype-desktop/src/apple_intelligence.rs`
- Modify: `crates/teletype-desktop/src/lib.rs` (declare the module, gated)

**Interfaces:**
- Produces: `apple_intelligence_available() -> bool`; `AppleIntelligenceProvider` (`InferenceProvider`).
- Consumes: `foundation_models::{SystemLanguageModel, LanguageModelSession}`; `teletype_core::llm::{InferenceProvider, GenerationParams}`.

- [ ] **Step 1: Write the failing test first (availability gate shape + provider conformance)**

Create `crates/teletype-desktop/src/apple_intelligence.rs` starting with the test module and the public shape, so the test fails to compile against a missing `apple_intelligence_available`:

```rust
//! Apple Intelligence system-model provider (P1.4).
//!
//! macOS 26+ and Apple Silicon only. The weights live inside the OS, so there
//! is nothing to download or delete; "installed" means
//! `SystemLanguageModel::is_available()`. Runs in-process (no child), so
//! `stop`/`shutdown` are no-ops and `is_local` is `true`.

#[cfg(target_os = "macos")]
use foundation_models::{LanguageModelSession, SystemLanguageModel};
use teletype_core::llm::{GenerationParams, InferenceProvider};

/// True only when the OS actually has the model: macOS 26+, Apple Silicon,
/// and Apple Intelligence enabled in System Settings. Cheap to call; safe to
/// call on every list/select.
pub fn apple_intelligence_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        SystemLanguageModel::is_available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The system model as a polish/transform engine. No warm-up, no child
/// process: `LanguageModelSession::new` is the whole "load".
pub struct AppleIntelligenceProvider {
    session: LanguageModelSession,
}

impl InferenceProvider for AppleIntelligenceProvider {
    fn model_id(&self) -> &str {
        "apple-intelligence"
    }

    fn model_name(&self) -> &str {
        "Apple Intelligence"
    }

    fn generate(&self, prompt: &str, _params: GenerationParams) -> Result<String, String> {
        self.session.respond(prompt).map_err(|e| e.to_string())
    }

    fn context_tokens(&self) -> Option<u32> {
        // The OS does not publish a stable window size for the system model.
        None
    }

    fn is_local(&self) -> bool {
        true
    }
}

impl AppleIntelligenceProvider {
    /// Builds and returns a ready provider, or an error when the session
    /// cannot be created (model not actually available at this moment).
    #[cfg(target_os = "macos")]
    pub fn new() -> Result<Self, String> {
        let session = LanguageModelSession::new().map_err(|e| e.to_string())?;
        Ok(Self { session })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_is_a_bool_and_false_off_macos() {
        // The gate must compile and run everywhere; on non-macOS it is false.
        #[cfg(not(target_os = "macos"))]
        assert!(!apple_intelligence_available());
        #[cfg(target_os = "macos")]
        let _ = apple_intelligence_available(); // on mac: just must not panic
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn provider_reports_stable_identity_and_local() {
        // Only run when the model is present; skip otherwise (CI may be Intel
        // or pre-26) so the test is not platform-fragile.
        if !apple_intelligence_available() {
            return;
        }
        let p = AppleIntelligenceProvider::new().expect("session on available host");
        assert_eq!(p.model_id(), "apple-intelligence");
        assert!(p.is_local());
        assert_eq!(p.context_tokens(), None);
    }
}
```

Note: if Step 1 of Task 1 found the crate/type names differ from `foundation_models::{SystemLanguageModel, LanguageModelSession}`, use the confirmed names here. The test's *contract* (availability is a bool, false off-macOS; identity string `apple-intelligence`; `is_local()` true; `context_tokens()` None) is what is being pinned, not the exact binding spelling.

- [ ] **Step 2: Declare the module in `lib.rs` (gated)**

In `crates/teletype-desktop/src/lib.rs`, where other modules are declared, add:

```rust
#[cfg(target_os = "macos")]
mod apple_intelligence;
```

On non-macOS the module does not exist, so callers must reference it through a `#[cfg]`-gated helper (Task 3 provides that).

- [ ] **Step 3: Run the test to see it fail / compile**

Run: `cargo test -p teletype-desktop apple_intelligence`
Expected: on a macOS host the tests compile and the off-macOS assert is skipped; the identity test passes only when the model is present (else it early-returns). If `LanguageModelSession::new`/`respond` signatures differ, fix the two call sites to the real signatures and re-run. Do not invent signatures - read the crate's actual docs (docs.rs/foundation-models) for the exact `new`/`respond` return types.

- [ ] **Step 4: Commit**

```bash
git add crates/teletype-desktop/src/apple_intelligence.rs crates/teletype-desktop/src/lib.rs
git commit -m "feat(macos): add AppleIntelligenceProvider and availability gate"
```

---

### Task 3: Provider-selection arms (select + rehydrate + lazy load)

**Files:**
- Modify: `crates/teletype-desktop/src/commands/models.rs` (new `select_model` branch; new `rehydrate_provider` arm; new `ensure_apple_intelligence_provider`)

**Interfaces:**
- Consumes: `apple_intelligence_available()`, `AppleIntelligenceProvider::new()` from Task 2 (both macOS-gated).
- Produces: the ability to select, restore at startup, and lazily load the `apple-intelligence` provider, and to record `selected_llm_provider == "apple-intelligence"`.

- [ ] **Step 1: Add a non-macOS-safe constructor helper**

Because the module is `#[cfg(target_os = "macos")]`, the command code needs a single call site that compiles on all platforms. Add near the top of `models.rs` (with the other free functions):

```rust
/// Returns a ready Apple Intelligence provider, or an explanatory error.
/// Off-macOS (or when the OS has no model) this is an error the UI turns into
/// a friendly "not available here" - it is never a crash.
fn make_apple_intelligence_provider() -> Result<std::sync::Arc<dyn teletype_core::llm::InferenceProvider>, String> {
    if !crate::apple_intelligence_available_or_false() {
        return Err("Apple Intelligence is not available on this Mac.".into());
    }
    #[cfg(target_os = "macos")]
    {
        let p = crate::apple_intelligence::AppleIntelligenceProvider::new()?;
        Ok(std::sync::Arc::new(p))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Apple Intelligence is macOS only.".into())
    }
}

/// Cross-platform availability check that does not require naming the
/// macOS-gated module. Delegates to the module on macOS.
fn apple_intelligence_available_or_false() -> bool {
    #[cfg(target_os = "macos")]
    { crate::apple_intelligence::apple_intelligence_available() }
    #[cfg(not(target_os = "macos"))]
    { false }
}
```

- [ ] **Step 2: Add the `select_model` branch**

In `select_model` (models.rs:170), the current body assumes a catalog entry (`catalog::find(&id)`, lines 175-200). The system model is not a catalog entry, so branch before that. Insert right after the function's opening (after line 174, before `let entry = ...`):

```rust
    // The Apple Intelligence system model is not a catalog entry (there is no
    // download). Handle it here and return before any catalog lookup.
    if id == "apple-intelligence" {
        return select_apple_intelligence(app, state);
    }
```

Add the helper below `select_model`:

```rust
/// Selects the Apple Intelligence system model as the active provider.
/// Mirrors `select_model`'s swap discipline (BUG-007): stop the old provider
/// first, then install the new one. No warm-up, no file checks.
async fn select_apple_intelligence(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
) -> CommandResult<()> {
    let provider = make_apple_intelligence_provider()?;

    let old_engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(old) = old_engine {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            old.stop();
        })
        .await;
    }

    state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(provider);

    let mut settings = state.settings();
    settings.selected_llm_model = id.clone();
    settings.selected_llm_provider = "apple-intelligence".into();
    state.replace_settings(settings)?;
    emit_runtime_status(&app, &state);
    Ok(())
}
```

- [ ] **Step 3: Add the `rehydrate_provider` arm**

In `rehydrate_provider`'s `match` (models.rs:838), add an arm before `other =>` (line 921):

```rust
        "apple-intelligence" => {
            if !apple_intelligence_available_or_false() {
                crate::log_entry(
                    crate::LogLevel::Info,
                    "LLM restore: Apple Intelligence not available on this host; will fall back on first use",
                );
                return;
            }
            match make_apple_intelligence_provider() {
                Ok(provider) => {
                    state
                        .inference
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .replace(provider);
                    crate::log_entry(
                        crate::LogLevel::Success,
                        "LLM restored: Apple Intelligence".to_string(),
                    );
                    emit_runtime_status(app, &state);
                }
                Err(e) => crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("LLM restore: Apple Intelligence failed: {e}"),
                ),
            }
        }
```

- [ ] **Step 4: Add the lazy-load path**

Mirror `ensure_local_provider` (models.rs:937) for the new provider. Add:

```rust
/// Lazily loads the Apple Intelligence provider on first use when it is the
/// selected provider but not yet resident. No-op when a provider is already
/// loaded or a different provider is selected. Blocking-safe: the session
/// load is in-process, so it may run on the dictation worker thread.
pub fn ensure_apple_intelligence_provider(app: &tauri::AppHandle) {
    let state = app.state::<crate::AppState>();
    if state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .is_some()
    {
        return;
    }
    let settings = state.settings();
    if settings.selected_llm_provider != "apple-intelligence" {
        return;
    }
    match make_apple_intelligence_provider() {
        Ok(provider) => {
            state
                .inference
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(provider);
            emit_runtime_status(app, &state);
        }
        Err(e) => crate::log_entry(
            crate::LogLevel::Warn,
            format!("LLM first-use load: Apple Intelligence unavailable: {e}"),
        ),
    }
}
```

- [ ] **Step 5: Wire the lazy loader into the dictation first-use path**

Find where `ensure_local_provider` is invoked from the dictation worker (grep `ensure_local_provider` in `crates/teletype-desktop/src/`). Add an `ensure_apple_intelligence_provider(&app)` call alongside it (same guard context). Do not change `ensure_local_provider` itself.

- [ ] **Step 6: Compile + run tests**

Run: `cargo check -p teletype-desktop --target x86_64-apple-darwin`
Expected: compiles on macOS. Then:
Run: `cargo test -p teletype-desktop --target x86_64-apple-darwin`
Expected: existing tests still pass; the new arms compile. (The arm logic itself is exercised by the manual check in the verification phase, since it needs a real macOS 26 host.)

- [ ] **Step 7: Commit**

```bash
git add crates/teletype-desktop/src/commands/models.rs
git commit -m "feat: select/rehydrate/lazily-load the Apple Intelligence provider"
```

---

### Task 4: Surface the system model in `list_models`

**Files:**
- Modify: `crates/teletype-desktop/src/commands/models.rs` (`model_list`, line 95)

**Interfaces:**
- Consumes: `apple_intelligence_available_or_false()` (Task 3); the `ModelStatus` struct (models.rs:29).
- Produces: a `ModelStatus` row for `apple-intelligence` appended to the model list, only when available.

- [ ] **Step 1: Add the row conditionally in `model_list`**

At the end of `model_list` (before it returns the `Vec<ModelStatus>`), append the system model when available:

```rust
    // The Apple Intelligence system model: offered only when the OS has it.
    // `downloaded: true` means "ready to use" (it is in the OS); the UI hides
    // Download and Delete for it via its own `id == "apple-intelligence"` check.
    if apple_intelligence_available_or_false() {
        list.push(ModelStatus {
            id: "apple-intelligence".into(),
            name: "Apple Intelligence".into(),
            size_mb: 0,
            description: "Apple's on-device system model. No download, no disk used.".into(),
            specialty: teletype_inference::catalog::Specialty::ALL_ROUND,
            downloaded: true,
            selected: selected_id == "apple-intelligence",
            recommended: true,
            polish_capable: true,
            verdict: None,
            verdict_note: None,
            requires_license_accept: false,
            license_name: None,
            runs_on_host: true,
        });
    }
```

IMPORTANT: match the actual `ModelStatus` field set from models.rs:29-56 (field names/types may differ from the sketch above). Read the struct, then fill every field to a value that compiles; do not invent fields. `verdict`/`verdict_note` and the license fields may be `Option<...>` or have different names - set them to their no-op/default values.

- [ ] **Step 2: Compile**

Run: `cargo check -p teletype-desktop --target x86_64-apple-darwin`
Expected: compiles after reconciling the struct fields.

- [ ] **Step 3: Commit**

```bash
git add crates/teletype-desktop/src/commands/models.rs
git commit -m "feat: list the Apple Intelligence system model when available"
```

---

### Task 5: Models screen row (select; no download; no delete; recommended badge)

**Files:**
- Modify: `ui/src/screens/ModelsScreen.tsx`

**Interfaces:**
- Consumes: `list_models` (now includes the `apple-intelligence` row); `select_model(id)`.
- Produces: a UI row for the system model whose primary action is "Use" (select), with no Download control and no Delete control.

- [ ] **Step 1: Add a test first**

In `ui/src/test/modelsScreen.test.tsx`, add a test that mocks `list_models` to include an `apple-intelligence` row (`downloaded: true`) and asserts: the row renders its name; no "Download" button appears for it; no "Delete" button appears for it; clicking "Use" invokes `select_model` with `apple-intelligence`.

```tsx
it("renders the system model row with Use only (no download, no delete)", async () => {
  const list = vi
    .fn()
    .mockResolvedValue([
      {
        id: "apple-intelligence",
        name: "Apple Intelligence",
        downloaded: true,
        selected: false,
        recommended: true,
        polishCapable: true,
        runsOnHost: true,
      },
    ]);
  const select = vi.fn().mockResolvedValue(undefined);
  // wire list/select into the existing invoke mock pattern used in this file
  render(<ModelsScreen />);
  const row = screen.getByText("Apple Intelligence");
  expect(row).toBeTruthy();
  expect(screen.queryByText(/Download/i)).toBeNull();
  expect(screen.queryByText(/Delete/i)).toBeNull();
  const useBtn = screen.getByRole("button", { name: /Use/i });
  await fireEvent.click(useBtn);
  expect(select).toHaveBeenCalledWith(expect.objectContaining({ id: "apple-intelligence" }));
});
```

Adapt the mock wiring to the existing pattern in that file (how it currently stubs `list_models`/`select_model`). The assertions are the contract.

- [ ] **Step 2: Run the test to see it fail**

Run: `npm test -- modelsScreen` (from `ui/`)
Expected: FAIL (no special handling for `apple-intelligence` yet; a Download or Delete control likely renders).

- [ ] **Step 3: Gate Download and Delete on the row id**

In the row render (where the primary `ActionButton`/Download and the `DeleteButton` are placed, around ModelsScreen.tsx:779 and :856), add an `isSystemModel` guard:

```tsx
const isSystemModel = m.id === "apple-intelligence";
```

- For the primary action: when `isSystemModel && m.downloaded`, render the "Use" select control and do NOT render the "Download" control.
- For `DeleteButton`: pass `visible={!isSystemModel && m.downloaded}` (the existing `visible` prop already hides the control when false; see DeleteButton at ModelsScreen.tsx:203).

- [ ] **Step 4: Show a "Recommended" badge**

Where the row renders its `recommended` affordance, the system row already carries `recommended: true` from Task 4. If the existing badge only shows for catalog models, confirm it shows for `apple-intelligence` (it should, since it reads `m.recommended`). No new code needed if the badge is purely `m.recommended`-driven; add nothing otherwise.

- [ ] **Step 5: Run the test to see it pass**

Run: `npm test -- modelsScreen` (from `ui/`)
Expected: PASS.

- [ ] **Step 6: Typecheck**

Run: `npx tsc --noEmit` (from `ui/`)
Expected: no type errors.

- [ ] **Step 7: Commit**

```bash
git add ui/src/screens/ModelsScreen.tsx ui/src/test/modelsScreen.test.tsx
git commit -m "feat(ui): show Apple Intelligence model row (select, no download/delete)"
```

---

## Verification

- **Rust (macOS):** `cargo test -p teletype-core -p teletype-desktop --target x86_64-apple-darwin` green; `cargo clippy -p teletype-desktop --all-targets --target x86_64-apple-darwin` no new warnings.
- **Windows regression (the L004/L048 guard):** the `windows-latest` CI job's `cargo check --workspace` stays green (the gated dep is absent there). If the target is available locally, run `cargo check -p teletype-desktop --target x86_64-pc-windows-msvc`.
- **Frontend:** `ui/` `npx tsc --noEmit` and `npm test` green.
- **Manual (macOS 26+, Apple Intelligence enabled, via `/ali-app-dev-loop`):**
  1. Open Settings > Models. The "Apple Intelligence" row is present with a "Recommended" badge, no Download button, no Delete button.
  2. Click "Use". The readiness panel shows it loaded; no file appears under `~/Library/.../models`.
  3. Dictate a phrase and confirm it is polished (the in-OS model ran).
  4. Restart the app and confirm the selection is restored (the `rehydrate_provider` arm fired) - Developer tab shows "LLM restored: Apple Intelligence".
  5. Switch back to a `local-server` model and confirm it still loads (existing path untouched).
- **Manual (availability-negative):** on a Mac where Apple Intelligence is off (or an Intel Mac), the row is absent from the list and selecting it is impossible; no crash.

## Out of scope (Part 2)

- Private Cloud Compute / off-device generation (the `foundation-models` crate exposes it; this plan uses the on-device `respond` only).
- Tool-calling / structured-output (`respond_generated`) for the Command intent - that is the Needle 3 decision path, a separate concern.
- Forcing the provider automatically (Q003 resolved as explicit user choice + a Recommended badge, not auto-switch).
- Windows/Linux Apple Intelligence (nonexistent; gated to macOS 26+).

## Self-review (run after writing, before executing)

- **Spec coverage:** P1.4 provider (Tasks 2-4), zero-download + no-delete row (Task 5, and Task 4 `downloaded: true` + id-gated controls), Q003 priority (Global Constraints + Task 5 recommended badge, no auto-switch), Windows-build safety (Task 1 + verification). All covered.
- **Placeholder scan:** the two spots that depend on an external, unverified API are called out explicitly (Task 1 Step 1 "confirm the crate name/version before editing"; Task 2 Step 3 "read docs.rs for the exact new/respond signatures"). Those are deliberate research steps, not TBDs to skip - if the crate name cannot be confirmed, the task says to stop and report, not to guess.
- **Type consistency:** `apple_intelligence_available_or_false` and `make_apple_intelligence_provider` are defined once (Task 3 Step 1) and referenced consistently in Tasks 3 and 4. `AppleIntelligenceProvider::new()` returns `Result<Self, String>` in Task 2 and is consumed as such in Task 3. `model_id()` is `"apple-intelligence"` everywhere (provider, list row, select branch, UI guard). `ModelStatus` field fill is flagged to reconcile against the real struct rather than trust the sketch.
