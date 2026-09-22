//! Tauri commands: the IPC surface the React UI calls.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use teletype_core::{
    autotext::{self, AutoTextEntry},
    dictionary::DictionaryWord,
    insights,
    personalization::{Preference, UserProfile},
    platform::{Permission, PermissionKind},
    scratchpad::ScratchEntry,
    style::StyleProfile,
    transforms::TransformDefinition,
};

use crate::AppState;

type CommandResult<T> = Result<T, String>;

// ---- Native Hotkey Capture (macOS) ----

/// Opens the native macOS hotkey capture panel. The user presses a key
/// combination, Enter confirms (writes the result to a temp file), Esc cancels.
/// Returns immediately; the UI should call `get_captured_hotkey` after a delay.
#[tauri::command]
pub fn start_hotkey_capture() -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn teletype_start_hotkey_capture();
        }
        // SAFETY: the C function creates an NSPanel on the main thread.
        // Tauri commands run on the main thread by default.
        std::fs::remove_file("/tmp/teletype_hotkey_result.txt").ok();
        // SAFETY: C function only creates an NSPanel on the main thread,
        // where Tauri commands run.
        unsafe { teletype_start_hotkey_capture() };
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Hotkey capture is only available on macOS".into())
    }
}

/// Closes the native hotkey capture panel.
#[tauri::command]
pub fn stop_hotkey_capture() -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn teletype_stop_hotkey_capture();
        }
        // SAFETY: C function only touches the NSPanel created above.
        unsafe { teletype_stop_hotkey_capture() };
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

/// Returns the hotkey captured by the native panel, if the user confirmed.
/// Returns `Ok(None)` if the user cancelled or hasn't confirmed yet.
#[tauri::command]
pub fn get_captured_hotkey() -> CommandResult<Option<String>> {
    let path = std::path::Path::new("/tmp/teletype_hotkey_result.txt");
    match std::fs::read_to_string(path) {
        Ok(h) if !h.trim().is_empty() => {
            let hotkey = h.trim().to_string();
            std::fs::remove_file(path).ok();
            Ok(Some(hotkey))
        }
        _ => Ok(None),
    }
}

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
    pub remove_filler_words: bool,
    pub filler_words: Vec<String>,
    /// Where the floating pill sits: a 3×3 grid, e.g. "bottomCenter".
    pub pill_position: String,
    /// Show the pill's idle bars even when not dictating.
    pub always_show_pill: bool,
    /// Id of the active style profile ("" = none).
    pub active_style_profile: String,
    /// When true and the scratchpad window is frontmost, dictation is
    /// appended to the scratchpad instead of being injected.
    pub scratchpad_enabled: bool,
    /// Selected app icon id: "white" (default) or "blue".
    #[serde(default = "default_app_icon")]
    pub app_icon: String,
    /// Folder where day-wise transcript files are written. Empty = default
    /// (a `transcripts` subfolder in the app config dir).
    #[serde(default)]
    pub transcripts_dir: String,
}

/// The default app icon is the white-background mark.
fn default_app_icon() -> String {
    "white".into()
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
            remove_filler_words: true,
            filler_words: default_filler_words(),
            pill_position: "bottomCenter".into(),
            always_show_pill: false,
            active_style_profile: String::new(),
            scratchpad_enabled: false,
            app_icon: default_app_icon(),
            transcripts_dir: String::new(),
        }
    }
}

/// The default filler words removed from transcripts when
/// `remove_filler_words` is enabled.
fn default_filler_words() -> Vec<String> {
    [
        "um", "uh", "er", "ah", "eh", "umm", "uhh", "err", "ahh", "ehh", "hmm", "hm", "mm", "mmm",
        "erm", "urm", "ugh",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
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
        // Unregister the old hotkey first so the new one can take its place.
        if !previous.hotkey.is_empty() {
            state.controller.unregister_hotkey(&app, &previous.hotkey);
        }
        if let Err(e) = state.controller.register_hotkey(&app, &settings.hotkey) {
            // Revert.
            if !previous.hotkey.is_empty() {
                let _ = state.controller.register_hotkey(&app, &previous.hotkey);
            }
            return Err(e);
        }
    }
    state.replace_settings(settings.clone())?;

    // If the app icon changed, apply it to the window and tray.
    if settings.app_icon != previous.app_icon {
        apply_app_icon(&app, &settings.app_icon);
    }
    Ok(settings)
}

