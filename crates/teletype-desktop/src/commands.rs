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

/// Hosts that are allowed to fall back to the legacy `openai` keychain
/// account (see [`read_api_key`]).
const OPENAI_HOSTS: &[&str] = &[
    "api.openai.com",
    "openai.com",
    "www.openai.com",
    "eu.api.openai.com",
];

/// Derives the keychain account id from a base URL (P0-9).
/// `https://openrouter.ai/api/v1` -> `openrouter.ai`
/// `http://127.0.0.1:11434/v1` -> `127.0.0.1`
///
/// Parsed with `url::Url` rather than by splitting on `:` and `/`. The manual
/// version read the *username* of a URL with userinfo as the host, so
/// `https://user:pass@evil.example/v1` resolved to the account `user`, found
/// no key for it, and fell through to the OpenAI key, which was then sent to
/// `evil.example`.
///
/// The port is deliberately not part of the account, so existing stored keys
/// keep resolving. Two local services on different ports therefore share one
/// key slot; that is a loopback-only exposure and changing it would silently
/// orphan every stored key.
fn secret_account_for_url(base_url: &str) -> String {
    match url::Url::parse(base_url) {
        Ok(u) => u
            .host_str()
            .map(|h| h.to_ascii_lowercase())
            .unwrap_or_else(|| "unknown".to_string()),
        Err(_) => "unknown".to_string(),
    }
}

/// True when `base_url` actually points at OpenAI.
fn is_openai_base_url(base_url: &str) -> bool {
    let host = secret_account_for_url(base_url);
    host == "openai" || OPENAI_HOSTS.contains(&host.as_str())
}

