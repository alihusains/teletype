//! Model commands: LLM catalog/download/select, secrets, Ollama, speech models, runtime and file transcription.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use super::*;
use crate::AppState;
use teletype_core::llm::InferenceProvider;

// ---- Models ----

/// What this row is worth choosing for, shipped to the UI as a chip.
///
/// `measured` is the half the old `purpose` string could not carry: the rail
/// must tell a role the bakeoff proved (a chip that reads as a score) apart
/// from a role inferred from the model's training or the publisher's claim
/// (a chip that reads as a description). Both are shown, differently.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecialtyStatus {
    /// The one-phrase job, e.g. "Dictation cleanup".
    pub tag: String,
    /// True when the tag is backed by a benchmark run in this repo.
    pub measured: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub size_mb: u32,
    pub description: String,
    /// What the row is worth choosing for, shown as a chip in Settings.
    pub specialty: SpecialtyStatus,
    pub downloaded: bool,
    pub selected: bool,
    pub recommended: bool,
    /// False for a model the app never loads as a polish or transform engine
    /// (the Command Mode decider, extraction tools). Downloadable, not
    /// selectable, and the UI keeps them out of the polish list.
    pub polish_capable: bool,
    pub verdict: String,
    pub verdict_note: String,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub requires_license_accept: bool,
    pub attribution: Option<String>,
    /// False when this host's llama-server build has no GPU backend and the row
    /// is too large to be comfortable on CPU. The full list still offers it (a
    /// user with a 32 GB machine can choose to wait); the first-run shortlist
    /// and the "works well here" grouping do not.
    pub runs_on_host: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelStatus {
    pub id: String,
    pub name: String,
    pub engine: String,
    pub engine_label: String,
    pub platform_label: String,
    pub size_mb: u32,
    pub description: String,
    pub recommended: bool,
    pub english_only: bool,
    pub downloaded: bool,
    pub selected: bool,
    pub speed: f64,
    pub accuracy: f64,
    pub min_ram_gb: u32,
    pub language_label: String,
    /// Language codes the model transcribes, or `None` for every Whisper
    /// language (99). Used by the Settings language picker.
    pub languages: Option<Vec<String>>,
}

/// Pure list behind [`list_models`].
///
/// The whole catalog is always offered (every row is downloadable and, with a
/// CPU-only llama-server, every GGUF still loads). What the host decides is
/// which rows a *first run* sees: `shortlist` trades the full list for
/// [`teletype_inference::catalog::polish_shortlist_ids`], so a Windows user is
/// not pointed at a 1.9 GB model that takes a minute per polish on a CPU build,
/// plus whatever the user already selected so a returning user is never shown a
/// screen that does not mention their own model.
///
/// `gpu_accelerated` is [`teletype_inference::catalog::host_gpu_accelerated`],
/// not an OS check: the axis is the bundled llama-server build (Metal on Apple
/// Silicon, CPU-only on Windows), and every row carries the answer in
/// `runs_on_host` so the screen can label the rows it still lists.
fn model_list(
    gpu_accelerated: bool,
    models_dir: &std::path::Path,
    selected_id: &str,
    shortlist: bool,
) -> Vec<ModelStatus> {
    let status = |e: &'static teletype_inference::catalog::CatalogEntry| ModelStatus {
        id: e.id.into(),
        name: e.name.into(),
        size_mb: e.size_mb,
        description: e.description.into(),
        specialty: SpecialtyStatus {
            tag: e.specialty.tag.into(),
            measured: e.specialty.measured,
        },
        downloaded: e.is_downloaded(models_dir),
        selected: selected_id == e.id,
        recommended: e.recommended,
        polish_capable: e.polish_capable,
        verdict: e.verdict.label().into(),
        verdict_note: e.verdict_note.into(),
        license_name: e.license_name.map(str::to_string),
        license_url: e.license_url.map(str::to_string),
        requires_license_accept: e.requires_license_accept,
        attribution: e.attribution.map(str::to_string),
        runs_on_host: e.runs_on_host(gpu_accelerated),
    };

    if !shortlist {
        let mut models: Vec<ModelStatus> = teletype_inference::catalog::CATALOG
            .iter()
            .map(status)
            .collect();
        // Stable sort by the catalog's explicit priority so the list order is
        // independent of array position (S1-mini first, then the rest).
        models.sort_by_key(|m| {
            teletype_inference::catalog::find(&m.id)
                .map(|e| e.sort_order)
                .unwrap_or(u32::MAX)
        });
        if apple_intelligence_available() {
            models.push(apple_intelligence_row(selected_id));
        }
        return models;
    }

    let ids = teletype_inference::catalog::polish_shortlist_ids(gpu_accelerated);
    let mut rows: Vec<ModelStatus> = ids
        .iter()
        .filter_map(|id| teletype_inference::catalog::find(id))
        .map(status)
        .collect();
    // A user who already chose a model keeps seeing it, even if that choice is
    // outside this host's shortlist. Dropping it would look like the app forgot
    // their setting and silently use it anyway.
    if let Some(entry) = teletype_inference::catalog::find(selected_id) {
        if entry.polish_capable && !ids.contains(&entry.id) {
            rows.push(status(entry));
        }
    }
    if apple_intelligence_available() {
        rows.push(apple_intelligence_row(selected_id));
    }
    rows
}

/// The Apple Intelligence system model is not a catalog entry: it ships in the
/// OS, so it has no download, no disk footprint, and no license. It appears as
/// a ready, selectable row on hosts where the OS exposes it.
fn apple_intelligence_row(selected_id: &str) -> ModelStatus {
    ModelStatus {
        id: "apple-intelligence".into(),
        name: "Apple Intelligence".into(),
        size_mb: 0,
        description: "Apple's on-device system model: no download, no disk use."
            .into(),
        specialty: SpecialtyStatus {
            tag: "On-device polish".into(),
            measured: false,
        },
        // "Downloaded" means ready to use; the model lives in the OS.
        downloaded: true,
        selected: selected_id == "apple-intelligence",
        recommended: true,
        polish_capable: true,
        verdict: "Not tested".into(),
        verdict_note: "Apple's on-device model; we have not benchmarked it".into(),
        license_name: None,
        license_url: None,
        requires_license_accept: false,
        attribution: None,
        runs_on_host: true,
    }
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    shortlist: Option<bool>,
) -> CommandResult<Vec<ModelStatus>> {
    let settings = state.settings();
    Ok(model_list(
        teletype_inference::catalog::host_gpu_accelerated(),
        &state.models_dir,
        &settings.selected_llm_model,
        shortlist.unwrap_or(false),
    ))
}

/// Runtime availability of the Apple Intelligence system model. The provider
/// module is macOS-only, so on other platforms this is unconditionally false
/// and the caller never names the module.
fn apple_intelligence_available() -> bool {
    #[cfg(target_os = "macos")]
    {
        crate::apple_intelligence::apple_intelligence_available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Builds a ready Apple Intelligence provider, or an error the UI turns into a
/// friendly "not available here". Never panics off-macOS or when the OS has no
/// model.
fn make_apple_intelligence_provider()
-> Result<Arc<dyn teletype_core::llm::InferenceProvider>, CommandError> {
    if !apple_intelligence_available() {
        return Err(CommandError::model(
            "Apple Intelligence is not available on this Mac.".to_string(),
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let provider = crate::apple_intelligence::AppleIntelligenceProvider::new()
            .map_err(CommandError::model)?;
        Ok(Arc::new(provider) as Arc<dyn teletype_core::llm::InferenceProvider>)
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Unreachable: the availability guard above is false off-macOS.
        Err(CommandError::model(
            "Apple Intelligence is macOS only.".to_string(),
        ))
    }
}

/// Selects the Apple Intelligence system model as the active provider.
/// Mirrors `select_model`'s swap discipline (BUG-007): stop the old provider
/// first, then install the new one. No warm-up, no file checks.
async fn select_apple_intelligence(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let provider = make_apple_intelligence_provider()?;

    let old_engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(old) = old_engine {
        crate::log_entry(
            crate::LogLevel::Info,
            "stopping previous LLM model before loading Apple Intelligence".to_string(),
        );
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
    settings.selected_llm_model = "apple-intelligence".into();
    settings.selected_llm_provider = "apple-intelligence".into();
    state.replace_settings(settings)?;
    crate::log_entry(
        crate::LogLevel::Success,
        "Apple Intelligence selected as the active model".to_string(),
    );
    emit_runtime_status(&app, &state);
    Ok(())
}

#[tauri::command]
pub async fn select_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    // The Apple Intelligence system model is not a catalog entry (there is no
    // download), so it is handled before any catalog lookup.
    if id == "apple-intelligence" {
        return select_apple_intelligence(app, state).await;
    }
    let entry =
        teletype_inference::catalog::find(&id).ok_or_else(|| format!("Unknown model '{id}'"))?;
    // Selecting a model here sets the polish and transform engine, so a decider
    // or an extraction tool would load successfully and then make every preset
    // return the input untouched. The rail keeps these out of the list; this
    // refuses them for anything that arrives by another route, such as a saved
    // setting pointing at an id whose role later changed.
    if !entry.polish_capable {
        return Err(CommandError::user(format!(
            "'{id}' is not a polish model. {desc}",
            desc = entry.description
        )));
    }
    // License acceptance gates download only; once installed the model is usable.
    if !entry.is_downloaded(&state.models_dir) {
        return Err(CommandError::model(format!(
            "Model '{id}' is not downloaded yet."
        )));
    }

    let path = entry.entrypoint(&state.models_dir);
    if !path.exists() {
        return Err(CommandError::model(format!(
            "Model file for '{id}' is missing. Re-download it."
        )));
    }

    // BUG-007: stop the old provider BEFORE spawning the new one. The old
    // server is ~5 GB resident; spawning first doubles that and fails on a
    // memory-strapped Mac, leaving "Select did nothing".
    // With EngineHandle, we swap the Arc atomically. The old provider is
    // stopped in the background and lives until in-flight dictations finish.
    let old_engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(old) = old_engine {
        crate::log_entry(
            crate::LogLevel::Info,
            "stopping previous LLM model before loading the new one".to_string(),
        );
        // stop() kills the child llama-server and waits for it to exit.
        let _ = tauri::async_runtime::spawn_blocking(move || {
            old.stop();
        })
        .await;
    }

    // warm_up uses reqwest::blocking and must not run on a tokio worker
    // (dropping the client there panics with "Cannot drop a runtime…").
    let spawn_id = id.clone();
    let spawn_name = entry.name.to_string();
    let spawn_path = path.clone();
    crate::log_entry(
        crate::LogLevel::Info,
        format!("loading LLM model: {spawn_id} ({spawn_name})"),
    );
    let started = std::time::Instant::now();
    let provider = tauri::async_runtime::spawn_blocking(move || {
        let provider = teletype_inference::ServerProvider::new(&spawn_id, spawn_name, &spawn_path);
        provider.warm_up()?;
        Ok::<_, String>(provider)
    })
    .await
    .map_err(|e| format!("warm-up task: {e}"))??;
    crate::log_entry_ms(
        crate::LogLevel::Success,
        format!(
            "LLM model loaded: {} ({})",
            provider.model_name(),
            provider.model_id()
        ),
        started.elapsed().as_millis() as u64,
    );

    state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(Arc::new(provider));

    let mut settings = state.settings();
    settings.selected_llm_model = id.clone();
    settings.selected_llm_provider = "local-server".into();
    state.replace_settings(settings)?;
    tracing::info!(model = %id, "llama-server provider ready");
    // The transform model changed (and the old engine is being stopped in the
    // background): tell the readiness panel now instead of on its next 5 s poll.
    emit_runtime_status(&app, &state);
    Ok(())
}

/// Removes a single-file LLM model's bytes from disk.
///
/// The path is the same one `CatalogEntry::is_downloaded`/`entrypoint` use, so
/// deletion never touches a file the rest of the app does not recognize.
fn remove_llm_model_files(
    models_dir: &std::path::Path,
    id: &str,
    is_multi_shard: bool,
) -> std::io::Result<()> {
    if is_multi_shard {
        std::fs::remove_dir_all(models_dir.join(id))
    } else {
        std::fs::remove_file(models_dir.join(format!("{id}.gguf")))
    }
}

/// Removes a speech model's bytes from disk.
///
/// Uses `install_path` (the "where the bytes live" layout, not the
/// `provider_path` the loader is handed) so Core ML's `fluid/<repo>` and
/// sherpa's extracted directory are both addressed correctly.
fn remove_speech_model_files(
    models_dir: &std::path::Path,
    entry: &teletype_speech::catalog::SpeechModel,
) -> std::io::Result<()> {
    let path = entry.install_path(models_dir);
    match path.metadata() {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(&path),
        _ => std::fs::remove_file(&path),
    }
}

/// Deletes a downloaded catalog model's on-disk files.
///
/// If the model is the one currently in use the engine is stopped first (the
/// same stop-the-old-server pattern [`select_model`] uses), so the llama-server
/// child holding the file is gone before the file is removed, and the settings
/// are left pointing at no local model so the readiness panel reflects that.
#[tauri::command]
pub async fn delete_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let entry =
        teletype_inference::catalog::find(&id).ok_or_else(|| format!("Unknown model '{id}'"))?;
    if !entry.is_downloaded(&state.models_dir) {
        return Err(CommandError::model(format!(
            "Model '{id}' is not downloaded, so there is nothing to delete."
        )));
    }

    // Stop the engine if this is the live local-server model, and clear the
    // selection so the app does not try to load a file we are about to remove.
    let settings = state.settings();
    let in_use = settings.selected_llm_provider == "local-server"
        && settings.selected_llm_model == id;
    if in_use {
        let old_engine = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(old) = old_engine {
            let _ = tauri::async_runtime::spawn_blocking(move || old.stop()).await;
        }
        state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        let mut settings = state.settings();
        settings.selected_llm_model = String::new();
        settings.selected_llm_provider = String::new();
        state.replace_settings(settings)?;
    }

    let models_dir = state.models_dir.clone();
    let id2 = id.clone();
    let is_multi = !entry.shards.is_empty();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        remove_llm_model_files(&models_dir, &id2, is_multi).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("delete task: {e}"))?
    .map_err(CommandError::internal)?;

    crate::log_entry(
        crate::LogLevel::Info,
        format!("deleted LLM model: {id}"),
    );
    emit_runtime_status(&app, &state);
    Ok(())
}

/// Deletes a downloaded speech model's on-disk files.
///
/// If the model is the user's current dictation model the owning engine slot
/// is unloaded first, then the selection is reset to this host's default so
/// dictation keeps working (the existing "pick a model" path re-downloads it
/// if the default is not on disk).
#[tauri::command]
pub async fn delete_speech_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let entry = teletype_speech::catalog::find(&id)
        .ok_or_else(|| CommandError::model(format!("Unknown speech model '{id}'")))?;

    // What "installed" means per engine already lives in `speech_model_installed`;
    // mirror it so we only ever delete files that are actually present.
    let fluid_available = teletype_fluid::bridge_built();
    if !speech_model_installed(entry, &state.models_dir, fluid_available) {
        return Err(CommandError::model(format!(
            "{} is not downloaded, so there is nothing to delete.",
            entry.name
        )));
    }

    let settings = state.settings();
    let in_use = settings.selected_speech_model == id;
    if in_use {
        // Unload the engine slot that serves this model so its files are not
        // held open by a live provider.
        unload_speech_slot_for(&state, entry);
        let mut settings = state.settings();
        settings.selected_speech_model = crate::commands::default_speech_model_id().into();
        state.replace_settings(settings)?;
    }

    let models_dir = state.models_dir.clone();
    let entry_owned = entry.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        remove_speech_model_files(&models_dir, &entry_owned).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("delete task: {e}"))?
    .map_err(CommandError::internal)?;

    crate::log_entry(
        crate::LogLevel::Info,
        format!("deleted speech model: {id}"),
    );
    emit_runtime_status(&app, &state);
    Ok(())
}

