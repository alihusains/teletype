//! The dictation controller: one thread owns the session state machine.
//!
//! All events (hotkey, toggle, escape, transcription result) are messages to
//! this thread, so state has a single owner and events are processed in order.

use std::{
    sync::mpsc::{self, Sender},
    thread,
    time::Duration,
};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use teletype_core::{
    audio::{Captured, Recording},
    pipeline::{InputSource, Pipeline, UnifiedInput},
    state::{self, Input, Phase, RecordingMode},
};

use crate::AppState;

pub enum Event {
    HotkeyDown,
    HotkeyUp,
    HotkeyInterrupted,
    Toggle,
    Cancel,
    Transcribed {
        session: u64,
        transcript: Result<String, String>,
    },
}

#[derive(Clone)]
pub struct Controller {
    tx: Sender<Event>,
}

impl Controller {
    pub fn spawn(app: AppHandle) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let controller = Self { tx };
        let mut session = Session {
            app,
            controller: controller.clone(),
            phase: Phase::Idle,
            hotkey_down: false,
            next_id: 0,
        };
        thread::Builder::new()
            .name("teletype-dictation".into())
            .spawn(move || {
                for event in rx {
                    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        session.handle(event);
                    }));
                    if handled.is_err() {
                        eprintln!("[dictation] event handler panicked");
                    }
                }
            })?;
        Ok(controller)
    }

    pub fn send(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn register_hotkey(&self, app: &AppHandle, hotkey: &str) -> Result<(), String> {
        self.unregister_hotkey(app, hotkey);
        let controller = self.clone();
        app.global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                controller.send(match event.state {
                    ShortcutState::Pressed => Event::HotkeyDown,
                    ShortcutState::Released => Event::HotkeyUp,
                });
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
    }

    /// Unregisters a previously registered global shortcut (no-op if not registered).
    pub fn unregister_hotkey(&self, app: &AppHandle, hotkey: &str) {
        let _ = app.global_shortcut().unregister(hotkey);
    }
}

struct Session {
    app: AppHandle,
    controller: Controller,
    phase: Phase,
    hotkey_down: bool,
    next_id: u64,
}

