//! Model commands: LLM catalog/download/select, secrets, Ollama, speech models, runtime and file transcription.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use teletype_core::llm::InferenceProvider;
use crate::AppState;
use super::*;

// ---- Models ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub id: String,
    pub name: String,
    pub size_mb: u32,
    pub description: String,
    pub downloaded: bool,
    pub selected: bool,
    pub recommended: bool,
    pub verdict: String,
    pub verdict_note: String,
    pub license_name: Option<String>,
    pub license_url: Option<String>,
    pub requires_license_accept: bool,
    pub attribution: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelStatus {
    pub id: String,
    pub name: String,
    pub engine: String,
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

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> CommandResult<Vec<ModelStatus>> {
    let settings = state.settings();
    let mut models: Vec<ModelStatus> = teletype_inference::catalog::CATALOG
        .iter()
        .map(|e| ModelStatus {
            id: e.id.into(),
            name: e.name.into(),
            size_mb: e.size_mb,
            description: e.description.into(),
            downloaded: e.is_downloaded(&state.models_dir),
            selected: settings.selected_llm_model == e.id,
            recommended: e.recommended,
            verdict: e.verdict.label().into(),
            verdict_note: e.verdict_note.into(),
            license_name: e.license_name.map(str::to_string),
            license_url: e.license_url.map(str::to_string),
            requires_license_accept: e.requires_license_accept,
            attribution: e.attribution.map(str::to_string),
        })
        .collect();
    // Stable sort by the catalog's explicit priority so the list order is
    // independent of array position (EG-1 first, then S1-mini, then the rest).
    models.sort_by_key(|m| {
        teletype_inference::catalog::find(&m.id)
            .map(|e| e.sort_order)
            .unwrap_or(u32::MAX)
    });
    Ok(models)
}

#[tauri::command]
pub async fn select_model(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let entry =
        teletype_inference::catalog::find(&id).ok_or_else(|| format!("Unknown model '{id}'"))?;
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
    let old_engine = state.inference.get();
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

    state.inference.set(Arc::new(provider));

    let mut settings = state.settings();
    settings.selected_llm_model = id.clone();
    settings.selected_llm_provider = "local-server".into();
    state.replace_settings(settings)?;
    tracing::info!(model = %id, "llama-server provider ready");
    Ok(())
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
            &models_dir,
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

    result
        .map(|handle| handle.to_string_lossy().to_string())
        .map_err(CommandError::internal)
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

    let old = state.inference.get();
    state.inference.set(Arc::new(provider));
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
            state.inference.set(Arc::new(provider));
            crate::log_entry(
                crate::LogLevel::Success,
                format!("LLM restored: API ({model})"),
            );
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
    if state.inference.get().is_some() {
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
    state.inference.set(Arc::new(provider));
    crate::log_entry_ms(
        crate::LogLevel::Success,
        format!("LLM model loaded: {model_id}"),
        started.elapsed().as_millis() as u64,
    );
}

#[tauri::command]
pub async fn list_speech_models(
    state: State<'_, AppState>,
) -> CommandResult<Vec<SpeechModelStatus>> {
    let settings = state.settings();
    let models: Vec<SpeechModelStatus> = teletype_speech::catalog::CATALOG
        .iter()
        .map(|m| {
            let path = state.models_dir.join(m.file);
            SpeechModelStatus {
                id: m.id.into(),
                name: m.name.into(),
                engine: format!("{:?}", m.engine).to_lowercase(),
                size_mb: m.size_mb,
                description: m.description.into(),
                recommended: m.recommended,
                english_only: m.english_only,
                downloaded: path.exists(),
                selected: settings.selected_speech_model == m.id,
                speed: m.speed,
                accuracy: m.accuracy,
                min_ram_gb: m.min_ram_gb,
                language_label: m.language_label(),
                languages: m
                    .languages
                    .map(|l| l.iter().map(|s| s.to_string()).collect()),
            }
        })
        .collect();
    Ok(models)
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
    if teletype_speech::catalog::find(&id).is_none() {
        return Err(CommandError::model(format!("Unknown speech model '{id}'")));
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

#[tauri::command]
pub async fn download_speech_model(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<String> {
    let entry = teletype_speech::catalog::find(&id)
        .ok_or_else(|| format!("Unknown speech model '{id}'"))?;
    let dest = state.models_dir.join(entry.file);
    if dest.exists() {
        return Ok(dest.to_string_lossy().to_string());
    }

    let entry_name = entry.name.to_string();

    // Pre-flight: fail fast on a full disk before any bytes are transferred.
    // `check_disk_space` already applies its 2.2x headroom factor internally,
    // so pass the raw size.
    let needed = (entry.size_mb as u64).saturating_mul(1024 * 1024);
    if needed > 0 {
        teletype_inference::download::check_disk_space(&state.models_dir, needed)
            .map_err(|e| format!("{entry_name}: {e}"))?;
    }

    // Stream to a temp file with live progress, then rename into place so
    // a half-finished download is never mistaken for an installed model.
    let dest_clone = dest.clone();
    let url = entry.url();
    let expected_total = (entry.size_mb as u64).saturating_mul(1024 * 1024);
    let event_id = id.clone();
    let file_name = entry.file.to_string();

    let handle = tauri::async_runtime::spawn_blocking(move || {
        let emit = |status: &str,
                    written: u64,
                    total: u64,
                    speed_bps: f64,
                    eta: Option<f64>,
                    err: Option<String>| {
            let payload = serde_json::json!({
                "id": event_id,
                "kind": "speech",
                "status": status,
                "fileName": file_name,
                "fileIndex": 1u32,
                "fileCount": 1u32,
                "fileDownloadedBytes": written,
                "fileTotalBytes": total,
                "downloadedBytes": written,
                "totalBytes": total,
                "percent": if total > 0 { (written as f64 / total as f64 * 100.0).min(100.0) } else { 0.0 },
                "speedBps": speed_bps,
                "etaSeconds": eta,
                "error": err,
            });
            let _ = app.emit("model-download-progress", payload);
        };

        let run = || -> Result<(), String> {
            let tmp = dest_clone.with_extension("bin.part");
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|e| e.to_string())?;
            let mut response = client
                .get(&url)
                .header("User-Agent", "teletype/0.1")
                .send()
                .map_err(|e| e.to_string())?;
            if !response.status().is_success() {
                return Err(format!("HTTP {}", response.status()));
            }
            let content_len = response.content_length().unwrap_or(expected_total);
            let total = if content_len > 0 {
                content_len
            } else {
                expected_total
            };

            let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            use std::io::{Read, Write};
            let mut buf = [0u8; 64 * 1024];
            let mut written: u64 = 0;
            let started = std::time::Instant::now();
            let mut last_emit = std::time::Instant::now()
                .checked_sub(std::time::Duration::from_secs(1))
                .unwrap_or_else(std::time::Instant::now);
            let mut speed = 0.0f64;

            loop {
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
                    emit("downloading", written, total, speed, eta, None);
                }
            }
            file.flush().map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);

            // Verify before installing. Previously this emitted a "verifying"
            // status and then renamed the file unconditionally, so a download
            // cut short at 90% was installed and reported as 100% Done, and
            // the Models screen then showed it as downloaded. The next
            // dictation either mis-transcribed or failed to load, with nothing
            // pointing back at the download.
            //
            // Two checks, in order of confidence:
            //   1. SHA-256, when the catalog carries one. No speech entry does
            //      yet (the field exists and is read here so filling it in is
            //      a one-line change per model).
            //   2. Size, always. `size_mb` is the publisher's own figure and is
            //      approximate, so it is a sanity bound, not proof of
            //      integrity, but it is what catches a truncated transfer.
            emit(
                "verifying",
                written,
                total.max(written),
                0.0,
                Some(0.0),
                None,
            );

            if let Some(want_sha) = entry.sha256 {
                let actual = sha256_file(&tmp)?;
                if !actual.eq_ignore_ascii_case(want_sha) {
                    let _ = std::fs::remove_file(&tmp);
                    return Err(format!(
                        "{entry_name} failed its checksum. The download was discarded; \
                         please try again."
                    ));
                }
            }

            let expected_bytes = (entry.size_mb as u64).saturating_mul(1024 * 1024);
            // Allow 1% slack: `size_mb` is rounded, and the last byte of the
            // reported figure is not exact.
            let floor = expected_bytes.saturating_mul(99) / 100;
            if expected_bytes > 0 && written < floor {
                let _ = std::fs::remove_file(&tmp);
                return Err(format!(
                    "{entry_name} downloaded {} of about {} bytes, so it is incomplete. \
                     The partial file was discarded; please try again.",
                    written, expected_bytes
                ));
            }

            std::fs::rename(&tmp, &dest_clone).map_err(|e| e.to_string())?;
            emit(
                "done",
                total.max(written),
                total.max(written),
                0.0,
                Some(0.0),
                None,
            );
            Ok(())
        };

        match run() {
            Ok(()) => Ok(()),
            Err(e) => {
                emit("error", 0, expected_total, 0.0, None, Some(e.clone()));
                Err(CommandError::internal(e))
            }
        }
    });
    handle.await.map_err(|e| format!("download task: {e}"))??;
    Ok(dest.to_string_lossy().to_string())
}

/// Pure decision for [`get_speech_model_ready_state`], factored out so it can
/// be unit-tested without a Tauri `State`.
fn speech_model_ready(id: &str, models_dir: &std::path::Path) -> SpeechModelReady {
    let entry = teletype_speech::catalog::find(id);
    let path = models_dir.join(entry.map(|e| e.file).unwrap_or(""));
    let exists = path.exists();
    let nonempty = exists && path.metadata().map(|m| m.len() > 0).unwrap_or(false);
    let ready = nonempty;
    let name = entry.map(|e| e.name).unwrap_or("Your model");
    let message = if ready {
        format!("{name} is ready. Hold your hotkey and speak.")
    } else if exists {
        "The model file is present but empty. Re-download it.".into()
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
    Ok(speech_model_ready(&id, &state.models_dir))
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
/// `on_disk` and `loaded` are passed in rather than read here so this stays a
/// pure function of its inputs.
fn speech_runtime(
    selected_id: &str,
    models_dir: &std::path::Path,
    engine_label: &str,
    on_disk: bool,
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

    // Read the real file rather than trusting `exists`: a 0-byte leftover from
    // an interrupted download reads as present and is not usable.
    let file = entry.map(|e| e.file.to_string());
    let usable = match file.as_deref() {
        Some(f) => models_dir
            .join(f)
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        // Unknown id: fall back to the loader's own naming so this agrees
        // with `resolve_speech_model`, which would look for `<id>.bin`.
        None => models_dir
            .join(format!("{selected_id}.bin"))
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
    };

    if !on_disk && !usable {
        return ComponentRuntime::new(
            "missing",
            name,
            &id,
            format!("{name} is selected but not downloaded yet."),
            true,
        );
    }
    if !usable {
        return ComponentRuntime::new(
            "missing",
            name,
            &id,
            format!("The {name} file is empty or incomplete. Download it again."),
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

#[tauri::command]
pub async fn get_runtime_status(state: State<'_, AppState>) -> CommandResult<RuntimeStatus> {
    let settings = state.settings();

    // Resolve through the same helper the dictation worker uses. If this
    // computed the path any other way it could report on a different model
    // than the one about to be loaded, which would be worse than reporting
    // nothing at all.
    let (_, engine) = crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    let (loaded, engine_label) = match engine {
        crate::commands::SpeechEngine::Parakeet => (state.parakeet.is_loaded(), "Parakeet engine"),
        crate::commands::SpeechEngine::ParakeetCpp => (state.parakeet_cpp.is_loaded(), "Parakeet.cpp engine"),
        _ => (state.speech.is_loaded(), "Whisper engine"),
    };
    let speech = speech_runtime(
        &settings.selected_speech_model,
        &state.models_dir,
        engine_label,
        loaded || !settings.selected_speech_model.is_empty(),
        loaded,
    );

    let llm = llm_runtime(&state, &settings);
    Ok(RuntimeStatus { speech, llm })
}

fn llm_runtime(state: &AppState, settings: &Settings) -> ComponentRuntime {
    let engine = state.inference.get();

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
    let engine = state.inference.get();
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
    let (model_path, engine) =
        crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    // The frontmost-app language override wins, same as the live path.
    let language = crate::dictation::effective_language_for_app(&state, engine);

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
        crate::dictation::transcribe(&app2, &model_path2, &captured, &language2, engine)
    })
    .await
    .map_err(|e| format!("transcribe task: {e}"))??;
    let raw = crate::dictation::ensure_latin_if_english(
        &raw,
        &crate::dictation::effective_language(
            settings.language.clone(),
            engine == crate::commands::SpeechEngine::Parakeet,
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
        let engine = match state.inference.get() {
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
            dictionary: &dictionary,
            styles: &styles,
            active_style: &active_style,
            explicit_style: "",
            auto_apply: settings.auto_apply_transform,
            restore_clipboard: settings.restore_clipboard,
            remove_filler_words: settings.remove_filler_words,
            filler_words: settings.filler_words.clone(),
            restore_emoji: settings.restore_emoji,
            spoken_emoji: settings.spoken_emoji,
            spoken_punctuation: settings.spoken_punctuation,
            system_autotext: teletype_core::autotext::system::entries(),
            token_sink: None,
            polish_gate_enabled: settings.polish_gate_enabled,
            polish_gate_threshold_words: settings.polish_gate_threshold_words,
            pack_terms: &pack_terms,
            word_checker: &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
            list_style: settings.list_style.resolve(),
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
        let r = speech_model_ready(id, &dir);
        assert!(!r.downloaded, "absent: {r:?}");
        assert!(!r.ready, "absent: {r:?}");
        assert!(r.message.contains("not downloaded"), "absent: {r:?}");

        // (b) 0-byte file.
        std::fs::write(&path, b"").unwrap();
        let r = speech_model_ready(id, &dir);
        assert!(r.downloaded, "empty: {r:?}");
        assert!(!r.ready, "empty: {r:?}");
        assert!(r.message.contains("empty"), "empty: {r:?}");

        // (c) non-empty file.
        std::fs::write(&path, b"model-bytes").unwrap();
        let r = speech_model_ready(id, &dir);
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

}