/// Unloads whichever `SpeechModelManager` slot serves this catalog model's
/// engine, so deleting the model's files never fights a live provider.
fn unload_speech_slot_for(state: &AppState, entry: &teletype_speech::catalog::SpeechModel) {
    use teletype_speech::catalog::Engine;
    match entry.engine {
        Engine::ParakeetCoreml | Engine::SherpaParakeet | Engine::Qwen3Asr => state.neural.unload(),
        Engine::ParakeetCpp => state.parakeet_cpp.unload(),
        Engine::Parakeet => state.parakeet.unload(),
        Engine::Whisper => state.speech.unload(),
    }
}

/// Downloads a catalog model (single-file or multi-shard) with SHA-256
/// verification. `license_accepted` must be true for gated entries.
///
/// Emits `model-download-progress` events while downloading.
#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    license_accepted: Option<bool>,
) -> CommandResult<String> {
    let entry =
        teletype_inference::catalog::find(&id).ok_or_else(|| format!("Unknown model '{id}'"))?;
    if entry.requires_license_accept && license_accepted != Some(true) {
        return Err(CommandError::model(format!(
            "Model '{id}' is under a restrictive license. Read and accept it first."
        )));
    }

    let models_dir = state.models_dir.clone();
    let event_id = id.clone();

    // Clone for the post-join cancelled-event emit: the original moves into
    // the progress callback closure below.
    let app_emit = app.clone();
    // Separate clone for the post-join status emit below.
    let app_status = app.clone();

    // Register a cancel flag so `pause_download` can stop this download.
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut map = state
            .download_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.insert(id.clone(), cancel_flag.clone());
    }
    let cancel_for_task = Some(cancel_flag);
    let models_dir_for_task = models_dir.clone();

    let cb: teletype_inference::ProgressFn = std::sync::Arc::new(move |mut p| {
        // Tag progress with kind so the UI can route speech vs LLM rows.
        p.id = event_id.clone();
        let mut payload = serde_json::to_value(&p).unwrap_or_default();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("kind".into(), serde_json::Value::String("llm".into()));
        }
        let _ = app.emit("model-download-progress", payload);
    });

    let result = tauri::async_runtime::spawn_blocking(move || {
        teletype_inference::download_entry_with_cancel(
            entry,
            &models_dir_for_task,
            Some(cb),
            cancel_for_task,
        )
    })
    .await
    .map_err(|e| format!("download task: {e}"))?;

    // Clean up the cancel flag whether the download succeeded, failed, or paused.
    {
        let mut map = state
            .download_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.remove(&id);
    }

    // If the user cancelled outright (not paused), delete the partial file(s)
    // and report the stop as `cancelled` rather than an error.
    let hard_cancelled = {
        let mut hard = state
            .download_cancel_hard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        hard.remove(&id)
    };
    if let Err(e) = result {
        if hard_cancelled {
            for part in partial_files_to_delete(&models_dir, &id) {
                let _ = std::fs::remove_file(&part);
            }
            let payload = serde_json::json!({
                "id": id,
                "kind": "llm",
                "status": "cancelled",
                "fileName": String::new(),
                "fileIndex": 0u32,
                "fileCount": 0u32,
                "fileDownloadedBytes": 0u64,
                "fileTotalBytes": 0u64,
                "downloadedBytes": 0u64,
                "totalBytes": 0u64,
                "percent": 0.0f64,
                "speedBps": 0.0f64,
                "etaSeconds": serde_json::Value::Null,
                "error": serde_json::Value::Null,
            });
            let _ = app_emit.emit("model-download-progress", payload);
            return Err(CommandError::internal("download cancelled"));
        }
        return Err(CommandError::internal(e));
    }
    // The model is installed: the Models screen and the readiness panel's
    // "downloaded" claim can both change from this, so push the new status
    // rather than waiting for the panel's next 5 s poll.
    emit_runtime_status(&app_status, &state);
    result
        .map(|handle| handle.to_string_lossy().to_string())
        .map_err(CommandError::internal)
}

/// Cancels an active model download (speech or polish) and deletes its
/// partial file(s). Idempotent: cancelling a download that already
/// finished is not an error.
#[tauri::command]
pub async fn cancel_download(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    // Mark hard-cancel first so the download task's cleanup path sees it
    // even if the loop finishes in the same instant.
    {
        let mut hard = state
            .download_cancel_hard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        hard.insert(id.clone());
    }
    // Set the existing pause flag if one is registered; the download loop
    // polls it and stops, after which the command's cleanup path handles
    // deletion and the `cancelled` event. A missing flag just means no
    // active download (or one in a pre-flight phase) — not an error.
    {
        let map = state
            .download_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(flag) = map.get(&id) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
    Ok(())
}

/// Partial `.part` files left by an interrupted download of `id`: the
/// single-file `<id>.gguf.part` plus any `<id>/*.part` shards. Finished
/// shards (e.g. `<id>/part-1.gguf`) are not touched.
fn partial_files_to_delete(models_dir: &std::path::Path, id: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let single = models_dir.join(format!("{id}.gguf.part"));
    if single.is_file() {
        out.push(single);
    }
    let shard_dir = models_dir.join(id);
    if let Ok(entries) = std::fs::read_dir(&shard_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().ends_with(".part"))
            {
                out.push(path);
            }
        }
    }
    out
}

/// Pauses an active model download by setting its cancel flag.
/// The partial `.part` file is preserved on disk for a later resume.
#[tauri::command]
pub async fn pause_download(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let map = state
        .download_cancel
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let flag = map
        .get(&id)
        .ok_or_else(|| "no active download for '{id}'".to_string())?;
    flag.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// Resumes a previously paused model download.
/// Re-invokes the download for the same model id; the existing `.part`
/// file is detected and the transfer continues from where it left off.
#[tauri::command]
pub async fn resume_download(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<String> {
    // Reset the cancel flag so the new download task doesn't immediately pause.
    {
        let map = state
            .download_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(flag) = map.get(&id) {
            flag.store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }
    // Delegate to the same logic as download_model (no license re-check needed
    // since the user already accepted it to start the download).
    download_model(app, state, id, Some(true)).await
}

// ---- LLM secrets + OpenAI-compatible connector ----

/// Stores an API key in the OS keychain. Never written to settings.json.
#[tauri::command]
pub async fn set_llm_secret(provider_id: String, secret: String) -> CommandResult<()> {
    crate::secrets::set_secret(&provider_id, &secret).map_err(CommandError::internal)
}

/// Whether a key is stored (does not return the key itself).
#[tauri::command]
pub async fn has_llm_secret(provider_id: String) -> CommandResult<bool> {
    Ok(crate::secrets::has_secret(&provider_id))
}

/// Removes a stored API key.
#[tauri::command]
pub async fn clear_llm_secret(provider_id: String) -> CommandResult<()> {
    crate::secrets::clear_secret(&provider_id).map_err(CommandError::internal)
}

/// Activates the OpenAI-compatible connector using settings + keychain key
/// and installs it as the live inference provider.
#[tauri::command]
pub async fn select_openai_provider(state: State<'_, AppState>) -> CommandResult<()> {
    let settings = state.settings();
    let base_url = settings.openai_base_url.clone();
    let api_key = read_api_key(&base_url);
    let model = settings.openai_model.clone();

    // Probe first so the UI gets a clear error instead of a dead provider.
    let probe = {
        let p = teletype_inference::OpenAiCompatProvider::new(
            "openai-compat",
            "probe",
            teletype_inference::OpenAiCompatConfig::new(
                base_url.clone(),
                api_key.clone(),
                model.clone(),
            ),
        );
        tauri::async_runtime::spawn_blocking(move || {
            p.test_connection(std::time::Duration::from_secs(10))
        })
        .await
        .map_err(|e| format!("probe task: {e}"))??
    };
    tracing::info!(detail = %probe, "openai-compat provider ready");

    let provider = teletype_inference::OpenAiCompatProvider::new(
        "openai-compat",
        format!("API ({model})"),
        teletype_inference::OpenAiCompatConfig::new(base_url, api_key, model),
    );

    let mut settings = state.settings();
    settings.selected_llm_provider = "openai-compat".into();
    state.replace_settings(settings)?;

    let old = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(Arc::new(provider));
    // Previous local-server Drop kills a child process; keep that off the async runtime.
    if let Some(old) = old {
        let _ = tauri::async_runtime::spawn_blocking(move || drop(old)).await;
    }
    Ok(())
}

/// One-shot connection test against the OpenAI-compatible settings.
#[tauri::command]
pub async fn test_llm_connection(state: State<'_, AppState>) -> CommandResult<String> {
    let settings = state.settings();
    let base_url = settings.openai_base_url.clone();
    let api_key = read_api_key(&base_url);
    let provider = teletype_inference::OpenAiCompatProvider::new(
        "openai-compat",
        "test",
        teletype_inference::OpenAiCompatConfig::new(
            settings.openai_base_url,
            api_key,
            settings.openai_model,
        ),
    );
    Ok(tauri::async_runtime::spawn_blocking(move || {
        provider.test_connection(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| CommandError::internal(format!("test task: {e}")))??)
}

/// Detects whether Ollama is running at `localhost:11434`.
#[tauri::command]
pub async fn detect_ollama() -> CommandResult<bool> {
    tauri::async_runtime::spawn_blocking(|| {
        reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(2000))
            .build()
            .ok()
            .and_then(|c| c.get("http://127.0.0.1:11434/api/tags").send().ok())
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    })
    .await
    .map_err(|e| CommandError::internal(e.to_string()))
}

/// Lists the user's Ollama models with verdicts from our benchmark data.
#[tauri::command]
pub async fn list_ollama_models() -> CommandResult<Vec<OllamaModelInfo>> {
    tauri::async_runtime::spawn_blocking(|| {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| format!("client: {e}"))?;
        let resp = client
            .get("http://127.0.0.1:11434/api/tags")
            .send()
            .map_err(|e| format!("Ollama not reachable: {e}"))?;
        if !resp.status().is_success() {
            return Err(CommandError::internal(format!(
                "Ollama returned HTTP {}",
                resp.status()
            )));
        }
        #[derive(serde::Deserialize)]
        struct TagsResp {
            #[serde(default)]
            models: Vec<serde_json::Value>,
        }
        let tags: TagsResp = resp.json().map_err(|e| format!("decode: {e}"))?;
        let mut out = Vec::new();
        for m in &tags.models {
            let name = m
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let size = m.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            let modified = m
                .get("modified_at")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // Strip the tag suffix for verdict lookup (e.g. "qwen2.5:3b" -> "qwen2.5:3b").
            let canonical = name.split(':').next().unwrap_or(&name);
            let verdict = ollama_verdict(canonical);
            out.push(OllamaModelInfo {
                name,
                size,
                modified,
                verdict: verdict.label().into(),
                verdict_note: verdict.note().into(),
            });
        }
        Ok(out)
    })
    .await
    .map_err(|e| format!("ollama list task: {e}"))?
}

/// Maps a canonical Ollama model name to our benchmark verdict.
/// Mirrors the data in EW's `OllamaModelVerdicts` (measured 2026-08-11).
fn ollama_verdict(canonical: &str) -> teletype_inference::catalog::ModelVerdict {
    use teletype_inference::catalog::ModelVerdict as V;
    match canonical {
        "qwen2.5" if canonical == "qwen2.5" => V::NotTested,
        "qwen2.5" => V::NotTested,
        _ => {
            // Try matching with the tag: "qwen2.5:3b" etc.
            match canonical {
                "qwen2.5:3b" => V::Recommended,
                "qwen3:0.6b" => V::Recommended,
                "qwen2.5:7b" => V::Recommended,
                "gemma2:2b" | "gemma2" | "gemma3n:e4b" => V::Mixed,
                "llama3.2" | "mistral" | "deepseek-r1:1.5b" => V::Unreliable,
                "phi3" | "llama3.2:1b" | "tinyllama" => V::NotRecommended,
                "eg-1" => V::FirstParty,
                _ => V::NotTested,
            }
        }
    }
}

/// Reinstalls the persisted LLM provider selection at startup (Fix P0).
///
/// The provider itself is memory-only: a local-server child process or an
/// API client built from the keychain key. `settings.selected_llm_provider`
/// records only the choice, so after a restart `AppState.inference` was empty
/// and the pipeline silently skipped every transform. Runs on a plain
/// background thread (blocking warm-up must never touch the tokio runtime).
pub fn rehydrate_provider(app: &AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    match settings.selected_llm_provider.as_str() {
        "local-server" => {
            // Load the selected local polish model at launch so the first
            // dictation is ready to go. The model stays resident for the
            // process lifetime (the 5 s idle-unload timer only disarms after
            // an explicit `end_dictation`). If the model is not downloaded
            // yet, log and skip — the user can pick one in Settings > Models.
            let model_id = settings.selected_llm_model.clone();
            if model_id.is_empty() {
                crate::log_entry(
                    crate::LogLevel::Info,
                    "LLM: no local model selected; dictation will run AutoText-only",
                );
                return;
            }
            let Some(entry) = teletype_inference::catalog::find(&model_id) else {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("LLM restore: '{model_id}' is not in the catalog"),
                );
                return;
            };
            if !entry.is_downloaded(&state.models_dir) {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("LLM: '{model_id}' not downloaded yet; will load on first use"),
                );
                return;
            }
            let path = entry.entrypoint(&state.models_dir);
            if !path.exists() {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("LLM restore: model file for '{model_id}' is missing"),
                );
                return;
            }
            crate::log_entry(
                crate::LogLevel::Info,
                format!("LLM: loading local model at startup: {model_id}"),
            );
            let app2 = app.clone();
            std::thread::spawn(move || {
                ensure_local_provider(&app2);
            });
        }
        "openai-compat" => {
            let base_url = settings.openai_base_url.clone();
            let api_key = read_api_key(&base_url);
            if api_key.is_none() {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    "LLM restore: no API key in the keychain (local endpoints may still work)",
                );
            }
            // Install without probing: a transient network failure at boot
            // must not leave transforms permanently dead; first use logs it.
            let model = settings.openai_model.clone();
            let provider = teletype_inference::OpenAiCompatProvider::new(
                "openai-compat",
                format!("API ({model})"),
                teletype_inference::OpenAiCompatConfig::new(
                    settings.openai_base_url.clone(),
                    api_key,
                    model.clone(),
                ),
            );
            state
                .inference
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(Arc::new(provider));
            crate::log_entry(
                crate::LogLevel::Success,
                format!("LLM restored: API ({model})"),
            );
            // Startup restored a provider the panel's first poll may have
            // missed while it was still "starting": push the current state.
            emit_runtime_status(app, &state);
        }
        "" => {
            // Never picked a provider; the UI's model screen is the entry point.
        }
        other => {
            crate::log_entry(
                crate::LogLevel::Warn,
                format!("LLM restore skipped: unknown provider '{other}'"),
            );
        }
    }
}