impl Session {
    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::HotkeyDown => self.input(Input::HotkeyDown),
            Event::HotkeyUp => self.input(Input::HotkeyUp),
            Event::HotkeyInterrupted => self.input(Input::HotkeyInterrupted),
            Event::Toggle => self.input(Input::Toggle),
            Event::Cancel => self.input(Input::Cancel),
            Event::Transcribed {
                session,
                transcript,
            } => {
                self.transcribed(session, transcript);
            }
        }
    }

    fn input(&mut self, input: Input) {
        let settings = self.state().settings();
        let mode = match settings.recording_mode.as_str() {
            "toggle" => RecordingMode::Toggle,
            _ => RecordingMode::Hold,
        };
        let action = state::decide(input, self.phase, mode, &mut self.hotkey_down);
        match action {
            state::Action::Nothing => {}
            state::Action::Start { by_hotkey } => self.start(by_hotkey),
            state::Action::Stop { cancelled } => self.stop(cancelled),
            state::Action::Discard => self.go_idle(),
            state::Action::CancelPipeline => {
                tracing::info!("pipeline cancelled by user");
                self.go_idle();
            }
        }
    }

    fn start(&mut self, _by_hotkey: bool) {
        let settings = self.state().settings();
        let app = self.app.clone();
        let recording = Recording::start(&settings.input_device, move |level| {
            let _ = app.emit_to("pill", "pill-level", level);
        });
        match recording {
            Ok(recording) => {
                let started_at_ms = teletype_core::storage::now_ms();
                self.phase = Phase::Listening;
                self.broadcast(teletype_core::state::UiState::Listening { started_at_ms });
                // Store the recording in a thread-local so `stop` can access it.
                RECORDING.with(|slot| *slot.borrow_mut() = Some(recording));
            }
            Err(e) => {
                eprintln!("[dictation] {e}");
                self.flash("Microphone unavailable");
            }
        }
    }

    fn stop(&mut self, cancelled: bool) {
        let recording = RECORDING.with(|slot| slot.borrow_mut().take());
        let Some(recording) = recording else {
            self.go_idle();
            return;
        };
        let captured = match recording.finish() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[dictation] {e}");
                self.flash("Recording failed");
                return;
            }
        };

        self.next_id += 1;
        let session = self.next_id;
        self.phase = Phase::Transcribing;
        self.broadcast(teletype_core::state::UiState::Transcribing);

        let app = self.app.clone();
        let controller = self.controller.clone();
        let (speech_model_path, use_parakeet) = {
            let state = self.state();
            let settings = state.settings();
            let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
            let use_parakeet = entry
                .map(|m| m.engine == teletype_speech::catalog::Engine::Parakeet)
                .unwrap_or(false);
            // Always resolve through the catalog so the on-disk file name
            // matches what the download command wrote (e.g.
            // ggml-large-v3-turbo-q5_0.bin, not large-v3-turbo-q5.bin).
            let file = entry
                .map(|m| m.file.to_string())
                .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
            (state.models_dir.join(file), use_parakeet)
        };
        let language = self.state().settings().language.clone();

        let worker = thread::Builder::new()
            .name("teletype-transcribe".into())
            .spawn(move || {
                let outcome =
                    transcribe(&app, &speech_model_path, &captured, &language, use_parakeet);
                controller.send(Event::Transcribed {
                    session,
                    transcript: outcome,
                });
            });
        if let Err(e) = worker {
            eprintln!("[dictation] couldn't start transcription: {e}");
            self.flash("Transcription failed");
        }
        if cancelled {
            self.phase = Phase::Cancelled;
        }
    }

    fn transcribed(&mut self, session: u64, transcript: Result<String, String>) {
        if self.next_id != session {
            return; // stale
        }
        match transcript {
            Ok(text) => {
                self.phase = Phase::Transforming;
                self.broadcast(teletype_core::state::UiState::Transforming);

                let app = self.app.clone();
                let controller = self.controller.clone();
                let worker = thread::Builder::new()
                    .name("teletype-pipeline".into())
                    .spawn(move || {
                        let state = app.state::<AppState>();
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
                        let profile = state
                            .profile
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        let inference = state
                            .inference
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);

                        let input = UnifiedInput {
                            source: InputSource::Voice,
                            text,
                        };
                        let pipeline = Pipeline {
                            platform,
                            autotext: &autotext,
                            transforms: &transforms,
                            profile: &profile,
                            inference: inference.as_deref(),
                            auto_apply: settings.auto_apply_transform,
                            restore_clipboard: settings.restore_clipboard,
                        };
                        let result = pipeline.run(input, None);
                        drop(inference);
                        drop(profile);
                        drop(transforms);
                        drop(autotext);

                        // Inject.
                        let paste = platform.paste_shortcut();
                        state.injector.inject(
                            result.final_text.clone(),
                            settings.restore_clipboard,
                            paste,
                        );

                        controller.send(Event::Transcribed {
                            session,
                            transcript: Ok(result.final_text),
                        });
                    });
                if let Err(e) = worker {
                    eprintln!("[dictation] pipeline thread failed: {e}");
                    self.flash("Transform failed");
                }
            }
            Err(e) => {
                eprintln!("[dictation] transcription failed: {e}");
                self.flash("Transcription failed");
            }
        }
    }

    fn flash(&mut self, message: &str) {
        self.broadcast(teletype_core::state::UiState::Message {
            text: message.into(),
        });
        // Auto-return to idle after 2s.
        let controller = self.controller.clone();
        thread::Builder::new()
            .name("teletype-flash".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(2000));
                controller.send(Event::Toggle); // no-op if idle
            })
            .ok();
        self.go_idle();
    }

    fn go_idle(&mut self) {
        self.phase = Phase::Idle;
        self.broadcast(teletype_core::state::UiState::Idle);
    }

    fn broadcast(&self, state: teletype_core::state::UiState) {
        let _ = self.app.emit("dictation-state", &state);
        crate::tray::show_state(&self.app, &state);
        *self
            .state()
            .dictation_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }
}

// Thread-local storage for the active recording (one at a time).
thread_local! {
    static RECORDING: std::cell::RefCell<Option<Recording>> = const { std::cell::RefCell::new(None) };
}

/// Runs on a worker thread: loads the speech model and transcribes.
fn transcribe(
    app: &AppHandle,
    model_path: &std::path::Path,
    captured: &Captured,
    language: &str,
    use_parakeet: bool,
) -> Result<String, String> {
    let state = app.state::<AppState>();
    let mut speech = (if use_parakeet {
        &state.parakeet
    } else {
        &state.speech
    })
    .lock()
    .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !speech.is_loaded() {
        if !model_path.exists() {
            return Err("Speech model not downloaded".into());
        }
        speech
            .load(model_path)
            .map_err(|e| format!("Speech model load failed: {e}"))?;
    }
    speech
        .transcribe(&captured.samples, language)
        .map_err(|e| e.to_string())
}
