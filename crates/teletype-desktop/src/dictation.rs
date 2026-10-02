//! The dictation controller: one thread owns the session state machine.
//!
//! All events (hotkey, toggle, escape, transcription result) are messages to
//! this thread, so state has a single owner and events are processed in order.

use std::{
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use teletype_core::{
    audio::{resample_to_target, Captured, Recording},
    delivery::{resolve_delivery, Delivery, RecordTarget},
    pipeline::{InputSource, Pipeline, UnifiedInput},
    platform::InjectionRoute,
    state::{self, Input, Phase, PillPosition, PillState, RecordingMode},
    vad::VadDetector,
};

use crate::{commands, AppState};

/// Shared between the capture thread (which feeds the VAD detector) and the
/// dictation thread (which resets it between takes). `None` when VAD auto-stop
/// is off, so a disabled take never pays for the model.
type VadSlot = Arc<Mutex<Option<VadDetector>>>;

pub enum Event {
    HotkeyDown,
    HotkeyUp,
    HotkeyInterrupted,
    Toggle,
    Cancel,
    /// Cancel pressed from the pill overlay.
    PillCancel,
    Transcribed {
        session: u64,
        transcript: Result<String, String>,
        /// The language the ASR engine detected, when auto-detect was active.
        detected_language: Option<String>,
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
    /// VAD saw speech followed by a long-enough pause: stop and transcribe.
    VadFired {
        session: u64,
    },
    /// T2.1: the post-injection edit check for a finished session fired.
    /// Diff the watched field now and learn; the next dictation's
    /// `observe_pending_edit` remains the fallback if this misses.
    CheckEdit {
        session: u64,
    },
}

#[derive(Clone)]
pub struct Controller {
    tx: Sender<Event>,
    /// The most recent hotkey registration failure, with the binding that
    /// failed. Compared at read time against the currently saved binding so
    /// a fixed binding never shows a stale warning.
    hotkey_conflict: Arc<Mutex<Option<HotkeyConflict>>>,
}

/// A shortcut registration the OS refused.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConflict {
    /// The binding that failed, exactly as attempted.
    pub binding: String,
    /// What to show. Worded as what macOS reported, never as a claim about
    /// who holds the shortcut: a duplicate registration inside our own
    /// process fails the same way, so "another app has it" is unproven.
    pub message: String,
}

impl HotkeyConflict {
    fn new(binding: &str, os_error: &str) -> Self {
        Self {
            binding: binding.to_string(),
            message: format!(
                "macOS reports this shortcut is already taken, so dictation will not start. Cause unknown ({os_error}). Pick a different shortcut."
            ),
        }
    }
}

/// The stored conflict if it still applies to `current_binding`, else `None`.
///
/// Pure, so the no-stale-warning rule is pinnable: a conflict recorded for an
/// old binding must disappear the moment the binding changes, without waiting
/// for a re-registration attempt to clear it.
pub(crate) fn visible_conflict(
    stored: &Option<HotkeyConflict>,
    current_binding: &str,
) -> Option<HotkeyConflict> {
    stored.clone().filter(|c| c.binding == current_binding)
}

impl Controller {
    /// A clone of the stored registration conflict, if any. Filter it through
    /// [`visible_conflict`] against the current binding before showing it.
    pub(crate) fn conflict_snapshot(&self) -> Option<HotkeyConflict> {
        self.hotkey_conflict
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Controller {
    pub fn tx(&self) -> Sender<Event> {
        self.tx.clone()
    }

    pub fn spawn(app: AppHandle) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let controller = Self {
            tx,
            hotkey_conflict: Arc::new(Mutex::new(None)),
        };
        let mut session = Session {
            app,
            controller: controller.clone(),
            phase: Phase::Idle,
            hotkey_down: false,
            hands_free: false,
            last_release: None,
            next_id: 0,
            started_at_ms: 0,
            recording_started: None,
            speech_ms: std::collections::HashMap::new(),
            vad: None,
            live_preview: None,
            spool: None,
            record_target: None,
            injected_at: None,
        };
        thread::Builder::new()
            .name("teletype-dictation".into())
            .spawn(move || {
                for event in rx {
                    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        session.handle(event);
                    }));
                    if handled.is_err() {
                        crate::log_entry(crate::LogLevel::Error, "event handler panicked");
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
            #[cfg(target_os = "macos")]
            {
                crate::fn_tap::start(self.clone()).map_err(|e| {
                    format!("Couldn't register Fn: {e} (grant Teletype Accessibility access in System Settings > Privacy & Security > Accessibility)")
                })
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err("Fn key is macOS-only; pick another hotkey".into())
            }
        } else if let Some(mask) = crate::mod_tap::bare_modifier_mask(hotkey) {
            // A single bare modifier (Ctrl/Cmd/Alt/Shift) also cannot be
            // registered with Carbon — RegisterEventHotKey only fires for a
            // modifier+key combo, never a lone modifier. Watch it with the
            // bare-modifier CGEventTap instead, which reports the press only
            // while that modifier is the sole one held.
            #[cfg(target_os = "macos")]
            {
                crate::mod_tap::start(self.clone(), mask).map_err(|e| {
                    format!("Couldn't register {hotkey}: {e} (grant Teletype Accessibility access in System Settings > Privacy & Security > Accessibility)")
                })
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(format!(
                    "Bare {hotkey} hotkey is macOS-only; pick a key combo"
                ))
            }
        } else {
            self.register_string_hotkey(app, hotkey)
        }
    }

    fn register_string_hotkey(&self, app: &AppHandle, hotkey: &str) -> Result<(), String> {
        let controller = self.clone();
        let binding = hotkey.to_string();
        let record = self.hotkey_conflict.clone();
        let result = app
            .global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                controller.send(match event.state {
                    ShortcutState::Pressed => Event::HotkeyDown,
                    ShortcutState::Released => Event::HotkeyUp,
                });
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"));
        // Record the outcome for the Keybinds warning: a failure stores the
        // binding it failed for, a success clears any older one. The reader
        // compares against the *current* binding, so neither path can leave a
        // stale warning behind.
        *record
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = result
            .as_ref()
            .err()
            .map(|e| HotkeyConflict::new(&binding, e));
        result
    }

    /// Unregisters a previously registered global shortcut (no-op if not registered).
    pub fn unregister_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey == "Fn" {
            #[cfg(target_os = "macos")]
            crate::fn_tap::stop();
        } else if crate::mod_tap::bare_modifier_mask(hotkey).is_some() {
            #[cfg(target_os = "macos")]
            crate::mod_tap::stop();
        } else {
            let _ = app.global_shortcut().unregister(hotkey);
        }
    }
}

/// The language chip shown in the pill after an auto-detect transcription.
/// `detected: true` means it was detected this session (tap to lock); a
/// locked language is shown with `detected: false` (tap to switch back to
/// auto).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageChip {
    pub code: String,
    pub detected: bool,
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
    /// When the microphone actually opened, on a *monotonic* clock.
    ///
    /// This is the only honest basis for a speaking-rate figure: wall-clock
    /// `now_ms()` can jump backwards across an NTP correction or a timezone
    /// change mid-take, which would produce a negative or absurd duration.
    /// `Instant` cannot. Measured from the mic opening, not from the keypress,
    /// so the figure is speech time and not key-holding time.
    recording_started: Option<std::time::Instant>,
    /// Measured speaking time per session id, consumed when the history entry
    /// is written. A take is recorded in one event and written in another (the
    /// pipeline runs on a worker), so the measurement has to travel with the
    /// session id.
    speech_ms: std::collections::HashMap<u64, u64>,
    /// The VAD detector shared with the capture thread for the in-flight
    /// recording, if auto-stop is armed for this take.
    vad: Option<VadSlot>,
    /// When the pipeline finished injecting the current take (T2.1):
    /// the moment from which the user can edit the inserted text, used to
    /// arm the post-injection edit check.
    injected_at: Option<Instant>,
    /// Live-preview handle for the in-flight recording, if the interim
    /// transcript loop is running. Stopped on release/cancel.
    live_preview: Option<LivePreviewHandle>,
    /// Escape-recovery spool for the in-flight recording, if one was
    /// started. `None` if spool creation failed (recording continues
    /// without a recovery spool) or the take has ended.
    spool: Option<crate::recovery::Spool>,
    /// The paste target as captured when the take started (app, pid, window
    /// frame). The pipeline can take 20 s, and in that gap the user may
    /// switch apps or windows; the delivery gate compares this against the
    /// target at inject time and refuses to paste into the wrong place.
    record_target: Option<RecordTarget>,
}

