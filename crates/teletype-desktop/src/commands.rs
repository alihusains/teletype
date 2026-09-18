//! Tauri commands: the IPC surface the React UI calls.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use teletype_core::{
    autotext::{self, AutoTextEntry},
    personalization::{Preference, UserProfile},
    platform::{Permission, PermissionKind},
    transforms::TransformDefinition,
};

use crate::AppState;

type CommandResult<T> = Result<T, String>;

// ---- Settings ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkey: String,
    pub recording_mode: String,
    pub selected_speech_model: String,
    pub language: String,
    pub input_device: String,
    pub restore_clipboard: bool,
    pub auto_apply_transform: bool,
    pub show_tray_icon: bool,
    pub has_completed_onboarding: bool,
    pub selected_llm_model: String,
    pub typing_autotext_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: String::new(),
            recording_mode: "hold".into(),
            selected_speech_model: "parakeet-tdt-v3".into(),
            language: "en".into(),
            input_device: String::new(),
            restore_clipboard: true,
            auto_apply_transform: true,
            show_tray_icon: true,
            has_completed_onboarding: false,
            selected_llm_model: String::new(),
            typing_autotext_enabled: false,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    os: &'static str,
    version: String,
    setup_notes: Vec<String>,
    permissions: Vec<Permission>,
}

#[tauri::command]
pub async fn get_status(app: AppHandle, state: State<'_, AppState>) -> CommandResult<Status> {
    Ok(Status {
        os: std::env::consts::OS,
        version: app.package_info().version.to_string(),
        setup_notes: state.platform.setup_notes(),
        permissions: state.platform.permissions(),
    })
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CommandResult<Settings> {
    Ok(state.settings())
}

#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> CommandResult<Settings> {
    let previous = state.settings();
    let hotkey_changed = settings.hotkey != previous.hotkey;

    if hotkey_changed && !settings.hotkey.is_empty() {
        if let Err(e) = state.controller.register_hotkey(&app, &settings.hotkey) {
            // Revert.
            if !previous.hotkey.is_empty() {
                let _ = state.controller.register_hotkey(&app, &previous.hotkey);
            }
            return Err(e);
        }
    }
    state.replace_settings(settings.clone())?;
    Ok(settings)
}

// ---- Devices & Permissions ----

#[tauri::command]
pub fn list_input_devices() -> CommandResult<Vec<teletype_core::audio::InputDevice>> {
    Ok(teletype_core::audio::list_input_devices())
}

#[tauri::command]
pub async fn get_permissions(state: State<'_, AppState>) -> CommandResult<Vec<Permission>> {
    Ok(state.platform.permissions())
}

#[tauri::command]
pub async fn request_permission(
    state: State<'_, AppState>,
    kind: PermissionKind,
) -> CommandResult<()> {
    state.platform.request_permission(kind);
    Ok(())
}