/// Reads the API key for the given base URL.
///
/// A pre-P0-9 install stored its OpenAI key under the account `openai`, so
/// that account is still honoured -- but **only when the configured base URL
/// really is OpenAI**. The fallback used to be unconditional, which sent the
/// user's OpenAI key to whatever third-party host they had pointed the
/// connector at (Groq, OpenRouter, an arbitrary pasted URL), on every app
/// launch, every connection test and every provider probe.
fn read_api_key(base_url: &str) -> Option<String> {
    let host = secret_account_for_url(base_url);
    if let Some(key) = crate::secrets::get_secret(&host).ok().flatten() {
        return Some(key);
    }
    if is_openai_base_url(base_url) {
        return crate::secrets::get_secret("openai").ok().flatten();
    }
    None
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

/// The current hotkey registration conflict, if the saved binding is the one
/// that failed. A registration failure used to surface only as a save-time
/// error or a log line at startup, so a shortcut taken by something else left
/// dictation silently dead with the change looking saved. `None` when the
/// binding works or when the stored failure belongs to an older binding.
#[tauri::command]
pub async fn hotkey_conflict(
    state: State<'_, AppState>,
) -> CommandResult<Option<crate::dictation::HotkeyConflict>> {
    let stored = state.controller.conflict_snapshot();
    Ok(crate::dictation::visible_conflict(
        &stored,
        &state.settings().hotkey,
    ))
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
    /// Leave the dictated text on the clipboard as an extra item, so it is
    /// added to the system clipboard history instead of being lost with the
    /// paste. Independent of `restore_clipboard`: the user's own copy is put
    /// back either way, and nothing is ever destroyed.
    #[serde(default = "default_true")]
    pub keep_text_on_clipboard: bool,
    pub auto_apply_transform: bool,
    pub show_tray_icon: bool,
    pub has_completed_onboarding: bool,
    pub selected_llm_model: String,
    /// Typed AutoText: expand a stored trigger (e.g. `/email`) as you type it
    /// followed by space/enter, in any app. On by default so the feature works
    /// out of the box; requires Accessibility permission on macOS.
    #[serde(default = "default_true")]
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
    /// Recording pill style: "default" | "classic" | "levelRail" | "well" | "dotGrid".
    #[serde(default = "default_pill_style")]
    pub pill_style: String,
    /// Stop recording automatically once the user pauses after speaking.
    /// Active only in hands-free hold mode (Hold mode + a double-tap to start),
    /// where the pause is the only stop signal. In plain hold-to-talk the key
    /// release already stops the take, and in push-to-talk (toggle) mode the
    /// second tap does, so both preempt VAD.
    #[serde(default)]
    pub vad_auto_stop: bool,
    /// How long the pause must last before auto-stop triggers, in
    /// milliseconds (the "stop after a pause of N ms" slider).
    #[serde(default = "default_vad_silence_ms")]
    pub vad_silence_ms: u64,
    /// P5.1: when true and a local LLM provider is active, a deterministic
    /// gate may skip the polish pass for short clean utterances. On by
    /// default: it only skips when a local LLM is active and the take is
    /// short and clean, so it is a pure latency win with no quality risk
    /// (roadmap P5.1).
    #[serde(default = "default_true")]
    pub polish_gate_enabled: bool,
    /// P5.1: max word count for the gate's short-clean-skip path.
    #[serde(default = "default_polish_gate_threshold_words")]
    pub polish_gate_threshold_words: usize,
    /// Per-app ASR language overrides (P3.3): normalized app key -> language
    /// code (e.g. "com.google.gmail" -> "en"). An override for the frontmost
    /// app wins over the global `language` setting.
    #[serde(default)]
    pub app_language_overrides: std::collections::BTreeMap<String, String>,
    /// When true, restore curated emoji phrases in voice transcripts after
    /// the transform (P3.2).
    #[serde(default = "default_true_emoji")]
    pub restore_emoji: bool,
    /// When true, convert spoken emoji phrases ("thumbs up emoji") to
    /// glyphs in voice transcripts after the transform (BUG-002, ported
    /// from EW's EmojiFormatterStep).
    #[serde(default = "default_true")]
    pub spoken_emoji: bool,
    /// When true, the System AutoText spoken-punctuation entries ("comma" →
    /// "," etc.) run in the pipeline. Off means those phrases stay as
    /// words; custom AutoText is unaffected (BUG-002).
    #[serde(default = "default_true")]
    pub spoken_punctuation: bool,
    #[serde(default)]
    pub model_unload_delay_secs: u64,
    /// Ids of enabled vocabulary packs ("tech", "medical", "legal", "brands",
    /// "names"). Off by default; each pack is toggled independently.
    #[serde(default)]
    pub enabled_packs: Vec<String>,
    /// UI color theme: "system" (default, follows the OS) | "light" | "dark".
    /// Applies to the main window only; the floating pill stays always-dark.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Reduce Motion: when true, force near-instant animation/transition
    /// durations in the UI. When false, the OS Reduce Motion preference still
    /// applies (P3.19).
    #[serde(default)]
    pub reduce_motion: bool,
    /// Show the live transcript in the recording pill while speaking
    /// (T1.2). On by default; the interim loop costs a re-decode per tick,
    /// so users who only want the animation can switch it off.
    #[serde(default = "default_true")]
    pub live_preview_enabled: bool,
    /// Play a short cue when a dictation starts and when it stops, so the
    /// user knows the hotkey registered without watching the pill.
    #[serde(default = "default_true")]
    pub play_recording_sounds: bool,
    /// Which cue to play: a macOS system sound name from
    /// [`RECORDING_SOUND_NAMES`]. Kept separate from the toggle so the choice
    /// survives the user switching cues off and back on.
    #[serde(default = "default_recording_sound")]
    pub recording_sound: String,
    /// Separate binding that aborts the recording in progress and injects
    /// nothing. Empty means disabled; the default is Escape.
    #[serde(default = "default_cancel_hotkey")]
    pub cancel_hotkey: String,
    /// Make injected text match the surroundings: reuse the leading space
    /// that is already there instead of adding another, and carry the
    /// capitalisation of the character the dictation follows.
    #[serde(default = "default_true")]
    pub smart_insertion: bool,
    /// Put every dictation on the clipboard as well as injecting it, so it
    /// survives a wrong target app and can be pasted elsewhere.
    #[serde(default = "default_true")]
    pub auto_copy_to_clipboard: bool,
    /// What happens to other apps' audio while a dictation is running: one of
    /// [`OTHER_AUDIO_ACTIONS`].
    #[serde(default = "default_other_audio_action")]
    pub other_audio_action: String,
    /// When to load the speech engine ahead of the hotkey press, as a policy
    /// string from `"off"`, `"10"`, `"30"`, `"60"`, `"always"`. Warming trades
    /// battery and memory for a faster first word.
    #[serde(default = "default_warm_engine_policy")]
    pub warm_engine_policy: String,
    /// Dictionary match strictness: one of `loose` | `standard` | `strict`
    /// (`MATCH_STRICTNESS_LEVELS`).
    #[serde(default = "default_match_strictness")]
    pub match_strictness: String,
    /// The word that triggers AutoText expansion, as in "say semicolon new
    /// line". One keyword app-wide, so a snippet cannot be triggered by an
    /// ordinary word that happens to match.
    #[serde(default = "default_autotext_keyword")]
    pub autotext_keyword: String,
}

/// Default word cap for the P5.1 polish gate (short-clean-skip path).
fn default_polish_gate_threshold_words() -> usize {
    8
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

/// The default UI theme follows the OS preference.
fn default_theme() -> String {
    "system".into()
}

/// The default recording pill style is the original Teletype design.
fn default_pill_style() -> String {
    "default".into()
}

/// Default pause duration before VAD auto-stop triggers.
fn default_vad_silence_ms() -> u64 {
    800
}

/// The default dictation model. Parakeet TDT v3 is the recommended engine
/// on macOS; Parakeet is macOS-only (CoreML), so Windows defaults to the
/// compressed Whisper Large v3 Turbo instead.
fn default_speech_model_id() -> &'static str {
    if cfg!(target_os = "macos") {
        "parakeet-tdt-v3"
    } else {
        "large-v3-turbo-q5"
    }
}

/// Emoji restore is on by default (P3.2): the curated list is small and the
/// matching rule is conservative (word-boundary, ambiguity-safe).
fn default_true_emoji() -> bool {
    true
}

/// Generic "on by default" serde default.
fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: String::new(),
            recording_mode: "hold".into(),
            selected_speech_model: default_speech_model_id().into(),
            language: "auto".into(),
            input_device: String::new(),
            restore_clipboard: true,
            keep_text_on_clipboard: true,
            auto_apply_transform: true,
            show_tray_icon: true,
            has_completed_onboarding: false,
            selected_llm_model: String::new(),
            typing_autotext_enabled: default_true(),
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
            vad_auto_stop: false,
            vad_silence_ms: default_vad_silence_ms(),
            polish_gate_enabled: default_true(),
            polish_gate_threshold_words: default_polish_gate_threshold_words(),
            app_language_overrides: std::collections::BTreeMap::new(),
            restore_emoji: true,
            spoken_emoji: default_true(),
            spoken_punctuation: default_true(),
            model_unload_delay_secs: 0,
            enabled_packs: Vec::new(),
            theme: default_theme(),
            reduce_motion: false,
            live_preview_enabled: default_true(),
            play_recording_sounds: default_true(),
            recording_sound: default_recording_sound(),
            cancel_hotkey: default_cancel_hotkey(),
            smart_insertion: default_true(),
            auto_copy_to_clipboard: default_true(),
            other_audio_action: default_other_audio_action(),
            warm_engine_policy: default_warm_engine_policy(),
            match_strictness: default_match_strictness(),
            autotext_keyword: default_autotext_keyword(),
        }
    }
}

/// macOS system sounds offered as dictation cues.
///
/// These are files that ship with every Mac (`/System/Library/Sounds`), so the
/// picker has real, working audio with no bundled assets and nothing to keep
/// in sync with an app release. They are short and distinct enough to tell
/// start from stop by ear, which is the only thing the cue has to do.
pub const RECORDING_SOUND_NAMES: &[&str] = &[
    "Submarine",
    "Tink",
    "Pop",
    "Ping",
    "Glass",
    "Hero",
    "Funk",
    "Blow",
];

fn default_recording_sound() -> String {
    "Tink".into()
}

/// The cancel binding, separate from the record binding.
///
/// Escape is the shipped default because it is the one chord a user reaches
/// for mid-recording without looking, and cancelling is the only action safe
/// to bind that way.
fn default_cancel_hotkey() -> String {
    "Escape".into()
}

/// `continue` | `lower` | `mute` | `pause`
fn default_other_audio_action() -> String {
    "continue".into()
}

/// `off` | `10` | `30` | `60` | `always`
fn default_warm_engine_policy() -> String {
    "30".into()
}

/// `loose` | `standard` | `strict`
fn default_match_strictness() -> String {
    "standard".into()
}