/// Lazily loads the selected local polish model on first use (local models
/// are not preloaded at launch). Called
/// from the dictation path just before a transform runs, so the first
/// dictation pays the model-load cost once and later ones reuse it. No-op when
/// a provider is already loaded, a remote provider is active, or nothing is
/// selected. Blocking: it spawns a local server subprocess and warm-ups it, so
/// callers must already be off the tokio runtime (the dictation worker thread).
pub fn ensure_local_provider(app: &AppHandle) {
    let state = app.state::<AppState>();
    // Already have a provider (e.g. a remote one installed at launch, or a
    // local one from an earlier dictation): nothing to do.
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
    if settings.selected_llm_provider != "local-server" {
        return;
    }
    let model_id = &settings.selected_llm_model;
    let Some(entry) = teletype_inference::catalog::find(model_id) else {
        if !model_id.is_empty() {
            crate::log_entry(
                crate::LogLevel::Warn,
                format!("LLM load skipped: '{model_id}' is not in the catalog"),
            );
        }
        return;
    };
    if !entry.is_downloaded(&state.models_dir) {
        crate::log_entry(
            crate::LogLevel::Warn,
            format!("LLM load skipped: '{model_id}' is not downloaded yet"),
        );
        return;
    }
    let path = entry.entrypoint(&state.models_dir);
    if !path.exists() {
        crate::log_entry(
            crate::LogLevel::Warn,
            format!("LLM load skipped: model file for '{model_id}' is missing"),
        );
        return;
    }
    // P1-A: guard against concurrent spawns. If another thread is already
    // warming up the server, skip — it will install the provider.
    {
        if *state
            .llm_loading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            return;
        }
        *state
            .llm_loading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }
    let provider = teletype_inference::ServerProvider::new(entry.id, entry.name, &path);
    crate::log_entry(
        crate::LogLevel::Info,
        format!(
            "loading LLM model on first use: {} ({})",
            entry.name, entry.id
        ),
    );
    let started = std::time::Instant::now();
    let result = provider.warm_up();
    // Clear the loading flag regardless of outcome.
    *state
        .llm_loading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = false;
    if let Err(e) = result {
        crate::log_entry(
            crate::LogLevel::Error,
            format!("LLM first-use load failed for '{model_id}': {e}"),
        );
        return;
    }
    // BUG-015: check-and-install atomically under the inference lock. The
    // warm-up above can take up to 120 s, during which the user may have
    // selected a remote provider (select_openai_provider / select_model),
    // which installed its provider. Installing unconditionally here would
    // overwrite the user's choice; the decision is delegated to
    // should_install_local and evaluated inside the locked section.
    {
        let mut inference = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let current = inference.is_some();
        let settings = state.settings();
        if should_install_local(current, &settings.selected_llm_provider) {
            inference.replace(Arc::new(provider));
        } else {
            // Another path installed a provider, or the user's selection
            // changed, while we warmed up: discard ours (kills the child
            // llama-server) and keep the user's choice.
            provider.stop();
            crate::log_entry(
                crate::LogLevel::Info,
                "LLM local provider discarded: a provider was selected during load",
            );
            return;
        }
    }
    crate::log_entry_ms(
        crate::LogLevel::Success,
        format!("LLM model loaded: {model_id}"),
        started.elapsed().as_millis() as u64,
    );
    // Engine just spawned: the transform went from "not loaded" to "loaded",
    // possibly out from under the readiness panel. Tell it now.
    emit_runtime_status(app, &state);
}

/// BUG-015: decide, right before installing, whether the warmed-up local
/// provider may replace the current state. `current` is whether a provider
/// is already installed (one that appeared while we warmed up, e.g. a remote
/// provider the user selected); `selected_provider` is the user's current
/// selection. We install only when nothing is installed AND the user still
/// wants the local server.
fn should_install_local(current: bool, selected_provider: &str) -> bool {
    !current && selected_provider == "local-server"
}

/// Whether a catalog model's bytes are installed under `models_dir`.
///
/// Per-engine layout: one ggml/GGUF file (non-empty); sherpa's extracted
/// directory with all four model files (the `.tar.bz2` archive alone is not
/// usable); Core ML is downloaded by the FluidAudio bridge on first load, so
/// a working bridge is what "usable now" means for it.
fn speech_model_installed(
    entry: &teletype_speech::catalog::SpeechModel,
    models_dir: &std::path::Path,
    fluid_available: bool,
) -> bool {
    use teletype_speech::catalog::Engine;
    match entry.engine {
        Engine::ParakeetCoreml => fluid_available,
        Engine::SherpaParakeet => {
            let dir = entry.provider_path(models_dir);
            teletype_speech::sherpa::MODEL_FILES
                .iter()
                .all(|f| dir.join(f).is_file())
        }
        Engine::Qwen3Asr => {
            // A llama-server started with only one of the pair fails to load,
            // so the row is downloaded only when both GGUFs are on disk.
            let dir = entry.provider_path(models_dir);
            teletype_speech::catalog::QWEN3_ASR_ARTIFACTS
                .iter()
                .all(|a| {
                    dir.join(a.file)
                        .metadata()
                        .map(|m| m.is_file() && m.len() > 0)
                        .unwrap_or(false)
                })
        }
        Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp => entry
            .provider_path(models_dir)
            .metadata()
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false),
    }
}

/// Pure list behind [`list_speech_models`].
///
/// Only models this host can load are offered: a Windows install cannot run
/// the ggml engines, and a macOS install has no use for the sherpa builds.
/// Auxiliary companions (the CTC spotter) are never dictation choices. The
/// recommended row is computed for this host (platform, the user's language,
/// and whether the build links the FluidAudio bridge), not from a compile-time
/// constant, so a Windows user is not pointed at a macOS-only default and a
/// bridge-less Mac is not pointed at a row this very filter removes.
///
/// The `recommended` flag and the list rows share one predicate
/// ([`teletype_speech::catalog::SpeechModel::offered_on`]): every row the list
/// emits is a possible recommendation and the recommendation is always one of
/// the emitted rows. Onboarding renders the recommended card only, so a
/// recommendation that matched no row left the user with a dead Continue
/// button.
///
/// `shortlist` is the first-run view: only the rows
/// [`teletype_speech::catalog::shortlist_ids`] picks for this host and language
/// (at most three), in that order, plus a model the user already selected so a
/// returning user is never looked at as if they had no model. Settings passes
/// `false` and keeps the full list; the catalog stays the single source of what
/// exists, this only chooses how much of it a first run shows.
fn speech_model_list(
    platform: teletype_speech::catalog::Platform,
    models_dir: &std::path::Path,
    selected_id: &str,
    fluid_available: bool,
    language: Option<&str>,
    shortlist: bool,
) -> Vec<SpeechModelStatus> {
    let recommended =
        teletype_speech::catalog::recommended_speech_model_id(platform, language, fluid_available);
    debug_assert!(
        teletype_speech::catalog::find(recommended).is_some(),
        "the recommendation must be a catalog entry"
    );

    let status = |m: &'static teletype_speech::catalog::SpeechModel| SpeechModelStatus {
        id: m.id.into(),
        name: m.name.into(),
        engine: format!("{:?}", m.engine).to_lowercase(),
        engine_label: m.engine.label().into(),
        platform_label: m.engine.platform_label().into(),
        size_mb: m.size_mb,
        description: m.description.into(),
        recommended: m.id == recommended,
        english_only: m.english_only,
        downloaded: speech_model_installed(m, models_dir, fluid_available),
        selected: selected_id == m.id,
        speed: m.speed,
        accuracy: m.accuracy,
        min_ram_gb: m.min_ram_gb,
        language_label: m.language_label(),
        languages: m
            .languages
            .map(|l| l.iter().map(|s| s.to_string()).collect()),
    };

    if !shortlist {
        return teletype_speech::catalog::CATALOG
            .iter()
            .filter(|m| m.offered_on(platform, fluid_available))
            .map(status)
            .collect();
    }

    let ids = teletype_speech::catalog::shortlist_ids(platform, language, fluid_available);
    let mut rows: Vec<SpeechModelStatus> = ids
        .iter()
        .filter_map(|id| teletype_speech::catalog::find(id))
        .map(status)
        .collect();
    if let Some(entry) = teletype_speech::catalog::find(selected_id) {
        if entry.offered_on(platform, fluid_available) && !ids.contains(&entry.id) {
            rows.push(status(entry));
        }
    }
    rows
}

#[tauri::command]
pub async fn list_speech_models(
    state: State<'_, AppState>,
    shortlist: Option<bool>,
) -> CommandResult<Vec<SpeechModelStatus>> {
    let settings = state.settings();
    Ok(speech_model_list(
        teletype_speech::catalog::host_platform(),
        &state.models_dir,
        &settings.selected_speech_model,
        teletype_fluid::bridge_built(),
        // The user's real language, so the recommended row can transcribe it.
        // "auto"/empty is what a fresh install stores, and the catalog treats
        // both as "no language set" (platform default, not the Whisper route).
        Some(settings.language.as_str()),
        shortlist.unwrap_or(false),
    ))
}

/// One Whisper-supported language for the settings picker.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechLanguage {
    pub code: String,
    pub name: String,
}

/// All languages Whisper can detect/transcribe, from the whisper.cpp
/// language table. The UI renders "Auto-detect" first, then this list.
#[tauri::command]
pub async fn list_speech_languages() -> CommandResult<Vec<SpeechLanguage>> {
    // whisper.cpp's language table is fixed (id 0 = "en"). The previous
    // implementation walked whisper_rs on macOS only; we now use the static
    // subset so the function is platform-agnostic.
    Ok(speech_languages_static())
}

/// Pure language table behind list_speech_languages (shared with tests).
///
/// BUG-008: the full whisper.cpp language table (99 entries), not a static
/// 8-entry subset. The UI label count is derived from this list's length so
/// the two can never drift.
fn speech_languages_static() -> Vec<SpeechLanguage> {
    let table = [
        ("en", "English"),
        ("zh", "Chinese"),
        ("de", "German"),
        ("es", "Spanish"),
        ("ru", "Russian"),
        ("ko", "Korean"),
        ("fr", "French"),
        ("ja", "Japanese"),
        ("pt", "Portuguese"),
        ("fi", "Finnish"),
        ("pl", "Polish"),
        ("ca", "Catalan"),
        ("nl", "Dutch"),
        ("tr", "Turkish"),
        ("ar", "Arabic"),
        ("sv", "Swedish"),
        ("it", "Italian"),
        ("hi", "Hindi"),
        ("da", "Danish"),
        ("he", "Hebrew"),
        ("fa", "Persian"),
        ("no", "Norwegian"),
        ("th", "Thai"),
        ("ur", "Urdu"),
        ("hr", "Croatian"),
        ("bg", "Bulgarian"),
        ("el", "Greek"),
        ("ro", "Romanian"),
        ("hu", "Hungarian"),
        ("lt", "Lithuanian"),
        ("la", "Latin"),
        ("mi", "Maori"),
        ("ml", "Malayalam"),
        ("cy", "Welsh"),
        ("sk", "Slovak"),
        ("te", "Telugu"),
        ("lv", "Latvian"),
        ("bn", "Bengali"),
        ("sr", "Serbian"),
        ("az", "Azerbaijani"),
        ("sl", "Slovenian"),
        ("kn", "Kannada"),
        ("et", "Estonian"),
        ("mk", "Macedonian"),
        ("br", "Breton"),
        ("uk", "Ukrainian"),
        ("hy", "Armenian"),
        ("mn", "Mongolian"),
        ("bs", "Bosnian"),
        ("kk", "Kazakh"),
        ("sq", "Albanian"),
        ("sw", "Swahili"),
        ("gl", "Galician"),
        ("mr", "Marathi"),
        ("pa", "Punjabi"),
        ("si", "Sinhala"),
        ("id", "Indonesian"),
        ("vi", "Vietnamese"),
        ("tl", "Tagalog"),
        ("my", "Burmese"),
        ("ne", "Nepali"),
        ("ta", "Tamil"),
        ("oc", "Occitan"),
        ("gu", "Gujarati"),
        ("be", "Belarusian"),
        ("is", "Icelandic"),
        ("af", "Afrikaans"),
        ("jw", "Javanese"),
        ("am", "Amharic"),
        ("lo", "Lao"),
        ("uz", "Uzbek"),
        ("su", "Sundanese"),
        ("ka", "Georgian"),
        ("mg", "Malagasy"),
        ("yue", "Cantonese"),
        ("ha", "Hausa"),
        ("yo", "Yoruba"),
        ("xh", "Xhosa"),
        ("lb", "Luxembourgish"),
        ("ht", "Haitian Creole"),
        ("ps", "Pashto"),
        ("mt", "Maltese"),
        ("co", "Corsican"),
        ("tg", "Tajik"),
        ("ny", "Nyanja"),
        ("sd", "Sindhi"),
        ("gd", "Scottish Gaelic"),
        ("lg", "Luganda"),
        ("or", "Odia"),
        ("ceb", "Cebuano"),
        ("haw", "Hawaiian"),
        ("ln", "Lingala"),
        ("rw", "Kinyarwanda"),
        ("so", "Somali"),
        ("zu", "Zulu"),
    ];
    table
        .iter()
        .map(|(c, n)| SpeechLanguage {
            code: c.to_string(),
            name: n.to_string(),
        })
        .collect()
}