#[tauri::command]
pub async fn open_permission_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: PermissionKind,
) -> CommandResult<()> {
    if let Some(url) = state.platform.permission_settings_url(kind) {
        use tauri_plugin_opener::OpenerExt;
        app.opener()
            .open_url(url, None::<&str>)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---- Dictation ----

#[tauri::command]
pub async fn toggle_dictation(state: State<'_, AppState>) -> CommandResult<()> {
    state.controller.send(crate::dictation::Event::Toggle);
    Ok(())
}

#[tauri::command]
pub async fn get_dictation_state(
    state: State<'_, AppState>,
) -> CommandResult<teletype_core::state::UiState> {
    Ok(state
        .dictation_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone())
}

// ---- AutoText ----

#[tauri::command]
pub async fn list_autotext(state: State<'_, AppState>) -> CommandResult<Vec<AutoTextEntry>> {
    Ok(state
        .autotext
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entries
        .clone())
}

#[tauri::command]
pub async fn create_autotext(
    state: State<'_, AppState>,
    entry: AutoTextEntry,
) -> CommandResult<AutoTextEntry> {
    autotext::validate_trigger(&entry.trigger).map_err(|e| e.to_string())?;
    let mut store = state
        .autotext
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.insert(entry.clone())?;
    state.autotext_store.save(&*store)?;
    Ok(entry)
}

#[tauri::command]
pub async fn update_autotext(
    state: State<'_, AppState>,
    entry: AutoTextEntry,
) -> CommandResult<AutoTextEntry> {
    autotext::validate_trigger(&entry.trigger).map_err(|e| e.to_string())?;
    let mut store = state
        .autotext
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.update(entry.clone())?;
    state.autotext_store.save(&*store)?;
    Ok(entry)
}

#[tauri::command]
pub async fn delete_autotext(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut store = state
        .autotext
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !store.remove(&id) {
        return Err("Entry not found".into());
    }
    state.autotext_store.save(&*store)?;
    Ok(())
}

// ---- Transforms ----

#[tauri::command]
pub async fn list_transforms(
    state: State<'_, AppState>,
) -> CommandResult<Vec<TransformDefinition>> {
    Ok(state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .transforms
        .clone())
}

#[tauri::command]
pub async fn create_transform(
    state: State<'_, AppState>,
    transform: TransformDefinition,
) -> CommandResult<TransformDefinition> {
    let mut store = state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.insert(transform.clone())?;
    state.transforms_store.save(&*store)?;
    Ok(transform)
}

#[tauri::command]
pub async fn update_transform(
    state: State<'_, AppState>,
    transform: TransformDefinition,
) -> CommandResult<TransformDefinition> {
    let mut store = state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.update(transform.clone())?;
    state.transforms_store.save(&*store)?;
    Ok(transform)
}

#[tauri::command]
pub async fn delete_transform(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut store = state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.remove(&id)?;
    state.transforms_store.save(&*store)?;
    Ok(())
}

#[tauri::command]
pub async fn reset_transforms(state: State<'_, AppState>) -> CommandResult<usize> {
    let mut store = state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let n = store.reset_built_ins();
    state.transforms_store.save(&*store)?;
    Ok(n)
}

/// Runs a real transform through the engine with the given input.
#[tauri::command]
pub async fn test_transform(
    state: State<'_, AppState>,
    transform_id: String,
    input: String,
) -> CommandResult<String> {
    let transforms = state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let transform = transforms
        .get(&transform_id)
        .ok_or("Transform not found")?
        .clone();
    drop(transforms);

    let inference = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(provider) = inference.as_deref() else {
        return Err("No model loaded".into());
    };

    let profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let packet = teletype_core::personalization::packet::resolve(&profile, &app_ctx);
    let prompt_ctx = teletype_core::transforms::prompt::PromptContext {
        app: Some(app_ctx),
        preferences: packet.style,
        preferred_terms: packet.terms,
        user_instruction: None,
        language: profile.language.clone(),
    };
    drop(profile);

    let result = teletype_core::transforms::engine::run_transform_blocking(
        provider,
        &transform,
        &input,
        &prompt_ctx,
    );
    Ok(result.text)
}

// ---- Personalization ----

#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>) -> CommandResult<UserProfile> {
    Ok(state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone())
}

#[tauri::command]
pub async fn add_preference(
    state: State<'_, AppState>,
    preference: Preference,
) -> CommandResult<Preference> {
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    profile.add(preference.clone());
    state.profile_store.save(&*profile)?;
    Ok(preference)
}

#[tauri::command]
pub async fn remove_preference(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !profile.remove(&id) {
        return Err("Preference not found".into());
    }
    state.profile_store.save(&*profile)?;
    Ok(())
}

#[tauri::command]
pub async fn clear_learned(state: State<'_, AppState>) -> CommandResult<usize> {
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let n = profile.clear_learned();
    state.profile_store.save(&*profile)?;
    Ok(n)
}

#[tauri::command]
pub async fn set_profile_settings(
    state: State<'_, AppState>,
    learn_from_edits: bool,
    learn_app_specific: bool,
    learn_terminology: bool,
) -> CommandResult<()> {
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    profile.learn_from_edits = learn_from_edits;
    profile.learn_app_specific = learn_app_specific;
    profile.learn_terminology = learn_terminology;
    state.profile_store.save(&*profile)?;
    Ok(())
}

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
}

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> CommandResult<Vec<ModelStatus>> {
    let settings = state.settings();
    let models: Vec<ModelStatus> = teletype_inference::catalog::CATALOG
        .iter()
        .map(|e| {
            let path = state.models_dir.join(format!("{}.gguf", e.id));
            ModelStatus {
                id: e.id.into(),
                name: e.name.into(),
                size_mb: e.size_mb,
                description: e.description.into(),
                downloaded: path.exists(),
                selected: settings.selected_llm_model == e.id,
            }
        })
        .collect();
    Ok(models)
}