/// The word that triggers AutoText expansion ("say backslash …").
fn default_autotext_keyword() -> String {
    "say".into()
}

/// What Teletype does with other apps' audio while a dictation is running.
///
/// `pause` is deliberately absent from the runtime behaviour on this platform
/// and the UI says so rather than offering a control that cannot work: pausing
/// an arbitrary app needs per-app support that does not exist here.
pub const OTHER_AUDIO_ACTIONS: &[&str] = &["continue", "lower", "mute"];

pub const MATCH_STRICTNESS_LEVELS: &[(&str, f32)] =
    &[("loose", 0.72), ("standard", 0.80), ("strict", 0.92)];

/// Resolves the similarity floor for a stored strictness level.
///
/// `standard` maps to the pipeline's existing default rather than restating
/// it, so changing the default in one place cannot leave this returning a
/// stale number.
pub fn match_strictness_floor(level: &str) -> Option<f32> {
    match level {
        "loose" | "strict" => MATCH_STRICTNESS_LEVELS
            .iter()
            .find(|(name, _)| *name == level)
            .map(|(_, v)| *v),
        _ => None,
    }
}

/// Plays one of the [`RECORDING_SOUND_NAMES`] cues so the user can hear it
/// before choosing.
///
/// Deliberately bypasses the master toggle: previewing a sound you have
/// switched off is exactly when a user most needs to hear it.
#[tauri::command]
pub fn preview_recording_sound(name: String) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        if !RECORDING_SOUND_NAMES.contains(&name.as_str()) {
            return Err(format!("Unknown recording sound: {name}"));
        }
        extern "C" {
            fn teletype_play_system_sound(name: *const std::os::raw::c_char) -> i32;
        }
        let c = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
        // SAFETY: `c` outlives the call and NUL-terminated; the callee copies
        // what it needs to build an NSSound.
        let rc = unsafe { teletype_play_system_sound(c.as_ptr()) };
        if rc != 0 {
            return Err("That sound is not available on this Mac".into());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = name;
        Err("Recording sounds are only available on macOS".into())
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
    // BUG-018: serialize the entire read-modify-write so two concurrent
    // saves (e.g. two settings tabs) cannot revert each other's fields.
    let _save_guard = state
        .save_lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let previous = state.settings();
    let hotkey_changed = settings.hotkey != previous.hotkey;
    let position_changed = settings.pill_position != previous.pill_position;
    let new_position = settings.pill_position.clone();

    if hotkey_changed && !settings.hotkey.is_empty() {
        // Unregister the old hotkey first so the new one can take its place.
        if !previous.hotkey.is_empty() {
            state.controller.unregister_hotkey(&app, &previous.hotkey);
        }
        if let Err(e) = state.controller.register_hotkey(&app, &settings.hotkey) {
            // Revert.
            if !previous.hotkey.is_empty() {
                if let Err(revert_e) = state.controller.register_hotkey(&app, &previous.hotkey) {
                    tracing::error!(
                        "Hotkey registration failed ({e}) AND revert to previous hotkey also failed: {revert_e}"
                    );
                }
            }
            return Err(e);
        }
    }
    state.replace_settings(settings.clone())?;

    // If the pill position changed, move the pill immediately (it may be
    // visible mid-dictation or via alwaysShowPill).
    if position_changed {
        crate::overlay::place(&app, crate::dictation::parse_position(&new_position));
    }

    // Let all webviews (main window + pill) refresh from the new settings.
    // The main window uses this to show/hide the Developer tab and re-fetch
    // settings; the pill uses it to update its visual style.
    let _ = app.emit("settings-changed", &());

    // If the app icon changed, apply it to the window and tray.
    if settings.app_icon != previous.app_icon {
        apply_app_icon(&app, &settings.app_icon);
    }

    // BUG-009: apply tray visibility at runtime when the toggle changes.
    if settings.show_tray_icon != previous.show_tray_icon {
        if let Err(e) = crate::tray::apply_visibility(&app, settings.show_tray_icon) {
            crate::log_entry(
                crate::LogLevel::Error,
                format!("tray visibility change failed: {e}"),
            );
        }
    }

    // BUG-010: apply the typing AutoText toggle at runtime.
    if settings.typing_autotext_enabled != previous.typing_autotext_enabled {
        crate::typing::set_enabled(&app, settings.typing_autotext_enabled);
    }

    Ok(settings)
}

/// The two bundled app icons, keyed by the id stored in settings.
pub const APP_ICONS: &[(&str, &str, &[u8])] = &[
    (
        "white",
        "Teletype (light)",
        include_bytes!("../icons/app-icon-white.png"),
    ),
    (
        "blue",
        "Teletype (blue)",
        include_bytes!("../icons/app-icon-blue.png"),
    ),
];

/// Resolves a settings icon id to its bundled PNG bytes.
fn app_icon_bytes(id: &str) -> Option<&'static [u8]> {
    APP_ICONS
        .iter()
        .find(|(k, _, _)| *k == id)
        .map(|(_, _, b)| *b)
}