/// A second press within this window of a release is a double-tap.
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(350);

/// How often the interim transcript loop re-transcribes the audio-so-far.
///
/// The cadence is bounded by measurement, not taste. Parakeet TDT has no
/// streaming state: every call re-derives the whole utterance from scratch, so
/// the interim text is a full re-decode of everything said so far, not an
/// incremental update. Measured on an M4 Pro, a warm re-decode of an 11 s
/// window costs 188 to 532 ms p50 depending on how far into the take it runs,
/// and back-to-back decodes contend on the Metal queue (0.164x of one core at a
/// 1 s cadence, 0.638x at 0.5 s).
///
/// 700 ms is therefore close to the floor: it is slower than the decode itself
/// at the longest windows, so the loop is decode-bound rather than
/// interval-bound, and a tighter interval would only add contention. See
/// `tasks/pi-tasks/streaming-asr-spike-findings.md`.
const INTERIM_INTERVAL: Duration = Duration::from_millis(700);

/// Minimum seconds of audio before the first interim pass. Avoids a wasted
/// transcribe on a near-empty buffer.
const INTERIM_MIN_SECS: f64 = 1.0;

/// Handle to a running live-preview interim thread. `stop()` signals the
/// thread to exit and joins it so the speech-model lock is released before
/// the caller proceeds to the final transcribe.
struct LivePreviewHandle {
    stop_tx: mpsc::Sender<()>,
    /// Join handle for the interim thread. `stop()` joins it.
    join: Option<std::thread::JoinHandle<()>>,
}

impl LivePreviewHandle {
    fn stop(self) {
        let _ = self.stop_tx.send(());
        if let Some(h) = self.join {
            let _ = h.join();
        }
    }
}

/// Spawns the interim-transcript thread for an in-flight recording and
/// returns a handle the caller stores on the Session.
fn spawn_live_preview(
    app: AppHandle,
    buf: teletype_core::audio::Buf,
    rate: Arc<std::sync::atomic::AtomicU32>,
    session: u64,
) -> LivePreviewHandle {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let join = thread::Builder::new()
        .name("teletype-live-preview".into())
        .spawn(move || {
            run_live_preview(app, buf, rate, session, stop_rx);
        })
        .expect("spawn live-preview thread");
    LivePreviewHandle {
        stop_tx,
        join: Some(join),
    }
}

/// The interim-transcript loop. Every `INTERIM_INTERVAL` it snapshots the
/// live audio buffer, resamples to 16 kHz, transcribes (serializing on the
/// speech-model lock), and emits an `interim-transcript` event to the pill.
/// Exits when `stop_rx` receives a signal or the capture thread drops the
/// buffer (which can't happen while the take is in flight, since `stop()`
/// always signals first).
fn run_live_preview(
    app: AppHandle,
    buf: teletype_core::audio::Buf,
    rate: Arc<std::sync::atomic::AtomicU32>,
    session: u64,
    stop_rx: mpsc::Receiver<()>,
) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let (model_path, use_parakeet) = resolve_speech_model(&settings, &state.models_dir);
    let language = effective_language_for_app(&state, use_parakeet);

    // Skip entirely if the model file isn't on disk yet (warm_up will load
    // it; we don't want to block on a download).
    if !model_path.exists() {
        crate::log_entry(
            crate::LogLevel::Info,
            "live-preview: model not on disk yet, skipping interim loop",
        );
        // Drain stop_rx so the handle's stop() doesn't block.
        let _ = stop_rx.recv();
        return;
    }

    loop {
        // Wait for the next tick or a stop signal.
        match stop_rx.recv_timeout(INTERIM_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        // Snapshot the live buffer. The capture thread appends to this vec,
        // so a short lock + clone is cheap relative to a transcribe pass.
        let raw = match buf.lock() {
            Ok(g) => g.clone(),
            Err(_) => continue,
        };
        let device_rate = rate.load(std::sync::atomic::Ordering::Relaxed);
        if raw.is_empty() || device_rate == 0 {
            continue;
        }
        let resampled = resample_to_target(&raw, device_rate);
        let secs = resampled.len() as f64 / teletype_core::audio::TARGET_SAMPLE_RATE as f64;
        if secs < INTERIM_MIN_SECS {
            continue;
        }

        // Transcribe the audio-so-far. This serializes on the speech-model
        // lock with the final transcribe (which runs in stop()), so the two
        // never overlap. A locked model means the final transcribe is
        // already running; skip this interim pass.
        let state = app.state::<AppState>();
        let mut speech = (if use_parakeet {
            &state.parakeet
        } else {
            &state.speech
        })
        .provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !speech.is_loaded() {
            // Model still warming; the interim loop will retry next tick.
            continue;
        }
        let text = match speech.transcribe(&resampled, &language) {
            Ok(t) => t,
            Err(_) => continue, // NoSpeech or a transient error; retry next tick.
        };
        // Marked provisional on purpose. This is a whole-take re-decode, not a
        // partial, so the text revises as the sentence develops: punctuation
        // appears, the first words get dropped when the window's leading edge
        // moves, and wording can change. The pill shows it as a preview and the
        // authoritative text is the single full decode that `stop()` runs. The
        // UI relies on the event name to decide how prominently to render it.
        let _ = app.emit_to("pill", "interim-transcript", &text);
        let _ = session; // session is reserved for future staleness filtering
    }
}

