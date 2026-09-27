//! Teletype desktop app (Tauri 2).
//!
//! Wires the platform-independent core to the OS: microphone, global hotkeys,
//! text injection, tray, and the React UI.

mod commands;
pub use commands::decode_audio_file_public;
mod dictation;
mod fn_tap;
mod recovery;
mod overlay;
mod platform;
mod secrets;
mod tray;
mod typing;

use std::collections::VecDeque;
use std::sync::{Mutex, RwLock};

use tauri::{Emitter, Manager};

use teletype_core::{
    autotext::AutoTextStore, injector::TextInjector, personalization::UserProfile,
    platform::Platform, storage::JsonStore, transforms::TransformStore,
};
use teletype_core::{dictionary::Dictionary, scratchpad::Scratchpad, style::StyleProfileStore};

/// All app state, shared across commands and the dictation controller.
pub struct AppState {
    pub settings: RwLock<commands::Settings>,
    pub settings_store: JsonStore<commands::Settings>,
    pub autotext: Mutex<AutoTextStore>,
    pub autotext_store: JsonStore<AutoTextStore>,
    pub transforms: Mutex<TransformStore>,
    pub transforms_store: JsonStore<TransformStore>,
    pub profile: Mutex<UserProfile>,
    pub profile_store: JsonStore<UserProfile>,
    pub history: Mutex<teletype_core::history::DictationHistory>,
    pub history_store: JsonStore<teletype_core::history::DictationHistory>,
    pub dictionary: Mutex<Dictionary>,
    pub dictionary_store: JsonStore<Dictionary>,
    pub styles: Mutex<StyleProfileStore>,
    pub styles_store: JsonStore<StyleProfileStore>,
    pub usage: Mutex<teletype_core::usage::UsageStats>,
    pub usage_store: JsonStore<teletype_core::usage::UsageStats>,
    pub scratchpad: Mutex<Scratchpad>,
    pub scratchpad_store: JsonStore<Scratchpad>,
    pub injector: TextInjector,
    pub controller: dictation::Controller,
    pub dictation_state: Mutex<teletype_core::state::UiState>,
    pub platform: Box<dyn Platform>,
    /// The loaded inference provider, if any.
    pub inference: Mutex<Option<Box<dyn teletype_core::llm::InferenceProvider>>>,
    /// The speech provider.
    pub speech: Mutex<Box<dyn teletype_speech::SpeechProvider>>,
    /// The Parakeet speech provider, kept separate so switching engines
    /// reloads the right one.
    pub parakeet: Mutex<Box<dyn teletype_speech::SpeechProvider>>,
    /// Models directory.
    pub models_dir: std::path::PathBuf,
    /// App config directory (used to derive the default transcripts folder).
    pub config_dir: std::path::PathBuf,
    /// App data directory (the escape-recovery spool lives under
    /// `<data_dir>/recovery/`).
    pub data_dir: std::path::PathBuf,
}

impl AppState {
    pub fn settings(&self) -> commands::Settings {
        self.settings
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn replace_settings(&self, settings: commands::Settings) -> Result<(), String> {
        self.settings_store.save(&settings)?;
        *self
            .settings
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = settings;
        Ok(())
    }

    /// The folder day-wise transcript files are written to. Uses the user's
    /// choice from settings, or `<config>/transcripts` when unset.
    pub fn transcripts_dir(&self) -> std::path::PathBuf {
        let settings = self.settings();
        if settings.transcripts_dir.trim().is_empty() {
            self.config_dir.join("transcripts")
        } else {
            std::path::PathBuf::from(&settings.transcripts_dir)
        }
    }
}

/// Log severity for Developer-tab entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum LogLevel {
    Info,
    Success,
    Warn,
    Error,
}

/// One structured log entry for the Developer tab.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    /// Elapsed time in whole milliseconds, when the entry measures a duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Ring buffer of recent log entries for the Developer tab.
const LOG_BUFFER_CAP: usize = 1000;
static LOG_BUFFER: Mutex<VecDeque<LogEntry>> = Mutex::new(VecDeque::new());

