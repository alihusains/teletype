//! Teletype desktop app (Tauri 2).
//!
//! Wires the platform-independent core to the OS: microphone, global hotkeys,
//! text injection, tray, and the React UI.

mod commands;
mod dictation;
mod fn_tap;
mod overlay;
mod platform;
mod tray;
mod typing;

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
        eprintln!("[teletype] PANIC: {msg}");
        if let Some(loc) = info.location() {
            eprintln!("[teletype]   at {loc}");
        }
        eprintln!(
            "[teletype]   thread: {}",
            std::thread::current().name().unwrap_or("?")
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
            let dictionary = dictionary_store.load(Dictionary::default());

            let styles_store = JsonStore::new(&config_dir, "styles.json");
            let styles = styles_store.load(StyleProfileStore::with_built_ins());

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
                eprintln!("[teletype] couldn't register hotkey {hotkey}: {e}");
            }

            let _show_tray = settings.show_tray_icon;

            let show_tray = settings.show_tray_icon;
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
            };
            app.manage(state);

            tray::build(app.handle(), show_tray)?;
            overlay::setup(app.handle())?;
            typing::start(app.handle().clone());

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
                        eprintln!("[teletype] warming up speech model…");
                        if use_parakeet {
                            let mut p = state
                                .parakeet
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if let Err(e) = p.load(&model_path) {
                                eprintln!("[teletype] warmup (parakeet) failed: {e}");
                            }
                        } else {
                            let mut s = state
                                .speech
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if let Err(e) = s.load(&model_path) {
                                eprintln!("[teletype] warmup (whisper) failed: {e}");
                            }
                        }
                        eprintln!("[teletype] speech model ready");
                    })
                    .ok();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_settings,
            commands::save_settings,
            commands::list_input_devices,
            commands::get_permissions,
            commands::request_permission,
            commands::open_permission_settings,
            commands::toggle_dictation,
            commands::get_dictation_state,
            // AutoText
            commands::list_autotext,
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
            // Speech models
            commands::list_speech_models,
            commands::select_speech_model,
            commands::download_speech_model,
            // Dictation history
            commands::list_dictation_history,
            commands::delete_dictation_entry,
            // Dashboard / insights
            commands::get_dashboard_stats,
            commands::get_insights,
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
            // Native hotkey capture
            commands::start_hotkey_capture,
            commands::stop_hotkey_capture,
            commands::get_captured_hotkey,
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