#[tauri::command]
pub async fn select_speech_model(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let entry = teletype_speech::catalog::find(&id)
        .ok_or_else(|| CommandError::model(format!("Unknown speech model '{id}'")))?;
    if entry.auxiliary {
        return Err(CommandError::user(format!(
            "'{}' is a vocabulary spotter, not a dictation model.",
            entry.name
        )));
    }
    if !entry.available_on(teletype_speech::catalog::host_platform()) {
        return Err(CommandError::model(format!(
            "'{}' cannot run on this computer.",
            entry.name
        )));
    }
    let mut settings = state.settings();
    settings.selected_speech_model = id;
    state.replace_settings(settings)
}

/// Hex SHA-256 of a file, streamed so a 1.2 GB model is never held in memory.
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = std::io::Read::read(&mut f, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex_encode(&h.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0xf) as u32, 16).unwrap());
    }
    out
}

/// One file of a speech download. Most catalog rows are a single model file;
/// Qwen3-ASR is a pair (GGUF plus audio projector) that only loads together.
struct SpeechFileSpec {
    name: String,
    url: String,
    dest: std::path::PathBuf,
    /// The publisher's SHA-256 when the catalog pins this file. Authoritative:
    /// a file already on disk that matches is not fetched again.
    sha256: Option<&'static str>,
    /// Published size in bytes: the progress denominator, and the completeness
    /// bound when there is no pin (`size_mb` is the publisher's rounded figure).
    bytes: u64,
}

/// True when `spec.dest` already holds the exact bytes this file must have, so
/// a retry after a cancelled multi-file download does not re-fetch 300 MB that
/// is already verified. Unpinned rows always re-download.
fn speech_file_already_installed(spec: &SpeechFileSpec) -> bool {
    let Some(want) = spec.sha256 else {
        return false;
    };
    let exact_size = spec
        .dest
        .metadata()
        .map(|m| m.is_file() && m.len() == spec.bytes)
        .unwrap_or(false);
    exact_size && sha256_file(&spec.dest).is_ok_and(|got| got.eq_ignore_ascii_case(want))
}

#[tauri::command]
pub async fn download_speech_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<String> {
    let entry = teletype_speech::catalog::find(&id)
        .ok_or_else(|| format!("Unknown speech model '{id}'"))?;
    let fluid_available = teletype_fluid::bridge_built();
    // Core ML: the FluidAudio bridge owns the checkpoint layout and downloads
    // it on first load, so there is nothing for this command to fetch. Its
    // "downloaded" state is the bridge's presence (see `speech_model_list`).
    if entry.engine == teletype_speech::catalog::Engine::ParakeetCoreml {
        if !fluid_available {
            return Err(CommandError::model(format!(
                "{} runs on the Neural Engine and needs the FluidAudio bridge, \
                 which this build does not include.",
                entry.name
            )));
        }
        return Ok(entry
            .provider_path(&state.models_dir)
            .to_string_lossy()
            .to_string());
    }
    if speech_model_installed(entry, &state.models_dir, fluid_available) {
        return Ok(entry
            .install_path(&state.models_dir)
            .to_string_lossy()
            .to_string());
    }
    // Sherpa ships a `.tar.bz2` release archive; everything else is one file.
    // Qwen3-ASR is the exception that proves the rule: the main GGUF and its
    // audio projector must sit in one directory, and llama.cpp refuses to load
    // with only one of the pair, so both are fetched and both are
    // checksum-verified before the row counts as installed.
    let files: Vec<SpeechFileSpec> = if entry.engine == teletype_speech::catalog::Engine::Qwen3Asr {
        let base = entry.base_url.ok_or_else(|| {
            CommandError::model(format!(
                "Catalog error: the '{}' row needs a base_url to fetch its two files.",
                entry.id
            ))
        })?;
        let dir = entry.provider_path(&state.models_dir);
        teletype_speech::catalog::QWEN3_ASR_ARTIFACTS
            .iter()
            .map(|a| SpeechFileSpec {
                name: a.file.to_string(),
                url: format!("{base}/{}", a.file),
                dest: dir.join(a.file),
                sha256: Some(a.sha256),
                bytes: a.bytes,
            })
            .collect()
    } else {
        let dest = match entry.engine {
            teletype_speech::catalog::Engine::SherpaParakeet => {
                state.models_dir.join("sherpa").join(entry.file)
            }
            _ => entry.install_path(&state.models_dir),
        };
        vec![SpeechFileSpec {
            name: entry.file.to_string(),
            url: entry.url(),
            dest,
            sha256: entry.sha256,
            bytes: (entry.size_mb as u64).saturating_mul(1024 * 1024),
        }]
    };

    let entry_name = entry.name.to_string();

    // Pre-flight: fail fast on a full disk before any bytes are transferred.
    // `check_disk_space` already applies its 2.2x headroom factor internally,
    // so pass the raw size.
    let needed = (entry.size_mb as u64).saturating_mul(1024 * 1024);
    if needed > 0 {
        teletype_inference::download::check_disk_space(&state.models_dir, needed)
            .map_err(|e| format!("{entry_name}: {e}"))?;
    }

    // Stream each file to a temp path with live progress, then rename it into
    // place, so a half-finished download is never mistaken for an installed
    // model.
    let state_dir = state.models_dir.clone();
    let event_id = id.clone();
    let grand_total: u64 = files.iter().map(|f| f.bytes).sum();
    let tmps: Vec<std::path::PathBuf> = files
        .iter()
        .map(|f| f.dest.with_extension("bin.part"))
        .collect();
    // Copies for the post-join cancel cleanup: the blocking task moves the
    // originals into its closure.
    let tmp_paths = tmps.clone();
    let file_count = files.len() as u32;
    let first_file_name = files[0].name.clone();
    // Register the cancel flag only once the download is about to start, so
    // an early return (e.g. the disk-space pre-flight) cannot leave a stale
    // flag in the map.
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut map = state
            .download_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.insert(id.clone(), cancel_flag.clone());
    }
    let cancel_flag_for_task = cancel_flag;
    // Clone for the post-join cancelled-event emit: the original moves into
    // the spawn_blocking closure below.
    let app_emit = app.clone();
    // Separate clone for the post-join status emit below.
    let app_status = app.clone();

    let handle = tauri::async_runtime::spawn_blocking(move || {
        // `done_before` is the bytes of the files already finished, so the
        // bar a two-file row shows is the whole download, not one file's.
        let emit = |status: &str,
                    file_index: usize,
                    written: u64,
                    file_total: u64,
                    done_before: u64,
                    speed_bps: f64,
                    eta: Option<f64>,
                    err: Option<String>| {
            let file = &files[file_index];
            let agg_written = done_before + written;
            let agg_total = grand_total.max(done_before + file_total);
            let payload = serde_json::json!({
                "id": event_id,
                "kind": "speech",
                "status": status,
                "fileName": file.name,
                "fileIndex": (file_index + 1) as u32,
                "fileCount": files.len() as u32,
                "fileDownloadedBytes": written,
                "fileTotalBytes": file_total,
                "downloadedBytes": agg_written,
                "totalBytes": agg_total,
                "percent": if agg_total > 0 { (agg_written as f64 / agg_total as f64 * 100.0).min(100.0) } else { 0.0 },
                "speedBps": speed_bps,
                "etaSeconds": eta,
                "error": err,
            });
            let _ = app.emit("model-download-progress", payload);
        };

        let run = || -> Result<(), String> {
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|e| e.to_string())?;
            // Bytes landed from the files already streamed, so the progress bar
            // of a two-file row tracks the whole download, not one file's.
            let mut done_before: u64 = 0;
            for (i, spec) in files.iter().enumerate() {
                let tmp = &tmps[i];
                if let Some(parent) = spec.dest.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                // A row interrupted while streaming a later file keeps the earlier
                // verified files, so a retry skips what is already byte-for-byte
                // correct instead of re-fetching 300 MB.
                if speech_file_already_installed(spec) {
                    done_before += spec.bytes;
                    continue;
                }
                let mut response = client
                    .get(&spec.url)
                    .header("User-Agent", "teletype/0.1")
                    .send()
                    .map_err(|e| e.to_string())?;
                if !response.status().is_success() {
                    return Err(format!("HTTP {}", response.status()));
                }
                let content_len = response.content_length().unwrap_or(spec.bytes);
                let total = if content_len > 0 {
                    content_len
                } else {
                    spec.bytes
                };

                let mut file = std::fs::File::create(tmp).map_err(|e| e.to_string())?;
                use std::io::{Read, Write};
                let mut buf = [0u8; 64 * 1024];
                let mut written: u64 = 0;
                let started = std::time::Instant::now();
                let mut last_emit = std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(1))
                    .unwrap_or_else(std::time::Instant::now);
                let mut speed = 0.0f64;

                loop {
                    // Cancel request between reads: delete the partial file and
                    // stop. Speech downloads have no resume, so this is a real
                    // cancel, not a pause.
                    if cancel_flag_for_task.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = std::fs::remove_file(tmp);
                        emit("cancelled", i, written, total, done_before, 0.0, None, None);
                        return Err("download cancelled".to_string());
                    }
                    let n = response.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                    written += n as u64;

                    let elapsed = started.elapsed().as_secs_f64();
                    if elapsed > 0.05 {
                        let sample = written as f64 / elapsed;
                        speed = if speed <= 0.0 {
                            sample
                        } else {
                            speed * 0.7 + sample * 0.3
                        };
                    }
                    if last_emit.elapsed() >= std::time::Duration::from_millis(100) {
                        last_emit = std::time::Instant::now();
                        let eta = if speed > 1024.0 && total > written {
                            Some((total - written) as f64 / speed)
                        } else {
                            None
                        };
                        emit(
                            "downloading",
                            i,
                            written,
                            total,
                            done_before,
                            speed,
                            eta,
                            None,
                        );
                    }
                }
                file.flush().map_err(|e| e.to_string())?;
                file.sync_all().map_err(|e| e.to_string())?;
                drop(file);

                // Covers a cancel during the final read: the loop may have just
                // finished before the flag was seen.
                if cancel_flag_for_task.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = std::fs::remove_file(tmp);
                    emit("cancelled", i, written, total, done_before, 0.0, None, None);
                    return Err("download cancelled".to_string());
                }

                // Verify before installing. Previously this emitted a "verifying"
                // status and then renamed the file unconditionally, so a download
                // cut short at 90% was installed and reported as 100% Done, and
                // the Models screen then showed it as downloaded. The next
                // dictation either mis-transcribed or failed to load, with nothing
                // pointing back at the download.
                //
                // A pinned SHA-256 is authoritative: a truncated or repointed
                // transfer fails the hash, so the approximate size bound is only
                // a fallback for entries without a pin (`size_mb` is the
                // publisher's rounded figure and would falsely reject several
                // correctly-downloaded models).
                emit(
                    "verifying",
                    i,
                    written,
                    total.max(written),
                    done_before,
                    0.0,
                    Some(0.0),
                    None,
                );

                let pinned = spec.sha256.is_some();
                if let Some(want_sha) = spec.sha256 {
                    let actual = sha256_file(tmp)?;
                    if !actual.eq_ignore_ascii_case(want_sha) {
                        let _ = std::fs::remove_file(tmp);
                        return Err(format!(
                            "{entry_name} failed its checksum. The download was discarded; \
                         please try again."
                        ));
                    }
                }

                let expected_bytes = spec.bytes;
                // Allow 1% slack: `size_mb` is rounded, and the last byte of the
                // reported figure is not exact.
                let floor = expected_bytes.saturating_mul(99) / 100;
                if !pinned && expected_bytes > 0 && written < floor {
                    let _ = std::fs::remove_file(tmp);
                    return Err(format!(
                        "{entry_name} downloaded {} of about {} bytes, so it is incomplete. \
                     The partial file was discarded; please try again.",
                        written, expected_bytes
                    ));
                }

                if entry.engine == teletype_speech::catalog::Engine::SherpaParakeet {
                    // The archive extracts to `<models>/sherpa/<archive stem>`,
                    // which is the directory the provider loads. Keep the archive
                    // only long enough to unpack it: sherpa releases are ~640 MB
                    // and the extracted files are what the app reads.
                    let extract_to = entry.provider_path(&state_dir);
                    let parent = extract_to.parent().ok_or("invalid sherpa model path")?;
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    let archive_file = std::fs::File::open(tmp).map_err(|e| e.to_string())?;
                    let decoder = bzip2::read::BzDecoder::new(archive_file);
                    let mut archive = tar::Archive::new(decoder);
                    archive
                        .unpack(parent)
                        .map_err(|e| format!("{entry_name} could not be extracted: {e}"))?;
                    let complete = teletype_speech::sherpa::MODEL_FILES
                        .iter()
                        .all(|f| extract_to.join(f).is_file());
                    if !complete {
                        let _ = std::fs::remove_dir_all(&extract_to);
                        let _ = std::fs::remove_file(tmp);
                        return Err(format!(
                            "{entry_name} archive did not contain the expected model files."
                        ));
                    }
                    let _ = std::fs::remove_file(tmp);
                } else {
                    std::fs::rename(tmp, &spec.dest).map_err(|e| e.to_string())?;
                }
                done_before += written;
            }
            let last = files.len() - 1;
            emit(
                "done",
                last,
                done_before,
                grand_total.max(done_before),
                0,
                0.0,
                Some(0.0),
                None,
            );
            Ok(())
        };

        match run() {
            Ok(()) => Ok(()),
            Err(e) => {
                for tmp in &tmps {
                    let _ = std::fs::remove_file(tmp);
                }
                if cancel_flag_for_task.load(std::sync::atomic::Ordering::Relaxed) {
                    emit("cancelled", 0, 0, grand_total, 0, 0.0, None, None);
                } else {
                    emit("error", 0, 0, grand_total, 0, 0.0, None, Some(e.clone()));
                }
                Err(e)
            }
        }
    });
    let joined = handle
        .await
        .map_err(|e| format!("download task: {e}"))
        .and_then(|r| r);
    // Remove the flag only after the task has settled. Removing it before the
    // await would let `cancel_download` run while the blocking task is still
    // alive but no longer find the flag, so a cancel click during a speech
    // download would never reach the loop.
    {
        let mut map = state
            .download_cancel
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.remove(&id);
    }
    // If the user cancelled outright, make sure the partial file is gone and
    // report the stop as `cancelled`. The loop above normally already emitted
    // the event and deleted the file; this keeps the post-join path race-safe
    // without trying to deduplicate perfectly.
    {
        let mut hard = state
            .download_cancel_hard
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if hard.remove(&id) && joined.is_err() {
            for part in &tmp_paths {
                if part.exists() {
                    let _ = std::fs::remove_file(part);
                }
            }
            let payload = serde_json::json!({
                "id": id,
                "kind": "speech",
                "status": "cancelled",
                "fileName": first_file_name,
                "fileIndex": 1u32,
                "fileCount": file_count,
                "fileDownloadedBytes": 0u64,
                "fileTotalBytes": 0u64,
                "downloadedBytes": 0u64,
                "totalBytes": 0u64,
                "percent": 0.0f64,
                "speedBps": 0.0f64,
                "etaSeconds": serde_json::Value::Null,
                "error": serde_json::Value::Null,
            });
            let _ = app_emit.emit("model-download-progress", payload);
            return Err(CommandError::internal("download cancelled"));
        }
        // Success raced with a late cancel click: nothing to undo, but
        // the hard-set entry is already removed above.
    }
    joined.map_err(CommandError::internal)?;
    // The speech model is installed: readiness can change ("not downloaded yet"
    // → "loads on your first dictation"). Push it rather than waiting for poll.
    emit_runtime_status(&app_status, &state);
    Ok(entry
        .install_path(&state.models_dir)
        .to_string_lossy()
        .to_string())
}