/// Sets the selected app icon, persists it, and applies it to the window
/// and tray immediately.
#[tauri::command]
pub async fn set_app_icon(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CommandResult<()> {
    let mut settings = state.settings();
    if !APP_ICONS.iter().any(|(k, _, _)| *k == id) {
        return Err(format!("Unknown app icon '{id}'"));
    }
    settings.app_icon = id.clone();
    state.replace_settings(settings)?;
    apply_app_icon(&app, &id);
    Ok(())
}

/// Sets the UI color theme ("system" | "light" | "dark") and persists it.
/// The UI applies it to `document.documentElement`; no Rust-side effect.
#[tauri::command]
pub async fn set_theme(state: State<'_, AppState>, theme: String) -> CommandResult<()> {
    if !matches!(theme.as_str(), "system" | "light" | "dark") {
        return Err(format!("Unknown theme '{theme}'"));
    }
    let mut settings = state.settings();
    settings.theme = theme;
    state.replace_settings(settings)
}

/// Toggles Reduce Motion and persists it. The UI applies it to
/// `document.documentElement.dataset.motion`; no Rust-side effect.
#[tauri::command]
pub async fn set_reduce_motion(state: State<'_, AppState>, enabled: bool) -> CommandResult<()> {
    let mut settings = state.settings();
    settings.reduce_motion = enabled;
    state.replace_settings(settings)
}

/// The pill webview reports its measured natural size (logical px) so the OS
/// window can be resized to fit the active style. The "well" style is 120px
/// tall and the capsules are 44px, so a single fixed window would clip or
/// letterbox them; resizing per style matches the reference implementation.
#[tauri::command]
pub fn set_pill_size(
    app: AppHandle,
    state: State<'_, AppState>,
    width: u32,
    height: u32,
) -> CommandResult<()> {
    let settings = state.settings();
    let position = crate::dictation::parse_position(&settings.pill_position);
    crate::overlay::resize_to(&app, width, height, position);
    Ok(())
}

/// Applies the selected icon to the main window and the tray.
///
/// Both `Window::set_icon` and `TrayIcon::set_icon` must run on the main
/// thread. Tauri's sync wrappers do that by blocking the *calling* thread on
/// `rx.recv()` until the main thread runs the task; called from an async
/// command's tokio worker that wedges the main-thread pipeline and freezes the
/// window. So we schedule fire-and-forget work on the main thread instead of
/// blocking on it. The window and tray handles are `Send + 'static`, so they
/// can be moved into the main-thread closure.
pub fn apply_app_icon(app: &AppHandle, id: &str) {
    let Some(bytes) = app_icon_bytes(id) else {
        return;
    };
    let Ok(icon) = tauri::image::Image::from_bytes(bytes) else {
        return;
    };
    let window = app.get_webview_window("main");
    let tray = app.tray_by_id(crate::tray::TRAY_ID);
    let _ = app.run_on_main_thread(move || {
        if let Some(win) = window {
            let _ = win.set_icon(icon.clone());
        }
        if let Some(tray) = tray {
            let _ = tray.set_icon(Some(icon));
        }
    });
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
/// The entire record+wait+transcribe runs on `spawn_blocking` so the tokio
/// runtime is never blocked (P1-C).
#[tauri::command]
pub async fn transcribe_word(app: AppHandle, state: State<'_, AppState>) -> CommandResult<String> {
    let settings = state.settings();
    let input_device = settings.input_device.clone();
    let language = crate::dictation::effective_language(settings.language.clone(), true);
    let (model_path, use_parakeet) =
        crate::dictation::resolve_speech_model(&settings, &state.models_dir);

    // P1-C: the record+silence-wait loop blocks for up to 5 s. Run it on a
    // blocking thread so the tokio worker pool stays responsive.
    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        transcribe_word_blocking(&app2, &input_device, &language, &model_path, use_parakeet)
    })
    .await
    .map_err(|e| format!("Transcription task failed: {e}"))??;
    Ok(result)
}