/// The two bundled app icons, keyed by the id stored in settings.
pub const APP_ICONS: &[(&str, &str, &[u8])] = &[
    ("white", "Teletype (light)", include_bytes!("../icons/app-icon-white.png")),
    ("blue", "Teletype (blue)", include_bytes!("../icons/app-icon-blue.png")),
];

/// Resolves a settings icon id to its bundled PNG bytes.
fn app_icon_bytes(id: &str) -> Option<&'static [u8]> {
    APP_ICONS.iter().find(|(k, _, _)| *k == id).map(|(_, _, b)| *b)
}

/// Sets the selected app icon, persists it, and applies it to the window
/// and tray immediately.
#[tauri::command]
pub async fn set_app_icon(app: AppHandle, state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut settings = state.settings();
    if !APP_ICONS.iter().any(|(k, _, _)| *k == id) {
        return Err(format!("Unknown app icon '{id}'").into());
    }
    settings.app_icon = id.clone();
    state.replace_settings(settings)?;
    apply_app_icon(&app, &id);
    Ok(())
}

/// Applies the selected icon to the main window and the tray.
pub fn apply_app_icon(app: &AppHandle, id: &str) {
    let Some(bytes) = app_icon_bytes(id) else {
        return;
    };
    let Ok(icon) = tauri::image::Image::from_bytes(bytes) else {
        return;
    };
    // Window icon (taskbar / window control).
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_icon(icon.clone());
    }
    // Tray / menu bar icon.
    if let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) {
        let _ = tray.set_icon(Some(icon));
    }
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

/// Records a short audio clip and transcribes it, returning the raw text.
/// Used by the "Teach Words" flow to capture how the user pronounces a word.
/// The recording runs on a background thread; this command blocks until done.
#[tauri::command]
pub async fn transcribe_word(app: AppHandle, state: State<'_, AppState>) -> CommandResult<String> {
    let settings = state.settings();
    let input_device = settings.input_device.clone();
    let language = crate::dictation::effective_language(settings.language.clone(), true);
    let (model_path, use_parakeet) = {
        let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
        let use_parakeet = entry
            .map(|m| m.engine == teletype_speech::catalog::Engine::Parakeet)
            .unwrap_or(false);
        let file = entry
            .map(|m| m.file.to_string())
            .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
        (state.models_dir.join(file), use_parakeet)
    };

    // Start the recording on the calling thread (it spawns its own capture thread).
    let recording = teletype_core::audio::Recording::start(&input_device, |_| {})
        .map_err(|e| format!("Microphone unavailable: {e}"))?;

    // Let the user speak for up to 5 seconds, then stop.
    std::thread::sleep(std::time::Duration::from_secs(3));
    let captured = recording
        .finish()
        .map_err(|e| format!("Recording failed: {e}"))?;

    // Transcribe on a worker thread to avoid blocking the command handler.
    let app_clone = app.clone();
    let model_path_clone = model_path.clone();
    let language_clone = language.clone();
    let result = std::thread::spawn(move || {
        crate::dictation::transcribe(&app_clone, &model_path_clone, &captured, &language_clone, use_parakeet)
    })
    .join()
    .map_err(|_| "Transcription thread panicked".to_string())??;

    Ok(result)
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

/// The built-in System AutoText entries (read-only; not user-editable).
#[tauri::command]
pub async fn list_system_autotext() -> CommandResult<Vec<AutoTextEntry>> {
    Ok(teletype_core::autotext::system::entries().to_vec())
}

#[tauri::command]
pub async fn create_autotext(
    state: State<'_, AppState>,
    entry: AutoTextEntry,
) -> CommandResult<AutoTextEntry> {
    autotext::validate_trigger(&entry.trigger).map_err(|e| e.to_string())?;
    autotext::validate_snippet(&entry.snippet)?;
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
    autotext::validate_snippet(&entry.snippet)?;
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
    pub speed: f64,
    pub accuracy: f64,
    pub min_ram_gb: u32,
    pub language_label: String,
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
                speed: m.speed,
                accuracy: m.accuracy,
                min_ram_gb: m.min_ram_gb,
                language_label: m.language_label(),
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

// ---- Dictation history ----

/// A history entry as returned to the UI (camelCase).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub created_at: u64,
    pub text: String,
    pub app_name: String,
    pub app_type: String,
}

fn entry_to_view(e: &teletype_core::history::DictationEntry) -> HistoryEntry {
    let ctx = e.context.clone().unwrap_or_default();
    HistoryEntry {
        id: e.id.clone(),
        created_at: e.created_at,
        text: e.text.clone(),
        app_name: ctx.application_name.clone(),
        app_type: format!("{:?}", ctx.application_type),
    }
}

#[tauri::command]
pub async fn list_dictation_history(
    state: State<'_, AppState>,
) -> CommandResult<Vec<HistoryEntry>> {
    let history = state
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(history.entries.iter().map(entry_to_view).collect())
}

#[tauri::command]
pub async fn delete_dictation_entry(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    {
        let mut history = state
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        history.remove(&id);
    }
    let history = state
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.history_store.save(&history)?;
    Ok(())
}

// ---- Misc ----

/// The folder day-wise transcript files are written to (resolved).
#[tauri::command]
pub async fn get_transcripts_dir(state: State<'_, AppState>) -> CommandResult<String> {
    let dir = state.transcripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.to_string_lossy().to_string())
}

