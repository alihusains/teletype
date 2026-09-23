//! The dictation controller: one thread owns the session state machine.
//!
//! All events (hotkey, toggle, escape, transcription result) are messages to
//! this thread, so state has a single owner and events are processed in order.

use std::{
    sync::mpsc::{self, Sender},
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use teletype_core::{
    audio::{Captured, Recording},
    pipeline::{InputSource, Pipeline, UnifiedInput},
    state::{self, Input, Phase, PillPosition, PillState, RecordingMode},
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
    /// The transform + inject pipeline for a session has finished.
    PipelineDone {
        session: u64,
    },
    /// A flash message's display timer elapsed; hide the pill.
    FlashDone {
        generation: u64,
    },
    /// The speech model finished loading in the background.
    WarmupDone {
        session: u64,
    },
}

#[derive(Clone)]
pub struct Controller {
    tx: Sender<Event>,
}

impl Controller {
    pub fn tx(&self) -> Sender<Event> {
        self.tx.clone()
    }

    pub fn spawn(app: AppHandle) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let controller = Self { tx };
        let mut session = Session {
            app,
            controller: controller.clone(),
            phase: Phase::Idle,
            hotkey_down: false,
            hands_free: false,
            last_release: None,
            next_id: 0,
            started_at_ms: 0,
        };
        thread::Builder::new()
            .name("teletype-dictation".into())
            .spawn(move || {
                for event in rx {
                    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        session.handle(event);
                    }));
                    if handled.is_err() {
                        crate::log_line("[dictation] event handler panicked");
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
        if hotkey == "Fn" {
            // Carbon RegisterEventHotKey never fires for Fn-only presses, so the
            // Fn key uses a CGEventTap (needs Accessibility permission, which the
            // app already requires for typing). Bare "Fn" means Fn with no other
            // modifiers; "Fn+<key>" combos work through the normal global-shortcut
            // path with the fn modifier flag.
            crate::fn_tap::start(self.clone()).map_err(|e| {
                format!("Couldn't register Fn: {e} (grant Teletype Accessibility access in System Settings > Privacy & Security > Accessibility)")
            })
        } else {
            self.register_string_hotkey(app, hotkey)
        }
    }

    fn register_string_hotkey(&self, app: &AppHandle, hotkey: &str) -> Result<(), String> {
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
        if hotkey == "Fn" {
            crate::fn_tap::stop();
        } else {
            let _ = app.global_shortcut().unregister(hotkey);
        }
    }
}

struct Session {
    app: AppHandle,
    controller: Controller,
    phase: Phase,
    hotkey_down: bool,
    hands_free: bool,
    /// Time of the last hotkey release, used to detect a double-tap in Hold
    /// mode (release then press again within DOUBLE_TAP_WINDOW).
    last_release: Option<Instant>,
    next_id: u64,
    /// When the current recording started (ms); used to re-emit Recording
    /// once the model finishes warming.
    started_at_ms: u64,
}

/// A second press within this window of a release is a double-tap.
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(350);

impl Session {
    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::HotkeyDown => self.hotkey_down_event(),
            Event::HotkeyUp => self.hotkey_up_event(),
            Event::HotkeyInterrupted => self.input(Input::HotkeyInterrupted),
            Event::Toggle => self.input(Input::Toggle),
            Event::Cancel => self.input(Input::Cancel),
            Event::Transcribed {
                session,
                transcript,
            } => {
                self.transcribed(session, transcript);
            }
            Event::PipelineDone { session } => {
                if self.next_id == session {
                    self.go_idle();
                }
            }
            Event::FlashDone { generation } => {
                if self.next_id == generation && self.phase == Phase::Idle {
                    self.flash_done();
                }
            }
            Event::WarmupDone { session } => {
                // Only the newest session may hide the warming pill.
                if self.next_id == session && self.phase == Phase::Listening {
                    self.show(PillState::Recording {
                        started_at_ms: self.started_at_ms,
                    });
                }
            }
        }
    }

    fn hotkey_down_event(&mut self) {
        let mode = self.recording_mode();
        // Double-tap detection (Hold mode): a press shortly after a release
        // while still recording switches to hands-free instead of stopping.
        if self.phase == Phase::Listening && mode == RecordingMode::Hold && !self.hands_free {
            if let Some(t) = self.last_release {
                if t.elapsed() <= DOUBLE_TAP_WINDOW {
                    self.hands_free = true;
                    self.last_release = None;
                    return;
                }
            }
        }
        self.input(Input::HotkeyDown);
    }

    fn hotkey_up_event(&mut self) {
        self.input(Input::HotkeyUp);
        self.last_release = Some(Instant::now());
    }

    fn recording_mode(&self) -> RecordingMode {
        let settings = self.state().settings();
        match settings.recording_mode.as_str() {
            "toggle" => RecordingMode::Toggle,
            _ => RecordingMode::Hold,
        }
    }

    fn input(&mut self, input: Input) {
        let mode = self.recording_mode();
        let action = state::decide(
            input,
            self.phase,
            mode,
            &mut self.hotkey_down,
            &mut self.hands_free,
        );
        match action {
            state::Action::Nothing => {}
            state::Action::Start { by_hotkey } => self.start(by_hotkey),
            state::Action::Stop { cancelled } => self.stop(cancelled),
            state::Action::GoHandsFree => {
                self.hands_free = true;
            }
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
        // Show the pill immediately, BEFORE the (slow) mic open, so the UI
        // responds instantly on keypress like Wispr Flow. The waveform begins
        // as soon as levels arrive.
        let started_at_ms = teletype_core::storage::now_ms();
        self.started_at_ms = started_at_ms;
        self.phase = Phase::Listening;
        self.broadcast(teletype_core::state::UiState::Listening { started_at_ms });
        self.show(PillState::Recording { started_at_ms });

        let recording = Recording::start(&settings.input_device, move |level| {
            let _ = app.emit_to("pill", "pill-level", level);
        });
        match recording {
            Ok(recording) => {
                // Store the recording in a thread-local so `stop` can access it.
                RECORDING.with(|slot| *slot.borrow_mut() = Some(recording));
                // Load the model while the user speaks, so it's ready on release.
                // If it isn't loaded yet, show the warming pill.
                self.warm_up();
            }
            Err(e) => {
                crate::log_line(&format!("[dictation] {e}"));
                self.flash("Microphone unavailable");
            }
        }
    }

    /// Loads the selected speech model in the background so the first
    /// dictation (or a model switch) doesn't block the release. Shows the
    /// warming pill while the load is in flight.
    fn warm_up(&mut self) {
        let (model_path, use_parakeet) = {
            let state = self.state();
            let settings = state.settings();
            let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
            let use_parakeet = entry
                .map(|m| m.engine == teletype_speech::catalog::Engine::Parakeet)
                .unwrap_or(false);
            let file = entry
                .map(|m| m.file.to_string())
                .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
            (state.models_dir.join(file), use_parakeet)
        };
        if !model_path.exists() {
            return;
        }
        // Already loaded → recording pill stays as-is.
        let state = self.state();
        let ready = if use_parakeet {
            state
                .parakeet
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_loaded()
        } else {
            state
                .speech
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_loaded()
        };
        if ready {
            crate::log_line(&format!(
                "[teletype] speech model already loaded: {}",
                model_path.display()
            ));
            return;
        }
        self.show(PillState::Warming);
        let next_id = {
            self.next_id += 1;
            self.next_id
        };
        let app = self.app.clone();
        let controller = self.controller.clone();
        crate::log_line(&format!(
            "[teletype] loading speech model: {} ({})",
            model_path.display(),
            if use_parakeet { "parakeet" } else { "whisper" }
        ));
        thread::Builder::new()
            .name("teletype-warmup".into())
            .spawn(move || {
                let state = app.state::<AppState>();
                let result = if use_parakeet {
                    state
                        .parakeet
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .load(&model_path)
                } else {
                    state
                        .speech
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .load(&model_path)
                };
                match result {
                    Ok(()) => crate::log_line(&format!(
                        "[teletype] speech model ready: {} ({})",
                        model_path.display(),
                        if use_parakeet { "parakeet" } else { "whisper" }
                    )),
                    Err(e) => crate::log_line(&format!("[dictation] warm-up failed: {e}")),
                }
                controller.send(Event::WarmupDone { session: next_id });
            })
            .ok();
    }

    fn stop(&mut self, cancelled: bool) {
        let recording = RECORDING.with(|slot| slot.borrow_mut().take());
        let Some(recording) = recording else {
            self.go_idle();
            return;
        };

        // Update the pill to "Transcribing" immediately on release; the
        // (blocking) audio finish + resample happens on the worker thread so
        // the UI never stalls.
        self.next_id += 1;
        let session = self.next_id;
        self.phase = Phase::Transcribing;
        self.broadcast(teletype_core::state::UiState::Transcribing);
        let message = if cancelled {
            "Stopping transcription…"
        } else {
            "Transcribing…"
        };
        self.show(PillState::Processing {
            message: message.into(),
        });

        let app = self.app.clone();
        let controller = self.controller.clone();
        let (speech_model_path, use_parakeet) = {
            let state = self.state();
            let settings = state.settings();
            let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
            let use_parakeet = entry
                .map(|m| m.engine == teletype_speech::catalog::Engine::Parakeet)
                .unwrap_or(false);
            let file = entry
                .map(|m| m.file.to_string())
                .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
            (state.models_dir.join(file), use_parakeet)
        };
        // Parakeet is English-only; "auto" there means English. Guarding here
        // prevents the Russian mis-transcription on short utterances.
        let language = effective_language(self.state().settings().language.clone(), use_parakeet);

        let worker = thread::Builder::new()
            .name("teletype-transcribe".into())
            .spawn(move || {
                let captured = match recording.finish() {
                    Ok(c) => c,
                    Err(e) => {
                        controller.send(Event::Transcribed {
                            session,
                            transcript: Err(e.to_string()),
                        });
                        return;
                    }
                };
                crate::log_line(&format!(
                    "[teletype] transcribing audio: {} samples, language {}, engine {}",
                    captured.samples.len(),
                    language,
                    if use_parakeet { "parakeet" } else { "whisper" }
                ));
                let outcome =
                    transcribe(&app, &speech_model_path, &captured, &language, use_parakeet);
                // Distinguish "nothing was recorded" from "the model found no
                // words in real speech" so the user gets an honest message.
                let transcript = match outcome {
                    Err(e) if e == "No speech detected" && captured.peak < 0.005 => {
                        Err("No audio captured — check the microphone".into())
                    }
                    other => other,
                };
                controller.send(Event::Transcribed {
                    session,
                    transcript,
                });
            });
        if let Err(e) = worker {
            crate::log_line(&format!("[dictation] couldn't start transcription: {e}"));
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
            Ok(raw) => {
                // If the language is English and the model misidentified the
                // speech as Russian (Cyrillic), transliterate back to Latin.
                let text = ensure_latin_if_english(&raw, &effective_language(self.state().settings().language.clone(), false));
                self.phase = Phase::Transforming;
                self.broadcast(teletype_core::state::UiState::Transforming);
                self.show(PillState::Processing {
                    message: "Transforming…".into(),
                });

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

                        let transform_model = inference
                            .as_ref()
                            .map(|p| format!("{} ({})", p.model_name(), p.model_id()))
                            .unwrap_or_else(|| "none (AutoText only)".into());
                        crate::log_line(&format!(
                            "[teletype] running dictation pipeline: auto_apply={}, transform_model={}",
                            settings.auto_apply_transform, transform_model
                        ));
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
                            dictionary: &dictionary,
                            styles: &styles,
                            active_style: &active_style,
                            auto_apply: settings.auto_apply_transform,
                            restore_clipboard: settings.restore_clipboard,
                            remove_filler_words: settings.remove_filler_words,
                            filler_words: settings.filler_words.clone(),
                            system_autotext: teletype_core::autotext::system::entries(),
                        };
                        let result = pipeline.run(input, None);

                        if let Some(transform) = &result.transform {
                            crate::log_line(&format!(
                                "[teletype] transform finished: transformed={}, fell_back={}, latency_ms={}",
                                transform.transformed,
                                transform.metrics.fell_back,
                                transform.metrics.latency_ms
                            ));
                        } else {
                            crate::log_line("[teletype] transform skipped: no transform ran");
                        }

                        // The fallback must never be silent: a transform was
                        // selected but no LLM provider was loaded, so only
                        // AutoText/filler cleanup ran.
                        if result.transform_skipped_no_model {
                            crate::log_line(
                                "[teletype] transform skipped: no LLM model loaded; \
                                 applied AutoText only. Pick a model in Settings > Models.",
                            );
                        }

                        // Record usage: which filler words were removed and
                        // which AutoText entries were used, from the raw
                        // transcript (before cleanup) and the autotext store.
                        {
                            let raw = result.raw_input.clone();
                            let ctx = result.context.clone();
                            let filler_counts = if settings.remove_filler_words {
                                teletype_core::usage::count_fillers(&raw, &settings.filler_words)
                            } else {
                                std::collections::BTreeMap::new()
                            };
                            let autotext_counts =
                                teletype_core::usage::count_autotext(&raw, &autotext, &ctx);
                            let mut usage = state
                                .usage
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            usage.record(&filler_counts, &autotext_counts);
                            let _ = state.usage_store.save(&usage);
                        }

                        drop(inference);
                        drop(dictionary);
                        drop(styles);
                        drop(profile);
                        drop(transforms);
                        drop(autotext);

                        // Save to dictation history (always, so it's
                        // recoverable even when there was no text field to
                        // paste into). The context was captured at pipeline
                        // run time (the app focused while dictating).
                        {
                            let created_at = teletype_core::storage::now_ms();
                            let mut history = state
                                .history
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            history.push(teletype_core::history::DictationEntry {
                                id: uuid::Uuid::new_v4().to_string(),
                                created_at,
                                text: result.final_text.clone(),
                                context: Some(result.context.clone()),
                            });
                            let _ = state.history_store.save(&history);
                        }

                        // Also append to a day-wise transcript file so the
                        // user can browse a plain-text archive on disk.
                        {
                            let dir = state.transcripts_dir();
                            let app_name = result.context.application_name.clone();
                            if let Err(e) = teletype_core::history::append_transcript_file(
                                &dir,
                                teletype_core::storage::now_ms(),
                                &result.final_text,
                                &app_name,
                            ) {
                                crate::log_line(&format!("[teletype] transcript file: {e}"));
                            }
                        }

                        // Route to the scratchpad when it's enabled and
                        // frontmost; otherwise inject into the previous focus.
                        let to_scratchpad = settings.scratchpad_enabled
                            && result
                                .context
                                .application_name
                                .eq_ignore_ascii_case("Teletype");
                        if to_scratchpad {
                            let mut pad = state
                                .scratchpad
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            pad.append(result.final_text.clone());
                            let _ = state.scratchpad_store.save(&*pad);
                        } else {
                            let paste = platform.paste_shortcut();
                            state.injector.inject(
                                result.final_text.clone(),
                                settings.restore_clipboard,
                                paste,
                            );
                        }

                        // Signal completion. Do NOT re-send Event::Transcribed —
                        // that would re-enter transcribed() and run the pipeline
                        // + inject a second time (the repeat-paste bug).
                        controller.send(Event::PipelineDone { session });
                    });
                if let Err(e) = worker {
                    crate::log_line(&format!("[dictation] pipeline thread failed: {e}"));
                    self.flash("Transform failed");
                }
            }
            Err(e) => {
                crate::log_line(&format!("[dictation] transcription failed: {e}"));
                self.flash(&e);
            }
        }
    }

    fn flash(&mut self, message: &str) {
        self.next_id += 1;
        let generation = self.next_id;
        self.phase = Phase::Idle;
        self.broadcast(teletype_core::state::UiState::Message {
            text: message.into(),
        });
        self.show(PillState::Processing {
            message: message.into(),
        });
        let controller = self.controller.clone();
        thread::Builder::new()
            .name("teletype-flash".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(2000));
                controller.send(Event::FlashDone { generation });
            })
            .ok();
    }

    fn flash_done(&mut self) {
        self.show(PillState::Idle);
        self.broadcast(teletype_core::state::UiState::Idle);
    }

    fn go_idle(&mut self) {
        self.phase = Phase::Idle;
        self.hands_free = false;
        self.last_release = None;
        self.broadcast(teletype_core::state::UiState::Idle);
        self.show(PillState::Idle);
    }

    fn show(&self, state: PillState) {
        let settings = self.state().settings();
        let position = parse_position(&settings.pill_position);
        crate::overlay::update(&self.app, state, position, settings.always_show_pill);
        // The pill takes clicks only while recording, so its controls work.
        crate::overlay::set_interactive(&self.app, matches!(self.phase, Phase::Listening));
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

fn parse_position(s: &str) -> PillPosition {
    use PillPosition::*;
    match s {
        "topLeft" => TopLeft,
        "topCenter" => TopCenter,
        "topRight" => TopRight,
        "centerLeft" => CenterLeft,
        "center" => Center,
        "centerRight" => CenterRight,
        "bottomLeft" => BottomLeft,
        "bottomCenter" => BottomCenter,
        "bottomRight" => BottomRight,
        _ => BottomCenter,
    }
}

/// The language to pass to the ASR engine. Defaults to English when the
/// setting is empty or "auto", since auto-detect is unreliable for short
/// utterances and the user's default is English.
pub fn effective_language(language: String, _english_only: bool) -> String {
    if language.is_empty() || language == "auto" {
        "en".into()
    } else {
        language
    }
}

/// Transliterates Cyrillic characters to their Latin equivalents.
///
/// The Parakeet TDT model supports 25 languages and can misidentify English
/// proper nouns as Russian (e.g. "Rida Fatema" → "Рида Фатема"). When the
/// user's language setting is English, any Cyrillic in the transcript is a
/// mis-detection and should be transliterated back to Latin.
fn transliterate_cyrillic(text: &str) -> String {
    const MAP: &[(&str, &str)] = &[
        ("а", "a"), ("б", "b"), ("в", "v"), ("г", "g"), ("д", "d"),
        ("е", "e"), ("ё", "yo"), ("ж", "zh"), ("з", "z"), ("и", "i"),
        ("й", "y"), ("к", "k"), ("л", "l"), ("м", "m"), ("н", "n"),
        ("о", "o"), ("п", "p"), ("р", "r"), ("с", "s"), ("т", "t"),
        ("у", "u"), ("ф", "f"), ("х", "kh"), ("ц", "ts"), ("ч", "ch"),
        ("ш", "sh"), ("щ", "shch"), ("ъ", ""), ("ы", "y"), ("ь", ""),
        ("э", "e"), ("ю", "yu"), ("я", "ya"),
        ("А", "A"), ("Б", "B"), ("В", "V"), ("Г", "G"), ("Д", "D"),
        ("Е", "E"), ("Ё", "Yo"), ("Ж", "Zh"), ("З", "Z"), ("И", "I"),
        ("Й", "Y"), ("К", "K"), ("Л", "L"), ("М", "M"), ("Н", "N"),
        ("О", "O"), ("П", "P"), ("Р", "R"), ("С", "S"), ("Т", "T"),
        ("У", "U"), ("Ф", "F"), ("Х", "Kh"), ("Ц", "Ts"), ("Ч", "Ch"),
        ("Ш", "Sh"), ("Щ", "Shch"), ("Ъ", ""), ("Ы", "Y"), ("Ь", ""),
        ("Э", "E"), ("Ю", "Yu"), ("Я", "Ya"),
    ];

    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if let Some((_, latin)) = MAP.iter().find(|(c, _)| *c == ch.to_string()) {
            out.push_str(latin);
        } else {
            out.push(ch);
        }
    }
    // Collapse any double spaces that result from dropping ъ/ь.
    let mut result = String::with_capacity(out.len());
    let mut prev_space = false;
    for ch in out.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(ch);
            }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }
    result.trim().to_string()
}