/// Pure decision for [`get_speech_model_ready_state`], factored out so it can
/// be unit-tested without a Tauri `State`.
fn speech_model_ready(
    id: &str,
    models_dir: &std::path::Path,
    fluid_available: bool,
) -> SpeechModelReady {
    let entry = teletype_speech::catalog::find(id);
    let ready = entry.is_some_and(|e| speech_model_installed(e, models_dir, fluid_available));
    let exists = entry.is_some_and(|e| {
        if e.engine == teletype_speech::catalog::Engine::ParakeetCoreml {
            fluid_available
        } else {
            e.provider_path(models_dir).exists()
        }
    });
    let name = entry.map(|e| e.name).unwrap_or("Your model");
    let message = if ready {
        format!("{name} is ready. Hold your hotkey and speak.")
    } else if exists {
        "The model files are present but incomplete. Re-download them.".into()
    } else {
        "Model not downloaded yet.".into()
    };
    SpeechModelReady {
        id: id.to_string(),
        downloaded: exists,
        ready,
        message,
    }
}

#[tauri::command]
pub async fn get_speech_model_ready_state(
    state: State<'_, AppState>,
) -> CommandResult<SpeechModelReady> {
    let settings = state.settings();
    let id = settings.selected_speech_model.clone();
    Ok(speech_model_ready(
        &id,
        &state.models_dir,
        teletype_fluid::bridge_built(),
    ))
}

/// The runtime state of one model, phrased the way a user would describe it.
///
/// `state` is the field to branch on; `detail` is safe to render verbatim.
/// The two exist because a UI that has to build its own sentence from a
/// boolean will eventually build a wrong one — that is how "not loaded (loads
/// 'X' on first use)" came to sit under a heading reading "Active transform".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentRuntime {
    /// One of `none`, `missing`, `loading`, `ready`, `loaded`, `error`.
    pub state: String,
    /// The model's display name, or the subsystem name when none is selected.
    pub label: String,
    /// The model id. Empty when nothing is selected.
    pub model_id: String,
    /// One sentence, never optimistic.
    pub detail: String,
    /// True when dictation cannot proceed until this changes.
    pub blocking: bool,
}

impl ComponentRuntime {
    fn new(state: &str, label: &str, model_id: &str, detail: String, blocking: bool) -> Self {
        Self {
            state: state.into(),
            label: label.into(),
            model_id: model_id.into(),
            detail,
            blocking,
        }
    }
}

/// What the app can actually do right now, for both models that matter.
///
/// The gap this fills: `SpeechModelManager::is_loaded()` was real and correct,
/// and was read at four places in the dictation pipeline — but not one of them
/// was inside a `#[tauri::command]`. Of the app's commands, none reported
/// speech readiness at all. Rust knew which model was in memory; the UI could
/// not find out, so it inferred readiness from "a file exists somewhere on
/// disk", which is a different and weaker claim.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    /// The speech model the next dictation will use, resolved through the
    /// same `resolve_speech_model` the worker uses, so this cannot disagree
    /// with what actually gets loaded.
    pub speech: ComponentRuntime,
    /// The transform (polish) model.
    pub llm: ComponentRuntime,
}

/// Pure speech half of [`runtime_status`], factored out so it is unit-testable
/// without a Tauri `State`.
///
/// `fluid_available` (the Core ML bridge) and `loaded` are passed in rather
/// than read here so this stays a pure function of its inputs.
fn speech_runtime(
    selected_id: &str,
    models_dir: &std::path::Path,
    engine_label: &str,
    fluid_available: bool,
    loaded: bool,
) -> ComponentRuntime {
    let entry = teletype_speech::catalog::find(selected_id);
    let name = entry.map(|e| e.name).unwrap_or("Dictation model");
    let id = selected_id.to_string();

    if selected_id.is_empty() {
        return ComponentRuntime::new(
            "none",
            "Dictation model",
            "",
            "No dictation model is selected. Choose one to start dictating.".into(),
            true,
        );
    }

    // Read the real files rather than trusting `exists`: a 0-byte leftover
    // from an interrupted download, or a sherpa archive that was never
    // extracted, reads as present and is not usable.
    let usable = match entry {
        Some(e) => speech_model_installed(e, models_dir, fluid_available),
        // Unknown id: fall back to the loader's own naming so this agrees
        // with `resolve_speech_model`, which would look for `<id>.bin`.
        None => models_dir
            .join(format!("{selected_id}.bin"))
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
    };

    if !usable {
        return ComponentRuntime::new(
            "missing",
            name,
            &id,
            format!("{name} is selected but not downloaded yet."),
            true,
        );
    }
    if loaded {
        return ComponentRuntime::new(
            "loaded",
            name,
            &id,
            format!("{name} is loaded and ready. {engine_label}"),
            false,
        );
    }
    // On disk, not in memory. This is the state that was previously
    // invisible: the model is fine, it has simply not been loaded yet, which
    // happens on the first dictation after an idle unload (BUG-003).
    ComponentRuntime::new(
        "ready",
        name,
        &id,
        format!("{name} is downloaded and loads on your first dictation."),
        false,
    )
}

/// Computes the current [`RuntimeStatus`]. Factored out of [`get_runtime_status`]
/// so the event path ([`emit_runtime_status`]) reports the exact same value the
/// polled command does, instead of a second copy of the logic that could drift.
fn compute_runtime_status(state: &AppState) -> RuntimeStatus {
    let settings = state.settings();

    // Resolve through the same helper the dictation worker uses. If this
    // computed the path any other way it could report on a different model
    // than the one about to be loaded, which would be worse than reporting
    // nothing at all.
    let (_, kind) = crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    let (loaded, engine_label) = match kind {
        crate::dictation::SpeechEngineKind::Fluid
        | crate::dictation::SpeechEngineKind::Sherpa
        | crate::dictation::SpeechEngineKind::Qwen3Asr => {
            // The neural slot keeps one provider for whichever model was
            // selected last; a different selection means this one is not
            // loaded, whatever the slot says.
            let current = state
                .neural_model_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                current.as_deref() == Some(settings.selected_speech_model.as_str())
                    && state.neural.is_loaded(),
                "Neural engine",
            )
        }
        crate::dictation::SpeechEngineKind::Whistle => (state.neural.is_loaded(), "Neural engine"),
        crate::dictation::SpeechEngineKind::Parakeet => {
            (state.parakeet.is_loaded(), "Parakeet engine")
        }
        crate::dictation::SpeechEngineKind::ParakeetCpp => {
            (state.parakeet_cpp.is_loaded(), "Parakeet.cpp engine")
        }
        crate::dictation::SpeechEngineKind::Whisper => (state.speech.is_loaded(), "Whisper engine"),
    };
    let speech = speech_runtime(
        &settings.selected_speech_model,
        &state.models_dir,
        engine_label,
        teletype_fluid::bridge_built(),
        loaded,
    );

    let llm = llm_runtime(state, &settings);
    RuntimeStatus { speech, llm }
}

#[tauri::command]
pub async fn get_runtime_status(state: State<'_, AppState>) -> CommandResult<RuntimeStatus> {
    Ok(compute_runtime_status(&state))
}

/// Pushes the current runtime status to the UI as a `runtime-status` event so
/// the readiness panel does not wait up to `POLL_MS` for the next poll to notice
/// a model that just finished loading, downloaded, or died.
///
/// The payload serialises to the identical camelCase shape the `get_runtime_status`
/// command returns (both build a [`RuntimeStatus`]), so the JS side can apply the
/// event payload and a polled value with the same code path. A full recompute per
/// call is deliberate: this is not on the dictation path and the state that
/// changes fast (dictation phase) already travels as its own event.
///
/// Call this at every point that can change the answer: model select and
/// download completion, engine spawn, engine stop, and startup restore. Engine
/// death (`is_alive()` turning false) has no observation point inside this crate
/// — the next `get_runtime_status` poll and the next `ensure_local_provider` call
/// both recompute it, which is why the poll stays as the safety net.
pub(crate) fn emit_runtime_status(app: &tauri::AppHandle, state: &AppState) {
    let status = compute_runtime_status(state);
    // Best-effort: an emit failure must never fail a model load or download.
    let _ = app.emit("runtime-status", status);
}

pub(crate) fn llm_runtime(state: &AppState, settings: &Settings) -> ComponentRuntime {
    let engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();

    if let Some(p) = engine.as_ref() {
        // "Loaded" must mean the backend can actually serve a request. A
        // crashed llama-server used to report "loaded and will clean up your
        // dictation" while every polish failed with "S1 generation failed".
        if !p.is_alive() {
            return ComponentRuntime::new(
                "error",
                p.model_name(),
                p.model_id(),
                format!(
                    "{}'s model server stopped. Select it again in Models to restart it.",
                    p.model_name()
                ),
                false,
            );
        }
        return ComponentRuntime::new(
            "loaded",
            p.model_name(),
            p.model_id(),
            format!(
                "{} is loaded and will clean up your dictation.",
                p.model_name()
            ),
            false,
        );
    }

    // A remote provider has nothing to load: the request is the load. Saying
    // "not loaded" here would be technically true and practically alarming.
    if settings.selected_llm_provider == "openai-compat" {
        let model = if settings.openai_model.is_empty() {
            "your API model".to_string()
        } else {
            settings.openai_model.clone()
        };
        return ComponentRuntime::new(
            "ready",
            "API transform",
            &settings.openai_model,
            format!("Uses {model} over your API connection. Nothing to load locally."),
            false,
        );
    }

    if *state
        .llm_loading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        return ComponentRuntime::new(
            "loading",
            "Local transform",
            &settings.selected_llm_model,
            "Starting the local model…".into(),
            false,
        );
    }
    if settings.selected_llm_provider.is_empty() || settings.selected_llm_model.is_empty() {
        return ComponentRuntime::new(
            "none",
            "Local transform",
            "",
            "No transform model selected. Dictation still works — it is inserted as spoken.".into(),
            false,
        );
    }
    ComponentRuntime::new(
        "ready",
        "Local transform",
        &settings.selected_llm_model,
        format!(
            "{} downloads on your first dictation and cleans up your text.",
            settings.selected_llm_model
        ),
        false,
    )
}

#[tauri::command]
pub async fn get_model_status(state: State<'_, AppState>) -> CommandResult<String> {
    let engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    match engine.as_ref() {
        Some(p) => Ok(format!("{} ({})", p.model_name(), p.model_id())),
        None => {
            // Distinguish "nothing configured" from "configured but not
            // loaded", so an empty slot is actionable. Kept short: the UI
            // embeds this inside "Local (...)".
            let settings = state.settings();
            let configured = match settings.selected_llm_provider.as_str() {
                "local-server" if !settings.selected_llm_model.is_empty() => format!(
                    "not loaded (loads '{}' on first use)",
                    settings.selected_llm_model
                ),
                "openai-compat" => format!(
                    "not loaded (API {}, transforms skipped)",
                    settings.openai_model
                ),
                _ => "No model selected".into(),
            };
            Ok(configured)
        }
    }
}

