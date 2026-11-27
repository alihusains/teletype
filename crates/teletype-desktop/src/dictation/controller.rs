use super::panic_text;
use super::*;

#[allow(clippy::large_enum_variant)] // `Transcribed` carries the take PCM
                                     // (`Option<Vec<f32>>`, A.3.3); boxing it
                                     // would ripple through every controller
                                     // match arm.
pub enum Event {
    HotkeyDown,
    HotkeyUp,
    HotkeyInterrupted,
    /// The watchdog armed by a suspected bounced release fired: no press
    /// followed within the bounce window, so the release was genuine after
    /// all. `seq` identifies the arming; any later press bumps it so a stale
    /// timer can never end a take the user is still holding.
    HotkeyBounceSettled {
        seq: u64,
        held_ms: u64,
    },
    /// P2.4: the dedicated Command Mode hotkey was pressed/released.
    /// Starts (and stops, in Hold mode) a `TakeKind::Command` take instead
    /// of dictation. Release shares the normal release path: ending the
    /// take in progress, whatever kind it is.
    CommandHotkeyDown,
    CommandHotkeyUp,
    /// P9.6: the e2e feeder thread is playing a 16 kHz frame of the queued
    /// WAV file into the in-flight take (file-input mode).
    E2eFeed {
        frame: Vec<f32>,
    },
    /// P8.4: the Paste-Last hotkey was pressed. Re-delivers the last final
    /// text through the same delivery path as a normal paste.
    PasteLast,
    /// P2.8: the tray's "Run Voice Command" item. Same as `Toggle` but the
    /// take it starts is a Command take.
    CommandToggle,
    Toggle,
    Cancel,
    /// Cancel pressed from the pill overlay.
    PillCancel,
    /// A settings save may have changed a take-scoped binding (the cancel
    /// hotkey) while a take could be running. Handled by the resync that runs
    /// after every event, so this arm itself does nothing.
    ResyncBindings,
    /// Test-only: the release worker has started (the test's event channel
    /// is open). Carries the session so a test can tell the worker apart
    /// from a stale event.
    #[cfg(test)]
    WorkerStarted {
        session: u64,
    },
    /// Deferred startup wiring: sent once by [`Controller::spawn`] after
    /// `AppState` has been managed. `spawn` runs before `app.manage(..)`, so
    /// nothing that needs settings can read them there; this event is the
    /// first point where the live settings are available on this thread.
    StartupWiring,
    Transcribed {
        session: u64,
        /// `(text, words)` where `words` is `Some` when the engine reports
        /// per-word timings/confidences (Fluid, sherpa) and `None` otherwise
        /// (Whisper, ggml Parakeet).
        transcript: Result<(String, Option<Vec<teletype_speech::Word>>), String>,
        /// The language the ASR engine detected, when auto-detect was active.
        detected_language: Option<String>,
        /// P0.2 trace marks: monotonic µs when the ASR pass started/ended,
        /// captured on the transcribe worker thread.
        asr_start_us: u64,
        asr_end_us: u64,
        /// HARD-4 row 2: the device stopped responding mid-take (the cpal
        /// stream error callback set the flag). The captured audio was
        /// still transcribed and pasted; the trace carries this boolean
        /// plus the captured duration in ms (no audio text, no cpal error
        /// text).
        device_lost: bool,
        /// HARD-4 row 2: captured audio duration in ms when `device_lost`
        /// is set, else 0.
        device_lost_ms: u32,
        /// A.3.3: the take audio at 16 kHz mono f32 (the same buffer the
        /// final transcribe consumed). Empty when the take had no audio
        /// (audio finish failed). The command decider classifies
        /// this audio; `None`/empty PCM degrades to the transcript-only
        /// fallback (the Needle/LLM deciders ignore PCM anyway).
        pcm: Option<Vec<f32>>,
        /// T10b: the streaming take's word-loss accounting (T03). `None`
        /// when the take had no committer (whole-take fallback path).
        losses: Option<crate::committer::CommitterFailureReport>,
        /// The whole take at 16 kHz mono with the pre-roll and without the
        /// VAD trim, so playback sounds natural. Saved by the bookkeeper as
        /// the Dictation tab's recording. `None` on a failed take, when
        /// `save_recordings` is off, and for e2e takes. Kept apart from
        /// `pcm`, whose semantics the command decider depends on.
        take_audio: Option<std::sync::Arc<[f32]>>,
        /// The streaming take's per-segment polish rewrote at least one
        /// segment, so `transcript` is not the raw ASR text. History then
        /// stores no raw text and no diff for the take.
        segment_polished: bool,
        /// The unpolished ASR text of each segment the per-segment polish
        /// rewrote, in commit order (a `None` slot is the never-polished
        /// tail). `None` when the take had no segment polish. Joined with a
        /// space and compared against `transcript` at release, this is the
        /// take's raw text and the Polish stage's before/after pair, so a
        /// long take shows its polish and filler edits like any other.
        segment_raws: Option<Vec<Option<String>>>,
    },
    /// The transform + inject pipeline for a session has finished.
    PipelineDone {
        session: u64,
    },
    /// The pipeline worker has finished its bookkeeping (history, transcript
    /// file, usage, edit-watch) and is asking the controller for permission
    /// to perform the only user-visible side effect: the text injection.
    ///
    /// The worker NEVER injects itself. It sends this event with the session
    /// id and the fully prepared payload; the controller — the single
    /// owner of the session state machine — decides whether the session is
    /// still valid (not cancelled, not superseded) and, only if so,
    /// performs the injection on its own thread and answers with
    /// `PipelineDone`. A `Cancel` processed before this event discards the
    /// result; one processed after it lets the injection proceed and then
    /// transitions the session. That ordering is the whole cancellation
    /// guarantee: one FIFO queue, one thread, no check-then-act race.
    PipelineReady {
        session: u64,
        payload: PipelinePayload,
    },
    /// The pipeline worker finished all bookkeeping (history, transcript file,
    /// usage, edit-watch) but has NOT yet injected or reset the pill. Sent
    /// from the worker thread; handled on the main thread so the pill reset
    /// goes through the normal `self.show()` path, which reliably delivers the
    /// event to the pill webview. `PipelineDone` alone is not enough: it fires
    /// after `go_idle()`, which hides the pill, so the webview never sees the
    /// transition out of "Transforming…" if a newer session superseded this
    /// one or if the IPC event was coalesced.
    PipelineWorkerDone {
        session: u64,
    },
    /// A take's worker ended without handing the session back to the
    /// controller: it panicked, or an early return skipped its terminal
    /// event (`Transcribed` / `PipelineReady`). Sent by [`WorkerExitGuard`]
    /// on drop.
    ///
    /// Without this, a panicking pipeline left the pill stuck in its busy
    /// state forever: the controller only hides the pill on `PipelineDone`,
    /// which is sent after `PipelineReady`, and a dead worker never sends
    /// either. The guard turns any worker death into a recoverable session.
    WorkerAbandoned {
        session: u64,
    },
    /// A flash message's display timer elapsed; hide the pill.
    FlashDone {
        generation: u64,
    },
    /// HARD-4 row 2: a device-loss notice to flash after the take's text
    /// is delivered. Sent by the transcribe worker (the take's session id
    /// must still be current when the controller handles it), carrying the
    /// message to show.
    FlashNote {
        session: u64,
        message: String,
    },
    /// The speech model finished loading in the background. `session` is the
    /// warm token (see `load_speech_model`), NOT a take session id: the
    /// warm-up must not bump `next_id`, or the `VadFired` guard of the take
    /// that is recording would go stale and drop the auto-stop (F-11).
    WarmupDone {
        session: u64,
    },
    /// The speech model failed to load in the background. The pill must end
    /// warming with an error state instead of sitting on "Getting model
    /// ready…" until the take fails with a generic flash. `session` is the
    /// warm token (see `WarmupDone`).
    WarmupFailed {
        session: u64,
        /// The reason class from `warmup_failure_reason`: "missing file",
        /// "unsupported", or "other".
        reason: &'static str,
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
    /// "Delete all learning data" closed the store handle: the file was
    /// deleted out from under the session cache, so drop it. The next
    /// `learning_store()` call reopens a fresh store (F-84).
    CloseLearningStore,
    /// The undo offer's expiry timer fired for `offer_id`. The live offer
    /// is cleared only when it still carries that id, so a newer offer's
    /// timer cannot kill it (F-84).
    OfferUndoExpired {
        offer_id: u64,
    },
    /// Phase 5.1: the prewarm tick (every 2 s from the tick thread spawned
    /// in `Controller::spawn`). Re-evaluates `should_arm(mode, app_active)`
    /// and arms/disarms the front end on the controller thread. No
    /// SettingsChanged event exists, so a setting flip (`instant_start`)
    /// applies on restart in v1 — the mode is parsed once at spawn.
    PrewarmTick,
    /// Phase 5.7: the transcribe worker computed this session's per-take
    /// quality signals. Sent before the session's `Transcribed` (one FIFO
    /// channel, so it always lands first); stashed by `take_quality` and
    /// attached to the trace in `finish_trace`.
    TakeQuality {
        session: u64,
        quality: TakeQuality,
    },
}

#[cfg(test)]
pub(in crate::dictation) type ForTestInject = Box<dyn FnMut(&PipelinePayload) + Send>;

#[derive(Clone)]
pub struct Controller {
    /// `None` in unit tests, where the event loop is driven directly on the
    /// test thread and the channel never needs to be closed.
    pub(in crate::dictation) tx: Option<Sender<Event>>,
    /// The most recent hotkey registration failure, with the binding that
    /// failed. Compared at read time against the currently saved binding so
    /// a fixed binding never shows a stale warning.
    hotkey_conflict: Arc<Mutex<Option<HotkeyConflict>>>,
    /// P9.6: the e2e hook slot, `Some` only in `--e2e` mode. Consulted at
    /// the take-start and injection seams by the controller.
    pub(crate) e2e: Option<std::sync::Arc<crate::e2e::E2eSlot>>,
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
    pub(in crate::dictation) fn new(binding: &str, os_error: &str) -> Self {
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
    pub fn tx(&self) -> Option<Sender<Event>> {
        self.tx.clone()
    }

    /// Spawns the controller event loop for unit tests: the same `handle`
    /// the production thread runs, with an `on_inject` hook standing in for
    /// the real injection (which needs the Tauri main thread). Returns the
    /// controller (to send events) and a finish handle that drops the
    /// channel and joins the loop thread.
    /// A do-nothing controller for `AppState::for_tests()`. The concurrency
    /// tests spawn their own controller via `for_tests(on_inject)`; this one
    /// just exists so the struct literal can be built.
    /// A do-nothing controller for `AppState::for_tests()`. The concurrency
    /// tests spawn their own controller via `for_tests(on_inject)`; this one
    /// just exists so the struct literal can be built.
    #[cfg(any(test, feature = "test"))]
    pub fn placeholder_for_tests() -> Self {
        Self {
            tx: None,
            hotkey_conflict: Arc::new(Mutex::new(None)),
            e2e: None,
        }
    }

    /// Private on purpose: only the in-file concurrency tests drive it, and
    /// it returns the crate-private `Session` so `pub` would trip clippy's
    /// `private_interfaces`.
    #[cfg(test)]
    pub(in crate::dictation) fn for_tests(
        on_inject: ForTestInject,
    ) -> (Self, Session, ForTestInject) {
        let controller = Self {
            tx: None,
            hotkey_conflict: Arc::new(Mutex::new(None)),
            e2e: None,
        };
        let mut session = Session {
            app: None,
            #[cfg(test)]
            headless_flash: true,
            #[cfg(test)]
            paste_calls: std::sync::atomic::AtomicU32::new(0),
            controller: controller.clone(),
            phase: Phase::Idle,
            hotkey_down: false,
            hands_free: false,
            last_release: None,
            bounce_settle_seq: 0,
            next_id: 0,
            take_kind: TakeKind::default(),
            started_at_ms: 0,
            recording_started: None,
            speech_ms: std::collections::HashMap::new(),
            streaming_marks: std::collections::HashMap::new(),
            vad: None,
            streaming: None,
            polish_app: None,
            live_preview: None,
            spool: None,
            take_resampler: None,
            device_error_flag: None,
            ducker: std::sync::Arc::new(std::sync::Mutex::new(crate::ducking::default_ducker())),
            duck_armed: false,
            record_target: None,
            pending_selection: None,
            injected_at: None,
            pending_polish_diff: None,
            cancelled_sessions: std::collections::HashSet::new(),
            traces: std::collections::HashMap::new(),
            trace_key: None,
            front_end: None,
            cancel_hotkey_armed: None,
            preroll_this_take: None,
            vad_flags: None,
            take_quality: std::collections::HashMap::new(),
            screen_context: None,
            learning_store: None,
            take_boost_terms: Vec::new(),
            boost_cache: teletype_core::learning::boost::BoostCache::new(),
            warm_token: 0,
            #[cfg(test)]
            headless_flashes: Vec::new(),
            #[cfg(test)]
            headless_broadcasts: Vec::new(),
            #[cfg(test)]
            headless_final_texts: Vec::new(),
            #[cfg(test)]
            headless_pipeline_count: 0,
            #[cfg(test)]
            test_events: None,
            #[cfg(test)]
            mock_asr: None,
        };
        // Mid-pipeline: session 1 is transforming, like a real take whose
        // worker is still running.
        session.next_id = 1;
        session.phase = Phase::Transforming;
        (controller, session, on_inject)
    }

    pub fn spawn(
        app: AppHandle,
        e2e: Option<std::sync::Arc<crate::e2e::E2eSlot>>,
    ) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        // The slot is passed in rather than looked up from AppState: the
        // state is managed only after this returns (it holds the controller),
        // so a lookup here always found `None` and silently disabled the whole
        // e2e hook, leaving every harness take recording from the real mic
        // with nothing to stop it.
        let controller = Self {
            tx: Some(tx),
            hotkey_conflict: Arc::new(Mutex::new(None)),
            e2e,
        };
        // Phase 5.1 (plan §5.1): instant start. Build the pre-warmed mic
        // front end once when the mode is not Off. There is no
        // SettingsChanged event, so a flip of `settings.instant_start`
        // applies on app restart in v1 (the mode is parsed here, once).
        // A failed build is logged and stays `None`: the per-take rebuild
        // path continues, so behaviour is unchanged.
        //
        // OVERLAP WITH `warm_engine_policy`, resolved here once (this is the
        // decision point). The two settings warm two different resources and
        // are NOT two tiers of one knob:
        //   * `instant_start`  = the AUDIO axis. It pre-arms the capture
        //     device (`MicFrontEnd`) so the take can start with a pre-roll.
        //   * `warm_engine_policy` = the MODEL axis, handled by
        //     [`engine_warm_tier`]: when the speech model is loaded, and
        //     whether it is allowed to go idle between takes.
        // Resolution: `warm_engine_policy` is the generalization on the model
        // axis, and `instant_start` never implies any model loading. A user
        // who sets the policy to `off` gets no resident model even with the
        // mic pre-armed, and a user who sets it to `always` gets a resident
        // model without this process ever holding the microphone open. The
        // reverse implication is refused for the same reason: arming a
        // microphone is a privacy surface, warming a model is not.
        //
        // Known gap, deliberately not touched by this unit: `spawn` runs
        // before `app.manage(AppState)` (`lib.rs`), so the `try_state` below
        // is `None` at this moment, `instant_start` reads as "" and
        // `prewarm_mode` is always `Off` here. The audio axis is therefore
        // inert as shipped (the per-take path is unaffected: it still builds
        // its own recording). Fixing it means moving this build to the
        // deferred `StartupWiring` event, which would start holding the
        // capture device open at launch for every `instant_start` user; that
        // is a product call, not a wiring fix, so it is reported rather than
        // made silently.
        let instant_start = app
            .try_state::<AppState>()
            .map(|s| s.settings().instant_start.clone())
            .unwrap_or_default();
        let prewarm_mode = match instant_start.as_str() {
            "always" => PrewarmMode::Always,
            "while_app_active" => PrewarmMode::WhileAppActive,
            _ => PrewarmMode::Off,
        };
        let mut session = Session {
            app: Some(app.clone()),
            #[cfg(test)]
            headless_flash: false,
            #[cfg(test)]
            paste_calls: std::sync::atomic::AtomicU32::new(0),
            controller: controller.clone(),
            phase: Phase::Idle,
            hotkey_down: false,
            hands_free: false,
            last_release: None,
            bounce_settle_seq: 0,
            next_id: 0,
            take_kind: TakeKind::default(),
            started_at_ms: 0,
            recording_started: None,
            speech_ms: std::collections::HashMap::new(),
            streaming_marks: std::collections::HashMap::new(),
            vad: None,
            streaming: None,
            polish_app: None,
            live_preview: None,
            spool: None,
            take_resampler: None,
            device_error_flag: None,
            ducker: std::sync::Arc::new(std::sync::Mutex::new(crate::ducking::default_ducker())),
            duck_armed: false,
            record_target: None,
            pending_selection: None,
            injected_at: None,
            pending_polish_diff: None,
            cancelled_sessions: std::collections::HashSet::new(),
            traces: std::collections::HashMap::new(),
            trace_key: None,
            front_end: None,
            cancel_hotkey_armed: None,
            preroll_this_take: None,
            vad_flags: None,
            take_quality: std::collections::HashMap::new(),
            screen_context: None,
            learning_store: None,
            take_boost_terms: Vec::new(),
            boost_cache: teletype_core::learning::boost::BoostCache::new(),
            warm_token: 0,
            #[cfg(test)]
            headless_flashes: Vec::new(),
            #[cfg(test)]
            headless_broadcasts: Vec::new(),
            #[cfg(test)]
            headless_final_texts: Vec::new(),
            #[cfg(test)]
            headless_pipeline_count: 0,
            #[cfg(test)]
            test_events: None,
            #[cfg(test)]
            mock_asr: None,
        };
        if prewarm_mode != PrewarmMode::Off {
            let app_ref = app.clone();
            let settings = app_ref.state::<AppState>().settings();
            let device = settings.input_device.clone();
            let noise = settings.noise_reduction_enabled;
            match MicFrontEnd::build(&device, noise) {
                Ok(fe) => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!("mic pre-warm built ({instant_start:?})"),
                    );
                    session.front_end = Some(fe);
                }
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("mic pre-warm build failed: {e}; per-take rebuild continues"),
                    );
                }
            }
        }
        if prewarm_mode != PrewarmMode::Off {
            // The tick thread holds a controller clone and nudges the
            // front end every 2 s (arm while desired, disarm otherwise).
            let tick_tx = controller.clone();
            thread::Builder::new()
                .name("teletype-prewarm-tick".into())
                .spawn(move || loop {
                    thread::sleep(Duration::from_secs(2));
                    tick_tx.send(Event::PrewarmTick);
                })
                .ok();
        }
        thread::Builder::new()
            .name("teletype-dictation".into())
            .spawn(move || {
                for event in rx {
                    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        session.handle(event);
                    }));
                    if let Err(payload) = handled {
                        crate::log_entry(
                            crate::LogLevel::Error,
                            format!(
                                "event handler panicked: {}; resetting the session",
                                panic_text(payload.as_ref())
                            ),
                        );
                        // The panic aborted the take with no terminal state:
                        // the phase still says in-flight (later hotkeys are
                        // swallowed) and the pill stays visible in its busy
                        // size. Recover the same way an abandoned worker does.
                        // Guarded so a second panic cannot kill this thread.
                        let recovered =
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                session.recover_after_panic()
                            }));
                        if recovered.is_err() {
                            crate::log_entry(
                                crate::LogLevel::Error,
                                "session reset after panic also panicked; pill may be stale",
                            );
                        }
                    }
                }
            })?;
        // Deferred startup wiring: wait out the `app.manage(AppState)` gap
        // (`lib.rs` manages the state after `spawn` returns) and then ask the
        // session thread to do the settings-driven work it cannot do here.
        // One-shot, bounded: if the state never appears (a failed setup, or a
        // harness that never builds it) this thread simply exits.
        if controller.e2e.is_none() {
            let wait_app = app.clone();
            let wait_tx = controller.clone();
            thread::Builder::new()
                .name("teletype-startup-wiring".into())
                .spawn(move || {
                    for _ in 0..150 {
                        if wait_app.try_state::<AppState>().is_some() {
                            break;
                        }
                        thread::sleep(Duration::from_millis(200));
                    }
                    if wait_app.try_state::<AppState>().is_some() {
                        wait_tx.send(Event::StartupWiring);
                    }
                })
                .ok();
        }
        Ok(controller)
    }

    pub fn send(&self, event: Event) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(event);
        }
    }

    pub fn register_hotkey(&self, app: &AppHandle, hotkey: &str) -> Result<(), String> {
        // A live Fn tap is the binding we were asked for: tearing it down and
        // rebuilding it only drops key edges and makes this thread wait for the
        // old tap thread to exit. The permission watcher calls this on every
        // "hotkey looks dead" episode and settings saves call it on every
        // change, so the churn is not free. Every other caller unregisters the
        // old binding before registering a new one, so this cannot skip a real
        // switch.
        #[cfg(target_os = "macos")]
        if hotkey == "Fn" && crate::fn_tap::is_live() {
            return Ok(());
        }
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
        } else {
            // A single bare modifier (Ctrl/Cmd/Alt/Shift) also cannot be
            // registered with Carbon — RegisterEventHotKey only fires for a
            // modifier+key combo, never a lone modifier. Watch it with the
            // bare-modifier CGEventTap instead, which reports the press only
            // while that modifier is the sole one held.
            #[cfg(target_os = "macos")]
            {
                if let Some(mask) = crate::mod_tap::bare_modifier_mask(hotkey) {
                    crate::mod_tap::start(self.clone(), mask).map_err(|e| {
                        format!("Couldn't register {hotkey}: {e} (grant Teletype Accessibility access in System Settings > Privacy & Security > Accessibility)")
                    })
                } else {
                    self.register_string_hotkey(app, hotkey)
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                self.register_string_hotkey(app, hotkey)
            }
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
                let event = match event.state {
                    ShortcutState::Pressed => {
                        mark_hotkey_down();
                        Event::HotkeyDown
                    }
                    ShortcutState::Released => {
                        mark_hotkey_up();
                        Event::HotkeyUp
                    }
                };
                controller.send(event);
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

    /// P2.4: registers the dedicated Command Mode hotkey. Unlike the
    /// dictation hotkey this is string-shortcuts only (no Fn tap, no
    /// bare-modifier tap): a command binding is always a full chord.
    /// Empty unregisters nothing and succeeds — the feature is simply off.
    /// Failures are logged, not returned: Command Mode is optional, so a
    /// bad binding must not fail the settings save (the Keybinds screen
    /// already flags duplicate bindings client-side).
    pub fn register_command_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let controller = self.clone();
        let binding = hotkey.to_string();
        if let Err(e) = app
            .global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                controller.send(match event.state {
                    ShortcutState::Pressed => Event::CommandHotkeyDown,
                    ShortcutState::Released => Event::CommandHotkeyUp,
                });
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
        {
            crate::log_entry(crate::LogLevel::Error, e);
        } else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("command hotkey registered: {binding}"),
            );
        }
    }

    /// Unregisters a previously registered Command Mode hotkey. Empty is a
    /// no-op.
    pub fn unregister_command_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let _ = app.global_shortcut().unregister(hotkey);
    }

    /// P8.4: registers the Paste-Last hotkey. Mirrors `register_command_hotkey`:
    /// empty unregisters nothing; failures are logged, not returned.
    pub fn register_paste_last_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let controller = self.clone();
        let binding = hotkey.to_string();
        if let Err(e) = app
            .global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state == ShortcutState::Pressed {
                    controller.send(Event::PasteLast);
                }
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
        {
            crate::log_entry(crate::LogLevel::Error, e);
        } else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("paste-last hotkey registered: {binding}"),
            );
        }
    }

    /// Unregisters the Paste-Last hotkey. Empty is a no-op.
    pub fn unregister_paste_last_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let _ = app.global_shortcut().unregister(hotkey);
    }

    /// Escape-to-abandon: registers `settings.cancel_hotkey` so a press
    /// aborts the take in progress and injects nothing. The handler sends
    /// `Event::Cancel`, which is the same in-process decision the pill's
    /// Cancel button uses (`commands::cancel_dictation` sends
    /// `Event::PillCancel`): both feed `state::decide(Input::Cancel, phase,
    /// …)` on this controller's FIFO queue, so a recording stops and is
    /// discarded, an in-flight pipeline is tagged cancelled (its
    /// `PipelineReady` is dropped), and a press with no take running is
    /// `Action::Nothing`. Press-only: unlike hold-to-talk, a cancel has no
    /// release half.
    ///
    /// Deliberately armed only *while a take is alive* (see
    /// [`Session::sync_cancel_hotkey`]) rather than persistently like
    /// Paste-Last. Reason: the shipped default is a bare `Escape`, and every
    /// other persistent binding in this app is a modified chord. A Carbon
    /// `RegisterEventHotKey` interception removes the keystroke from the
    /// normal event flow, so a persistent bare-Escape registration would eat
    /// Escape in every other app (closing dialogs, cancelling find, leaving
    /// fullscreen) for the whole session. Arming for the duration of a take
    /// gives exactly the promised behaviour while it can matter and touches
    /// nothing otherwise. Failures are logged, not returned, like the other
    /// optional bindings: a bad cancel binding must not fail a settings save
    /// and must not abort a recording.
    pub fn register_cancel_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let controller = self.clone();
        let binding = hotkey.to_string();
        if let Err(e) = app
            .global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state == ShortcutState::Pressed {
                    controller.send(Event::Cancel);
                }
            })
            .map_err(|e| format!("Couldn't register cancel hotkey {hotkey}: {e}"))
        {
            crate::log_entry(crate::LogLevel::Error, e);
        } else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("cancel hotkey armed: {binding}"),
            );
        }
    }

    /// Unregisters the cancel hotkey. Empty is a no-op.
    pub fn unregister_cancel_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let _ = app.global_shortcut().unregister(hotkey);
    }

    /// Asks the session loop to re-evaluate the take-scoped bindings after a
    /// settings change (the cancel hotkey). Fire-and-forget: when no take is
    /// running the resync is a no-op, so a save while idle costs nothing.
    pub fn resync_bindings(&self) {
        self.send(Event::ResyncBindings);
    }

    /// Registers a global shortcut that, when pressed, applies the named
    /// transform to the text currently selected in the focused app (Wispr
    /// Flow style). On macOS the primary path is the unified InputEngine's
    /// ShortcutManager via `sync_transform_shortcuts`; this Carbon path is
    /// non-macOS plus degraded fallback when the tap is unavailable. The
    /// handler spawns the (blocking) transform work off the main thread via
    /// the Tauri command so a slow local model never stalls the UI.
    pub fn register_transform_shortcut(
        &self,
        app: &AppHandle,
        hotkey: &str,
        transform_id: &str,
    ) -> Result<(), String> {
        self.unregister_transform_shortcut(app, hotkey);
        if hotkey.trim().is_empty() {
            return Ok(());
        }
        let app_clone = app.clone();
        let id = transform_id.to_string();
        let hotkey_owned = hotkey.to_string();
        crate::log_entry(
            crate::LogLevel::Info,
            format!("registering transform shortcut: {hotkey} -> {id}"),
        );
        app.global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state == ShortcutState::Pressed {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!("transform shortcut FIRED: {hotkey_owned} -> {id}"),
                    );
                    // Read the selection SYNCHRONOUSLY in the callback, before
                    // the async task runs. On macOS Carbon does NOT swallow,
                    // so this path is fallback only; the InputEngine tap
                    // already swallowed the keystroke and captured the
                    // selection synchronously. The sync read stays to avoid
                    // re-reading after focus changed.
                    let selected = crate::text_selection::selected_text();
                    let app_clone = app_clone.clone();
                    let id_clone = id.clone();
                    tauri::async_runtime::spawn(async move {
                        let st = app_clone.state::<crate::AppState>();
                        let a2 = app_clone.clone();
                        // Pass the pre-captured selection to the transform so
                        // it doesn't re-read a corrupted selection.
                        match commands::transform_selection_with_text(a2, st, id_clone, selected)
                            .await
                        {
                            Ok(result) => crate::log_entry(
                                crate::LogLevel::Success,
                                format!("transform_selection OK: {} chars", result.len()),
                            ),
                            Err(e) => crate::log_entry(
                                crate::LogLevel::Error,
                                format!("transform_selection failed: {e}"),
                            ),
                        }
                    });
                }
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
    }

    /// Unregisters a transform shortcut (no-op if not registered).
    pub fn unregister_transform_shortcut(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let _ = app.global_shortcut().unregister(hotkey);
    }

    /// Registers the Quick Add global shortcut (item 8): the word selected in
    /// the focused app is added to the dictionary. The AX read must run on the
    /// main thread, so the handler spawns the work as a Tauri command — the
    /// same pattern as the transform shortcuts.
    pub fn register_quick_add_shortcut(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let app_clone = app.clone();
        app.global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state == ShortcutState::Pressed {
                    let a = app_clone.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        if let Err(message) = quick_add_from_selection(&a) {
                            tracing::warn!("quick add: {message}");
                            crate::tray::show_skip(&a, &message);
                        }
                    });
                }
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
            .ok();
    }

    /// Unregisters a previously registered global shortcut (no-op if not registered).
    pub fn unregister_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey == "Fn" {
            #[cfg(target_os = "macos")]
            crate::fn_tap::stop();
            #[cfg(not(target_os = "macos"))]
            {
                let _ = app.global_shortcut().unregister(hotkey);
            }
        } else {
            #[cfg(target_os = "macos")]
            {
                if crate::mod_tap::bare_modifier_mask(hotkey).is_some() {
                    crate::mod_tap::stop();
                } else {
                    let _ = app.global_shortcut().unregister(hotkey);
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = app.global_shortcut().unregister(hotkey);
            }
        }
    }
}