/// If the language is English and the transcript contains Cyrillic,
/// transliterate it back to Latin. This handles the case where the ASR
/// model misidentifies an English proper noun as Russian.
fn ensure_latin_if_english(text: &str, language: &str) -> String {
    if language == "en" && text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
        transliterate_cyrillic(text)
    } else {
        text.to_string()
    }
}

// Thread-local storage for the active recording (one at a time).
thread_local! {
    static RECORDING: std::cell::RefCell<Option<Recording>> = const { std::cell::RefCell::new(None) };
}

/// Runs on a worker thread: loads the speech model and transcribes.
pub fn transcribe(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transliterates_cyrillic_name() {
        assert_eq!(transliterate_cyrillic("Рида Фатема"), "Rida Fatema");
    }

    #[test]
    fn transliteration_preserves_latin() {
        assert_eq!(transliterate_cyrillic("Hello world"), "Hello world");
    }

    #[test]
    fn transliteration_handles_mixed_script() {
        assert_eq!(transliterate_cyrillic("Рида hello Фатема"), "Rida hello Fatema");
    }

    #[test]
    fn transliteration_drops_hard_soft_signs() {
        assert_eq!(transliterate_cyrillic("объём"), "obyom");
    }

    #[test]
    fn ensure_latin_transliterates_when_english() {
        assert_eq!(ensure_latin_if_english("Рида Фатема", "en"), "Rida Fatema");
    }

    #[test]
    fn ensure_latin_preserves_when_not_english() {
        assert_eq!(ensure_latin_if_english("Рида Фатема", "ru"), "Рида Фатема");
    }

    #[test]
    fn ensure_latin_preserves_latin_text() {
        assert_eq!(ensure_latin_if_english("Rida Fatema", "en"), "Rida Fatema");
    }

    #[test]
    fn effective_language_defaults_to_english() {
        assert_eq!(effective_language("".into(), false), "en");
        assert_eq!(effective_language("auto".into(), false), "en");
        assert_eq!(effective_language("ru".into(), false), "ru");
        assert_eq!(effective_language("en".into(), true), "en");
    }
}