#[tauri::command]
pub async fn select_model(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut settings = state.settings();
    settings.selected_llm_model = id.clone();
    state.replace_settings(settings)?;

    // Try to load the model.
    let path = state.models_dir.join(format!("{id}.gguf"));
    if path.exists() {
        match teletype_inference::LlamaProvider::new(&id, &id, &path).warm_up() {
            Ok(()) => {
                let mut inference = state
                    .inference
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *inference = Some(Box::new(teletype_inference::LlamaProvider::new(
                    &id, &id, &path,
                )));
                tracing::info!(model = %id, "model loaded");
            }
            Err(e) => {
                tracing::warn!(model = %id, error = %e, "model warm-up failed");
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn download_model(state: State<'_, AppState>, id: String) -> CommandResult<String> {
    let entry =
        teletype_inference::catalog::find(&id).ok_or_else(|| format!("Unknown model '{id}'"))?;
    let dest = state.models_dir.join(format!("{id}.gguf"));
    if dest.exists() {
        return Ok(dest.to_string_lossy().to_string());
    }

    // Download in a blocking thread.
    let dest_clone = dest.clone();
    let url = entry.url.to_string();
    let handle = std::thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        let response = client
            .get(&url)
            .header("User-Agent", "teletype/0.1")
            .send()
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        let mut file = std::fs::File::create(&dest_clone).map_err(|e| e.to_string())?;
        use std::io::Write;
        let bytes = response.bytes().map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    });
    handle
        .join()
        .map_err(|_| "Download thread panicked".to_string())??;
    Ok(dest.to_string_lossy().to_string())
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
            }
        })
        .collect();
    Ok(models)
}

#[tauri::command]
pub async fn select_speech_model(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    if teletype_speech::catalog::find(&id).is_none() {
        return Err(format!("Unknown speech model '{id}'"));
    }
    let mut settings = state.settings();
    settings.selected_speech_model = id;
    state.replace_settings(settings)
}

#[tauri::command]
pub async fn download_speech_model(
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<String> {
    let entry = teletype_speech::catalog::find(&id)
        .ok_or_else(|| format!("Unknown speech model '{id}'"))?;
    let dest = state.models_dir.join(entry.file);
    if dest.exists() {
        return Ok(dest.to_string_lossy().to_string());
    }

    // Download in a blocking thread to a temp file, then rename into place so
    // a half-finished download is never mistaken for an installed model.
    let dest_clone = dest.clone();
    let url = entry.url();
    let handle = std::thread::spawn(move || {
        let tmp = dest_clone.with_extension("bin.part");
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .get(&url)
            .header("User-Agent", "teletype/0.1")
            .send()
            .map_err(|e| e.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        let mut file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        use std::io::Write;
        let bytes = response.bytes().map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&tmp, &dest_clone).map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    });
    handle
        .join()
        .map_err(|_| "Download thread panicked".to_string())??;
    Ok(dest.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn get_model_status(state: State<'_, AppState>) -> CommandResult<String> {
    let inference = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match inference.as_ref() {
        Some(p) => Ok(format!("{} ({})", p.model_name(), p.model_id())),
        None => Ok("No model loaded".into()),
    }
}

// ---- Misc ----

#[tauri::command]
pub async fn open_main_window(app: AppHandle, route: Option<String>) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(route) = route {
            let _ = window.emit("navigate", route);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn quit_app(app: AppHandle) -> CommandResult<()> {
    app.exit(0);
    Ok(())
}