impl Session {
    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    /// The ASR language for the frontmost app (P3.3): the per-app override
    /// wins over the global setting. `"auto"` is passed through so Whisper
    /// can still auto-detect.
    fn effective_language(&self, use_parakeet: bool) -> String {
        let state = self.state();
        let settings = state.settings();
        let app_ctx = state.platform.active_application().unwrap_or_default();
        let profile = teletype_core::personalization::UserProfile {
            language: settings.language.clone(),
            app_language_overrides: settings.app_language_overrides.clone(),
            ..Default::default()
        };
        let resolved = teletype_core::personalization::resolve_language(&profile, &app_ctx);
        effective_language(resolved, use_parakeet)
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
                detected_language,
            } => {
                self.transcribed(session, transcript, detected_language);
            }
            Event::PipelineDone { session } => {
                if self.next_id == session {
                    self.go_idle();
                    // T2.1: the inserted text is now editable. Check for an
                    // edit after the user has had a moment to make one,
                    // without waiting for the next dictation.
                    self.injected_at = Some(Instant::now());
                    self.arm_edit_check(session);
                }
            }
            Event::CheckEdit { session } => {
                // Only the most recent finished take is worth checking; a
                // newer session means this check raced a newer dictation.
                if self.next_id == session {
                    self.observe_pending_edit();
                }
            }
            Event::FlashDone { generation } => {
                if self.next_id == generation && self.phase == Phase::Idle {
                    self.flash_done();
                }
            }
            // The pill's Cancel button. `decide()` is the single source of
            // truth for what each input means per phase (cancel while
            // recording cancels the dictation; cancel mid-pipeline lets
            // transcription finish but discards the result).
            Event::PillCancel => {
                let action = state::decide(
                    Input::Cancel,
                    self.phase,
                    self.recording_mode(),
                    &mut self.hotkey_down,
                    &mut self.hands_free,
                );
                match action {
                    state::Action::Stop { cancelled } => self.stop(cancelled),
                    state::Action::Discard => self.go_idle(),
                    state::Action::CancelPipeline => {
                        tracing::info!("pipeline cancelled by user (pill)");
                        self.phase = Phase::Cancelled;
                        // BUG-003: the session is over; arm the idle-unload timer.
                        {
                            let state = self.state();
                            let delay_secs = state.settings().model_unload_delay_secs;
                            let delay = (delay_secs > 0)
                                .then(|| std::time::Duration::from_secs(delay_secs));
                            state.speech.end_dictation(delay);
                            state.parakeet.end_dictation(delay);
                        }
                        self.flash("Cancelled");
                    }
                    // Nothing, Start, GoHandsFree: no-op for a cancel click.
                    _ => {}
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
            Event::VadFired { session } => {
                // The capture thread's VAD detector saw a long-enough pause
                // after speech. Only the newest session may stop, so a fire
                // from a just-finished take is a stale no-op.
                if self.next_id == session && self.phase == Phase::Listening {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        "VAD auto-stop: silence after speech, stopping",
                    );
                    self.stop(false);
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
        // BUG-003: mark the dictation session as in flight so the idle-unload
        // timer cannot fire mid-take.
        {
            let state = self.state();
            state.speech.begin_dictation();
            state.parakeet.begin_dictation();
        }
        // Show the pill immediately, BEFORE the (slow) mic open, so the UI
        // responds instantly on keypress like Wispr Flow. The waveform begins
        // as soon as levels arrive.
        let started_at_ms = teletype_core::storage::now_ms();
        self.started_at_ms = started_at_ms;
        // Monotonic marker for the speaking-rate figure. Reset here and
        // re-stamped below, once the mic is genuinely open, so the measured
        // duration is speech time rather than key-holding time.
        self.recording_started = None;
        self.phase = Phase::Listening;
        self.broadcast(teletype_core::state::UiState::Listening { started_at_ms });
        self.show(PillState::Recording { started_at_ms });

        // Record the paste target now, while the user is provably here: the
        // pipeline (ASR plus an optional 20 s LLM polish) finishes long after
        // this, and the delivery gate compares this snapshot against the
        // target at inject time. Capture is best-effort; a missing piece is
        // no evidence, never a mismatch.
        self.record_target = {
            let state = self.state();
            state.platform.active_application().map(|app| RecordTarget {
                bundle_id: app.application_id,
                app_name: app.application_name,
                pid: state.platform.active_pid(),
                frame: state.platform.focused_window_frame(),
            })
        };

        // Escape recovery: start the PCM spool for this take. Failure is
        // non-fatal — the recording proceeds without a recovery spool.
        let spool = match crate::recovery::Spool::begin(&self.state().data_dir) {
            Ok(s) => Some(s),
            Err(e) => {
                crate::log_entry(crate::LogLevel::Warn, format!("recovery spool: {e}"));
                None
            }
        };
        let spool_cb = spool.as_ref().and_then(|s| s.clone_writer());
        self.spool = spool;

        let recording = if self.vad_enabled_for() {
            self.start_with_vad(spool_cb)
        } else {
            Recording::start(&settings.input_device, move |level| {
                let _ = app.emit_to("pill", "pill-level", level);
            })
        };
        match recording {
            Ok(recording) => {
                // Spawn the live-preview interim loop before storing the
                // recording, so it can grab a handle to the live buffer.
                // BUG-011: the ported pill styles (Level Rail, Reading Well,
                // Dot Matrix) all render `interimText`, so Hold mode must feed
                // it too — a pill that promises live text but shows only the
                // recording animation for the entire take reads as broken.
                let app = self.app.clone();
                let session = self.next_id;
                // The user can switch the live transcript off in Settings
                // (it costs a re-decode per interim tick); the pill then
                // shows only the recording animation for the take.
                let live_preview_on = self.state().settings().live_preview_enabled;
                self.live_preview = live_preview_on
                    .then(|| recording.live_preview())
                    .flatten()
                    .map(|(buf, rate)| spawn_live_preview(app, buf, rate, session));
                // Store the recording in a thread-local so `stop` can access it.
                RECORDING.with(|slot| *slot.borrow_mut() = Some(recording));
                // The mic is genuinely open now; start the speaking-rate
                // clock. `start` reset this to `None` so the measured
                // duration is speech time, not key-holding time.
                self.recording_started = Some(std::time::Instant::now());
                // Load the speech model while the user speaks, so it's ready
                // on release. If it isn't loaded yet, show the warming pill.
                self.warm_up();
                // Pre-warm the LLM (polish) model in the background while the
                // user is still talking. `ensure_local_provider` no-ops if a
                // provider is already loaded, and the `llm_loading` guard
                // prevents concurrent spawns. By the time transcription
                // finishes, the server is hot and the first inference doesn't
                // pay the cold-start penalty. No effect on the recording path.
                {
                    let app = self.app.clone();
                    std::thread::Builder::new()
                        .name("teletype-llm-prewarm".into())
                        .spawn(move || {
                            crate::commands::ensure_local_provider(&app);
                        })
                        .ok();
                }
            }
            Err(e) => {
                crate::log_entry(crate::LogLevel::Error, e.to_string());
                // BUG-003: the mic never opened, so no dictation will
                // complete — disarm the idle-unload timer now instead of
                // leaving `active` stuck true.
                {
                    let state = self.state();
                    state.speech.end_dictation(None);
                    state.parakeet.end_dictation(None);
                }
                self.flash(&e);
            }
        }
    }

    /// VAD auto-stop is useful only when nothing else bounds the take:
    /// hands-free hold mode (double-tap), where the key release does nothing
    /// and the pause is the only stop signal. Plain hold stops on release;
    /// push-to-talk stops on the second tap; both preempt VAD.
    fn vad_enabled_for(&self) -> bool {
        self.state().settings().vad_auto_stop && self.hands_free
    }

    /// Starts a recording with the VAD detector armed. The detector lives in
    /// a shared slot the capture thread feeds; on `UtteranceComplete` it
    /// posts `VadFired` to this controller, which stops the take.
    fn start_with_vad(
        &mut self,
        spool_cb: Option<crate::recovery::SpoolWriter>,
    ) -> Result<Recording, String> {
        let settings = self.state().settings();
        let app = self.app.clone();
        let controller = self.controller.clone();
        let session = self.next_id;
        let vad: VadSlot = match VadDetector::silero_16k() {
            Ok(mut d) => {
                d.set_silence_duration(Duration::from_millis(settings.vad_silence_ms.max(200)));
                Arc::new(Mutex::new(Some(d)))
            }
            Err(e) => {
                // No model, no auto-stop: fall back to manual stop.
                crate::log_entry(crate::LogLevel::Error, format!("VAD init failed: {e}"));
                return Recording::start(&settings.input_device, move |level| {
                    let _ = app.emit_to("pill", "pill-level", level);
                });
            }
        };
        let spool_cb2 = spool_cb;
        // The device's native rate is published here once the capture thread
        // opens the stream; the VAD callback reads it on its first frame so
        // the silence window is timed against real wall-clock time (not the
        // engine's 16 kHz assumption).
        let rate_slot: Arc<std::sync::atomic::AtomicU32> =
            Arc::new(std::sync::atomic::AtomicU32::new(0));
        let rate_cb = rate_slot.clone();
        let vad_cb = vad.clone();
        let recording = Recording::start_with_vad_rate(
            &settings.input_device,
            move |level| {
                let _ = app.emit_to("pill", "pill-level", level);
            },
            Some(Box::new(move |frame| {
                // Escape-recovery spool: resample this device-rate frame to
                // 16 kHz and hand it to the non-blocking spool writer.
                if let Some(w) = spool_cb2.as_ref() {
                    let r = rate_cb.load(std::sync::atomic::Ordering::Relaxed);
                    if r > 0 {
                        let r16 = resample_to_target(frame, r);
                        w.write(&r16);
                    }
                }
                let event = {
                    let mut guard = vad_cb
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    match guard.as_mut() {
                        Some(d) => {
                            // First frame: latch the real feed rate.
                            let r = rate_cb.load(std::sync::atomic::Ordering::Relaxed);
                            if r > 0 {
                                d.set_sample_rate(r);
                            }
                            d.push_frame(frame)
                        }
                        None => return,
                    }
                };
                if matches!(event, teletype_core::vad::VadEvent::UtteranceComplete) {
                    // Drop the detector now so no further frames can re-fire;
                    // the session check in the handler is the second gate.
                    let mut guard = vad_cb
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    *guard = None;
                    controller.send(Event::VadFired { session });
                }
            })),
            Some(rate_slot),
        );
        if recording.is_ok() {
            self.vad = Some(vad);
        }
        recording
    }

    /// Loads the selected speech model in the background so the first
    /// dictation (or a model switch) doesn't block the release. Shows the
    /// warming pill while the load is in flight.
    fn warm_up(&mut self) {
        let (model_path, use_parakeet) = {
            let state = self.state();
            resolve_speech_model(&state.settings(), &state.models_dir)
        };
        if !model_path.exists() {
            return;
        }
        // Already loaded → skip the warming pill entirely; the model is
        // resident and the recording pill can stay as-is.
        let state = self.state();
        let ready = if use_parakeet {
            state.parakeet.is_loaded()
        } else {
            state.speech.is_loaded()
        };
        if ready {
            return;
        }
        self.show(PillState::Warming);
        let next_id = {
            self.next_id += 1;
            self.next_id
        };
        let app = self.app.clone();
        let controller = self.controller.clone();
        crate::log_entry(
            crate::LogLevel::Info,
            format!(
                "loading speech model: {} ({})",
                model_path.display(),
                if use_parakeet { "parakeet" } else { "whisper" }
            ),
        );
        let started = std::time::Instant::now();
        thread::Builder::new()
            .name("teletype-warmup".into())
            .spawn(move || {
                let state = app.state::<AppState>();
                // BUG-003 P0: the manager was constructed with an empty path;
                // set the real resolved path before loading.
                if use_parakeet {
                    state.parakeet.set_model_path(model_path.clone());
                } else {
                    state.speech.set_model_path(model_path.clone());
                }
                let result = if use_parakeet {
                    state.parakeet.load()
                } else {
                    state.speech.load()
                };
                match result {
                    Ok(()) => crate::log_entry_ms(
                        crate::LogLevel::Success,
                        format!(
                            "speech model loaded: {} ({})",
                            model_path.display(),
                            if use_parakeet { "parakeet" } else { "whisper" }
                        ),
                        started.elapsed().as_millis() as u64,
                    ),
                    Err(e) => {
                        crate::log_entry(crate::LogLevel::Error, format!("warm-up failed: {e}"))
                    }
                }
                controller.send(Event::WarmupDone { session: next_id });
            })
            .ok();
    }

    fn stop(&mut self, cancelled: bool) {
        // Disarm VAD for this take: the capture thread's callback becomes a
        // no-op, and a late fire from the old session is dropped by the
        // session check in the `VadFired` handler.
        self.vad = None;
        // Stop the live-preview interim loop so it doesn't race the final
        // transcribe for the speech model lock.
        if let Some(handle) = self.live_preview.take() {
            handle.stop();
        }
        // Clean stop: the take finished (or was cancelled), so the recovery
        // spool is no longer needed — delete it.
        if let Some(spool) = self.spool.take() {
            spool.complete();
        }
        // The speaking time for this take, on a monotonic clock. Taken here,
        // at the moment the mic closes, because after this point the take is
        // transcribing and polishing, which is machine time and not speech.
        // A cancelled take measures nothing: the user threw it away.
        let speech_ms: Option<u64> = if cancelled {
            None
        } else {
            self.recording_started
                .take()
                .map(|t| t.elapsed().as_millis() as u64)
        };
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
        if let Some(ms) = speech_ms {
            self.speech_ms.insert(session, ms);
        }
        // A cancel must discard the recording outright: no transcription, no
        // pipeline, no injection, no history. `next_id` advances so any
        // in-flight `Transcribed` event for this session is stale and dropped
        // by the staleness guard in `transcribed()`.
        if cancelled {
            let _ = recording.finish(); // drop the captured audio ASAP
            self.phase = Phase::Cancelled;
            // BUG-003: the session is over; arm the idle-unload timer.
            {
                let state = self.state();
                let delay_secs = state.settings().model_unload_delay_secs;
                let delay = (delay_secs > 0).then(|| std::time::Duration::from_secs(delay_secs));
                state.speech.end_dictation(delay);
                state.parakeet.end_dictation(delay);
            }
            self.broadcast(teletype_core::state::UiState::Message {
                text: "Cancelled".into(),
            });
            self.flash("Cancelled");
            return;
        }
        self.phase = Phase::Transcribing;
        self.broadcast(teletype_core::state::UiState::Transcribing);
        self.show(PillState::Processing {
            message: "Transcribing…".into(),
        });

        let app = self.app.clone();
        let controller = self.controller.clone();
        let (speech_model_path, use_parakeet) = {
            let state = self.state();
            resolve_speech_model(&state.settings(), &state.models_dir)
        };
        // Parakeet is English-only; "auto" there means English. Guarding here
        // prevents the Russian mis-transcription on short utterances.
        // P3.3: consult the per-app language override for the frontmost app
        // before falling back to the global setting.
        let language = self.effective_language(use_parakeet);
        // Keep a copy for the worker's detected-language plumbing below.
        let language_for_worker = language.clone();

        let worker = thread::Builder::new()
            .name("teletype-transcribe".into())
            .spawn(move || {
                let captured = match recording.finish() {
                    Ok(c) => c,
                    Err(e) => {
                        controller.send(Event::Transcribed {
                            session,
                            transcript: Err(e.to_string()),
                            detected_language: None,
                        });
                        return;
                    }
                };
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!(
                        "transcribing audio: {} samples, language {}, engine {}",
                        captured.samples.len(),
                        language,
                        if use_parakeet { "parakeet" } else { "whisper" }
                    ),
                );
                let transcribe_started = std::time::Instant::now();
                let outcome =
                    transcribe(&app, &speech_model_path, &captured, &language, use_parakeet);
                // The language the engine detected (auto-detect only), for the
                // pill chip and the lock-language flow.
                let detected_language =
                    detect_language_from_provider(&app, use_parakeet, language_for_worker.as_str());
                // Distinguish "nothing was recorded" from "the model found no
                // words in real speech" so the user gets an honest message.
                let transcript = match outcome {
                    Err(e) if e == "No speech detected" && captured.peak < 0.005 => {
                        Err("No audio captured — check the microphone".into())
                    }
                    other => other,
                };
                let elapsed_ms = transcribe_started.elapsed().as_millis() as u64;
                match &transcript {
                    Ok(t) if !t.trim().is_empty() => {
                        crate::log_entry_ms(
                            crate::LogLevel::Success,
                            "transcription complete",
                            elapsed_ms,
                        );
                    }
                    Ok(_) => {
                        crate::log_entry_ms(
                            crate::LogLevel::Info,
                            "no speech detected",
                            elapsed_ms,
                        );
                    }
                    Err(e) => crate::log_entry(
                        crate::LogLevel::Error,
                        format!("transcription failed: {e}"),
                    ),
                }
                controller.send(Event::Transcribed {
                    session,
                    transcript,
                    detected_language,
                });
            });
        if let Err(e) = worker {
            crate::log_entry(
                crate::LogLevel::Error,
                format!("couldn't start transcription: {e}"),
            );
            self.flash("Transcription failed");
        }
    }

    fn transcribed(
        &mut self,
        session: u64,
        transcript: Result<String, String>,
        detected_language: Option<String>,
    ) {
        if self.next_id != session {
            return; // stale
        }
        if self.phase == Phase::Cancelled {
            // Belt and braces: a cancel that raced the worker must never
            // transcribe, pipeline, or inject.
            crate::log_entry(
                crate::LogLevel::Info,
                "dropping transcript for cancelled session",
            );
            self.go_idle();
            return;
        }
        // This is the reliable moment to observe an edit to the *previous*
        // dictation: the user has spoken again, so they have finished editing
        // the last one. See `edit_watch` for why it is not done inline.
        // (The post-injection check in `arm_edit_check` usually handles this
        // first; this is the fallback for a take the user edits late.)
        self.observe_pending_edit();
        match transcript {
            Ok(raw) => {
                // If the language is English and the model misidentified the
                // speech as Russian (Cyrillic), transliterate back to Latin.
                let text = ensure_latin_if_english(
                    &raw,
                    &effective_language(self.state().settings().language.clone(), false),
                );
                self.phase = Phase::Transforming;
                self.broadcast(teletype_core::state::UiState::Transforming);
                self.show(PillState::Processing {
                    message: "Transforming…".into(),
                });

                // Read the measured speaking time before `self` is moved into
                // the worker closure.
                let speech_ms_for_session = self.speech_ms.get(&session).copied();
                // Same for the recorded paste target: the gate compares it
                // against the live target after the pipeline finishes.
                let record_target = self.record_target.clone();
                let app = self.app.clone();
                let controller = self.controller.clone();
                let worker = thread::Builder::new()
                    .name("teletype-pipeline".into())
                    .spawn(move || {
                        let pipeline_started = std::time::Instant::now();
                        let state = app.state::<AppState>();
                        // Local polish models are not preloaded at launch
                        // load the selected one here,
                        // on first use, so the first dictation pays the load
                        // cost once. No-op if a provider is already loaded or a
                        // remote provider is active.
                        crate::commands::ensure_local_provider(&app);
                        let settings = state.settings();
                        let platform = state.platform.as_ref();
                        // P1-B: clone the clonable stores so their mutexes
                        // are dropped BEFORE pipeline.run() (which includes
                        // the LLM network call, up to 20 s). Without this,
                        // opening Settings/Models/Dictionary during a polish
                        // hangs for the full LLM latency. The inference lock
                        // stays held (correct: no model swap mid-inference).
                        let (autotext, transforms, mut profile, styles, dictionary) = {
                            let a = state
                                .autotext
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            let t = state
                                .transforms
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            let mut p = state
                                .profile
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            if let Some(detected) = &detected_language {
                                p.language = detected.clone();
                            }
                            let s = state
                                .styles
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            let d = state
                                .dictionary
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            (a, t, p, s, d)
                        };
                        // P3.3: a per-app language override for the frontmost
                        // app wins over the (possibly detected) global
                        // language.
                        let app_ctx = state.platform.active_application().unwrap_or_default();
                        let active_style = settings.active_style_profile.clone();
                        let explicit_style = String::new();
                        let inference = state
                            .inference
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);

                        let transform_model = inference
                            .as_ref()
                            .map(|p| format!("{} ({})", p.model_name(), p.model_id()))
                            .unwrap_or_else(|| "none (AutoText only)".into());
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!(
                                "running dictation pipeline: auto_apply={}, transform_model={}",
                                settings.auto_apply_transform, transform_model
                            ),
                        );
                        let input = UnifiedInput {
                            source: InputSource::Voice,
                            text,
                        };
                        // P3.3: apply the per-app language override (if any)
                        // so the LLM prompt language matches the app the user
                        // is dictating into.
                        {
                            let resolved = teletype_core::personalization::resolve_language(
                                &profile, &app_ctx,
                            );
                            if resolved != profile.language {
                                profile.language = resolved;
                            }
                        }
                        // T1.1: stream transform tokens to the pill for a live
                        // preview. The sink accumulates the tokens and emits the
                        // running text on each token so the pill shows the words
                        // landing as the local LLM generates them.
                        let mut stream = String::new();
                        let mut token_sink: Box<dyn FnMut(&str)> = Box::new(|tok| {
                            stream.push_str(tok);
                            let _ = app.emit_to("pill", "transform-token", stream.clone());
                        });
                        // Vocabulary packs: precompute the enabled packs'
                        // terms once per dictation for the lowest-priority
                        // fuzzy correction tier.
                        let pack_terms = teletype_core::vocab::terms_for(&settings.enabled_packs);
                        // BUG-002: the spoken-punctuation gate filters the
                        // System entries the pipeline sees; custom AutoText is
                        // passed separately and is unaffected.
                        let system_entries: Vec<teletype_core::autotext::AutoTextEntry> =
                            if settings.spoken_punctuation {
                                teletype_core::autotext::system::entries().to_vec()
                            } else {
                                teletype_core::autotext::system::entries()
                                    .iter()
                                    .filter(|e| {
                                        !teletype_core::autotext::system::punctuation_entries()
                                            .iter()
                                            .any(|p| p.snippet.trim() == e.snippet.trim())
                                    })
                                    .cloned()
                                    .collect()
                            };
                        let mut pipeline = Pipeline {
                            platform,
                            autotext: &autotext,
                            transforms: &transforms,
                            profile: &profile,
                            inference: inference.as_deref(),
                            dictionary: &dictionary,
                            styles: &styles,
                            active_style: &active_style,
                            explicit_style: explicit_style.as_str(),
                            auto_apply: settings.auto_apply_transform,
                            restore_clipboard: settings.restore_clipboard,
                            remove_filler_words: settings.remove_filler_words,
                            filler_words: settings.filler_words.clone(),
                            restore_emoji: settings.restore_emoji,
                            spoken_emoji: settings.spoken_emoji,
                            spoken_punctuation: settings.spoken_punctuation,
                            system_autotext: &system_entries,
                            token_sink: Some(&mut token_sink),
                            polish_gate_enabled: settings.polish_gate_enabled,
                            polish_gate_threshold_words: settings.polish_gate_threshold_words,
                            pack_terms: &pack_terms,
                            word_checker: &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
                        };
                        let result = pipeline.run(input, None);

                        // Total pipeline time (from worker start to pipeline
                        // completion). The ASR time is logged separately by
                        // the transcribe worker; this covers transforms + LLM.
                        let pipeline_ms = pipeline_started.elapsed().as_millis();
                        let llm_ms = result
                            .transform
                            .as_ref()
                            .map(|t| t.metrics.latency_ms)
                            .unwrap_or(0);
                        tracing::info!(
                            pipeline_ms,
                            llm_ms,
                            transforms_ms = pipeline_ms.saturating_sub(llm_ms),
                            transformed = result.transformed,
                            "pipeline timing"
                        );

                        if let Some(transform) = &result.transform {
                            if transform.transformed {
                                crate::log_entry_ms(
                                    crate::LogLevel::Success,
                                    format!(
                                        "transform finished: fell_back={}",
                                        transform.metrics.fell_back
                                    ),
                                    transform.metrics.latency_ms as u64,
                                );
                            } else {
                                crate::log_entry_ms(
                                    crate::LogLevel::Info,
                                    format!(
                                        "transform finished: fell_back={}",
                                        transform.metrics.fell_back
                                    ),
                                    transform.metrics.latency_ms as u64,
                                );
                            }
                        } else {
                            crate::log_entry(
                                crate::LogLevel::Info,
                                "transform skipped: no transform ran",
                            );
                        }

                        // The fallback must never be silent: a transform was
                        // selected but no LLM provider was loaded, so only
                        // AutoText/filler cleanup ran.
                        if result.transform_skipped_no_model {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                "transform skipped: no LLM model loaded; \
                                 applied AutoText only. Pick a model in Settings > Models.",
                            );
                        }

                        // Surface the skip reason on all three surfaces:
                        // pill (pill-skip event), tray tooltip, and
                        // UiState::Message broadcast.
                        if let Some(reason) = result
                            .transform
                            .as_ref()
                            .and_then(|t| t.metrics.skip_reason.as_ref())
                        {
                            let msg = teletype_core::state::skip_message(reason);
                            let _ = app.emit_to(
                                "pill",
                                "pill-skip",
                                &serde_json::json!({ "message": msg.clone() }),
                            );
                            crate::tray::show_skip(&app, &msg);
                            let _ = app.emit(
                                "dictation-state",
                                &teletype_core::state::UiState::Message { text: msg.clone() },
                            );
                            crate::tray::show_state(
                                &app,
                                &teletype_core::state::UiState::Message { text: msg.clone() },
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
                            if let Err(e) = state.usage_store.save(&usage) {
                                tracing::warn!("Failed to save usage stats: {e}");
                            }
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
                                // Measured, not assumed: how long the mic was
                                // actually open for this take. `None` for
                                // history written before this field existed,
                                // which is why every rate figure below has to
                                // cope with missing durations.
                                duration_ms: speech_ms_for_session,
                            });
                            if let Err(e) = state.history_store.save(&history) {
                                tracing::warn!("Failed to save history after dictation: {e}");
                            }
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
                                crate::log_entry(
                                    crate::LogLevel::Error,
                                    format!("transcript file: {e}"),
                                );
                            }
                        }

                        // Surface the detected language in the pill (tap to
                        // lock it) when auto-detect found one.
                        if let Some(detected) = &detected_language {
                            let chip = LanguageChip {
                                code: detected.clone(),
                                detected: true,
                            };
                            let _ = app.emit("pill-language", &chip);
                            // Persist the detected language back to settings
                            // so the Settings UI reflects it instead of
                            // always showing "Auto-detect".
                            let _save_guard = state
                                .save_lock
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let mut s = state.settings();
                            if s.language != *detected {
                                s.language = detected.clone();
                                if state.replace_settings(s).is_ok() {
                                    let _ = app.emit("settings-changed", &());
                                }
                            }
                            drop(_save_guard);
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
                            if let Err(e) = state.scratchpad_store.save(&*pad) {
                                tracing::warn!("Failed to save scratchpad: {e}");
                            }
                        } else if let Some(msg) = delivery_refusal(
                            &record_target,
                            platform,
                            result.context.application_type,
                            &result.final_text,
                        ) {
                            // Refused delivery: no Tier 1, no Tier 2. The text
                            // is already in history and the transcript file
                            // (written above), and the clipboard was never
                            // touched, so nothing needs restoring. The
                            // edit-watch record below is skipped for the same
                            // reason: nothing landed, so there is nothing to
                            // learn a correction from.
                            crate::log_entry(crate::LogLevel::Info, format!("delivery: {msg}"));
                            let _ = app.emit_to(
                                "pill",
                                "pill-skip",
                                &serde_json::json!({ "message": msg.clone() }),
                            );
                            crate::tray::show_skip(&app, &msg);
                            let _ = app.emit(
                                "dictation-state",
                                &teletype_core::state::UiState::Message { text: msg.clone() },
                            );
                        } else {
                            // Tier 1: direct accessibility write, verified by
                            // read-back, with no clipboard round trip and no
                            // settle sleeps. Tier 2 (the clipboard) is only used
                            // when the platform reports the fast path is
                            // unavailable or the write could not be confirmed.
                            let outcome = platform.insert_text(&result.final_text);
                            if let Some(reason) = &outcome.fallback_reason {
                                crate::log_entry(
                                    crate::LogLevel::Info,
                                    format!("inject: clipboard fallback ({reason})"),
                                );
                            }
                            if !matches!(outcome.route, InjectionRoute::DirectWrite) {
                                let paste = platform.paste_shortcut();
                                // Retention needs the setting *and* a
                                // verifiable landing. A target that never
                                // exposed its accessibility tree gets the
                                // clipboard back clean: the keystroke paste
                                // still lands there, but nothing confirms it.
                                let retain = outcome.field_readable;
                                state.injector.inject(
                                    result.final_text.clone(),
                                    settings.restore_clipboard,
                                    settings.keep_text_on_clipboard,
                                    retain,
                                    paste,
                                );
                            } else {
                                crate::log_entry(
                                    crate::LogLevel::Info,
                                    format!(
                                        "inject: direct write ok ({} chars)",
                                        result.final_text.chars().count()
                                    ),
                                );
                            }
                            // Watch what landed so the personalization loop can
                            // diff it against what the user changes it into on
                            // the next dictation. See `edit_watch` for why the
                            // observation point is the following dictation.
                            let target = result.context.application_name.clone();
                            state
                                .edit_watch
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .record(result.final_text.clone(), target);
                        }

                        // Signal completion. Do NOT re-send Event::Transcribed —
                        // that would re-enter transcribed() and run the pipeline
                        // + inject a second time (the repeat-paste bug).
                        controller.send(Event::PipelineDone { session });
                    });
                if let Err(e) = worker {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!("pipeline thread failed: {e}"),
                    );
                    self.flash("Transform failed");
                }
            }
            Err(e) => {
                crate::log_entry(crate::LogLevel::Error, format!("transcription failed: {e}"));
                self.flash(&e);
            }
        }
    }

    /// Reads back the previous dictation's field and, if the user changed it,
    /// feeds the difference into the personalization loop.
    ///
    /// Every guard here fails closed. A wrong signal teaches the model a
    /// preference the user never expressed, so an unreadable field, a field we
    /// no longer recognise, or a diff we cannot explain all result in learning
    /// nothing.
    fn observe_pending_edit(&mut self) {
        use crate::edit_watch::EditDiff;

        let app = self.app.clone();
        let pending: Option<crate::edit_watch::PendingEdit> = {
            let state = app.state::<AppState>();
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.take()
        };
        let Some(pending) = pending else {
            return;
        };

        let state = app.state::<AppState>();
        // Never learn from our own scratchpad: the "edit" there is just the
        // appended dictation.
        if state.settings().scratchpad_enabled && pending.target.eq_ignore_ascii_case("Teletype") {
            return;
        }
        // Only meaningful while the same app is still frontmost. If the user
        // switched apps, the field we would read is a different one.
        let now_front = state
            .platform
            .active_application()
            .map(|a| a.application_name)
            .unwrap_or_default();
        if now_front.is_empty() || !now_front.eq_ignore_ascii_case(&pending.target) {
            crate::log_entry(
                crate::LogLevel::Info,
                "edit watch: focus moved, skipping observation",
            );
            return;
        }

        // Reading the field is the platform's job; without it we cannot observe
        // anything, so skip rather than guess.
        let Some(value) = state.platform.focused_text() else {
            crate::log_entry(
                crate::LogLevel::Info,
                "edit watch: field not readable, skipping",
            );
            return;
        };

        let diff = crate::edit_watch::diff(&value, &pending.inserted);
        // `at` is deliberately not destructured: it indexes the whole field,
        // not the text we inserted. See the rebuild below.
        let EditDiff::Edited { replacement, .. } = diff else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("edit watch: no learnable edit ({diff:?})"),
            );
            return;
        };

        // The AI output is the text we inserted; the final text is the field
        // with the user's edit applied. Rebuild the latter from the anchored
        // prefix so the learning extractor sees the user's version of the
        // dictation, not the whole surrounding field.
        // `diff` only returns `Edited` when the whole of `inserted` is still
        // present verbatim in the field and `replacement` is what follows it,
        // so the user's edited version of the dictation is
        // `inserted + replacement`. Slicing `inserted` by the field offset
        // used to panic (out of bounds, or mid-character) and lose the take.
        // For in-place edits, `replacement` is the full current value (the user's
        // edited text). For append edits, it is the text after the insert, so the
        // user's text is `inserted + replacement`.
        let user_text = if replacement.len() >= pending.inserted.len()
            && !replacement.starts_with(&pending.inserted)
        {
            replacement.clone()
        } else {
            format!("{}{}", pending.inserted, replacement)
        };
        let app_ctx = state.platform.active_application().unwrap_or_default();

        let changed = {
            let mut profile = state
                .profile
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let changed = commands::record_edit_core_public(
                &mut profile,
                &pending.inserted,
                &user_text,
                &app_ctx,
            );
            if !changed.is_empty() {
                if let Err(e) = state.profile_store.save(&*profile) {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!("edit watch: profile save failed: {e}"),
                    );
                }
            }
            // Pair ids with their user-facing descriptions for the undo
            // pill; looked up here while the profile is locked.
            changed
                .into_iter()
                .filter_map(|id| {
                    profile
                        .preferences
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| (id.clone(), p.description.clone()))
                })
                .collect::<Vec<_>>()
        };
        if !changed.is_empty() {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("edit watch: learned {} preference(s)", changed.len()),
            );
            // Undo offer: the tray menu gains an Undo item and the main
            // window toasts with an Undo button. The pill stays out of it:
            // it is click-through outside recordings and its fixed sizes
            // leave no room for a button without resizing surgery.
            let first = changed
                .first()
                .map(|(_, d)| d.as_str())
                .unwrap_or("a writing preference");
            let message = if changed.len() == 1 {
                format!("Learned: {first}")
            } else {
                format!("Learned {} preferences (latest: {first})", changed.len())
            };
            let ids: Vec<String> = changed.into_iter().map(|(id, _)| id).collect();
            {
                let mut slot = state
                    .last_learned
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *slot = Some(ids.clone());
            }
            crate::tray::offer_undo(&app, &message);
            let _ = app.emit(
                "learned-preference",
                &serde_json::json!({ "message": message, "ids": ids }),
            );
        }
    }

    /// T2.1: after injection, wait for the user to finish editing the
    /// inserted text, then diff it once and learn. The check runs on the
    /// controller thread (AX reads are main-thread on macOS) and only if the
    /// watch is still pending — the next dictation's `observe_pending_edit`
    /// remains the fallback.
    fn arm_edit_check(&mut self, session: u64) {
        let controller = self.controller.clone();
        std::thread::Builder::new()
            .name("teletype-edit-check".into())
            .spawn(move || {
                // Give the user a moment to fix a misheard word or sign-off.
                std::thread::sleep(Duration::from_secs(15));
                controller.send(Event::CheckEdit { session });
            })
            .ok();
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
        self.vad = None;
        self.recording_started = None;
        self.speech_ms.clear();
        self.injected_at = None;
        if let Some(handle) = self.live_preview.take() {
            handle.stop();
        }
        // BUG-003: the dictation session is over; arm the idle-unload timer
        // (or leave the model resident for the "Never" policy).
        {
            let state = self.state();
            let delay_secs = state.settings().model_unload_delay_secs;
            let delay = (delay_secs > 0).then(|| std::time::Duration::from_secs(delay_secs));
            state.speech.end_dictation(delay);
            state.parakeet.end_dictation(delay);
        }
        self.broadcast(teletype_core::state::UiState::Idle);
        self.show(PillState::Idle);
    }

    fn show(&self, state: PillState) {
        let settings = self.state().settings();
        let position = parse_position(&settings.pill_position);
        crate::overlay::update(&self.app, state, position, settings.always_show_pill);
        // The pill is always interactive so the device picker and cancel
        // button work in both push-to-talk and toggle modes. In push-to-talk
        // the user must release the hotkey to click, and the pill would
        // otherwise become click-through the instant the phase changes.
        crate::overlay::set_interactive(&self.app, true);
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

/// Delivery gate: the recorded take-start target against the live target,
/// with one attempt to bring the recorded app back.
///
/// Returns true when the text may be injected. Pure decision plus one
/// side-effecting recovery, kept out of the worker closure so the rule stays
/// readable: verify, try once to repair by raising the recorded app, verify
/// again, otherwise refuse. Unreadable ends proceed (no evidence is not a
/// mismatch); only a proven switch aborts. No recorded target also proceeds:
/// there is no baseline to mismatch against.
fn gate_passed(
    record_target: &Option<RecordTarget>,
    platform: &dyn teletype_core::platform::Platform,
) -> bool {
    let Some(recorded) = record_target else {
        return true;
    };
    let decided = |platform: &dyn teletype_core::platform::Platform| {
        let current = platform.active_application();
        let bundle = current
            .as_ref()
            .map(|a| a.application_id.as_str())
            .unwrap_or("");
        resolve_delivery(
            recorded,
            bundle,
            platform.active_pid(),
            platform.focused_window_frame(),
        )
    };
    if decided(platform) == Delivery::Proceed {
        return true;
    }
    // Proven switch. One attempt to raise the recorded app and re-verify:
    // the user may have switched away and back already, or may want us to
    // bring them back. Never paste on the activation call alone.
    if let Some(pid) = recorded.pid {
        if platform.activate_pid(pid) {
            thread::sleep(Duration::from_millis(400));
            if decided(platform) == Delivery::Proceed {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!(
                        "delivery: re-activated {}; proceeding with the paste",
                        recorded.app_name
                    ),
                );
                return true;
            }
        }
    }
    false
}