/// Reveals the transcripts folder in the system file manager (Finder / Explorer).
#[tauri::command]
pub async fn reveal_transcripts_dir(app: AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    let dir = state.transcripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(dir.to_string_lossy().to_string()).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(dir.to_string_lossy().to_string()).spawn();
    }
    let _ = app;
    Ok(())
}

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

#[tauri::command]
pub async fn get_username() -> CommandResult<String> {
    Ok(std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "there".into()))
}

// ---- Insights ----

#[tauri::command]
pub async fn get_insights(
    state: State<'_, AppState>,
    range: Option<String>,
) -> CommandResult<insights::Insights> {
    let now = teletype_core::storage::now_ms();
    let history = state
        .history
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let filtered: Vec<teletype_core::history::DictationEntry> = match range.as_deref() {
        Some("week") => history
            .entries
            .iter()
            .filter(|r| r.created_at >= now.saturating_sub(7 * 86_400_000))
            .cloned()
            .collect(),
        Some("month") => history
            .entries
            .iter()
            .filter(|r| r.created_at >= now.saturating_sub(30 * 86_400_000))
            .cloned()
            .collect(),
        Some("year") => history
            .entries
            .iter()
            .filter(|r| r.created_at >= now.saturating_sub(365 * 86_400_000))
            .cloned()
            .collect(),
        _ => history.entries.clone(),
    };
    let filtered_history = teletype_core::history::DictationHistory { entries: filtered };
    Ok(insights::compute(&filtered_history, now))
}

// ---- Usage stats (filler words removed, AutoText used) ----

#[tauri::command]
pub async fn get_usage_stats(state: State<'_, AppState>) -> CommandResult<teletype_core::usage::UsageStats> {
    let usage = state
        .usage
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let autotext = state
        .autotext
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let settings = state.settings();
    let mut usage = usage.clone();
    // Only show counters for things that still exist / are active.
    usage.prune_autotext(&autotext);
    if settings.remove_filler_words {
        usage.prune_fillers(&settings.filler_words);
    } else {
        usage.filler_counts.clear();
    }
    Ok(usage)
}

#[tauri::command]
pub async fn reset_usage_stats(state: State<'_, AppState>) -> CommandResult<()> {
    let mut usage = state
        .usage
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    *usage = teletype_core::usage::UsageStats::default();
    state.usage_store.save(&usage)?;
    Ok(())
}