// ---- File transcription (P3.17) ----

/// Result of a file transcription: the inserted history entry plus a word
/// count so the UI can confirm.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscribeFileResult {
    pub id: String,
    pub word_count: usize,
}

/// File size cap for file transcription (512 MB).
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
/// Decoded-audio duration cap for file transcription (2 hours at 16 kHz).
const MAX_FILE_SECONDS: f64 = 2.0 * 3600.0;

/// Extensions accepted by `transcribe_file` (audio only; no video).
const AUDIO_EXTENSIONS: &[&str] = &[
    "m4a", "aac", "wav", "mp3", "flac", "ogg", "m4b", "aiff", "aif",
];

/// Validates the path's extension against [`AUDIO_EXTENSIONS`].
fn validate_audio_path(path: &str) -> CommandResult<std::path::PathBuf> {
    let p = std::path::Path::new(path);
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        return Err(CommandError::user(format!(
            "Unsupported file type '.{ext}'. Accepted audio formats: {}.",
            AUDIO_EXTENSIONS.join(", ")
        )));
    }
    Ok(p.to_path_buf())
}

/// Decodes an audio file to 16 kHz mono f32 PCM using symphonia (pure Rust),
/// resampling with the same resampler the mic capture path uses
/// (`teletype_core::audio::resample_to_target`). Enforces the 512 MB file
/// size and 2 hour duration caps so a huge file can't blow up memory.
fn decode_audio_file(path: &std::path::Path) -> CommandResult<Vec<f32>> {
    use symphonia::core::audio::{AudioBuffer, Signal};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::probe::Hint;

    let meta = std::fs::metadata(path).map_err(|e| format!("Can't read file: {e}"))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(CommandError::user(format!(
            "File is too large ({} MB). The limit is 512 MB.",
            meta.len() / (1024 * 1024)
        )));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("Can't open file: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &Default::default(), &Default::default())
        .map_err(|e| format!("Unrecognized or unsupported audio file: {e}"))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)
        .ok_or("No audio track found in file")?;
    let _track_id = track.id;
    let from_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &Default::default())
        .map_err(|e| format!("Unsupported audio codec: {e}"))?;

    let mut pcm: Vec<f32> = Vec::new();
    // `next_packet` returns `Err` at end of stream, so the decode loop is a
    // `while let`. A decode error is handled inside the body, not by the loop
    // condition, so a recoverable packet does not end the decode.
    while let Ok(packet) = format.next_packet() {
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(e) => {
                // IoError / DecodeError are recoverable packet-level errors.
                let msg = e.to_string();
                if !msg.contains("IoError") && !msg.contains("DecodeError") {
                    return Err(CommandError::internal(format!("Decode error: {e}")));
                }
                continue;
            }
        };
        // Convert to f32 (the decoder may output S16/P24/etc), then mix to mono.
        let mut f32_buf: AudioBuffer<f32> =
            AudioBuffer::new(decoded.capacity().max(1) as u64, *decoded.spec());
        decoded.convert(&mut f32_buf);
        let n = f32_buf.frames();
        if channels <= 1 {
            pcm.extend_from_slice(&f32_buf.chan(0)[..n]);
        } else {
            // `AudioBuffer` exposes per-channel planes via `chan(i)`, so hold
            // the slices and read across them. This is the "loop variable only
            // used to index" pattern clippy flags, and here the fix is to say
            // what the data is: a set of channel planes.
            let planes: Vec<&[f32]> = (0..channels).map(|c| f32_buf.chan(c)).collect();
            for sample in planes.iter().map(|p| p.iter().take(n)) {
                let sum: f32 = sample.sum();
                pcm.push(sum / channels as f32);
            }
        }
        // Duration cap, checked on the decoded sample count.
        if pcm.len() as f64 / from_rate as f64 > MAX_FILE_SECONDS {
            return Err(CommandError::user(
                "Audio is longer than 2 hours. The limit is 2 hours.",
            ));
        }
    }
    if pcm.is_empty() {
        return Err(CommandError::user("No audio data found in file"));
    }
    Ok(teletype_core::audio::resample_to_target(&pcm, from_rate))
}

/// Public seam for integration tests: decodes an audio file to 16 kHz mono
/// f32 PCM (see the private [`decode_audio_file`]).
#[allow(dead_code)]
#[doc(hidden)]
pub fn decode_audio_file_public(path: &std::path::Path) -> CommandResult<Vec<f32>> {
    decode_audio_file(path)
}