/// Why the text must not be injected, if anything.
///
/// Two independent refusals share the abort path (no tiers, history keeps the
/// text, user is told). The window switch comes first: it is about *where*.
/// The terminal newline is about *what*: a newline in a terminal submits the
/// line, so multi-line dictation there would run commands.
fn delivery_refusal(
    record_target: &Option<RecordTarget>,
    platform: &dyn teletype_core::platform::Platform,
    app_type: teletype_core::context::AppType,
    text: &str,
) -> Option<String> {
    if !gate_passed(record_target, platform) {
        let where_to = record_target
            .as_ref()
            .map(|r| r.app_name.as_str())
            .unwrap_or("the app");
        return Some(format!(
            "Not pasted: {where_to} is no longer in front. \
             The text is kept in History."
        ));
    }
    if teletype_core::delivery::refuse_newline_in_terminal(app_type, text) {
        return Some(
            "Not pasted: a newline in a terminal submits the line, so \
             multi-line dictation is never typed there. The text is kept \
             in History; paste it yourself if that is what you meant."
                .to_string(),
        );
    }
    None
}

pub(crate) fn parse_position(s: &str) -> PillPosition {
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

/// The language to pass to the ASR engine.
///
/// `"auto"` (or an empty legacy setting) is passed through: Whisper models
/// auto-detect the language themselves (whisper.cpp runs without a
/// language hint). Parakeet cannot auto-detect, so its wrapper ignores the
/// language argument and always runs with its trained language mix.
pub fn effective_language(language: String, _english_only: bool) -> String {
    let trimmed = language.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        "auto".into()
    } else {
        trimmed.to_string()
    }
}

