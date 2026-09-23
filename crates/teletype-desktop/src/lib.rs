//! Teletype desktop app (Tauri 2).
//!
//! Wires the platform-independent core to the OS: microphone, global hotkeys,
//! text injection, tray, and the React UI.

mod commands;
mod dictation;
mod fn_tap;
mod overlay;
mod platform;
mod secrets;
mod tray;
mod typing;

use std::collections::VecDeque;
use std::sync::{Mutex, RwLock};

use tauri::Manager;

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

/// Ring buffer of recent log lines for the Developer tab.
const LOG_BUFFER_CAP: usize = 1000;
static LOG_BUFFER: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Snapshot of the log ring buffer (oldest first).
pub fn logs_snapshot() -> Vec<String> {
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

/// Writes a line to stderr without panicking if the stream is closed or
/// unavailable (e.g. when the app is relaunched and the original pipe is gone).
/// A closed stderr must never be allowed to crash the app.
/// Also records the line in the Developer-tab ring buffer.
pub fn log_line(msg: &str) {
    {
        let mut buf = LOG_BUFFER
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buf.len() >= LOG_BUFFER_CAP {
            buf.pop_front();
        }
        buf.push_back(msg.to_string());
    }
    use std::io::Write as _;
    let _ = std::io::stderr().write_all(format!("{msg}\n").as_bytes());
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
        log_line(&format!("[teletype] PANIC: {msg}"));
        if let Some(loc) = info.location() {
            log_line(&format!("[teletype]   at {loc}"));
        }
        log_line(&format!(
            "[teletype]   thread: {}",
            std::thread::current().name().unwrap_or("?")
        ));
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
                log_line(&format!("[teletype] couldn't register hotkey {hotkey}: {e}"));
            }

            let _show_tray = settings.show_tray_icon;

            let show_tray = settings.show_tray_icon;
            let icon_id = settings.app_icon.clone();
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
            };
            app.manage(state);

            tray::build(app.handle(), show_tray)?;
            overlay::setup(app.handle())?;
            typing::start(app.handle().clone());

            // Apply the user's chosen app icon (window + tray) at startup.
            commands::apply_app_icon(app.handle(), &icon_id);

            // Warm up the speech model in the background so the first
            // dictation doesn't pay the model-load cost (feels laggy).
            {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("teletype-warmup".into())
                    .spawn(move || {
                        let state = handle.state::<AppState>();
                        let settings = state.settings();
                        let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
                        let use_parakeet = entry
                            .map(|m| m.engine == teletype_speech::catalog::Engine::Parakeet)
                            .unwrap_or(false);
                        let file = entry
                            .map(|m| m.file.to_string())
                            .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
                        let model_path = state.models_dir.join(file);
                        if !model_path.exists() {
                            return; // not downloaded yet; first use will load it
                        }
                        log_line("[teletype] warming up speech model…");
                        if use_parakeet {
                            let mut p = state
                                .parakeet
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if let Err(e) = p.load(&model_path) {
                                log_line(&format!("[teletype] warmup (parakeet) failed: {e}"));
                            }
                        } else {
                            let mut s = state
                                .speech
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if let Err(e) = s.load(&model_path) {
                                log_line(&format!("[teletype] warmup (whisper) failed: {e}"));
                            }
                        }
                        log_line("[teletype] speech model ready");
                    })
                    .ok();
            }

            // Rehydrate the selected LLM provider from settings. The provider
            // itself is memory-only (a local server child process or a keyed
            // API client); without this, a restart left AppState.inference
            // empty and the pipeline silently skipped text transforms.
            {
                let handle = app.handle().clone();
                std::thread::Builder::new()
                    .name("teletype-llm-restore".into())
                    .spawn(move || commands::rehydrate_provider(&handle))
                    .ok();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_settings,
            commands::save_settings,
            commands::set_app_icon,
            commands::list_input_devices,
            commands::get_permissions,
            commands::request_permission,
            commands::open_permission_settings,
            commands::toggle_dictation,
            commands::get_dictation_state,
            commands::transcribe_word,
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
            // Style profiles
            commands::list_style_profiles,
            commands::create_style_profile,
            commands::update_style_profile,
            commands::delete_style_profile,
            commands::set_active_style_profile,
            commands::reset_style_profiles,
            // Scratchpad
            commands::list_scratchpad,
            commands::append_scratchpad,
            commands::delete_scratchpad_entry,
            commands::clear_scratchpad,
            commands::get_scratchpad_text,
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
