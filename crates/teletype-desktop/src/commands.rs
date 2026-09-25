//! Tauri commands: the IPC surface the React UI calls.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use teletype_core::{
    autotext::{self, AutoTextEntry},
    dictionary::DictionaryWord,
    insights,
    llm::InferenceProvider,
    personalization::{Preference, UserProfile},
    platform::{Permission, PermissionKind},
    scratchpad::ScratchEntry,
    style::StyleProfile,
    transforms::TransformDefinition,
};

use crate::AppState;

type CommandResult<T> = Result<T, String>;

/// Derives the keychain account id from a base URL (P0-9).
/// `https://openrouter.ai/api/v1` -> `openrouter.ai`
/// `http://127.0.0.1:11434/v1` -> `127.0.0.1`
fn secret_account_for_url(base_url: &str) -> String {
    let host = base_url
        .strip_prefix("https://")
        .or_else(|| base_url.strip_prefix("http://"))
        .unwrap_or(base_url)
        .split('/')
        .next()
        .unwrap_or("unknown");
    // Strip port if present.
    host.split(':').next().unwrap_or(host).to_string()
}

/// Reads the API key for the given base URL, falling back to the legacy
/// "openai" account so existing users keep working (P0-9).
fn read_api_key(base_url: &str) -> Option<String> {
    let host = secret_account_for_url(base_url);
    if host != "openai" {
        if let Some(key) = crate::secrets::get_secret(&host).ok().flatten() {
            return Some(key);
        }
    }
    crate::secrets::get_secret("openai").ok().flatten()
}

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

// ---- Developer tab ----

/// Recent app log entries (oldest first), for the Developer tab.
#[tauri::command]
pub fn get_logs() -> Vec<crate::LogEntry> {
    crate::logs_snapshot()
}

/// Clears the in-memory log buffer.
#[tauri::command]
pub fn clear_logs() {
    crate::clear_logs();
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
    /// Active LLM provider: "local-server" | "openai-compat" | "" (none).
    #[serde(default)]
    pub selected_llm_provider: String,
    /// Base URL for the OpenAI-compatible connector (includes `/v1`).
    #[serde(default = "default_openai_base_url")]
    pub openai_base_url: String,
    /// Remote model id for the OpenAI-compatible connector.
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    /// Show the Developer tab (live app logs) in the sidebar.
    #[serde(default)]
    pub enable_developer_tab: bool,
    /// Recording pill style: "default" | "classic" | "levelRail" | "well".
    #[serde(default = "default_pill_style")]
    pub pill_style: String,
}

fn default_openai_base_url() -> String {
    "https://api.openai.com/v1".into()
}

fn default_openai_model() -> String {
    "gpt-4o-mini".into()
}

/// The default app icon is the white-background mark.
fn default_app_icon() -> String {
    "white".into()
}