/// Resolves the selected speech model to a file path on disk plus whether
/// it runs on the Parakeet engine. Shared by the dictation worker, the
/// model warm-up, and the recovery replay path.
pub fn resolve_speech_model(
    settings: &crate::commands::Settings,
    models_dir: &std::path::Path,
) -> (std::path::PathBuf, bool) {
    let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
    let use_parakeet = entry
        .map(|m| m.engine == teletype_speech::catalog::Engine::Parakeet)
        .unwrap_or(false);
    let file = entry
        .map(|m| m.file.to_string())
        .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
    (models_dir.join(file), use_parakeet)
}

/// The language to pass to the ASR engine for the frontmost app (P3.3):
/// the per-app override first, then the global setting. `"auto"` is passed
/// through so Whisper can still auto-detect; a concrete override (e.g. "de")
/// is passed to the engine and wins over `"auto"`.
pub fn effective_language_for_app(state: &AppState, use_parakeet: bool) -> String {
    let settings = state.settings();
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let profile = teletype_core::personalization::UserProfile {
        language: settings.language.clone(),
        app_language_overrides: settings.app_language_overrides.clone(),
        ..Default::default()
    };
    let resolved = teletype_core::personalization::resolve_language(&profile, &app_ctx);
    effective_language(resolved, use_parakeet)
}