/// Snapshot of the log ring buffer (oldest first).
pub fn logs_snapshot() -> Vec<LogEntry> {
    LOG_BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .cloned()
        .collect()
}

/// Clears the log ring buffer.
pub fn clear_logs() {
    LOG_BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// Pushes a log entry into the Developer-tab ring buffer under the given
/// severity.
pub fn log_entry(level: LogLevel, message: impl Into<String>) {
    let entry = LogEntry {
        level,
        message: message.into(),
        duration_ms: None,
    };
    log_entry_with_ms(entry);
}

/// Pushes a log entry that carries a measured duration (shown right-aligned
/// in the Developer tab).
pub fn log_entry_ms(level: LogLevel, message: impl Into<String>, duration_ms: u64) {
    log_entry_with_ms(LogEntry {
        level,
        message: message.into(),
        duration_ms: Some(duration_ms),
    });
}

fn log_entry_with_ms(entry: LogEntry) {
    {
        let mut buf = LOG_BUFFER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buf.len() >= LOG_BUFFER_CAP {
            buf.pop_front();
        }
        buf.push_back(entry.clone());
    }
    // Mirror to stderr as plain text so terminal debugging keeps working.
    let line = match entry.duration_ms {
        Some(ms) => format!("{:?} {} — {} ms", entry.level, entry.message, ms),
        None => format!("{:?} {}", entry.level, entry.message),
    };
    use std::io::Write as _;
    let _ = std::io::stderr().write_all(format!("{line}\n").as_bytes());
    let _ = std::io::stderr().flush();
}

pub fn run() {
    std::panic::set_hook(Box::new(|info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        log_entry(LogLevel::Error, format!("PANIC: {msg}"));
        if let Some(loc) = info.location() {
            log_entry(LogLevel::Error, format!("  at {loc}"));
        }
        log_entry(
            LogLevel::Error,
            format!("  thread: {}", std::thread::current().name().unwrap_or("?")),
        );
    }));
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let models_dir = data_dir.join("models");
            std::fs::create_dir_all(&models_dir).ok();

            let settings_store = JsonStore::new(&config_dir, "settings.json");
            let settings = settings_store.load(commands::Settings::default());

            let autotext_store = JsonStore::new(&config_dir, "autotext.json");
            let autotext = autotext_store.load(AutoTextStore::default());

            let transforms_store = JsonStore::new(&config_dir, "transforms.json");
            let transforms = transforms_store.load(TransformStore::with_built_ins());

            let profile_store = JsonStore::new(&config_dir, "profile.json");
            let profile = profile_store.load(UserProfile::default());

            let (history_store, history) = teletype_core::history::open_history(&config_dir);

            let dictionary_store = JsonStore::new(&config_dir, "dictionary.json");
            let mut dictionary = dictionary_store.load(Dictionary::default());
            // One-time merge of the built-in brand/acronym words (records its
            // version, so it never re-adds or overwrites the user's own words).
            if dictionary.seed_builtins() > 0 {
                let _ = dictionary_store.save(&dictionary);
            }

            let styles_store = JsonStore::new(&config_dir, "styles.json");
            let styles = styles_store.load(StyleProfileStore::with_built_ins());

            let usage_store = JsonStore::new(&config_dir, "usage.json");
            let usage = usage_store.load(teletype_core::usage::UsageStats::default());

            let scratchpad_store = JsonStore::new(&config_dir, "scratchpad.json");
            let scratchpad = scratchpad_store.load(Scratchpad::default());

            let platform: Box<dyn Platform> = platform::create();
            let injector = TextInjector::spawn();
            let controller = dictation::Controller::spawn(app.handle().clone())?;

            // Register the default hotkey.
            let hotkey = if settings.hotkey.is_empty() {
                platform.default_hotkey()
            } else {
                settings.hotkey.clone()
            };
            if let Err(e) = controller.register_hotkey(app.handle(), &hotkey) {
                log_entry(
                    LogLevel::Error,
                    format!("couldn't register hotkey {hotkey}: {e}"),
                );
            }

            let _show_tray = settings.show_tray_icon;

            let show_tray = settings.show_tray_icon;
            let icon_id = settings.app_icon.clone();
            // Reflect the persisted AutoText-while-typing setting now, before
            // `settings` is moved into AppState.
            let typing_autotext_enabled = settings.typing_autotext_enabled;
            // Give the typing watcher its own view of the AutoText store so
            // it never touches Tauri state from its background thread.
            typing::set_autotext_store(std::sync::Arc::new(std::sync::Mutex::new(
                autotext.clone(),
            )));

            let state = AppState {
                settings: RwLock::new(settings),
                settings_store,
                autotext: Mutex::new(autotext),
                autotext_store,
                transforms: Mutex::new(transforms),
                transforms_store,
                profile: Mutex::new(profile),
                profile_store,
                history: Mutex::new(history),
                history_store,
                dictionary: Mutex::new(dictionary),
                dictionary_store,
                styles: Mutex::new(styles),
                styles_store,
                usage: Mutex::new(usage),
                usage_store,
                scratchpad: Mutex::new(scratchpad),
                scratchpad_store,
                injector,
                controller,
                dictation_state: Mutex::new(Default::default()),
                platform,
                inference: Mutex::new(None),
                speech: Mutex::new(Box::new(teletype_speech::whisper::WhisperProvider::new())),
                parakeet: Mutex::new(Box::new(teletype_speech::parakeet::ParakeetProvider::new())),
                models_dir,
                config_dir,
                data_dir,
            };
            app.manage(state);

            // Escape recovery: if the last run crashed mid-dictation, a PCM
            // spool was left behind. Surface it as an event (the UI toast is
            // a follow-up task); the recovery commands are registered below.
            {
                let handle = app.handle().clone();
                let state = app.state::<AppState>();
                if let Some(pending) = recovery::find_pending(&state.data_dir) {
                    log_entry(
                        LogLevel::Info,
                        format!(
                            "recovery: found {} s of unfinished dictation",
                            pending.seconds
                        ),
                    );
                    let _ = handle.emit(
                        "recovery-available",
                        serde_json::json!({
                            "path": pending.path.to_string_lossy(),
                            "seconds": pending.seconds,
                        }),
                    );
                }
            }

            tray::build(app.handle(), show_tray)?;
            // DEBUG: Open devtools to diagnose blank screen
            if let Some(w) = app.get_webview_window("main") {
                #[cfg(debug_assertions)]
                w.open_devtools();
            }
            overlay::setup(app.handle())?;
            typing::start(app.handle().clone());
            if typing_autotext_enabled {
                // Must run on the main thread: the CGEventTap is added to the
                // main run loop (Tauri's setup closure runs there).
                typing::set_enabled(app.handle(), true);
            }

            // Apply the user's chosen app icon (window + tray) at startup.
            commands::apply_app_icon(app.handle(), &icon_id);

            // Warm up the models in the background so the first dictation
            // doesn't pay the model-load cost (feels laggy). The transcription
            // (speech) model is loaded at launch, the remote (openai-compat)
            // polish provider is reinstalled by `rehydrate_provider`, and the
            // local/downloaded polish model (llama-server) is loaded too so it
            // is ready the moment the user dictates. All three run off the
            // tokio runtime so startup stays fast.
            {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("teletype-warmup".into())
                    .spawn(move || {
                        let state = handle.state::<AppState>();

                        // 1. Transcription (speech) model.
                        let settings = state.settings();
                        let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
                        #[cfg(target_os = "macos")]
                        let use_parakeet = entry
                            .map(|m| m.engine == teletype_speech::catalog::Engine::Parakeet)
                            .unwrap_or(false);
                        #[cfg(not(target_os = "macos"))]
                        let use_parakeet = false;
                        let file = entry
                            .map(|m| m.file.to_string())
                            .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
                        let model_path = state.models_dir.join(file);
                        if model_path.exists() {
                            log_entry(LogLevel::Info, "warming up speech model…");
                            let started = std::time::Instant::now();
                            let load_result = if use_parakeet {
                                let mut p = state
                                    .parakeet
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                p.load(&model_path)
                            } else {
                                let mut s = state
                                    .speech
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                                s.load(&model_path)
                            };
                            let elapsed_ms = started.elapsed().as_millis() as u64;
                            let engine = if use_parakeet { "parakeet" } else { "whisper" };
                            match load_result {
                                Ok(()) => log_entry_ms(
                                    LogLevel::Success,
                                    format!("speech model loaded ({engine})"),
                                    elapsed_ms,
                                ),
                                Err(e) => log_entry(
                                    LogLevel::Error,
                                    format!("warmup ({engine}) failed: {e}"),
                                ),
                            }
                        } else {
                            log_entry(
                                LogLevel::Warn,
                                "speech model not downloaded; skipping warmup",
                            );
                        }

                        // 2. Remote polish provider (openai-compat) only.
                        commands::rehydrate_provider(&handle);

                        // 3. Local polish model (llama-server). Load it now so
                        // the user never has to click "Select" after a download.
                        // No-op when a remote provider is active or nothing is
                        // selected/downloaded yet. Matches the reference app,
                        // which keeps the local model resident by default.
                        commands::ensure_local_provider(&handle);
                    })
                    .ok();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_settings,
            commands::save_settings,
            commands::set_app_icon,
            commands::set_theme,
            commands::set_reduce_motion,
            commands::set_pill_size,
            commands::list_input_devices,
            commands::get_permissions,
            commands::request_permission,
            commands::open_permission_settings,
            commands::toggle_dictation,
            commands::cancel_dictation,
            commands::get_dictation_state,
            commands::transcribe_word,
            commands::transcribe_file,
            // AutoText
            commands::list_autotext,
            commands::list_system_autotext,
            commands::create_autotext,
            commands::update_autotext,
            commands::delete_autotext,
            // Transforms
            commands::list_transforms,
            commands::create_transform,
            commands::update_transform,
            commands::delete_transform,
            commands::reset_transforms,
            commands::test_transform,
            // Personalization
            commands::get_profile,
            commands::add_preference,
            commands::remove_preference,
            commands::clear_learned,
            commands::set_profile_settings,
            commands::record_dictation_edit,
            commands::set_app_language_override,
            commands::get_app_language_overrides,
            commands::set_s1_control,
            // Models
            commands::list_models,
            commands::select_model,
            commands::download_model,
            commands::get_model_status,
            // LLM provider / secrets
            commands::set_llm_secret,
            commands::has_llm_secret,
            commands::clear_llm_secret,
            commands::select_openai_provider,
            commands::test_llm_connection,
            // Speech models
            commands::list_speech_models,
            commands::list_speech_languages,
            commands::select_speech_model,
            commands::download_speech_model,
            // Dictation history
            commands::list_dictation_history,
            commands::delete_dictation_entry,
            // Insights
            commands::get_insights,
            // Usage stats
            commands::get_usage_stats,
            commands::reset_usage_stats,
            // Dictionary
            commands::list_dictionary,
            commands::add_dictionary_word,
            commands::remove_dictionary_word,
            commands::export_custom_words,
            commands::import_custom_words,
            // Vocabulary packs
            commands::list_packs,
            commands::set_pack_enabled,
            // Style profiles
            commands::list_style_profiles,
            commands::create_style_profile,
            commands::update_style_profile,
            commands::delete_style_profile,
            commands::set_active_style_profile,
            commands::reset_style_profiles,
            commands::set_app_style_override,
            commands::get_app_style_overrides,
            // Scratchpad
            commands::list_scratchpad,
            commands::append_scratchpad,
            commands::delete_scratchpad_entry,
            commands::clear_scratchpad,
            commands::get_scratchpad_text,
            // Escape recovery
            commands::recover_last_dictation,
            commands::discard_recovery,
            commands::recovery_status,
            // Misc
            commands::open_main_window,
            commands::quit_app,
            commands::get_username,
            commands::get_transcripts_dir,
            commands::reveal_transcripts_dir,
            // Native hotkey capture
            commands::start_hotkey_capture,
            commands::stop_hotkey_capture,
            commands::get_captured_hotkey,
            // Developer tab
            commands::get_logs,
            commands::clear_logs,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep running in the tray; only quit from the menu.
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Teletype");
}