/// Transcribes an audio file (m4a/aac/wav/mp3/flac/ogg/m4b/aiff) through the
/// same ASR engine and the same ITN/transform pipeline as live dictation, and
/// saves the result to dictation history with the file name as its context.
#[tauri::command]
pub async fn transcribe_file(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<TranscribeFileResult> {
    let path_buf = validate_audio_path(&path)?;
    let file_name = path_buf
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    // Decode on a worker thread (symphonia can be slow on long files) and
    // off the async runtime.
    let decode_path = path_buf.clone();
    let pcm = tauri::async_runtime::spawn_blocking(move || decode_audio_file(&decode_path))
        .await
        .map_err(|e| format!("decode task: {e}"))??;

    let settings = state.settings();
    let (model_path, kind) = crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    // The frontmost-app language override wins, same as the live path.
    let language = crate::dictation::effective_language_for_app(&state, kind);

    // Same transcribe call the live dictation path uses (dictation::transcribe
    // loads the model if needed and serializes on the speech-model lock, so
    // this never races an in-flight live take).
    let captured = teletype_core::audio::Captured {
        samples: pcm.clone(),
        duration_secs: pcm.len() as f64 / teletype_core::audio::TARGET_SAMPLE_RATE as f64,
        peak: 0.0,
    };
    // The file's own length, kept before `captured` is moved into the
    // transcribe closure. This is a real measurement, so it belongs in the
    // history entry: it is what makes a speaking rate possible for a
    // transcribed file.
    let audio_duration_ms = (captured.duration_secs * 1000.0).round().max(0.0) as u64;
    let app2 = app.clone();
    let model_path2 = model_path.clone();
    let language2 = language.clone();
    let raw = tauri::async_runtime::spawn_blocking(move || {
        crate::dictation::transcribe(&app2, &model_path2, &captured, &language2, kind, &[])
    })
    .await
    .map_err(|e| format!("transcribe task: {e}"))??;
    let (raw, _words) = raw;
    let raw = crate::dictation::ensure_latin_if_english(
        &raw,
        &crate::dictation::effective_language(
            settings.language.clone(),
            crate::dictation::is_english_only(kind),
        ),
    );
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        return Err(CommandError::user("No speech detected in the audio file"));
    }

    // Same pipeline the live path runs (ITN, AutoText, filler removal,
    // transform, emoji restore) with the current settings, so file output
    // matches live output quality. The live path keeps this inline in the
    // dictation controller's worker; factoring it out of the event loop
    // would be more invasive than assembling the same Pipeline here, so the
    // pipeline is assembled directly with the controller's profile.
    let app3 = app.clone();
    let polished = tauri::async_runtime::spawn_blocking(move || {
        crate::commands::ensure_local_provider(&app3);
        let state = app3.state::<AppState>();
        let settings = state.settings();
        let platform = state.platform.as_ref();
        let autotext = state.autotext.get();
        let transforms = state.transforms.get();
        let mut profile = (*state.profile.get()).clone();
        profile.language = crate::dictation::effective_language(settings.language.clone(), false);
        let styles = state.styles.get();
        let dictionary = state.dictionary.get();
        let active_style = settings.active_style_profile.clone();
        let engine = match state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
        {
            Some(e) => e,
            None => return "No inference engine loaded".to_string(),
        };
        let engine_ref = Some(engine.as_ref());
        let mut pack_terms = teletype_core::vocab::terms_for(&settings.enabled_packs);
        // User-created packs (item 15) feed the same correction tier.
        {
            let user_terms = state.user_packs.get().terms_for();
            pack_terms.extend(user_terms);
        }
        let mut pipeline = teletype_core::pipeline::Pipeline {
            platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: engine_ref,
            decision: None,
            force_command: false,
            dictionary: &dictionary,
            styles: &styles,
            active_style: &active_style,
            explicit_style: "",
            auto_apply: settings.auto_apply_transform,
            restore_clipboard: settings.restore_clipboard,
            remove_filler_words: settings.remove_filler_words,
            filler_words: settings.filler_words.clone(),
            filler_strength: settings.filler_strength.to_core(),
            restore_emoji: settings.restore_emoji,
            spoken_emoji: settings.spoken_emoji,
            spoken_punctuation: settings.spoken_punctuation,
            system_autotext: teletype_core::autotext::system::entries(),
            token_sink: None,
            polish_gate_enabled: settings.polish_gate_enabled,
            polish_skip_when_clean: settings.polish_skip_when_clean,
            polish_gate_threshold_words: settings.polish_gate_threshold_words,
            pack_terms: &pack_terms,
            word_checker: &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
            list_style: settings.list_style.resolve(),
            polish_max_change_percent: settings.polish_max_change_percent,
        };
        let result = pipeline.run(
            teletype_core::pipeline::UnifiedInput {
                source: teletype_core::pipeline::InputSource::Voice,
                text: raw,
            },
            None,
        );
        result.final_text
    })
    .await
    .map_err(|e| format!("pipeline task: {e}"))?;

    // Insert into history with the file name as the context label, plus a
    // day-wise transcript line, mirroring the live dictation path. The
    // in-memory snapshot is updated for immediate reads; the disk write goes
    // through the persistence actor (A4).
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = teletype_core::storage::now_ms();
    {
        let new_entry = teletype_core::history::DictationEntry {
            id: id.clone(),
            created_at,
            text: polished.clone(),
            context: Some(teletype_core::context::ApplicationContext {
                application_name: format!("File: {file_name}"),
                ..Default::default()
            }),
            duration_ms: Some(audio_duration_ms),
        };
        let mut next = (*state.history.get()).clone();
        next.push(new_entry.clone());
        state.history.set_snapshot(next);
        state.persist.history(new_entry);
    }
    {
        let dir = state.transcripts_dir();
        if let Err(e) = teletype_core::history::append_transcript_file(
            &dir,
            created_at,
            &polished,
            &format!("File: {file_name}"),
        ) {
            crate::log_entry(crate::LogLevel::Error, format!("transcript file: {e}"));
        }
    }
    let word_count = polished.split_whitespace().count();
    crate::log_entry(
        crate::LogLevel::Success,
        format!("file transcribed: {file_name} ({word_count} words)"),
    );
    Ok(TranscribeFileResult { id, word_count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_install_local_rejects_when_a_provider_appeared_during_warmup() {
        // BUG-015: a provider installed while we warmed up wins.
        assert!(!should_install_local(true, "local-server"));
    }

    #[test]
    fn should_install_local_rejects_when_selection_moved_off_local_server() {
        // BUG-015: the user picked a remote provider during warm-up.
        assert!(!should_install_local(false, "openai"));
    }

    #[test]
    fn should_install_local_installs_when_still_wanted_and_slot_is_free() {
        assert!(should_install_local(false, "local-server"));
    }

    #[test]
    fn list_speech_languages_returns_static_table() {
        let langs = speech_languages_static();
        // BUG-008: the full Whisper table, not the old 8-entry subset.
        assert!(
            langs.len() >= 90,
            "expected the full Whisper table, got {}",
            langs.len()
        );
        let codes: Vec<&str> = langs.iter().map(|l| l.code.as_str()).collect();
        // The 8 originals plus the long-tail languages the old table dropped.
        for expected in [
            "en", "zh", "de", "es", "ru", "ko", "fr", "pt", "it", "hi", "nl", "ja", "pl",
        ] {
            assert!(codes.contains(&expected), "missing {expected}");
        }
        assert!(langs.iter().all(|l| !l.name.is_empty()));
        // No duplicate codes — a dup would double a dropdown option.
        assert_eq!(
            codes.len(),
            codes.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }

    #[test]
    fn speech_model_list_only_offers_what_the_host_can_load() {
        use teletype_speech::catalog::Platform;
        let dir =
            std::env::temp_dir().join(format!("teletype-speech-list-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // "auto" is what a fresh install stores; it means "no language set".
        let unset = Some("auto");

        let windows = speech_model_list(Platform::Windows, &dir, "", true, unset, false);
        let on_windows: Vec<&str> = windows.iter().map(|m| m.id.as_str()).collect();
        assert!(on_windows.contains(&"sherpa-parakeet-v3-int8"));
        assert!(on_windows.contains(&"sherpa-parakeet-v2-int8"));
        assert!(windows.iter().all(|m| m.engine == "sherpaparakeet"));
        assert_eq!(
            windows
                .iter()
                .find(|m| m.recommended)
                .map(|m| m.id.as_str()),
            Some("sherpa-parakeet-v3-int8"),
            "Windows must recommend what it can run"
        );

        let mac = Platform::MacosAppleSilicon { major: 15 };
        let apple_silicon = speech_model_list(mac, &dir, "", true, unset, false);
        let ids: Vec<&str> = apple_silicon.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"parakeet-ultra-coreml"));
        assert!(ids.contains(&"parakeet-tdt-v3"));
        assert!(ids.contains(&"large-v3-turbo-q5"));
        // sherpa-onnx still *loads* on macOS, but its Windows-default rows are
        // deliberately not offered here: every native macOS engine beats them
        // and surfacing "Default on Windows" models confused onboarding.
        assert!(!ids.contains(&"sherpa-parakeet-v3-int8"));
        assert!(!ids.contains(&"sherpa-parakeet-v2-int8"));
        assert_eq!(
            apple_silicon
                .iter()
                .find(|m| m.recommended)
                .map(|m| m.id.as_str()),
            Some("parakeet-ultra-coreml")
        );

        // The CTC spotter is a boosting companion, never a dictation pick.
        assert!(!ids.contains(&"parakeet-ctc-110m-coreml"));

        let intel = speech_model_list(Platform::MacosIntel, &dir, "", true, unset, false);
        let intel_ids: Vec<&str> = intel.iter().map(|m| m.id.as_str()).collect();
        assert!(!intel_ids.contains(&"parakeet-ultra-coreml"));
        assert_eq!(
            intel.iter().find(|m| m.recommended).map(|m| m.id.as_str()),
            Some("parakeet-tdt-v3")
        );

        // No bridge: Core ML entries disappear even on eligible hardware, and
        // the recommendation moves with them. This is the onboarding bug: when
        // the badge named the filtered-out Ultra row, the model step rendered
        // no card and Continue stayed disabled forever.
        let no_bridge = speech_model_list(mac, &dir, "", false, unset, false);
        assert!(no_bridge.iter().all(|m| m.engine != "parakeetcoreml"));
        let recommended = no_bridge
            .iter()
            .find(|m| m.recommended)
            .expect("a bridge-less Mac must still recommend a listed row");
        assert_eq!(recommended.id, "parakeet-tdt-v3");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The recommendation and the list share one predicate, for every host the
    /// catalog knows. Two rows would make the badge a coin flip; zero would
    /// leave onboarding with nothing to render.
    #[test]
    fn speech_model_list_always_emits_exactly_one_recommended_row() {
        use teletype_speech::catalog::Platform;
        let dir = std::env::temp_dir().join(format!(
            "teletype-speech-list-reco-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        for platform in [
            Platform::MacosAppleSilicon { major: 26 },
            Platform::MacosAppleSilicon { major: 14 },
            Platform::MacosAppleSilicon { major: 13 },
            Platform::MacosAppleSilicon { major: 0 },
            Platform::MacosIntel,
            Platform::Windows,
            Platform::Other,
        ] {
            for language in [None, Some("auto"), Some("en"), Some("de"), Some("ar")] {
                for fluid in [true, false] {
                    let list = speech_model_list(platform, &dir, "", fluid, language, false);
                    let recommended: Vec<&str> = list
                        .iter()
                        .filter(|m| m.recommended)
                        .map(|m| m.id.as_str())
                        .collect();
                    assert_eq!(
                        recommended.len(),
                        1,
                        "{platform:?}/{language:?}/bridge={fluid} recommended {recommended:?} \
                         from {} listed rows",
                        list.len()
                    );
                    assert!(
                        !list.is_empty(),
                        "{platform:?}/bridge={fluid} listed no model at all"
                    );
                }
            }
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A language the recommended engine cannot transcribe must change the
    /// recommendation, not just the description: Ultra and Parakeet v3 cover
    /// the same 25 languages, so an Arabic user got a model that returns
    /// garbage. Windows has no Whisper, so it keeps the loadable sherpa build.
    #[test]
    fn speech_model_list_recommends_for_the_users_language() {
        use teletype_speech::catalog::Platform;
        let dir = std::env::temp_dir().join(format!(
            "teletype-speech-list-lang-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mac = Platform::MacosAppleSilicon { major: 15 };

        for fluid in [true, false] {
            let arabic = speech_model_list(mac, &dir, "", fluid, Some("ar"), false);
            assert_eq!(
                arabic.iter().find(|m| m.recommended).map(|m| m.id.as_str()),
                Some("large-v3-turbo-q5"),
                "macOS bridge={fluid} must recommend a model that does Arabic"
            );
            // German is inside Parakeet's list: the platform default stands.
            let german = speech_model_list(mac, &dir, "", fluid, Some("de-DE"), false);
            assert_eq!(
                german.iter().find(|m| m.recommended).map(|m| m.id.as_str()),
                Some(if fluid {
                    "parakeet-ultra-coreml"
                } else {
                    "parakeet-tdt-v3"
                }),
                "German is covered by Parakeet, so the platform default stands (bridge={fluid})"
            );

            let windows = speech_model_list(Platform::Windows, &dir, "", fluid, Some("ar"), false);
            assert_eq!(
                windows
                    .iter()
                    .find(|m| m.recommended)
                    .map(|m| m.id.as_str()),
                Some("sherpa-parakeet-v3-int8"),
                "Windows ships no Whisper, so it keeps the engine it can load"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The stored pick and the recommendation are independent flags on the same
    /// row list: an existing selection keeps its `selected` marker while the
    /// badge still points at what this host should install first.
    #[test]
    fn speech_model_list_selected_row_survives_alongside_the_recommendation() {
        use teletype_speech::catalog::Platform;
        let dir = std::env::temp_dir().join(format!(
            "teletype-speech-list-sel-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let list = speech_model_list(
            Platform::MacosAppleSilicon { major: 15 },
            &dir,
            "parakeet-tdt-v3",
            true,
            Some("en"),
            false,
        );
        assert_eq!(
            list.iter().filter(|m| m.selected).count(),
            1,
            "the stored pick is marked, independently of the recommendation"
        );
        assert_eq!(
            list.iter().find(|m| m.recommended).map(|m| m.id.as_str()),
            Some("parakeet-ultra-coreml")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The first-run view: three cards on an Apple Silicon Mac, not thirteen,
    /// and never empty, because onboarding renders the recommended card and a
    /// shortlist that dropped it would leave no card and a dead Continue button.
    #[test]
    fn speech_model_list_shortlists_for_first_run() {
        use teletype_speech::catalog::{Platform, SHORTLIST_MAX};
        let dir = std::env::temp_dir().join(format!(
            "teletype-speech-shortlist-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mac = Platform::MacosAppleSilicon { major: 15 };
        fn ids(list: &[SpeechModelStatus]) -> Vec<&str> {
            list.iter().map(|m| m.id.as_str()).collect()
        }

        let full = speech_model_list(mac, &dir, "", true, Some("en"), false);
        let first_run = speech_model_list(mac, &dir, "", true, Some("en"), true);
        assert_eq!(
            ids(&first_run),
            [
                "parakeet-ultra-coreml",
                "parakeet-tdt-v3",
                "large-v3-turbo-q5"
            ],
            "the Mac first run shows one card per decision"
        );
        assert!(
            full.len() > first_run.len() + 5,
            "Settings must keep the catalog the shortlist hides: {} vs {}",
            full.len(),
            first_run.len()
        );

        // Exactly one recommendation, and it leads.
        for (platform, language, fluid) in [
            (mac, Some("en"), true),
            (mac, Some("ar"), true),
            (mac, Some("en"), false),
            (Platform::MacosIntel, Some("en"), true),
            (Platform::Windows, Some("en"), true),
            (Platform::Windows, Some("ar"), true),
        ] {
            let list = speech_model_list(platform, &dir, "", fluid, language, true);
            assert!(
                !list.is_empty(),
                "{platform:?}/{language:?} shortlisted nothing"
            );
            assert!(
                list.len() <= SHORTLIST_MAX,
                "{platform:?} shortlisted too much"
            );
            assert_eq!(
                list.iter().filter(|m| m.recommended).count(),
                1,
                "{platform:?}/{language:?}/fluid={fluid}"
            );
            assert!(
                list[0].recommended,
                "{platform:?}/{language:?}/fluid={fluid} does not lead with its \
                 recommendation: {:?}",
                ids(&list)
            );
        }

        // A returning user's stored pick is not invisible just because the
        // shortlist prefers other rows; the ready check reads `selected`.
        let picked = speech_model_list(mac, &dir, "parakeet-tdt-v3-f16", true, Some("en"), true);
        assert_eq!(picked.iter().filter(|m| m.selected).count(), 1);
        assert!(ids(&picked).contains(&"parakeet-tdt-v3-f16"));

        // An unloadable or auxiliary stored pick is not surfaced.
        let stale = speech_model_list(
            Platform::Windows,
            &dir,
            "large-v3-turbo-q5",
            true,
            Some("en"),
            true,
        );
        assert!(!ids(&stale).contains(&"large-v3-turbo-q5"));
        let aux = speech_model_list(
            mac,
            &dir,
            "parakeet-ctc-110m-coreml",
            true,
            Some("en"),
            true,
        );
        assert!(!ids(&aux).contains(&"parakeet-ctc-110m-coreml"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Apple Intelligence row is a pure function of `selected_id`, so its
    /// field values are asserted here without needing a host where the OS
    /// exposes the model: `model_list` only builds it when available, which
    /// this test's Mac does not have.
    #[test]
    fn apple_intelligence_row_reports_a_ready_license_free_system_model() {
        let row = apple_intelligence_row("");
        assert_eq!(row.id, "apple-intelligence");
        assert_eq!(row.name, "Apple Intelligence");
        // Ready to use: it lives in the OS, so "downloaded" is true with a
        // zero footprint, and it must not sit behind a license gate.
        assert!(row.downloaded);
        assert!(row.polish_capable);
        assert!(row.recommended);
        assert_eq!(row.size_mb, 0);
        assert!(!row.requires_license_accept);
        assert!(row.license_name.is_none());
        assert!(row.license_url.is_none());
        assert!(!row.selected);

        let selected = apple_intelligence_row("apple-intelligence");
        assert!(selected.selected);
    }

    /// The polish list splits on the engine build, not the OS: the same GGUFs
    /// download everywhere, so the only rows a host may be steered away from are
    /// the ones its llama-server cannot carry. Settings keeps all of them; a
    /// first run sees the shortlist plus the user's own pick.
    #[test]
    fn model_list_shortlists_polish_rows_for_the_hosts_engine_build() {
        use teletype_inference::catalog::POLISH_SHORTLIST_MAX;
        let dir =
            std::env::temp_dir().join(format!("teletype-model-list-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        fn ids(list: &[ModelStatus]) -> Vec<&str> {
            list.iter().map(|m| m.id.as_str()).collect()
        }

        let ai_available = apple_intelligence_available();
        let full_cpu = model_list(false, &dir, "", false);
        let full_gpu = model_list(true, &dir, "", false);
        assert_eq!(full_cpu.len(), full_gpu.len(), "no row is hidden");
        assert!(
            full_cpu.len() >= 8,
            "the settings list must keep the catalog"
        );
        // The Apple Intelligence row is appended after the sorted catalog, so
        // the catalog order still leads the list.
        assert_eq!(full_gpu[0].id, "s1-mini");
        assert_eq!(
            ids(&full_gpu),
            ids(&full_cpu),
            "the full list must not reorder by host"
        );
        // The system model appears in the full list iff the OS exposes it.
        assert_eq!(
            ids(&full_cpu).contains(&"apple-intelligence"),
            ai_available,
            "the Apple Intelligence row must appear iff available"
        );

        // The host flag is what actually differs.
        let heavy = full_cpu.iter().filter(|m| !m.runs_on_host).count();
        assert_eq!(heavy, 6, "the rows a CPU llama-server is not sold");
        assert!(
            full_gpu.iter().all(|m| m.runs_on_host),
            "an accelerated host can carry every row"
        );

        let cpu_run = model_list(false, &dir, "", true);
        let gpu_run = model_list(true, &dir, "", true);
        assert_eq!(
            ids(&cpu_run),
            if ai_available {
                vec!["s1-mini", "speakoflow-mini", "apple-intelligence"]
            } else {
                vec!["s1-mini", "speakoflow-mini"]
            }
        );
        assert_eq!(
            ids(&gpu_run),
            if ai_available {
                vec!["s1-mini", "qwen3.5-2b", "apple-intelligence"]
            } else {
                vec!["s1-mini", "qwen3.5-2b"]
            }
        );
        for list in [&cpu_run, &gpu_run] {
            assert!(!list.is_empty());
            assert!(list.len() <= POLISH_SHORTLIST_MAX);
            assert!(
                list.iter().all(|m| m.polish_capable),
                "a decider reached the polish shortlist: {:?}",
                ids(list)
            );
            assert!(
                list.iter().all(|m| m.runs_on_host),
                "a shortlist row the host cannot carry: {:?}",
                ids(list)
            );
            assert!(list[0].recommended, "the first card must be a badge");
        }

        // A returning user's stored pick stays visible even when this host would
        // not have shortlisted it: the app uses that setting whether or not the
        // screen mentions it, so hiding it would be a lie by omission.
        let picked = model_list(false, &dir, "qwen2.5-3b", true);
        assert!(ids(&picked).contains(&"qwen2.5-3b"));
        assert_eq!(picked.iter().filter(|m| m.selected).count(), 1);

        // The Command Mode decider is not a polish choice on either host, so it
        // must never be appended as someone's "pick".
        let decider = model_list(true, &dir, "neudecide-q4", true);
        assert!(!ids(&decider).contains(&"neudecide-q4"));
        assert!(
            model_list(true, &dir, "neudecide-q4", false)
                .iter()
                .any(|m| m.id == "neudecide-q4"),
            "Settings still has to show the decider it uses for Command Mode"
        );

        // Unknown ids append nothing rather than panicking.
        assert_eq!(
            ids(&model_list(true, &dir, "ollama/llama3", true)),
            if ai_available {
                vec!["s1-mini", "qwen3.5-2b", "apple-intelligence"]
            } else {
                vec!["s1-mini", "qwen3.5-2b"]
            }
        );

        // A user who already chose the system model keeps seeing it selected.
        let ai = model_list(true, &dir, "apple-intelligence", true);
        assert_eq!(
            ids(&ai).contains(&"apple-intelligence"),
            ai_available,
            "the stored Apple Intelligence pick must be visible iff available"
        );
        if ai_available {
            assert_eq!(ai.iter().filter(|m| m.selected).count(), 1);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn speech_model_installed_follows_each_engine_layout() {
        use teletype_speech::catalog::find;
        let dir = std::env::temp_dir().join(format!(
            "teletype-speech-installed-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let ggml = find("parakeet-tdt-v3").unwrap();
        assert!(!speech_model_installed(ggml, &dir, true));
        std::fs::write(ggml.provider_path(&dir), b"x").unwrap();
        assert!(speech_model_installed(ggml, &dir, true));

        let sherpa = find("sherpa-parakeet-v3-int8").unwrap();
        let sherpa_dir = sherpa.provider_path(&dir);
        std::fs::create_dir_all(&sherpa_dir).unwrap();
        // The archive itself is not the model.
        std::fs::write(dir.join("sherpa").join(sherpa.file), b"archive").unwrap();
        assert!(!speech_model_installed(sherpa, &dir, true));
        for file in teletype_speech::sherpa::MODEL_FILES {
            std::fs::write(sherpa_dir.join(file), b"x").unwrap();
        }
        assert!(speech_model_installed(sherpa, &dir, true));

        // Core ML is downloaded by the bridge on first load, so the bridge's
        // presence is what makes it usable.
        let fluid = find("parakeet-ultra-coreml").unwrap();
        assert!(!speech_model_installed(fluid, &dir, false));
        assert!(speech_model_installed(fluid, &dir, true));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn speech_model_ready_reports_absent_empty_and_present() {
        // Use a real catalog id so the file name is known.
        let id = "parakeet-tdt-v3";
        let file = teletype_speech::catalog::find(id).unwrap().file;
        let dir = std::env::temp_dir().join(format!(
            "teletype-speech-ready-test-{}-{}",
            std::process::id(),
            id
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(file);

        // (a) file absent.
        let r = speech_model_ready(id, &dir, false);
        assert!(!r.downloaded, "absent: {r:?}");
        assert!(!r.ready, "absent: {r:?}");
        assert!(r.message.contains("not downloaded"), "absent: {r:?}");

        // (b) 0-byte file.
        std::fs::write(&path, b"").unwrap();
        let r = speech_model_ready(id, &dir, false);
        assert!(r.downloaded, "empty: {r:?}");
        assert!(!r.ready, "empty: {r:?}");
        assert!(r.message.contains("incomplete"), "empty: {r:?}");

        // (c) non-empty file.
        std::fs::write(&path, b"model-bytes").unwrap();
        let r = speech_model_ready(id, &dir, false);
        assert!(r.downloaded, "present: {r:?}");
        assert!(r.ready, "present: {r:?}");
        assert!(r.message.contains("ready"), "present: {r:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The readiness contract the UI branches on. These are the states
    /// `get_runtime_status` promises, and the distinction between `missing`
    /// and `ready` is the whole point: a file on disk that has not been loaded
    /// is a working setup, and telling the user otherwise sends them to fix
    /// something that is not broken.
    #[test]
    fn speech_runtime_reports_the_state_the_user_can_act_on() {
        let dir = std::env::temp_dir().join("teletype-speech-runtime");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let id = teletype_speech::catalog::CATALOG[0].id;
        let file = dir.join(teletype_speech::catalog::CATALOG[0].file);

        // Nothing selected.
        let r = speech_runtime("", &dir, "Whisper engine", false, false);
        assert_eq!(r.state, "none");
        assert!(r.blocking, "no model selected must block: {r:?}");

        // Selected but absent from disk. This is the state the old Home
        // banner got wrong: another model existed, so it said nothing.
        let r = speech_runtime(id, &dir, "Whisper engine", false, false);
        assert_eq!(r.state, "missing");
        assert!(r.blocking, "a missing model must block: {r:?}");
        assert!(
            r.detail.contains("not downloaded"),
            "should say what to do: {r:?}"
        );

        // Present but empty: exists() would call this ready.
        std::fs::write(&file, b"").unwrap();
        let r = speech_runtime(id, &dir, "Whisper engine", false, false);
        assert_eq!(r.state, "missing", "0-byte is not usable: {r:?}");

        // On disk, not loaded. Must NOT be blocking: dictation loads it on
        // first use, and this is the normal state after an idle unload.
        std::fs::write(&file, b"model-bytes").unwrap();
        let r = speech_runtime(id, &dir, "Whisper engine", true, false);
        assert_eq!(r.state, "ready");
        assert!(
            !r.blocking,
            "an unloaded-but-present model is not a blocker: {r:?}"
        );
        assert!(r.detail.contains("first dictation"), "{r:?}");

        // Loaded.
        let r = speech_runtime(id, &dir, "Parakeet engine", true, true);
        assert_eq!(r.state, "loaded");
        assert!(!r.blocking, "{r:?}");
        assert!(
            r.detail.contains("Parakeet"),
            "engine should be named: {r:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `on_disk` is passed separately from the file check, so a caller that
    /// gets it wrong must be caught here rather than in the field: claiming
    /// loaded with no bytes on disk would be the worst possible lie.
    #[test]
    fn speech_runtime_never_claims_loaded_without_a_usable_file() {
        let dir = std::env::temp_dir().join("teletype-speech-runtime-loaded");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let id = teletype_speech::catalog::CATALOG[0].id;

        std::fs::write(dir.join(teletype_speech::catalog::CATALOG[0].file), b"").unwrap();
        let r = speech_runtime(id, &dir, "Whisper engine", true, true);
        assert_ne!(
            r.state, "loaded",
            "reported loaded with an empty file: {r:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- B8: patch-based settings (lost-update fix) ----

    #[test]
    fn should_add_space_empty_context() {
        // Start of the field: nothing to separate from.
        assert!(!should_add_space(""));
    }

    #[test]
    fn should_add_space_after_word() {
        // Mid-sentence: a space is needed before the dictated word.
        assert!(should_add_space("hello"));
        assert!(should_add_space("hello world"));
    }

    #[test]
    fn should_add_space_after_existing_space() {
        // The gap is already there; adding another would double it.
        assert!(!should_add_space("hello "));
    }

    #[test]
    fn should_add_space_after_tab() {
        // A tab is whitespace: the gap is already there.
        assert!(!should_add_space("hello\t"));
    }

    #[test]
    fn should_add_space_after_newline() {
        // A newline is whitespace: the gap is already there.
        assert!(!should_add_space("hello\n"));
        assert!(!should_add_space("line one\nline two\n"));
    }

    #[test]
    fn should_add_space_after_sentence_end() {
        // The dictated text starts a new sentence: no space in front.
        assert!(!should_add_space("hello."));
        assert!(!should_add_space("hello!"));
        assert!(!should_add_space("hello?"));
        assert!(!should_add_space("hello:"));
    }

    #[test]
    fn should_add_space_after_cjk() {
        // CJK scripts do not separate words with spaces.
        assert!(!should_add_space("你好"));
        assert!(!should_add_space("hello 世界"));
        assert!(!should_add_space("こんにちは"));
        assert!(!should_add_space("カタカナ"));
        assert!(!should_add_space("안녕"));
    }

    #[test]
    fn should_add_space_after_mid_word_punctuation() {
        // Mid-word punctuation (a hyphen, an apostrophe, a comma that is
        // not sentence-ending) still needs a space: the dictated word
        // continues the same sentence.
        assert!(should_add_space("well-"));
        assert!(should_add_space("don't"));
        assert!(should_add_space("hello,"));
    }

    #[test]
    fn should_add_space_after_digit() {
        // A digit is a word character: a space is needed.
        assert!(should_add_space("2026"));
        assert!(should_add_space("room 42"));
    }

    #[test]
    fn partial_files_to_delete_finds_part_files_only() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path();
        let id = "test-model";
        // Single-file partial.
        std::fs::write(models.join(format!("{id}.gguf.part")), b"partial").unwrap();
        // Shard dir: one partial, one finished shard.
        let shard_dir = models.join(id);
        std::fs::create_dir_all(&shard_dir).unwrap();
        std::fs::write(shard_dir.join("part-1.part"), b"partial shard").unwrap();
        std::fs::write(shard_dir.join("part-1.gguf"), b"finished shard").unwrap();

        let found = partial_files_to_delete(models, id);
        assert_eq!(
            found.len(),
            2,
            "only .part files should be returned: {found:?}"
        );
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n == "part-1.part"));
        assert!(!names.iter().any(|n| n == "part-1.gguf"));
        assert!(names.iter().any(|n| *n == format!("{id}.gguf.part")));
    }

    #[test]
    fn partial_files_to_delete_empty_when_nothing_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let found = partial_files_to_delete(dir.path(), "missing-model");
        assert!(found.is_empty());
    }

    #[test]
    fn remove_llm_model_files_single_removes_gguf() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model-a.gguf");
        std::fs::write(&file, b"gguf").unwrap();
        remove_llm_model_files(dir.path(), "model-a", false).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn remove_llm_model_files_multi_removes_shard_dir() {
        let dir = tempfile::tempdir().unwrap();
        let shard = dir.path().join("model-b");
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::write(shard.join("part-1.gguf"), b"shard").unwrap();
        remove_llm_model_files(dir.path(), "model-b", true).unwrap();
        assert!(!shard.exists());
        assert!(!dir.path().join("model-b.gguf").exists());
    }

    #[test]
    fn remove_llm_model_files_missing_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(remove_llm_model_files(dir.path(), "ghost", false).is_err());
    }

    #[test]
    fn qwen3_asr_is_installed_only_when_both_ggufs_are_present() {
        use teletype_speech::catalog::QWEN3_ASR_ARTIFACTS as Arts;
        let dir = tempfile::tempdir().unwrap();
        let entry = teletype_speech::catalog::find("qwen3-asr").expect("qwen3-asr is in the catalog");
        let d = entry.provider_path(dir.path());
        assert!(!speech_model_installed(entry, dir.path(), true));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(Arts[0].file), b"gguf").unwrap();
        assert!(
            !speech_model_installed(entry, dir.path(), true),
            "the audio projector is still missing"
        );
        std::fs::write(d.join(Arts[1].file), b"mmproj").unwrap();
        assert!(speech_model_installed(entry, dir.path(), true));
        std::fs::write(d.join(Arts[1].file), b"").unwrap();
        assert!(
            !speech_model_installed(entry, dir.path(), true),
            "a zero-byte file is not a model"
        );
    }

    /// The pins are only as good as the publisher's listing. A silently
    /// republished GGUF downloads, fails verification, and the row is dead
    /// forever with no test telling us. Run this after a Hugging Face repo
    /// change, or whenever a Qwen3-ASR download is suspected broken:
    ///
    /// ```text
    /// cargo test -p teletype-desktop --lib qwen3_asr_published \
    ///     -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "queries huggingface.co; run explicitly"]
    fn qwen3_asr_published_bytes_are_the_pinned_ones() {
        use teletype_speech::catalog::QWEN3_ASR_ARTIFACTS as Arts;
        let url = "https://huggingface.co/api/models/ggml-org/Qwen3-ASR-1.7B-GGUF/tree/main?recursive=true";
        let body = reqwest::blocking::get(url)
            .expect("HF tree API request")
            .text()
            .expect("HF tree API body");
        let tree: serde_json::Value = serde_json::from_str(&body).expect("HF tree JSON");
        let items = tree.as_array().expect("HF tree is an array");
        for art in Arts {
            let entry = items
                .iter()
                .find(|e| e["path"].as_str() == Some(art.file))
                .unwrap_or_else(|| panic!("{} is no longer in the repo", art.file));
            assert_eq!(
                entry["size"].as_u64(),
                Some(art.bytes),
                "{} size changed from the pin",
                art.file
            );
            // The LFS oid is the sha256 of the resolved bytes.
            assert_eq!(
                entry["lfs"]["oid"].as_str().map(|s| s.to_ascii_lowercase()),
                Some(art.sha256.to_ascii_lowercase()),
                "{} was republished; re-pin it",
                art.file
            );
        }
    }

    #[test]
    fn speech_file_already_installed_requires_the_pinned_digest_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.gguf");
        std::fs::write(&path, b"ggml").unwrap();
        let digest: &'static str =
            Box::leak(sha256_file(&path).unwrap().into_boxed_str());
        let spec = |sha: &'static str, bytes: u64| SpeechFileSpec {
            name: "a.gguf".into(),
            url: String::new(),
            dest: path.clone(),
            sha256: Some(sha),
            bytes,
        };
        assert!(
            speech_file_already_installed(&spec(digest, 4)),
            "the verified file must not be re-fetched"
        );
        assert!(
            !speech_file_already_installed(&spec(digest, 5)),
            "a size mismatch means the pin no longer describes this file"
        );
        assert!(
            !speech_file_already_installed(&spec("0".repeat(64).leak(), 4)),
            "a foreign digest must re-download"
        );
        assert!(
            !speech_file_already_installed(&SpeechFileSpec {
                name: "a.gguf".into(),
                url: String::new(),
                dest: dir.path().join("absent.gguf"),
                sha256: Some(digest),
                bytes: 4,
            }),
            "no file on disk is never already installed"
        );
    }

    #[test]
    fn remove_speech_model_files_file_engine() {
        let dir = tempfile::tempdir().unwrap();
        // Whisper/Parakeet/ParakeetCpp: install_path is models_dir/<file>.
        let entry = teletype_speech::catalog::find("large-v3-turbo-q5")
            .expect("large-v3-turbo-q5 is in the catalog");
        let path = entry.install_path(dir.path());
        std::fs::write(&path, b"ggml").unwrap();
        remove_speech_model_files(dir.path(), entry).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn remove_speech_model_files_missing_errors() {
        let dir = tempfile::tempdir().unwrap();
        let entry = teletype_speech::catalog::find("large-v3-turbo-q5")
            .expect("large-v3-turbo-q5 is in the catalog");
        assert!(remove_speech_model_files(dir.path(), entry).is_err());
    }

    #[test]
    fn model_status_ships_the_polish_role_under_the_key_the_ui_reads() {
        // The Models rail splits on `polishCapable`, so the camelCase key is a
        // cross-language contract with no compiler on the far end: drop
        // `rename_all` and serde emits `polish_capable`, the TS field reads
        // `undefined`, and every row lands in the polish list again.
        let status = ModelStatus {
            id: "neudecide-q4".into(),
            name: "NeuDecide Q4 (decider)".into(),
            size_mb: 41,
            description: "Speech-to-tool-call decider for Command Mode.".into(),
            specialty: SpecialtyStatus {
                tag: String::new(),
                measured: false,
            },
            downloaded: true,
            selected: false,
            recommended: false,
            polish_capable: false,
            verdict: "Not tested".into(),
            verdict_note: String::new(),
            license_name: None,
            license_url: None,
            requires_license_accept: false,
            attribution: None,
            runs_on_host: true,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"polishCapable\":false"), "{json}");
        assert!(!json.contains("polish_capable"), "{json}");
        assert!(json.contains("\"runsOnHost\":true"), "{json}");
        assert!(
            json.contains("\"specialty\":{\"tag\":\"\",\"measured\":false}"),
            "{json}"
        );
        assert!(!json.contains("runs_on_host"), "{json}");
    }
}