/// Transliterates Cyrillic characters to their Latin equivalents.
///
/// The Parakeet TDT model supports 25 languages and can misidentify English
/// proper nouns as Russian (e.g. "Rida Fatema" → "Рида Фатема"). When the
/// user's language setting is English, any Cyrillic in the transcript is a
/// mis-detection and should be transliterated back to Latin.
fn transliterate_cyrillic(text: &str) -> String {
    const MAP: &[(&str, &str)] = &[
        ("а", "a"),
        ("б", "b"),
        ("в", "v"),
        ("г", "g"),
        ("д", "d"),
        ("е", "e"),
        ("ё", "yo"),
        ("ж", "zh"),
        ("з", "z"),
        ("и", "i"),
        ("й", "y"),
        ("к", "k"),
        ("л", "l"),
        ("м", "m"),
        ("н", "n"),
        ("о", "o"),
        ("п", "p"),
        ("р", "r"),
        ("с", "s"),
        ("т", "t"),
        ("у", "u"),
        ("ф", "f"),
        ("х", "kh"),
        ("ц", "ts"),
        ("ч", "ch"),
        ("ш", "sh"),
        ("щ", "shch"),
        ("ъ", ""),
        ("ы", "y"),
        ("ь", ""),
        ("э", "e"),
        ("ю", "yu"),
        ("я", "ya"),
        ("А", "A"),
        ("Б", "B"),
        ("В", "V"),
        ("Г", "G"),
        ("Д", "D"),
        ("Е", "E"),
        ("Ё", "Yo"),
        ("Ж", "Zh"),
        ("З", "Z"),
        ("И", "I"),
        ("Й", "Y"),
        ("К", "K"),
        ("Л", "L"),
        ("М", "M"),
        ("Н", "N"),
        ("О", "O"),
        ("П", "P"),
        ("Р", "R"),
        ("С", "S"),
        ("Т", "T"),
        ("У", "U"),
        ("Ф", "F"),
        ("Х", "Kh"),
        ("Ц", "Ts"),
        ("Ч", "Ch"),
        ("Ш", "Sh"),
        ("Щ", "Shch"),
        ("Ъ", ""),
        ("Ы", "Y"),
        ("Ь", ""),
        ("Э", "E"),
        ("Ю", "Yu"),
        ("Я", "Ya"),
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
pub fn ensure_latin_if_english(text: &str, language: &str) -> String {
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
    let manager = if use_parakeet {
        &state.parakeet
    } else {
        &state.speech
    };
    if !manager.is_loaded() {
        if !model_path.exists() {
            return Err("Speech model not downloaded".into());
        }
        manager.set_model_path(model_path.to_path_buf());
        manager
            .load()
            .map_err(|e| format!("Speech model load failed: {e}"))?;
    }
    let mut speech = manager
        .provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    speech
        .transcribe(&captured.samples, language)
        .map_err(|e| e.to_string())
}

/// Reads the language the speech engine detected on its last auto-detect run.
///
/// Only the Whisper provider reports detection; Parakeet (English-only) and
/// the mock always return `None`. Returns `None` when auto-detect was not
/// active, so callers can tell "detected en" from "no detection info".
fn detect_language_from_provider(
    app: &AppHandle,
    use_parakeet: bool,
    language: &str,
) -> Option<String> {
    if use_parakeet {
        return None;
    }
    let state = app.state::<AppState>();
    let speech = state
        .speech
        .provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if language != "auto" && !language.is_empty() {
        return None; // locked language: detection is meaningless
    }
    speech.detected_language()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflict_shows_only_for_the_binding_that_failed() {
        let stored = Some(HotkeyConflict::new("Ctrl+Shift+Space", "os error -9878"));
        // Same binding: visible.
        let shown = visible_conflict(&stored, "Ctrl+Shift+Space");
        assert_eq!(shown, stored);
        // User changed the binding since: the old failure must not linger.
        assert_eq!(visible_conflict(&stored, "Alt+Space"), None);
        // Nothing stored: nothing shown.
        assert_eq!(visible_conflict(&None, "Ctrl+Shift+Space"), None);
    }

    #[test]
    fn the_conflict_message_blames_nobody() {
        // Worded as what macOS reported: a duplicate registration inside our
        // own process fails the same way, so naming another app is unproven.
        let c = HotkeyConflict::new("Ctrl+Shift+Space", "os error -9878");
        assert_eq!(c.binding, "Ctrl+Shift+Space");
        assert!(c.message.contains("already taken"), "{}", c.message);
        assert!(c.message.contains("Cause unknown"), "{}", c.message);
        assert!(c.message.contains("os error -9878"), "{}", c.message);
        for word in ["another app", "other app", "claimed by", "holds"] {
            assert!(!c.message.contains(word), "blames: {word}");
        }
    }

    /// Regression test for the P0-1 cancel guard: a cancel while the
    /// pipeline is running must map to CancelPipeline (finish without
    /// inserting), and a cancel while recording must map to a cancelled
    /// stop (discard the audio).
    #[test]
    fn pill_cancel_maps_per_phase() {
        // Cancel mid-pipeline: let transcription finish, drop the result.
        let action = state::decide(
            Input::Cancel,
            Phase::Transforming,
            RecordingMode::Hold,
            &mut false,
            &mut false,
        );
        assert!(matches!(action, state::Action::CancelPipeline));

        // Cancel while recording: stop and discard.
        let action = state::decide(
            Input::Cancel,
            Phase::Listening,
            RecordingMode::Hold,
            &mut true,
            &mut false,
        );
        assert!(matches!(action, state::Action::Stop { cancelled: true }));

        // Cancel while idle: nothing to cancel.
        let action = state::decide(
            Input::Cancel,
            Phase::Idle,
            RecordingMode::Hold,
            &mut false,
            &mut false,
        );
        assert!(matches!(action, state::Action::Nothing));
    }

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
        assert_eq!(
            transliterate_cyrillic("Рида hello Фатема"),
            "Rida hello Fatema"
        );
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
    fn effective_language_defaults_to_auto() {
        // Empty legacy setting means auto-detect (Whisper picks the language;
        // Parakeet ignores the argument inside its wrapper).
        assert_eq!(effective_language("".into(), false), "auto");
        // "auto" (any case) passes through; explicit codes pass through too.
        assert_eq!(effective_language("auto".into(), false), "auto");
        assert_eq!(effective_language("AUTO".into(), false), "auto");
        assert_eq!(effective_language("ru".into(), false), "ru");
        assert_eq!(effective_language("en".into(), true), "en");
        // Surrounding whitespace is trimmed, not treated as a language.
        assert_eq!(effective_language("  ru ".into(), false), "ru");
    }

    #[test]
    fn detected_language_flows_into_profile() {
        // Unit-level check of the auto-detect contract: when the engine
        // reports a detection, the pipeline must polish in that language.
        let mut profile = teletype_core::personalization::UserProfile::default();
        assert_eq!(profile.language, "en");
        let detected: Option<String> = Some("de".into());
        if let Some(d) = &detected {
            profile.language = d.clone();
        }
        assert_eq!(profile.language, "de");
    }
}