// ---- Dictionary ----

#[tauri::command]
pub async fn list_dictionary(state: State<'_, AppState>) -> CommandResult<Vec<DictionaryWord>> {
    Ok(state
        .dictionary
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .words
        .clone())
}

#[tauri::command]
pub async fn add_dictionary_word(
    state: State<'_, AppState>,
    word: DictionaryWord,
) -> CommandResult<DictionaryWord> {
    if word.word.trim().is_empty() {
        return Err("Word can't be empty".into());
    }
    let mut dict = state
        .dictionary
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    dict.insert(word.clone())?;
    state.dictionary_store.save(&*dict)?;
    Ok(word)
}

#[tauri::command]
pub async fn remove_dictionary_word(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut dict = state
        .dictionary
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !dict.remove(&id) {
        return Err("Word not found".into());
    }
    state.dictionary_store.save(&*dict)?;
    Ok(())
}

// ---- Style profiles ----

#[tauri::command]
pub async fn list_style_profiles(state: State<'_, AppState>) -> CommandResult<Vec<StyleProfile>> {
    Ok(state
        .styles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .profiles
        .clone())
}

#[tauri::command]
pub async fn create_style_profile(
    state: State<'_, AppState>,
    profile: StyleProfile,
) -> CommandResult<StyleProfile> {
    let mut store = state
        .styles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.insert(profile.clone())?;
    state.styles_store.save(&*store)?;
    Ok(profile)
}

#[tauri::command]
pub async fn update_style_profile(
    state: State<'_, AppState>,
    profile: StyleProfile,
) -> CommandResult<StyleProfile> {
    let mut store = state
        .styles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.update(profile.clone())?;
    state.styles_store.save(&*store)?;
    Ok(profile)
}

#[tauri::command]
pub async fn delete_style_profile(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut store = state
        .styles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.remove(&id)?;
    // If the deleted profile was active, clear the selection.
    if settings_active_style_is(&state, &id) {
        drop(store);
        let mut settings = state.settings();
        settings.active_style_profile = String::new();
        state.replace_settings(settings)?;
    } else {
        state.styles_store.save(&*store)?;
    }
    Ok(())
}

fn settings_active_style_is(state: &AppState, id: &str) -> bool {
    state.settings().active_style_profile == id
}

#[tauri::command]
pub async fn set_active_style_profile(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    // "" means none; any other id must exist.
    if !id.is_empty() {
        let store = state
            .styles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if store.get(&id).is_none() {
            return Err("Profile not found".into());
        }
    }
    let mut settings = state.settings();
    settings.active_style_profile = id;
    state.replace_settings(settings)
}

#[tauri::command]
pub async fn reset_style_profiles(state: State<'_, AppState>) -> CommandResult<usize> {
    let mut store = state
        .styles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let n = store.reset_built_ins();
    state.styles_store.save(&*store)?;
    Ok(n)
}

// ---- Scratchpad ----

#[tauri::command]
pub async fn list_scratchpad(state: State<'_, AppState>) -> CommandResult<Vec<ScratchEntry>> {
    Ok(state
        .scratchpad
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entries
        .clone())
}

#[tauri::command]
pub async fn append_scratchpad(state: State<'_, AppState>, text: String) -> CommandResult<()> {
    let mut pad = state
        .scratchpad
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pad.append(text);
    state.scratchpad_store.save(&*pad)?;
    Ok(())
}

#[tauri::command]
pub async fn delete_scratchpad_entry(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut pad = state
        .scratchpad
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pad.remove(&id);
    state.scratchpad_store.save(&*pad)?;
    Ok(())
}

#[tauri::command]
pub async fn clear_scratchpad(state: State<'_, AppState>) -> CommandResult<()> {
    let mut pad = state
        .scratchpad
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    pad.clear();
    state.scratchpad_store.save(&*pad)?;
    Ok(())
}

#[tauri::command]
pub async fn get_scratchpad_text(state: State<'_, AppState>) -> CommandResult<String> {
    let pad = state
        .scratchpad
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(pad.combined())
}