/// The default recording pill style is the original Teletype design.
fn default_pill_style() -> String {
    "default".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: String::new(),
            recording_mode: "hold".into(),
            selected_speech_model: "parakeet-tdt-v3".into(),
            language: "auto".into(),
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
            selected_llm_provider: String::new(),
            openai_base_url: default_openai_base_url(),
            openai_model: default_openai_model(),
            enable_developer_tab: false,
            pill_style: default_pill_style(),
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
        return Err(format!("Unknown app icon '{id}'"));
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

/// Cancels the current dictation from the pill. While recording, the audio is
/// discarded; mid-pipeline, the in-flight transcription is dropped and the
/// result will not be inserted.
#[tauri::command]
pub async fn cancel_dictation(state: State<'_, AppState>) -> CommandResult<()> {
    state.controller.send(crate::dictation::Event::PillCancel);
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
    // P0-12: energy-based silence stop instead of a fixed sleep. Stop after
    // 800 ms of continuous silence below the threshold, capped at 5 s.
    let silence = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let silence_since = std::sync::Arc::new(std::sync::Mutex::new(None::<std::time::Instant>));
    let s_silence = silence.clone();
    let s_since = silence_since.clone();
    let recording = teletype_core::audio::Recording::start(&input_device, move |level| {
        const THRESHOLD: f32 = 0.01;
        const SILENCE_MS: u64 = 800;
        let now = std::time::Instant::now();
        if level < THRESHOLD {
            let mut since = s_since.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match *since {
                None => *since = Some(now),
                Some(t) if now.duration_since(t).as_millis() >= SILENCE_MS as u128 => {
                    s_silence.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                _ => {}
            }
        } else {
            *s_since.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    })
    .map_err(|e| format!("Microphone unavailable: {e}"))?;

    // Wait for silence detection or the 5 s cap.
    let cap = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !silence.load(std::sync::atomic::Ordering::Relaxed) {
        if std::time::Instant::now() >= cap {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
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
        s1_control: profile.s1_control,
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

/// Sets the S1-mini control axes. Values are the wire strings from the model
/// card (e.g. "semi-formal"); an unknown value falls back to that axis's
/// default so a stale UI can never send an off-set control token.
#[tauri::command]
pub async fn set_s1_control(
    state: State<'_, AppState>,
    styling: String,
    structure: String,
    context: String,
) -> CommandResult<()> {
    use teletype_core::transforms::prompt::{S1Context, S1Control, S1Styling, S1Structure};
    fn parse_styling(s: &str) -> S1Styling {
        match s {
            "casual" => S1Styling::Casual,
            "semi-casual" => S1Styling::SemiCasual,
            "formal" => S1Styling::Formal,
            _ => S1Styling::SemiFormal,
        }
    }
    fn parse_structure(s: &str) -> S1Structure {
        if s == "prose" {
            S1Structure::Prose
        } else {
            S1Structure::Lists
        }
    }
    fn parse_context(s: &str) -> S1Context {
        if s == "email" {
            S1Context::Email
        } else {
            S1Context::General
        }
    }
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    profile.s1_control = S1Control {
        styling: parse_styling(&styling),
        structure: parse_structure(&structure),
        context: parse_context(&context),
    };
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
    let models: Vec<ModelStatus> = teletype_inference::catalog::CATALOG
        .iter()
        .map(|e| ModelStatus {
            id: e.id.into(),
            name: e.name.into(),
            size_mb: e.size_mb,
            description: e.description.into(),
            downloaded: e.is_downloaded(&state.models_dir),
            selected: settings.selected_llm_model == e.id,
            license_name: e.license_name.map(str::to_string),
            license_url: e.license_url.map(str::to_string),
            requires_license_accept: e.requires_license_accept,
            attribution: e.attribution.map(str::to_string),
        })
        .collect();
    Ok(models)
}

#[tauri::command]
pub async fn select_model(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let entry = teletype_inference::catalog::find(&id)
        .ok_or_else(|| format!("Unknown model '{id}'"))?;
    // License acceptance gates download only; once installed the model is usable.
    if !entry.is_downloaded(&state.models_dir) {
        return Err(format!("Model '{id}' is not downloaded yet."));
    }

    let path = entry.entrypoint(&state.models_dir);
    if !path.exists() {
        return Err(format!("Model file for '{id}' is missing. Re-download it."));
    }

    // warm_up uses reqwest::blocking and must not run on a tokio worker
    // (dropping the client there panics with "Cannot drop a runtime…").
    let spawn_id = id.clone();
    let spawn_name = entry.name.to_string();
    let spawn_path = path.clone();
    crate::log_entry(crate::LogLevel::Info, format!("loading LLM model: {spawn_id} ({spawn_name})"));
    let started = std::time::Instant::now();
    let provider = tauri::async_runtime::spawn_blocking(move || {
        let provider =
            teletype_inference::ServerProvider::new(&spawn_id, spawn_name, &spawn_path);
        provider.warm_up()?;
        Ok::<_, String>(provider)
    })
    .await
    .map_err(|e| format!("warm-up task: {e}"))??;
    crate::log_entry_ms(
        crate::LogLevel::Success,
        format!("LLM model loaded: {} ({})", provider.model_name(), provider.model_id()),
        started.elapsed().as_millis() as u64,
    );

    let old = {
        let mut inference = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Dropping any previous provider kills its child process.
        inference.replace(Box::new(provider))
    };
    // Drop the old provider off the async runtime too (Stop/drop can block).
    if let Some(old) = old {
        let _ = tauri::async_runtime::spawn_blocking(move || drop(old)).await;
    }

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
        return Err(format!(
            "Model '{id}' is under a restrictive license. Read and accept it first."
        ));
    }

    let models_dir = state.models_dir.clone();
    let event_id = id.clone();
    let cb: teletype_inference::ProgressFn = std::sync::Arc::new(move |mut p| {
        // Tag progress with kind so the UI can route speech vs LLM rows.
        p.id = event_id.clone();
        let mut payload = serde_json::to_value(&p).unwrap_or_default();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("kind".into(), serde_json::Value::String("llm".into()));
        }
        let _ = app.emit("model-download-progress", payload);
    });

    let handle = tauri::async_runtime::spawn_blocking(move || {
        teletype_inference::download_entry_with_progress(entry, &models_dir, Some(cb))
    })
    .await
    .map_err(|e| format!("download task: {e}"))??;
    Ok(handle.to_string_lossy().to_string())
}

// ---- LLM secrets + OpenAI-compatible connector ----

/// Stores an API key in the OS keychain. Never written to settings.json.
#[tauri::command]
pub async fn set_llm_secret(provider_id: String, secret: String) -> CommandResult<()> {
    crate::secrets::set_secret(&provider_id, &secret)
}

/// Whether a key is stored (does not return the key itself).
#[tauri::command]
pub async fn has_llm_secret(provider_id: String) -> CommandResult<bool> {
    Ok(crate::secrets::has_secret(&provider_id))
}

/// Removes a stored API key.
#[tauri::command]
pub async fn clear_llm_secret(provider_id: String) -> CommandResult<()> {
    crate::secrets::clear_secret(&provider_id)
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

    let old = {
        let mut inference = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inference.replace(Box::new(provider))
    };
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
    tauri::async_runtime::spawn_blocking(move || {
        provider.test_connection(std::time::Duration::from_secs(10))
    })
    .await
    .map_err(|e| format!("test task: {e}"))?
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
            // Follow EnviousWispr: a local/downloaded polish model is NOT
            // loaded at launch. It loads lazily on the first dictation that
            // needs a transform (see `ensure_local_provider`), so the app starts
            // fast and doesn't hold a ~1-3 GB model in RAM when not dictating.
            crate::log_entry(crate::LogLevel::Info, "LLM: local model will load on first use");
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
            let old = state
                .inference
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(Box::new(provider));
            drop(old);
            crate::log_entry(crate::LogLevel::Success, format!("LLM restored: API ({model})"));
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

/// Lazily loads the selected local polish model on first use, following
/// EnviousWispr's behavior (local models are not preloaded at launch). Called
/// from the dictation path just before a transform runs, so the first
/// dictation pays the model-load cost once and later ones reuse it. No-op when
/// a provider is already loaded, a remote provider is active, or nothing is
/// selected. Blocking: it spawns a local server subprocess and warm-ups it, so
/// callers must already be off the tokio runtime (the dictation worker thread).
pub fn ensure_local_provider(app: &AppHandle) {
    let state = app.state::<AppState>();
    // Already have a provider (e.g. a remote one installed at launch, or a
    // local one from an earlier dictation): nothing to do.
    {
        let inference = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inference.is_some() {
            return;
        }
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
    let provider = teletype_inference::ServerProvider::new(entry.id, entry.name, &path);
    crate::log_entry(
        crate::LogLevel::Info,
        format!("loading LLM model on first use: {} ({})", entry.name, entry.id),
    );
    let started = std::time::Instant::now();
    if let Err(e) = provider.warm_up() {
        crate::log_entry(
            crate::LogLevel::Error,
            format!("LLM first-use load failed for '{model_id}': {e}"),
        );
        return;
    }
    let old = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace(Box::new(provider));
    drop(old); // dropping kills any previous child process
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
                languages: m.languages.map(|l| l.iter().map(|s| s.to_string()).collect()),
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
    // Id 0 is "en" in whisper.cpp's table but `whisper_lang_str(0)` returns
    // null, so seed it explicitly and enumerate the rest.
    let mut langs = vec![SpeechLanguage {
        code: "en".to_string(),
        name: "English".to_string(),
    }];
    for id in 1..=whisper_rs::get_lang_max_id() {
        if let (Some(code), Some(name)) = (
            whisper_rs::get_lang_str(id),
            whisper_rs::get_lang_str_full(id),
        ) {
            langs.push(SpeechLanguage {
                code: code.to_string(),
                name: name.to_string(),
            });
        }
    }
    Ok(langs)
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

            let mut file =
                std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
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

            emit(
                "verifying",
                written,
                total.max(written),
                0.0,
                Some(0.0),
                None,
            );
            std::fs::rename(&tmp, &dest_clone).map_err(|e| e.to_string())?;
            emit("done", total.max(written), total.max(written), 0.0, Some(0.0), None);
            Ok(())
        };

        match run() {
            Ok(()) => Ok(()),
            Err(e) => {
                emit("error", 0, expected_total, 0.0, None, Some(e.clone()));
                Err(e)
            }
        }
    });
    handle
        .await
        .map_err(|e| format!("download task: {e}"))??;
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
        None => {
            // Distinguish "nothing configured" from "configured but not
            // loaded", so an empty slot is actionable. Kept short: the UI
            // embeds this inside "Local (...)".
            let settings = state.settings();
            let configured = match settings.selected_llm_provider.as_str() {
                "local-server" if !settings.selected_llm_model.is_empty() => format!(
                    "ready (loads '{}' on first use)",
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
    let mut insights = insights::compute(&filtered_history, now);
    // P1-16: surface the polish/transform state so the user knows why
    // nothing was rewritten.
    let inference = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if inference.is_none() {
        insights.polish_status = Some(
            "No polish model loaded. Dictations are not being rewritten. \
             Pick a model in Settings > Models."
                .into(),
        );
    }
    Ok(insights)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The pure language-table walk behind `list_speech_languages`.
    fn speech_languages() -> Vec<SpeechLanguage> {
        // Id 0 is "en" (english) in whisper.cpp's table; ids 1..=max are the
        // rest. `whisper_lang_str(0)` returns null, so enumerate 0..=max and
        // fall back to the known code/name for id 0.
        let max_id = whisper_rs::get_lang_max_id();
        let mut langs = Vec::new();
        for id in 0..=max_id {
            let (code, name) = if id == 0 {
                ("en", "English")
            } else {
                match (whisper_rs::get_lang_str(id), whisper_rs::get_lang_str_full(id)) {
                    (Some(code), Some(name)) => (code, name),
                    _ => continue,
                }
            };
            langs.push(SpeechLanguage {
                code: code.to_string(),
                name: name.to_string(),
            });
        }
        langs
    }

    #[test]
    fn list_speech_languages_returns_whisper_table() {
        // The whisper.cpp language table has 100 languages (ids 0..=99).
        let langs = speech_languages();
        assert_eq!(langs.len(), 100, "expected 100 Whisper languages");
        // Spot-check a few well-known codes.
        let codes: Vec<&str> = langs.iter().map(|l| l.code.as_str()).collect();
        for expected in ["en", "zh", "de", "es", "ru", "ja", "fr"] {
            assert!(codes.contains(&expected), "missing {expected}");
        }
        // Every entry has a non-empty display name.
        assert!(langs.iter().all(|l| !l.name.is_empty()));
    }
}