fn transcribe_word_blocking(
    app: &AppHandle,
    input_device: &str,
    language: &str,
    model_path: &std::path::Path,
    use_parakeet: bool,
) -> Result<String, String> {
    // Start the recording on the calling thread (it spawns its own capture thread).
    // P0-12: energy-based silence stop instead of a fixed sleep. Stop after
    // 800 ms of continuous silence below the threshold, capped at 5 s.
    let silence = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let silence_since = std::sync::Arc::new(std::sync::Mutex::new(None::<std::time::Instant>));
    let s_silence = silence.clone();
    let s_since = silence_since.clone();
    let recording = teletype_core::audio::Recording::start(input_device, move |level| {
        const THRESHOLD: f32 = 0.01;
        const SILENCE_MS: u64 = 800;
        let now = std::time::Instant::now();
        if level < THRESHOLD {
            let mut since = s_since
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match *since {
                None => *since = Some(now),
                Some(t) if now.duration_since(t).as_millis() >= SILENCE_MS as u128 => {
                    s_silence.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                _ => {}
            }
        } else {
            *s_since
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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

    // Already on a blocking thread (spawn_blocking), so call transcribe directly.
    let result = crate::dictation::transcribe(app, model_path, &captured, language, use_parakeet)?;

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
        &mut None,
    );
    Ok(result.text)
}

// ---- Personalization ----

/// Records one observed edit of AI-polished text (T2.1): extracts candidate
/// signals (greeting / sign-off / terminology) and applies them to the
/// learned preferences, respecting the profile's learning gates. Returns the
/// ids of the preferences that changed; `Ok(vec![])` when nothing learned.
#[tauri::command]
pub async fn record_dictation_edit(
    state: State<'_, AppState>,
    ai_output: String,
    final_text: String,
) -> CommandResult<Vec<String>> {
    if ai_output.trim().is_empty() || final_text.trim().is_empty() || ai_output == final_text {
        return Ok(Vec::new());
    }

    let app_ctx = state.platform.active_application().unwrap_or_default();
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let changed = record_edit_core(&mut profile, &ai_output, &final_text, &app_ctx);
    if !changed.is_empty() {
        state.profile_store.save(&*profile)?;
    }
    Ok(changed)
}

/// The pure gate+extract+apply core of [`record_dictation_edit`], exposed to
/// the dictation controller's edit watcher so an observed edit and a
/// UI-reported edit go through exactly the same gates.
///
/// This is the seam that makes the personalization loop actually reachable:
/// before it existed, `record_dictation_edit` was a Tauri command with no
/// production caller, so nothing ever learned from the user's edits.
pub(crate) fn record_edit_core_public(
    profile: &mut UserProfile,
    ai_output: &str,
    final_text: &str,
    app: &teletype_core::context::ApplicationContext,
) -> Vec<String> {
    record_edit_core(profile, ai_output, final_text, app)
}

/// The pure gate+extract+apply core of [`record_dictation_edit`], factored
/// out so it can be unit-tested without a Tauri `State`.
fn record_edit_core(
    profile: &mut UserProfile,
    ai_output: &str,
    final_text: &str,
    app: &teletype_core::context::ApplicationContext,
) -> Vec<String> {
    use teletype_core::personalization::learn::{apply_signals, extract_signals};
    use teletype_core::personalization::PreferenceScope;

    if ai_output.trim().is_empty() || final_text.trim().is_empty() || ai_output == final_text {
        return Vec::new();
    }
    if !profile.learn_from_edits {
        return Vec::new();
    }
    let signals: Vec<_> = extract_signals(ai_output, final_text, app)
        .into_iter()
        .filter(|s| {
            (matches!(s.scope, PreferenceScope::Global) || profile.learn_app_specific)
                && (!s.key.starts_with("term:") || profile.learn_terminology)
        })
        .collect();
    apply_signals(profile, &signals)
}

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

/// Replaces one preference's description and phrase (T5.6). Preserves the
/// learned preference's evidence (count and provenance) and `created_at`.
#[tauri::command]
pub async fn update_preference(
    state: State<'_, AppState>,
    id: String,
    description: String,
    phrase: String,
) -> CommandResult<()> {
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !profile.update(&id, &description, &phrase) {
        return Err("Preference not found".into());
    }
    state.profile_store.save(&*profile)?;
    Ok(())
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

/// Removes just-learned preferences by id (the undo pill). Returns how many
/// were actually removed; unknown ids are skipped, so a double-click or a
/// pill/ backend race cannot error.
#[tauri::command]
pub async fn undo_learned(state: State<'_, AppState>, ids: Vec<String>) -> CommandResult<usize> {
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut removed = 0;
    for id in &ids {
        if profile.remove(id) {
            removed += 1;
        }
    }
    if removed > 0 {
        state.profile_store.save(&*profile)?;
    }
    Ok(removed)
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

/// Master "Personalization: Off" switch (T5.9). Disabling empties the
/// preference packet and stops learning; the stored preferences survive, so
/// re-enabling restores them.
#[tauri::command]
pub async fn set_personalization_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    let mut profile = state
        .profile
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    profile.set_personalization_enabled(enabled);
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
    use teletype_core::transforms::prompt::{S1Context, S1Control, S1Structure, S1Styling};
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

// ---- Per-app language overrides (P3.3) ----

/// Sets the ASR language override for one app key. Pass an empty `lang` to
/// remove an existing override. The override is keyed by the normalized app
/// key (lowercased bundle id, or lowercased app name when no id is exposed).
#[tauri::command]
pub async fn set_app_language_override(
    state: State<'_, AppState>,
    app_key: String,
    lang: String,
) -> CommandResult<()> {
    let key = app_key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return Err("app key must not be empty".into());
    }
    let mut settings = state.settings();
    let lang = lang.trim().to_string();
    if lang.is_empty() {
        settings.app_language_overrides.remove(&key);
    } else {
        settings.app_language_overrides.insert(key, lang);
    }
    state.replace_settings(settings)?;
    Ok(())
}

/// Returns the full map of per-app language overrides.
#[tauri::command]
pub async fn get_app_language_overrides(
    state: State<'_, AppState>,
) -> CommandResult<std::collections::BTreeMap<String, String>> {
    Ok(state.settings().app_language_overrides)
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
        return Err(format!("Model '{id}' is not downloaded yet."));
    }

    let path = entry.entrypoint(&state.models_dir);
    if !path.exists() {
        return Err(format!("Model file for '{id}' is missing. Re-download it."));
    }

    // BUG-007: kill the old provider BEFORE spawning the new one. The old
    // server is ~5 GB resident; spawning first doubles that and fails on a
    // memory-strapped Mac, leaving "Select did nothing".
    let old = {
        let mut inference = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inference.take()
    };
    if let Some(old) = old {
        crate::log_entry(
            crate::LogLevel::Info,
            "stopping previous LLM model before loading the new one".to_string(),
        );
        // stop() kills the child llama-server and waits for it to exit.
        let _ = tauri::async_runtime::spawn_blocking(move || {
            old.stop();
            drop(old);
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

    {
        let mut inference = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inference.replace(Box::new(provider));
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

    result.map(|handle| handle.to_string_lossy().to_string())
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

/// One Ollama model as reported by `GET /api/tags`.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OllamaModelInfo {
    pub name: String,
    pub size: u64,
    pub modified: String,
    pub verdict: String,
    pub verdict_note: String,
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
    .map_err(|e| format!("detect task: {e}"))
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
            return Err(format!("Ollama returned HTTP {}", resp.status()));
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
            let old = state
                .inference
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(Box::new(provider));
            drop(old);
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
    // P1-A: guard against concurrent spawns. If another thread is already
    // warming up the server, skip — it will install the provider.
    {
        let mut loading = state
            .llm_loading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *loading {
            return;
        }
        *loading = true;
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
        return Err(format!("Unknown speech model '{id}'"));
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
                Err(e)
            }
        }
    });
    handle.await.map_err(|e| format!("download task: {e}"))??;
    Ok(dest.to_string_lossy().to_string())
}

/// G0.5: reports whether the selected dictation model is installed and ready.
/// Used by onboarding to confirm "you can start speaking" before finishing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelReady {
    /// The selected model id.
    pub id: String,
    /// True when the model file is present on disk.
    pub downloaded: bool,
    /// True when `downloaded` and the file is non-empty (a 0-byte file is not usable).
    pub ready: bool,
    /// Short user-facing line, e.g. "Parakeet v3 is ready" or
    /// "Model not downloaded yet".
    pub message: String,
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
    let (_, use_parakeet) = crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    let (loaded, engine_label) = if use_parakeet {
        (state.parakeet.is_loaded(), "Parakeet engine")
    } else {
        (state.speech.is_loaded(), "Whisper engine")
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
    let inference = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    if let Some(p) = inference.as_ref() {
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
    drop(inference);

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

    let loading = *state
        .llm_loading
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if loading {
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
        return Err(format!(
            "Unsupported file type '.{ext}'. Accepted audio formats: {}.",
            AUDIO_EXTENSIONS.join(", ")
        ));
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
        return Err(format!(
            "File is too large ({} MB). The limit is 512 MB.",
            meta.len() / (1024 * 1024)
        ));
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
                    return Err(format!("Decode error: {e}"));
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
            return Err("Audio is longer than 2 hours. The limit is 2 hours.".to_string());
        }
    }
    if pcm.is_empty() {
        return Err("No audio data found in file".into());
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
    let (model_path, use_parakeet) =
        crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    // The frontmost-app language override wins, same as the live path.
    let language = crate::dictation::effective_language_for_app(&state, use_parakeet);

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
        crate::dictation::transcribe(&app2, &model_path2, &captured, &language2, use_parakeet)
    })
    .await
    .map_err(|e| format!("transcribe task: {e}"))??;
    let raw = crate::dictation::ensure_latin_if_english(
        &raw,
        &crate::dictation::effective_language(settings.language.clone(), use_parakeet),
    );
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        return Err("No speech detected in the audio file".into());
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
        let autotext = state
            .autotext
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let transforms = state
            .transforms
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut profile = state
            .profile
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A file has no "frontmost app" meaning; the pipeline's own
        // context lookup would pick up whatever app happens to be active,
        // so resolve the language from the global setting instead.
        profile.language = crate::dictation::effective_language(settings.language.clone(), false);
        let styles = state
            .styles
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let dictionary = state
            .dictionary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let active_style = settings.active_style_profile.clone();
        let inference = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let pack_terms = teletype_core::vocab::terms_for(&settings.enabled_packs);
        let mut pipeline = teletype_core::pipeline::Pipeline {
            platform,
            autotext: &autotext,
            transforms: &transforms,
            profile: &profile,
            inference: inference.as_deref(),
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
    // day-wise transcript line, mirroring the live dictation path.
    let id = uuid::Uuid::new_v4().to_string();
    let created_at = teletype_core::storage::now_ms();
    {
        let mut history = state
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        history.push(teletype_core::history::DictationEntry {
            id: id.clone(),
            created_at,
            text: polished.clone(),
            context: Some(teletype_core::context::ApplicationContext {
                application_name: format!("File: {file_name}"),
                ..Default::default()
            }),
            duration_ms: Some(audio_duration_ms),
        });
        if let Err(e) = state.history_store.save(&history) {
            tracing::warn!("Failed to save history after file transcription: {e}");
        }
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
pub async fn reveal_transcripts_dir(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let dir = state.transcripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
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
    // Clone under the lock, then release it before computing.
    //
    // `insights::compute` is quadratic in the number of stored entries (the
    // `subsumed` n-gram sweep does a `contains` per candidate pair): measured
    // at 0.67 s for 100 entries and 64 s for the 1000-entry cap. The dictation
    // worker takes this same mutex before it injects text
    // (`dictation.rs` history push -> `insert_text`), so holding it across the
    // computation delayed the user's words landing in their document by up to
    // a minute, possibly after they had switched windows.
    let filtered: Vec<teletype_core::history::DictationEntry> = {
        let history = state
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let filtered = match range.as_deref() {
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
        filtered
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
pub async fn get_usage_stats(
    state: State<'_, AppState>,
) -> CommandResult<teletype_core::usage::UsageStats> {
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

/// Exports the custom-words dictionary to `path` as a versioned JSON
/// envelope (`{"version":1,"words":[...]}`).
#[tauri::command]
pub async fn export_custom_words(state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let dict = state
        .dictionary
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    teletype_core::dictionary::export_to_file(&dict, &path)
}

/// Imports a versioned JSON dictionary export from `path`, merging into the
/// existing store (import wins on key conflict). Returns
/// `{imported, updated, skipped}`. A bad file is rejected and the existing
/// store is never truncated.
#[tauri::command]
pub async fn import_custom_words(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<teletype_core::dictionary::ImportCounts> {
    let mut dict = state
        .dictionary
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let counts = teletype_core::dictionary::import_from_file(&mut dict, &path)?;
    state.dictionary_store.save(&*dict)?;
    Ok(counts)
}

// ---- Vocabulary packs ----

/// One vocabulary pack as seen by the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub term_count: usize,
    pub enabled: bool,
}

/// Lists the built-in vocabulary packs with their enabled state.
#[tauri::command]
pub async fn list_packs(state: State<'_, AppState>) -> CommandResult<Vec<PackInfo>> {
    let settings = state.settings();
    Ok(teletype_core::vocab::all()
        .iter()
        .map(|p| PackInfo {
            id: p.id.to_string(),
            name: p.name.to_string(),
            description: p.description.to_string(),
            term_count: p.term_count(),
            enabled: settings.enabled_packs.iter().any(|id| *id == p.id),
        })
        .collect())
}

/// Enables or disables one vocabulary pack, persisting the change.
#[tauri::command]
pub async fn set_pack_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CommandResult<()> {
    if teletype_core::vocab::find(&id).is_none() {
        return Err("Unknown vocabulary pack".into());
    }
    let mut settings = state.settings();
    let present = settings.enabled_packs.contains(&id);
    if enabled && !present {
        settings.enabled_packs.push(id);
    } else if !enabled && present {
        settings.enabled_packs.retain(|x| x != &id);
    }
    state.replace_settings(settings)?;
    Ok(())
}

/// One term inside a vocabulary pack, for the pack detail view.
#[derive(serde::Serialize)]
pub struct PackTermInfo {
    pub canonical: String,
    pub mishearings: Vec<String>,
}

/// The words in a single vocabulary pack (canonical spelling + the known
/// mis-hearings the corrector maps back to it). Lets the UI drill into a pack
/// and show its full word list, matching the reference app's pack detail.
#[tauri::command]
pub async fn list_pack_terms(id: String) -> CommandResult<Vec<PackTermInfo>> {
    let pack =
        teletype_core::vocab::find(&id).ok_or_else(|| format!("Unknown vocabulary pack: {id}"))?;
    Ok(pack
        .terms
        .iter()
        .map(|(canonical, mishearings)| PackTermInfo {
            canonical: (*canonical).to_string(),
            mishearings: mishearings.iter().map(|m| (*m).to_string()).collect(),
        })
        .collect())
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

/// Sets the per-app style override: the style profile that applies
/// automatically when the given app is frontmost and the user has not
/// explicitly picked a style for this dictation. An empty `style_id`
/// removes the override. Keys are normalized (trimmed, lowercased).
#[tauri::command]
pub async fn set_app_style_override(
    state: State<'_, AppState>,
    app_key: String,
    style_id: String,
) -> CommandResult<()> {
    let mut store = state
        .styles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.set_app_style_override(&app_key, &style_id)?;
    state.styles_store.save(&*store)
}

/// All per-app style overrides (normalized app key → style profile id).
#[tauri::command]
pub async fn get_app_style_overrides(
    state: State<'_, AppState>,
) -> CommandResult<std::collections::BTreeMap<String, String>> {
    let store = state
        .styles
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(store.app_style_overrides.clone())
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

// ---- Escape recovery ----

/// The pending recovery spool, if one was left behind by a crash.
/// `seconds` is the amount of 16 kHz mono audio available.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatus {
    pub seconds: u64,
    pub truncated: bool,
}

/// Reports the pending recovery spool, if any. `Ok(None)` when there is
/// nothing to recover.
#[tauri::command]
pub async fn recovery_status(state: State<'_, AppState>) -> CommandResult<Option<RecoveryStatus>> {
    Ok(
        crate::recovery::find_pending(&state.data_dir).map(|p| RecoveryStatus {
            seconds: p.seconds,
            truncated: p.truncated,
        }),
    )
}

/// Transcribes the pending recovery spool through the normal ASR path and
/// records the result like a normal dictation (history + day-wise transcript
/// file), then deletes the spool. The transcript is NOT injected: recovery
/// happens at startup, before the user's previous target app is known.
#[tauri::command]
pub async fn recover_last_dictation(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    let pending = crate::recovery::find_pending(&state.data_dir)
        .ok_or_else(|| "No pending dictation to recover".to_string())?;
    let pending_seconds = pending.seconds;

    // Transcribe on a worker: the speech-model load can take seconds.
    let handle = std::thread::Builder::new()
        .name("teletype-recover".into())
        .spawn(move || crate::recovery::recover(&app, &pending))
        .map_err(|e| format!("recover thread: {e}"))?;
    let transcript = handle
        .join()
        .map_err(|_| "recovery thread panicked".to_string())??;
    let transcript = transcript.trim().to_string();
    if transcript.is_empty() {
        return Err("Recovered audio contained no speech".into());
    }

    // Record the recovered dictation where normal dictations land.
    {
        let created_at = teletype_core::storage::now_ms();
        let mut history = state
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        history.push(teletype_core::history::DictationEntry {
            id: uuid::Uuid::new_v4().to_string(),
            created_at,
            text: transcript.clone(),
            context: None,
            // The recovered take's audio is on disk, and the spool knows how
            // many seconds it holds, so this is a measurement too.
            duration_ms: Some(pending_seconds.saturating_mul(1000)),
        });
        if let Err(e) = state.history_store.save(&history) {
            tracing::warn!("Failed to save history after dictation: {e}");
        }
    }
    {
        let dir = state.transcripts_dir();
        if let Err(e) = teletype_core::history::append_transcript_file(
            &dir,
            teletype_core::storage::now_ms(),
            &transcript,
            "(recovered)",
        ) {
            crate::log_entry(
                crate::LogLevel::Error,
                format!("recovery transcript file: {e}"),
            );
        }
    }
    crate::log_entry(
        crate::LogLevel::Success,
        format!(
            "recovered {} s of dictation: {} chars",
            pending_seconds,
            transcript.len()
        ),
    );
    // The spool has served its purpose; delete it.
    crate::recovery::discard(&state.data_dir)?;
    Ok(transcript)
}

/// Deletes the pending recovery spool without transcribing.
#[tauri::command]
pub async fn discard_recovery(state: State<'_, AppState>) -> CommandResult<()> {
    crate::recovery::discard(&state.data_dir)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_settings_round_trip_through_json() {
        // Every new field carries a #[serde(default)], so an old settings.json
        // must still parse — that is the whole reason the defaults exist.
        let legacy = serde_json::json!({ "hotkey": "Cmd+Shift+Space" });
        let s: Settings = serde_json::from_value(legacy).expect("legacy settings must parse");
        assert_eq!(s.cancel_hotkey, "Escape");
        assert_eq!(s.recording_sound, "Tink");
        assert_eq!(s.other_audio_action, "continue");
        assert_eq!(s.warm_engine_policy, "30");
        assert_eq!(s.match_strictness, "standard");
        assert_eq!(s.autotext_keyword, "say");
        assert!(s.smart_insertion && s.auto_copy_to_clipboard && s.play_recording_sounds);
    }

    #[test]
    fn new_settings_survive_a_save_cycle() {
        let mut s = Settings::default();
        s.smart_insertion = false;
        s.warm_engine_policy = "always".into();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert!(!back.smart_insertion);
        assert_eq!(back.warm_engine_policy, "always");
    }

    #[test]
    fn match_strictness_only_resolves_the_levels_it_names() {
        assert_eq!(match_strictness_floor("loose"), Some(0.72));
        assert_eq!(match_strictness_floor("strict"), Some(0.92));
        // "standard" returns None on purpose: the caller keeps the pipeline's
        // own default, so the two cannot drift apart.
        assert_eq!(match_strictness_floor("standard"), None);
        // Unknown levels must not silently become a floor.
        assert_eq!(match_strictness_floor("nonsense"), None);
        assert_eq!(match_strictness_floor(""), None);
    }

    #[test]
    fn strictness_levels_are_ordered_and_complete() {
        let names: Vec<&str> = MATCH_STRICTNESS_LEVELS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["loose", "standard", "strict"]);
        let floors: Vec<f32> = MATCH_STRICTNESS_LEVELS.iter().map(|(_, f)| *f).collect();
        assert!(floors.windows(2).all(|w| w[0] < w[1]), "must get stricter");
    }

    #[test]
    fn only_shipped_sounds_are_offered() {
        // The picker reads these names, so a name that is not a real macOS
        // system sound would produce a preview that always fails.
        for name in RECORDING_SOUND_NAMES {
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric()),
                "sound name must be a bare filesystem-safe token: {name}"
            );
        }
        let mut sorted = RECORDING_SOUND_NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), RECORDING_SOUND_NAMES.len(), "no duplicates");
        assert!(
            RECORDING_SOUND_NAMES.contains(&default_recording_sound().as_str()),
            "the default cue must be one of the offered sounds"
        );
    }

    #[test]
    fn permission_state_decides_whether_asking_again_can_work() {
        use teletype_core::platform::{Permission, PermissionKind, PermissionState};
        let p = |state| Permission {
            kind: PermissionKind::Microphone,
            state,
        };
        assert!(p(PermissionState::Granted).can_prompt());
        assert!(p(PermissionState::NotDetermined).can_prompt());
        // The point of the tri-state: a refused permission must not keep
        // offering "Grant Access", because macOS will never prompt again.
        assert!(!p(PermissionState::Denied).can_prompt());
        assert!(!p(PermissionState::Unsupported).can_prompt());
        assert_eq!(p(PermissionState::Granted).granted(), true);
        assert_eq!(p(PermissionState::Denied).granted(), false);
    }

    #[test]
    fn record_edit_identical_texts_learn_nothing() {
        let mut profile = UserProfile::default();
        let app = teletype_core::context::normalize("com.google.gmail", "Gmail");
        let changed = record_edit_core(&mut profile, "hi john", "hi john", &app);
        assert!(changed.is_empty());
        assert!(profile.preferences.is_empty());
        // Empty inputs are a no-op too.
        let changed = record_edit_core(&mut profile, "", "", &app);
        assert!(changed.is_empty());
    }

    #[test]
    fn record_edit_signoff_swap_creates_preference() {
        let mut profile = UserProfile::default();
        let app = teletype_core::context::normalize("com.google.gmail", "Gmail");
        // "Best" -> "Regards" is a closing swap (last line of each text). It
        // also trips the position-aligned terminology heuristic ("Best" vs
        // "Regards" differ by 3 chars, 4+ chars each, trailing context
        // matches), so the edit yields one sign-off and one term signal.
        let changed = record_edit_core(
            &mut profile,
            "Hi team,\nPlease review the report.\nBest",
            "Hi team,\nPlease review the report.\nRegards",
            &app,
        );
        assert!(changed.iter().all(|id| profile.get(id).is_some()));
        let signoffs: Vec<_> = profile
            .preferences
            .iter()
            .filter(|p| p.description.contains("sign-off"))
            .collect();
        assert_eq!(signoffs.len(), 1);
        assert!(signoffs[0].description.contains("'Regards'"));
    }

    #[test]
    fn record_edit_respects_learning_gates() {
        let app = teletype_core::context::normalize("com.google.gmail", "Gmail");
        // Unknown app: extract_signals keys every signal Global, so the
        // app-specific gate cannot filter them out (the scope gate is
        // exercised by the sign-off test on a known app instead).
        let unknown = teletype_core::context::ApplicationContext::unknown();
        let ai = "Dear John,\nPlease send the report.\nBest";
        let fin = "Hi John,\nPlease send the document.\nRegards";

        // All gates off: nothing learned.
        let mut profile = UserProfile {
            learn_from_edits: false,
            ..Default::default()
        };
        assert!(record_edit_core(&mut profile, ai, fin, &app).is_empty());

        // Terminology off, app-specific on (unknown app → Global scope): the
        // greeting and sign-off land, the term signal is gated out.
        let mut profile = UserProfile {
            learn_terminology: false,
            ..Default::default()
        };
        let changed = record_edit_core(&mut profile, ai, fin, &unknown);
        assert_eq!(changed.len(), 2);
        assert!(profile
            .preferences
            .iter()
            .all(|p| !p.description.contains("document")));

        // Terminology off AND app-specific off, on a known app (Gmail): all
        // signals are app-scoped, so nothing learns.
        let mut profile = UserProfile {
            learn_app_specific: false,
            ..Default::default()
        };
        profile.learn_terminology = false;
        assert!(record_edit_core(&mut profile, ai, fin, &app).is_empty());

        // Terminology on, app-specific off: the greeting and sign-off are
        // app-scoped (Gmail) and gated out. The terminology signal is
        // app-scoped too (the scope is chosen per-context, not per-key), so
        // with app-specific learning off nothing lands.
        let mut profile = UserProfile {
            learn_app_specific: false,
            ..Default::default()
        };
        let changed = record_edit_core(&mut profile, ai, fin, &app);
        assert!(changed.is_empty());
        assert!(profile.preferences.is_empty());

        // Terminology off: greeting + sign-off land, term does not.
        let mut profile = UserProfile {
            learn_terminology: false,
            ..Default::default()
        };
        let changed = record_edit_core(&mut profile, ai, fin, &unknown);
        assert_eq!(changed.len(), 2);
        assert!(profile
            .preferences
            .iter()
            .all(|p| !p.description.contains("document")));

        // All gates on: greeting + sign-off land. "report"→"document" is a
        // synonym swap (both common words), so BUG-001's judge vetoes it —
        // it must NOT be learned as a terminology preference.
        let mut profile = UserProfile::default();
        let changed = record_edit_core(&mut profile, ai, fin, &unknown);
        assert_eq!(changed.len(), 2);
        assert_eq!(profile.preferences.len(), 2);
        assert!(profile
            .preferences
            .iter()
            .all(|p| !p.description.contains("document")));
    }
    /// BUG-06. The keychain account has to be the real host, and a URL's
    /// userinfo must never be mistaken for it. The old hand-rolled parser
    /// produced the account `user` for
    /// `https://user:pass@evil.example/v1`, which then missed and fell through
    /// to the OpenAI key, sending it to `evil.example`.
    #[test]
    fn secret_account_is_the_host_not_the_userinfo() {
        for (url, want) in [
            ("https://api.openai.com/v1", "api.openai.com"),
            ("https://openrouter.ai/api/v1", "openrouter.ai"),
            ("http://127.0.0.1:11434/v1", "127.0.0.1"),
            ("http://localhost:8080/v1", "localhost"),
            ("https://user:pass@evil.example/v1", "evil.example"),
            ("https://user@evil.example/v1", "evil.example"),
            ("https://API.OpenAI.com/v1", "api.openai.com"),
        ] {
            assert_eq!(secret_account_for_url(url), want, "url: {url}");
        }
        // A malformed base URL must not silently become a usable account.
        assert_eq!(secret_account_for_url("not a url"), "unknown");
    }

    /// The legacy `openai` keychain account is only honoured when the
    /// configured base URL really is OpenAI. Unconditionally, the user's
    /// OpenAI key was sent to whatever third-party host they had configured.
    #[test]
    fn the_legacy_openai_key_is_only_used_for_openai() {
        for url in [
            "https://api.openai.com/v1",
            "https://openai.com/v1",
            "https://eu.api.openai.com/v1",
            "https://openai.com",
        ] {
            assert!(is_openai_base_url(url), "should count as OpenAI: {url}");
        }
        // Notably: a local endpoint is never OpenAI, so a key saved for Ollama
        // can never be the OpenAI key by way of a loopback base URL.
        for url in [
            "https://openrouter.ai/api/v1",
            "https://api.groq.com/openai/v1",
            "http://127.0.0.1:11434/v1",
            "http://localhost:1234/v1",
            "https://api.openai.com.evil.example/v1",
            "https://evil.example/v1",
            "not a url",
        ] {
            assert!(
                !is_openai_base_url(url),
                "must NOT be able to reach the openai key: {url}"
            );
        }
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
}
