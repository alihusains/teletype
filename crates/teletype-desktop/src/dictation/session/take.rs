use crate::dictation::*;

impl Session {
    /// A start with no kind: plain dictation.
    pub(in crate::dictation) fn input(&mut self, input: Input) {
        self.input_with_kind(input, TakeKind::Dictation)
    }

    /// P2.3: every start goes through the state machine, tagged with the
    /// kind of take it begins. `hotkey_down_event` passes
    /// [`TakeKind::Dictation`]; a command take passes [`TakeKind::Command`].
    /// Because all kinds track the hotkey the same way, a Hold-mode release
    /// stops a command take exactly as it stops a dictation.
    ///
    /// The kind is applied ONLY when the state machine actually decides
    /// `Start` (inside `start`). Every other input — a release, a toggle
    /// stop, a press swallowed by the hotkey latch — leaves the in-flight
    /// take's kind alone. Tagging before `decide` (the original P2-A
    /// shape) retagged a command take as dictation on release, silently
    /// misrouting the whole feature.
    pub(in crate::dictation) fn input_with_kind(&mut self, input: Input, kind: TakeKind) {
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
            state::Action::Start { by_hotkey } => self.start(by_hotkey, kind),
            state::Action::Stop { cancelled } => self.stop(cancelled),
            state::Action::GoHandsFree => {
                self.hands_free = true;
            }
            state::Action::Discard => self.go_idle(),
            state::Action::CancelPipeline => {
                tracing::info!(
                    session = self.next_id,
                    "pipeline cancelled by user (hotkey); marking session cancelled"
                );
                // Same contract as the pill cancel: the session id is tagged
                // cancelled on this thread, so the in-flight worker's
                // `PipelineReady` is discarded when it reaches the queue.
                self.cancel_session(self.next_id);
                self.phase = Phase::Cancelled;
                #[cfg(test)]
                if self.headless_flash {
                    // Test path: no model managers.
                } else if let Some(state) = self.state() {
                    release_take_session(&state);
                }
                self.flash_note("Cancelled");
            }
        }
    }

    pub(in crate::dictation) fn start(&mut self, _by_hotkey: bool, kind: TakeKind) {
        // P2.2: the kind is set at start and read in `transcribed()`. The
        // old arm-flag was cleared before classification, so the take was
        // always misread as dictation; the kind travels with the session.
        self.take_kind = kind;
        // P2.9: a dedicated command take (hotkey, toggle, tray, hands-free)
        // is armed and starts right here. A wake-routed take is armed later,
        // in `transcribed()`: its recording began before the wake phrase
        // revealed it as a command, so `armed` lands without a second
        // `started`. Runs before the headless early return so tests
        // exercise the same emission as production.
        if kind == TakeKind::Command {
            self.ensure_trace();
            if let Some(key) = self.trace_key {
                self.trace_take_kind(key, kind);
                self.trace_command_event(key, "armed");
                self.trace_command_event(key, "started");
            }
        } else if let Some(key) = self.trace_key {
            self.trace_take_kind(key, kind);
        }
        #[cfg(test)]
        if self.headless_flash {
            // No mic, pill, or platform in headless tests: mark Listening
            // so the take can drive the pipeline phase, like a real take
            // whose mic opened successfully. P4: headless takes get their
            // streaming state from the test (which owns the mock engine),
            // so a fresh take starts with a clean slot. The start stamp is
            // planted now so the visible-recording floor (F-03) can be
            // measured from a known point.
            self.streaming = None;
            self.phase = Phase::Listening;
            self.started_at_ms = teletype_core::storage::now_ms();
            return;
        }
        let Some(settings) = self.state().map(|s| s.settings()) else {
            return;
        };
        let Some(app) = self.app().cloned() else {
            return;
        };
        // F-12: gate the press on the selected model being installed.
        // Recording with a missing model would fail on release with a
        // generic "Speech model not downloaded" flash, so the user speaks
        // into a take that can never transcribe. The pill message uses the
        // existing `flash` path (no overlay.rs or pill UI change).
        if !selected_speech_model_installed(
            &settings.selected_speech_model,
            &self
                .state()
                .map(|s| s.models_dir.clone())
                .unwrap_or_default(),
        ) {
            crate::log_entry(
                crate::LogLevel::Warn,
                "press gated: selected speech model is not installed",
            );
            self.flash("Speech model not downloaded. Open Teletype to download it.");
            return;
        }
        // P0.2: Toggle-mode and hands-free takes never see a hotkey press,
        // so make sure they still get a trace (stamped at start as the
        // closest available point to the press).
        self.ensure_trace();
        // BUG-003: mark the dictation session as in flight so the idle-unload
        // timer cannot fire mid-take.
        if let Some(state) = self.state() {
            each_speech_manager(&state, |m| m.begin_dictation());
        }
        let started_at_ms = teletype_core::storage::now_ms();
        self.started_at_ms = started_at_ms;
        // Monotonic marker for the speaking-rate figure. Reset here and
        // re-stamped below, once the mic is genuinely open, so the measured
        // duration is speech time rather than key-holding time.
        self.recording_started = None;
        self.phase = Phase::Listening;
        self.broadcast(teletype_core::state::UiState::Listening { started_at_ms });
        // Show the pill immediately, BEFORE the (slow) mic open, so the UI
        // responds instantly on keypress like Wispr Flow. The waveform begins
        // as soon as levels arrive.
        //
        // A cold press (the model was unloaded while the app sat idle) shows
        // the warm-up instead, and starts the load here rather than after the
        // mic opens. Painting the recording pill first and flipping to
        // "Getting model ready…" only once capture was live made the first
        // seconds of that take read as a dead hotkey: a flat waveform with no
        // explanation, while the press was in fact working. `WarmupDone`
        // repaints the recording pill when the model is ready.
        if !self.warm_on_press() {
            self.show(PillState::Recording {
                started_at_ms,
                command: kind == TakeKind::Command,
            });
        }

        // Record the paste target now, while the user is provably here: the
        // pipeline (ASR plus an optional 20 s LLM polish) finishes long after
        // this, and the delivery gate compares this snapshot against the
        // target at inject time. Capture is best-effort; a missing piece is
        // no evidence, never a mismatch.
        self.record_target = self.state().and_then(|state| {
            state.platform.active_application().map(|app| RecordTarget {
                bundle_id: app.application_id,
                app_name: app.application_name,
                pid: state.platform.active_pid(),
                frame: state.platform.focused_window_frame(),
            })
        });
        // P6.4: capture the focused screen context once, at the same moment
        // the paste target is recorded. Best-effort: a focused field that
        // cannot be read (no AX permission, no field, secure field) yields
        // `None` and the take proceeds with empty context, exactly as
        // before. The raw text is ephemeral — it feeds the paste-time
        // anchor and the engine boost list; only a privacy-safe summary may
        // reach a trace.
        self.screen_context = self.capture_screen_context();
        // P7.5: build the take's boost list now that the screen context is
        // known. Cached by lexicon fingerprint + category + screen hash, so
        // back-to-back takes in the same context reuse it. An empty or failed
        // build just means the take runs unboosted.
        self.take_boost_terms = self.build_boost_terms();

        // Escape recovery: start the PCM spool for this take. Failure is
        // non-fatal — the recording proceeds without a recovery spool.
        let spool = self
            .state()
            .and_then(|s| match crate::recovery::Spool::begin(&s.data_dir) {
                Ok(s) => Some(s),
                Err(e) => {
                    crate::log_entry(crate::LogLevel::Warn, format!("recovery spool: {e}"));
                    None
                }
            });
        let spool_cb = spool.as_ref().and_then(|s| s.clone_writer());
        self.spool = spool;
        // F-02: the take's shared block resampler, owned by the frame
        // callback (created on the first frame) and flushed into the take
        // at take end. `None` for synthetic (e2e) takes, which have no
        // frame callback.
        self.take_resampler = Some(Arc::new(Mutex::new(None)));
        // HARD-4 row 2: clear the previous take's flag here; the take gets
        // the live recording's own flag once the mic opens (see the Ok arm
        // below). A stale `true` from an earlier take can never leak into
        // this one because every recording carries a freshly created flag.
        self.device_error_flag = None;
        // Phase 5.6: lower the output volume while the take records (and
        // optionally send the media play/pause key), so playback audio
        // stops bleeding into the microphone. Fire-and-forget: a failure
        // never breaks a dictation. `duck` is a no-op when already active
        // (a second take while the first is still ducking).
        self.duck_armed = false;
        if settings.duck_other_audio {
            let level = settings.duck_level;
            let mut ducker = self
                .ducker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Err(e) = ducker.duck(level) {
                crate::log_entry(crate::LogLevel::Warn, format!("audio duck: {e}"));
            }
            if ducker.active() {
                self.duck_armed = true;
            }
            drop(ducker);
        }
        if settings.pause_media_while_dictating {
            let _ = crate::ducking::toggle_media_play_pause();
        }
        // Phase 5.1: a new take starts with a clean pre-roll slot; the
        // take-start block below (recording OK + front end armed) fills it.
        self.preroll_this_take = None;

        let streaming_slot = self.streaming_for_take(&settings, &app);
        let recording = if self.controller.e2e.is_some() {
            // P9.6: file input replaces the audio SOURCE as well as the
            // trigger. Opening the real mic here made the final decode read
            // the room (the fed file only reached the streaming committer),
            // so every harness take transcribed ambient noise.
            self.take_resampler = None;
            Ok(Recording::synthetic())
        } else if self.vad_enabled_for() {
            self.start_with_vad(spool_cb)
        } else {
            // P4: the streaming frame callback is installed for EVERY take
            // (not only hands-free auto-stop takes): the capture thread feeds
            // the streaming take state on every frame, so mid-take commits
            // happen regardless of the auto-stop setting. The auto-stop
            // detector stays gated by `vad_enabled_for()` above — its
            // semantics are unchanged.
            // F-02: the frame callback owns the rate slot the audio layer
            // publishes the device rate into (see `start_with_vad`).
            let rate_slot: Arc<std::sync::atomic::AtomicU32> =
                Arc::new(std::sync::atomic::AtomicU32::new(0));
            let on_frame = self.build_frame_callback(
                spool_cb,
                Arc::new(Mutex::new(None)),
                streaming_slot.clone(),
                None,
                self.controller.clone(),
                self.next_id,
                rate_slot.clone(),
            );
            let dfn3_slot = if settings.noise_reduction_enabled {
                Some(teletype_core::audio::build_dfn3_slot(
                    &self
                        .state()
                        .map(|s| s.models_dir.clone())
                        .unwrap_or_default(),
                    48_000,
                ))
            } else {
                None
            };
            // T10a: the level stream is throttled to ~20 Hz per take so a
            // 48–480 Hz capture rate cannot flood the pill's reducer. The
            // throttle is shared with the capture thread through a mutex:
            // the callback is `Fn` (the audio crate calls it repeatedly),
            // so the mutability lives inside.
            let level_throttle = Arc::new(Mutex::new(PillLevelThrottle::new()));
            Recording::start_with_vad_df(
                &settings.input_device,
                move |level| {
                    let mut t = level_throttle
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if t.should_emit(teletype_core::storage::now_ms(), level) {
                        let _ = app.emit_to("pill", "pill-level", level);
                    }
                },
                on_frame,
                Some(rate_slot),
                settings.noise_reduction_enabled,
                dfn3_slot,
            )
        };
        match recording {
            Ok(recording) => {
                // HARD-4 row 2: hand the take the same Arc the cpal stream
                // error callback holds, so a mid-take device failure is
                // visible to `stop()` without any extra plumbing.
                self.device_error_flag = Some(recording.device_error_flag());
                // P4: the streaming state is shared with the capture thread
                // from here on; `stop()` moves it into the release worker.
                if let Some(slot) = streaming_slot {
                    self.streaming = Some(slot);
                    // P4.3b: the capture thread needs the app handle for
                    // the deferred per-segment polish install.
                    self.polish_app = Some(Arc::new(Mutex::new(self.app().cloned())));
                    // Phase 5.7: the flag collector is only useful when the
                    // take has a streaming state (whose VAD detector feeds
                    // it); no detector -> no flags -> quality reports only
                    // `clipped`.
                    self.vad_flags = Some(Arc::new(Mutex::new(VadFlagCollector::new())));
                }
                // Phase 5.1: when the pre-warm front end is armed, the take
                // starts with the last 300 ms of pre-roll. Feed it into the
                // streaming take NOW so the first words land in the
                // streaming take's first committed segment, and keep a copy
                // for the release worker to prepend to the ASR samples.
                let prewarm_armed = self
                    .front_end
                    .as_ref()
                    .is_some_and(|fe| fe.state() == FrontEndState::Armed);
                if prewarm_armed {
                    if let Some(fe) = self.front_end.as_ref() {
                        let raw = fe.take_preroll(300);
                        if !raw.is_empty() {
                            let rate = fe.rate();
                            self.feed_capture_frame(&raw, rate);
                            self.preroll_this_take = Some((raw, rate));
                        }
                    }
                }
                // The mic is genuinely open now, so this is the honest moment
                // for the cue — see `play_cue`.
                self.play_cue();
                // Spawn the live-preview interim loop before storing the
                // recording, so it can grab a handle to the live buffer.
                // BUG-011: the ported pill styles (Level Rail, Reading Well,
                // Dot Matrix) all render `interimText`, so Hold mode must feed
                // it too — a pill that promises live text but shows only the
                // recording animation for the entire take reads as broken.
                let session = self.next_id;
                // The user can switch the live transcript off in Settings
                // (it costs a re-decode per interim tick); the pill then
                // shows only the recording animation for the take.
                let live_preview_on = self
                    .state()
                    .map(|s| s.settings().live_preview_enabled)
                    .unwrap_or(false);
                self.live_preview = live_preview_on
                    .then(|| recording.live_preview())
                    .flatten()
                    .and_then(|(buf, rate)| {
                        self.app()
                            .and_then(|app| spawn_live_preview(app.clone(), buf, rate, session))
                    });
                // Store the recording in a thread-local so `stop` can access it.
                RECORDING.with(|slot| *slot.borrow_mut() = Some(recording));
                // The mic is genuinely open now; start the speaking-rate
                // clock. `start` reset this to `None` so the measured
                // duration is speech time, not key-holding time.
                self.recording_started = Some(std::time::Instant::now());
                // P0.2: closest available point to "first audio" — the mic
                // is open and frames are flowing from here on.
                if let Some(key) = self.trace_key {
                    if let Some(t) = self.traces.get_mut(&key) {
                        t.mark(|s, now| s.t_first_audio = Some(now));
                    }
                }
                // The speech-model load already started at the press (see
                // `warm_on_press`), so it overlaps this mic open instead of
                // following it. No second warm here: another
                // `load_speech_model` would bump the warm token and supersede
                // the in-flight load, giving two concurrent loads and dropping
                // the first `WarmupDone`.
                // Pre-warm the LLM (polish) model in the background while the
                // user is still talking. `ensure_local_provider` no-ops if a
                // provider is already loaded, and the `llm_loading` guard
                // prevents concurrent spawns. By the time transcription
                // finishes, the server is hot and the first inference doesn't
                // pay the cold-start penalty. No effect on the recording path.
                if let Some(app) = self.app().cloned() {
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
                // F-02: the mic never opened, so the frame callback never
                // ran; drop the take's resampler slot with the take.
                self.take_resampler = None;
                // BUG-003: the mic never opened, so no dictation will
                // complete — end the session now instead of leaving `active`
                // stuck true. The model itself stays resident (D052).
                if let Some(state) = self.state() {
                    release_take_session(&state);
                }
                self.flash(&e);
            }
        }
    }

    /// VAD auto-stop is useful only when nothing else bounds the take:
    /// hands-free hold mode (double-tap), where the key release does nothing
    /// and the pause is the only stop signal. Plain hold stops on release;
    /// push-to-talk stops on the second tap; both preempt VAD.
    pub(in crate::dictation) fn vad_enabled_for(&self) -> bool {
        self.state()
            .map(|s| s.settings().vad_auto_stop)
            .unwrap_or(false)
            && self.hands_free
    }

    /// Starts a recording with the VAD detector armed. The detector lives in
    /// a shared slot the capture thread feeds; on `UtteranceComplete` it
    /// posts `VadFired` to this controller, which stops the take.
    pub(in crate::dictation) fn start_with_vad(
        &mut self,
        spool_cb: Option<crate::recovery::SpoolWriter>,
    ) -> Result<Recording, String> {
        let Some(settings) = self.state().map(|s| s.settings()) else {
            return Err("no app state available".to_string());
        };
        let Some(app) = self.app().cloned() else {
            return Err("no app handle available".to_string());
        };
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
                let level_throttle = Arc::new(Mutex::new(PillLevelThrottle::new()));
                return if settings.noise_reduction_enabled {
                    Recording::start_with_noise(&settings.input_device, move |level| {
                        let mut t = level_throttle
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if t.should_emit(now_ms(), level) {
                            let _ = app.emit_to("pill", "pill-level", level);
                        }
                    })
                } else {
                    Recording::start(&settings.input_device, move |level| {
                        let mut t = level_throttle
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        if t.should_emit(now_ms(), level) {
                            let _ = app.emit_to("pill", "pill-level", level);
                        }
                    })
                };
            }
        };
        // The device's native rate is published here once the capture thread
        // opens the stream; the VAD callback reads it on its first frame so
        // the silence window is timed against real wall-clock time (not the
        // engine's 16 kHz assumption).
        let rate_slot: Arc<std::sync::atomic::AtomicU32> =
            Arc::new(std::sync::atomic::AtomicU32::new(0));
        let streaming_slot = self.streaming_for_take(&settings, &app);
        let noise_reduction = settings.noise_reduction_enabled;
        // Plan §5.5: Smart mode (noise_reduction_enabled == true) uses the
        // DFN3 split pipeline; the ASR path stays raw, the detection path
        // runs through DFN3. Off mode (false) uses the legacy path.
        let dfn3_slot = if noise_reduction {
            Some(teletype_core::audio::build_dfn3_slot(
                &self
                    .state()
                    .map(|s| s.models_dir.clone())
                    .unwrap_or_default(),
                48_000,
            ))
        } else {
            None
        };
        let on_frame = self.build_frame_callback(
            spool_cb,
            vad.clone(),
            streaming_slot.clone(),
            None,
            controller,
            session,
            rate_slot.clone(),
        );
        let level_throttle = Arc::new(Mutex::new(PillLevelThrottle::new()));
        let recording = Recording::start_with_vad_df(
            &settings.input_device,
            move |level| {
                let mut t = level_throttle
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if t.should_emit(now_ms(), level) {
                    let _ = app.emit_to("pill", "pill-level", level);
                }
            },
            on_frame,
            Some(rate_slot),
            noise_reduction,
            dfn3_slot,
        );
        if recording.is_ok() {
            self.vad = Some(vad);
            if let Some(slot) = streaming_slot {
                self.streaming = Some(slot);
                // P4.3b: the capture thread needs the app handle for the
                // deferred per-segment polish install.
                self.polish_app = Some(Arc::new(Mutex::new(self.app().cloned())));
                // Phase 5.7: same rule as the non-VAD path — the collector
                // only exists when the streaming take state exists.
                self.vad_flags = Some(Arc::new(Mutex::new(VadFlagCollector::new())));
            }
        }
        recording
    }

    /// P4: build the streaming take state for a new take, or `None` when
    /// streaming is disabled, the Silero model cannot be loaded, or the
    /// selected engine cannot be resolved (the take then decodes the whole
    /// audio after release, exactly as before). Owns its own committer
    /// (its own engine handle, mirroring the three `SpeechModelManager`s
    /// the app builds for the selected engine kind).
    pub(in crate::dictation) fn streaming_for_take(
        &mut self,
        settings: &crate::commands::Settings,
        app: &AppHandle,
    ) -> Option<StreamingSlot> {
        if !STREAMING_COMMIT_DEFAULT {
            return None;
        }
        let (model_path, kind) =
            resolve_speech_model(settings, &app.state::<AppState>().models_dir);
        let language = self.effective_language(kind);
        let vad = match VadDetector::silero_16k() {
            Ok(vad) => vad,
            Err(e) => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("streaming VAD init failed: {e}; take decodes whole after release"),
                );
                return None;
            }
        };
        // The committer's own engine handle, mirroring the provider the
        // selected engine kind uses in the main managers.
        let model_id = settings.selected_speech_model.clone();
        let mut provider = match streaming_provider_for(kind, &model_id) {
            Some(provider) => provider,
            None => {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("streaming off: no {kind:?} streaming provider"),
                );
                return None;
            }
        };
        crate::log_entry(
            crate::LogLevel::Info,
            format!(
                "streaming take armed: {kind:?} engine, model {}",
                model_path.display()
            ),
        );
        // P7.5: the streaming path decodes segments as the user speaks, so the
        // engine needs the boost list before the first segment is submitted.
        // Whisper accepts it unloaded (it feeds the initial prompt); a failure
        // only means the segments decode unboosted.
        if !self.take_boost_terms.is_empty() {
            if let Err(e) = provider.set_boost_terms(&self.take_boost_terms) {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("streaming boost terms not applied: {e}"),
                );
            }
        }
        let committer = crate::committer::Committer::new(provider);
        // P4 completion: the committer's provider is constructed UNLOADED
        // (an empty path), so streaming would never actually commit — every
        // segment decode failed with `NoModel` and the take fell back to the
        // whole-take decode. Enqueue a background load now, resolving the
        // model path with the SAME per-engine semantics as the warm-up and
        // release path (`resolve_speech_model`): Whisper/Parakeet →
        // `<models_dir>/<file>`, Fluid → `<models_dir>/fluid`, Sherpa →
        // `<models_dir>/sherpa/<file>`, Whistle → `<models_dir>/whistle.cact`.
        // The load jumps the committer's queue (before any segment), runs on
        // the worker thread, and never blocks the recording-start path. A
        // failed load is harmless: the segments then fail per-result and the
        // take falls back byte-identically, exactly as before.
        committer.load_model(model_path);
        Some(Arc::new(Mutex::new(Some(StreamingTake {
            buf16: Vec::new(),
            base_offset: 0,
            segmenter: Segmenter::with_rate(SegmenterConfig::default(), 16_000),
            vad,
            carry: Vec::new(),
            last_commit_end: 0,
            committer,
            language,
        }))))
    }

    /// P4: the capture-thread frame callback. Resamples the device-rate
    /// frame to 16 kHz ONCE, in 100 ms blocks ([`BlockResampler`]), and
    /// shares the result between the escape-recovery spool, the streaming
    /// take state (when streaming is on), and the per-frame VAD flag
    /// collector (Phase 5.7). The auto-stop detector is fed the
    /// DEVICE-rate frame exactly as before (its own 512-sample carry
    /// inside `push_frame` plus `set_sample_rate` for wall-clock timing),
    /// and the `None`-drop-on-fire behavior is preserved.
    ///
    /// `rate_slot` is the slot the audio layer publishes the device's
    /// native rate into before the first frame. The callback owns one
    /// `BlockResampler` per take, created lazily on the first frame once
    /// the rate is known; the take-end flush runs in
    /// [`Self::flush_resampler_into_take`].
    #[allow(
        clippy::too_many_arguments,
        clippy::type_complexity // `Box<dyn Fn(&[f32]) + Send + Sync + 'static>`
                                // is the callback shape the audio layer wants.
    )]
    pub(in crate::dictation) fn build_frame_callback(
        &mut self,
        spool_cb: Option<crate::recovery::SpoolWriter>,
        vad: VadSlot,
        streaming: Option<StreamingSlot>,
        flags: Option<std::sync::Arc<std::sync::Mutex<VadFlagCollector>>>,
        controller: Controller,
        session: u64,
        rate_slot: Arc<std::sync::atomic::AtomicU32>,
    ) -> Option<Box<dyn Fn(&[f32]) + Send + Sync + 'static>> {
        let rate_cb = rate_slot.clone();
        let vad_cb = vad.clone();
        let streaming_cb = streaming.clone();
        let flags_cb = flags.clone();
        // One block resampler per take, created on the first frame once
        // the device rate is known. Shared with the take-end flush
        // ([`Self::take_resampler`]); the callback is `Fn` (the audio
        // layer calls it repeatedly), so the mutability lives inside.
        // A missing slot (headless tests) gets a private resampler so the
        // callback still works.
        let resampler = self
            .take_resampler
            .clone()
            .unwrap_or_else(|| Arc::new(std::sync::Mutex::new(None)));
        Some(Box::new(move |frame| {
            // F-02: resample ONCE at the top, in 100 ms blocks, and reuse
            // the result for the spool, the streaming feed, and the flag
            // collector. The auto-stop detector still gets the DEVICE-rate
            // frame below (its own carry), so its semantics are unchanged.
            let r16: Vec<f32> = {
                let r = rate_cb.load(std::sync::atomic::Ordering::Relaxed);
                if r == 0 {
                    Vec::new()
                } else {
                    let mut guard = resampler
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let inner = guard.get_or_insert_with(|| BlockResampler::new(r));
                    inner.push(frame)
                }
            };
            // Escape-recovery spool: hand the 16 kHz frame to the
            // non-blocking spool writer.
            if let Some(w) = spool_cb.as_ref() {
                if !r16.is_empty() {
                    w.write(&r16);
                }
            }
            // VAD auto-stop: semantics unchanged from before P4.
            let mut last_prob = 0.0f32;
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
                        let ev = d.push_frame(frame);
                        last_prob = d.last_probability();
                        ev
                    }
                    None => {
                        // Detector already fired and dropped: no more
                        // auto-stop, but the streaming path still runs
                        // below (the take continues until release).
                        teletype_core::vad::VadEvent::Continue
                    }
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
            if r16.is_empty() {
                return;
            }
            // P4: feed the streaming take state (already 16 kHz; never
            // hold the lock while resampling).
            if let Some(streaming_slot) = &streaming_cb {
                let mut guard = streaming_slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(take) = guard.as_mut() {
                    take.feed_frame(&r16, 16_000);
                }
            }
            // Phase 5.7: one flag per 512-sample (32 ms) 16 kHz frame.
            // When the auto-stop detector has fired/dropped, reuse the last
            // observed probability (the take keeps running until release).
            // No detector -> no flags (quality then reports only `clipped`).
            if let Some(flags_slot) = &flags_cb {
                let mut guard = flags_slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                guard.push(&r16, last_prob);
            }
        }))
    }

    /// F-02: take-end flush. Pushes the `BlockResampler`'s leftover
    /// (the trailing partial 100 ms block) into the same consumers the
    /// frame callback feeds: the streaming take and the flag collector.
    /// The escape-recovery spool is flushed by the audio layer's own
    /// take-end resample of the raw buffer, so it is not fed here.
    pub(in crate::dictation) fn flush_resampler_into_take(
        &mut self,
        resampler: &std::sync::Mutex<Option<BlockResampler>>,
    ) {
        let tail = {
            let mut guard = resampler
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.as_mut().map(BlockResampler::flush)
        };
        let Some(tail) = tail.filter(|t| !t.is_empty()) else {
            return;
        };
        if let Some(streaming_slot) = &self.streaming {
            let mut guard = streaming_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(take) = guard.as_mut() {
                take.feed_frame(&tail, 16_000);
            }
        }
        if let Some(flags_slot) = &self.vad_flags {
            let mut guard = flags_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            guard.push(&tail, 0.0);
        }
    }

    /// P4: feed one captured device-rate frame into the in-flight take's
    /// streaming state. The real capture callback calls this (via
    /// [`Self::build_frame_callback`]); headless tests call it directly
    /// with synthetic frames so the REAL controller path is exercised
    /// without a microphone. No-op when the take has no streaming state
    /// (streaming disabled, VAD init failed, or the mic never opened).
    pub fn feed_capture_frame(&mut self, frame: &[f32], rate: u32) {
        if rate == 0 {
            return;
        }
        let streaming = match &self.streaming {
            Some(s) => s.clone(),
            None => return,
        };
        // P4.3b: clone the polish app slot before taking the streaming
        // lock (the install takes `&mut self`, so it cannot run while a
        // borrow of `self.polish_app` is live).
        let app_slot = self.polish_app.clone();
        // Resample OUTSIDE the lock (the task's explicit rule: never hold
        // the streaming lock while calling `resample_to_target`).
        let Some(r16) = resample_to_target(frame, rate) else {
            return;
        };
        let mut install = None;
        {
            let mut guard = streaming
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(take) = guard.as_mut() {
                take.feed_frame(&r16, 16_000);
                // P4.3b: remember the take's audio length once the
                // threshold is crossed (the install runs after the lock
                // is released).
                if let Some(slot) = app_slot.as_ref() {
                    let len = take.buf16.len();
                    if crate::committer::tail_audio_ms(len, 16_000)
                        >= crate::committer::TOTAL_AUDIO_SEGMENTED_POLISH_MS as u32
                    {
                        install = Some((slot, len));
                    }
                }
            }
        }
        if let Some((slot, len)) = install {
            let app = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(app) = app {
                let state = app.state::<AppState>();
                self.install_segment_polish(&streaming, &state, len);
            }
        }
    }

    /// P9.6: the e2e feeder's entry point. Feeds one 16 kHz frame of the
    /// file's audio into the in-flight take's streaming state (the same
    /// path the capture callback uses). No-op when the take is not
    /// recording or has no streaming state.
    pub fn e2e_feed(&mut self, frame16k: &[f32]) {
        if self.phase != Phase::Listening {
            return;
        }
        // The file is this take's audio source, so it must reach the release
        // buffer too: the final decode reads that buffer, not the streaming
        // state, whenever no mid-take segment was committed.
        // e2e takes are file-fed but still drive the real stop path, so they
        // carry the take's own device-error flag. A synthetic recording has
        // no cpal stream, so nothing ever sets it; the harness can, which is
        // how the row-2 test proves `stop` reads the same object the capture
        // side writes.
        let mut feed_flag = None;
        RECORDING.with(|slot| {
            if let Some(rec) = slot.borrow().as_ref() {
                rec.push(frame16k);
                feed_flag = Some(rec.device_error_flag());
            }
        });
        if feed_flag.is_some() {
            self.device_error_flag = feed_flag;
        }
        let streaming = match &self.streaming {
            Some(s) => s.clone(),
            None => return,
        };
        let mut guard = streaming
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(take) = guard.as_mut() {
            take.feed_frame(frame16k, 16_000);
        }
    }

    /// P4.3b (plan §4.3): install the per-segment polish function on a
    /// long take's committer. Called from the capture thread (via
    /// [`Self::feed_capture_frame`]) once the take's audio crosses
    /// [`crate::committer::TOTAL_AUDIO_SEGMENTED_POLISH_MS`] — the take's
    /// length is not known at take start, so the install is deferred to
    /// the first frame past the threshold. The take buffer is monotonically
    /// growing, so once it crosses it never un-crosses, and the install is
    /// idempotent.
    ///
    /// Runs on the capture thread, never on the controller. The
    /// `AppState` reads are short lock clones (the same seam the pipeline
    /// worker uses); the LLM call itself happens on the committer's worker
    /// thread. No text is logged. Takes `&AppState` (resolved at the call
    /// site) so headless unit tests can exercise it without a Tauri app.
    pub(in crate::dictation) fn install_segment_polish(
        &mut self,
        streaming_slot: &StreamingSlot,
        state: &AppState,
        buf16_len: usize,
    ) {
        const SAMPLE_RATE: u32 = 16_000;
        if crate::committer::tail_audio_ms(buf16_len, SAMPLE_RATE)
            < crate::committer::TOTAL_AUDIO_SEGMENTED_POLISH_MS as u32
        {
            return;
        }
        let settings = state.settings();
        if settings.polish_whole_text_at_end {
            return;
        }
        let has_provider = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some();
        if !has_provider {
            return;
        }
        let Some(f) = build_segment_polish_fn(state) else {
            // No auto-apply transform: nothing to run per segment; the
            // release path behaves exactly as today.
            return;
        };
        // Install under the streaming lock so the committer's own `polish`
        // mutex swap is atomic with respect to the worker's reads.
        let mut g = streaming_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(t) = g.as_mut() {
            t.committer.set_polish_fn(f);
        }
    }

    pub(in crate::dictation) fn play_cue(&self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let Some(settings) = self.state().map(|s| s.settings()) else {
            return;
        };
        if settings.play_recording_sounds {
            play_cue(&settings.recording_sound);
        }
    }

    /// Item 2: pre-warm for the take in progress. Owns the pill (shows
    /// "Warming", then takes back the recording pill through `WarmupDone`)
    /// and is the only warm the timed policies need. Returns true when the
    /// warming pill was shown, i.e. a load is in flight.
    pub(in crate::dictation) fn warm_up(&mut self) -> bool {
        self.load_speech_model(true)
    }

    /// The press-time warm. When the policy loads on press and the selected
    /// model is not resident (the app unloaded it while idle), this starts the
    /// load before the pill is chosen so the take opens on "Getting model
    /// ready…" and the load overlaps the mic open. Returns true when the
    /// warming pill owns the pill now; the caller must not paint the recording
    /// pill over it.
    pub(in crate::dictation) fn warm_on_press(&mut self) -> bool {
        if !press_warms_visibly(self.engine_tier(), self.speech_model_ready()) {
            return false;
        }
        self.warm_up()
    }

    /// True when the selected speech model is already loaded, so a press can
    /// go straight to the recording pill. The same rule `load_speech_model`
    /// uses to skip a redundant load.
    pub(in crate::dictation) fn speech_model_ready(&self) -> bool {
        let Some(state) = self.state() else {
            return false;
        };
        let settings = state.settings();
        let (_path, kind) = resolve_speech_model(&settings, &state.models_dir);
        speech_model_ready_for(&state, kind, &settings.selected_speech_model)
    }

    /// Item 2: the same load with no pill and no session id, for warming that
    /// happens *outside* a take (the `always` policy: once at startup, and
    /// again after a take ends). `WarmupDone` is deliberately not sent for
    /// these: the handler compares it against `next_id` to repaint a
    /// recording pill, so an idle load reporting one could repaint a pill a
    /// newer take owns, and bumping `next_id` here would invalidate the
    /// session guards of a take that starts while the load is in flight.
    pub(in crate::dictation) fn warm_in_background(&mut self) {
        let _ = self.load_speech_model(false);
    }

    /// Loads the selected speech model in the background so the first
    /// dictation (or a model switch) doesn't block the release, and shows the
    /// warming pill while the load is in flight. Returns true when it announced
    /// the load by showing `PillState::Warming` (i.e. a `WarmupDone` or
    /// `WarmupFailed` will follow for `warm_token`), false when there was
    /// nothing to load or nothing to load it with.
    pub(in crate::dictation) fn load_speech_model(&mut self, announce: bool) -> bool {
        let Some(state) = self.state() else {
            return false;
        };
        let (model_path, kind, model_id) = {
            let settings = state.settings();
            let (path, kind) = resolve_speech_model(&settings, &state.models_dir);
            (path, kind, settings.selected_speech_model)
        };
        // Core ML downloads on first load (the provider owns the checkpoint
        // layout), so its base directory need not exist yet.
        // Core ML and Apple speech download on first load (the provider owns
        // the asset layout), so their base directories need not exist yet.
        if !matches!(
            kind,
            SpeechEngineKind::Fluid | SpeechEngineKind::AppleSpeech
        ) && !model_path.exists()
        {
            return false;
        }
        // Already loaded → skip the warming pill entirely; the model is
        // resident and the recording pill can stay as-is.
        if speech_model_ready_for(&state, kind, &model_id) {
            return false;
        }
        // Resolve the app handle before claiming the pill: an early return
        // after `show(PillState::Warming)` would leave a warm pill that no
        // `WarmupDone` ever takes back.
        let Some(app) = self.app().cloned() else {
            return false;
        };
        // F-11: the warm-up must not bump the take's session id. The
        // `VadFired` handler (hands-free auto-stop) requires
        // `next_id == session`, so an id bump here would invalidate the
        // session guard of the take that is recording and drop the auto-stop
        // on every cold take. The warm token is its own counter: it only
        // needs to be unique per warm-up, so a stale `WarmupDone` from a
        // superseded warm is dropped, and it never collides with the
        // session-id space the take owns.
        let warm_token: Option<u64> = if announce {
            self.show(PillState::Warming);
            self.warm_token += 1;
            Some(self.warm_token)
        } else {
            None
        };
        let controller = self.controller.clone();
        crate::log_entry(
            crate::LogLevel::Info,
            format!(
                "loading speech model: {} ({:?})",
                model_path.display(),
                kind
            ),
        );
        let started = std::time::Instant::now();
        thread::Builder::new()
            .name("teletype-warmup".into())
            .spawn(move || {
                let state = app.state::<AppState>();
                if matches!(
                    kind,
                    SpeechEngineKind::Fluid
                        | SpeechEngineKind::Sherpa
                        | SpeechEngineKind::Qwen3Asr
                        | SpeechEngineKind::AppleSpeech
                        | SpeechEngineKind::TranscribeCpp
                ) {
                    if let Err(e) = ensure_neural_provider(&state, &model_id) {
                        crate::log_entry(crate::LogLevel::Error, format!("warm-up failed: {e}"));
                        if let Some(token) = warm_token {
                            controller.send(Event::WarmupFailed {
                                session: token,
                                reason: warmup_failure_reason_str(&e, &model_path),
                            });
                        }
                        return;
                    }
                }
                // BUG-003 P0: the manager was constructed with an empty path;
                // set the real resolved path before loading.
                let manager = match kind {
                    SpeechEngineKind::Fluid
                    | SpeechEngineKind::Sherpa
                    | SpeechEngineKind::Whistle
                    | SpeechEngineKind::Qwen3Asr
                    | SpeechEngineKind::AppleSpeech
                    | SpeechEngineKind::TranscribeCpp => &state.neural,
                    SpeechEngineKind::Parakeet => &state.parakeet,
                    SpeechEngineKind::ParakeetCpp => &state.parakeet_cpp,
                    SpeechEngineKind::Whisper => &state.speech,
                };
                manager.set_model_path(model_path.clone());
                let result = manager.load();
                match result {
                    Ok(()) => crate::log_entry_ms(
                        crate::LogLevel::Success,
                        format!("speech model loaded: {} ({:?})", model_path.display(), kind),
                        started.elapsed().as_millis() as u64,
                    ),
                    Err(e) => {
                        crate::log_entry(crate::LogLevel::Error, format!("warm-up failed: {e}"));
                        if let Some(token) = warm_token {
                            // The load failed: end warming with an error
                            // state instead of a bare WarmupDone that would
                            // repaint the recording pill over a model that
                            // will fail again on release.
                            controller.send(Event::WarmupFailed {
                                session: token,
                                reason: warmup_failure_reason_str(&e, &model_path),
                            });
                        }
                        return;
                    }
                }
                if let Some(token) = warm_token {
                    controller.send(Event::WarmupDone { session: token });
                }
            })
            .ok();
        warm_token.is_some()
    }

    pub(in crate::dictation) fn stop(&mut self, cancelled: bool) {
        // A clean stop (a release that transcribes) carries no note: the
        // pill moves to the Transcribing state, not to a flash. A cancelled
        // stop shows the note. `cancel_note` is empty exactly for the clean
        // release, which is what the headless path in `stop_with_note` uses
        // to tell "stopped and transcribing" from "discarded silently".
        let note = if cancelled { "Cancelled" } else { "" };
        self.stop_with_note(cancelled, note);
    }

    /// `cancel_note` is what the pill shows after a cancelled take. The
    /// cancel path is shared by the pill's Cancel button ("Cancelled") and a
    /// quick-tap clipboard paste ("Pasted").
    pub(in crate::dictation) fn stop_with_note(&mut self, cancelled: bool, cancel_note: &str) {
        // No headless early return: the platform-touching calls below
        // (cue, broadcast, flash, pill, model managers) each guard on
        // `headless_flash`, so a headless session runs this real path,
        // including the silent-discard rule on the cancel branch.
        // P0.2: the release is the latency baseline every later claim is
        // measured against (release→paste).
        if let Some(key) = self.trace_key {
            if let Some(t) = self.traces.get_mut(&key) {
                t.mark(|s, now| s.t_hotkey_up = Some(now));
            }
        }
        // The take is over, so close the cue. Played for a cancel too: the
        // recording did stop, and silence here would read as "still listening"
        // — which for a cancel is exactly the state the user just escaped.
        self.play_cue();
        // Disarm VAD for this take: the capture thread's callback becomes a
        // no-op, and a late fire from the old session is dropped by the
        // session check in the `VadFired` handler.
        self.vad = None;
        // F-02: flush the block resampler's leftover (the trailing partial
        // 100 ms block) into the streaming take and the flag collector
        // BEFORE the capture thread drops its callback. The escape-recovery
        // spool is not fed here: the audio layer's take-end resample of the
        // raw buffer already covers it (see `run_capture`).
        if let Some(resampler) = self.take_resampler.take() {
            self.flush_resampler_into_take(&resampler);
        }
        // P4: the streaming state travels with the release. On a cancel it
        // is dropped outright (the committer's worker exits on drop); on a
        // clean release it is moved into the transcribe worker, which
        // finishes the committer with the uncommitted tail. The capture
        // thread's callback sees `None` and stops feeding.
        //
        // Moved (not re-taken) into the worker below: this is the ONLY take,
        // so the worker always receives the take. Re-taking `self.streaming`
        // there yielded `None` and silently forced every production take
        // through the whole-take fallback.
        let mut streaming = self.streaming.take();
        // P4.3b: the polish install's app handle travels with the release
        // (dropped on cancel, released on a clean stop).
        self.polish_app.take();
        // Phase 5.7: the flag collector travels with the release like
        // `streaming` (dropped on cancel, moved into the worker on a clean
        // release). Phase 5.1: the pre-roll for this take.
        let vad_flags = self.vad_flags.take();
        let preroll = self.preroll_this_take.take();
        // Stop the live-preview interim loop. P1.4: this sets the cancel
        // flag and signals the thread but does NOT join it — the final
        // transcribe must not wait for an in-flight interim decode to
        // release the speech-model lock. The interim thread abandons the
        // decode on its own; `go_idle()` joins it once the final pass is
        // done and no lock contention is possible.
        if let Some(handle) = self.live_preview.take() {
            handle.stop();
        }
        // Stop capture BEFORE joining the recovery spool (2026-10-19 pill-stuck
        // incident): the capture thread's `on_frame` closure holds a
        // `SpoolWriter` clone that keeps the spool channel open, so joining
        // the spool writer while capture is still live blocked the controller
        // thread forever — the pill froze and every input went dead. This is
        // a non-blocking signal: the worker's `recording.finish()` below still
        // drains and receives the done message as before.
        RECORDING.with(|slot| {
            if let Some(rec) = slot.borrow().as_ref() {
                rec.stop_capture();
            }
        });
        // Clean stop: the take finished (or was cancelled), so the recovery
        // spool is no longer needed — delete it.
        if let Some(spool) = self.spool.take() {
            spool.complete();
        }
        // Phase 5.6: put the output volume back (and re-send the media
        // key). Runs on EVERY stop path — clean release, cancel, and the
        // early return when the mic never opened — because `stop` is the
        // one function every take end flows through. `restore` is a
        // warning no-op when the ducker was never active.
        if self.duck_armed {
            if let Err(e) = self
                .ducker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .restore()
            {
                crate::log_entry(crate::LogLevel::Warn, format!("audio duck restore: {e}"));
            }
            self.duck_armed = false;
            // The media key is a platform service; a headless session has
            // no app, so skip it (the ducker restore above is pure state).
            #[cfg(test)]
            if self.headless_flash {
                // Test path: no media key to re-send.
            } else if self
                .state()
                .map(|s| s.settings().pause_media_while_dictating)
                .unwrap_or(false)
            {
                let _ = crate::ducking::toggle_media_play_pause();
            }
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
        // HARD-4 row 2: the device stopped responding mid-take (the cpal
        // stream error callback set the flag). Read the flag and the
        // captured duration NOW, before the session id advances below, so
        // the trace can carry them. The take still transcribes and pastes
        // the audio it captured; the worker flashes the device-loss message
        // after the text is delivered. A zero-sample take (the device died
        // before any audio) keeps today's mic-failure message: the worker
        // routes that error before the device-loss flash would run.
        let device_lost = self
            .device_error_flag
            .as_ref()
            .is_some_and(|f| f.load(std::sync::atomic::Ordering::SeqCst));
        // The duration the take captured before the device died. `speech_ms`
        // above took the stop-clock reading (and the test path sets
        // `started_at_ms` directly), so measure from the take's own start
        // stamp, not from the clock `speech_ms` already consumed.
        let device_lost_ms = if device_lost {
            teletype_core::storage::now_ms().saturating_sub(self.started_at_ms) as u32
        } else {
            0
        };
        self.device_error_flag = None;
        let recording = RECORDING.with(|slot| slot.borrow_mut().take());
        let Some(recording) = recording else {
            // The mic never opened: no take, no trace. Drop the pending
            // marks so a later take starts clean.
            if let Some(key) = self.trace_key.take() {
                self.traces.remove(&key);
            }
            if cancelled {
                // F-03: an empty note is the silent discard (a mistap
                // under the visible-recording floor). It shows nothing.
                // A named cancel keeps the note.
                if !cancel_note.is_empty() {
                    self.broadcast(teletype_core::state::UiState::Message {
                        text: cancel_note.into(),
                    });
                    self.flash(cancel_note);
                }
                self.go_idle();
            } else {
                // Clean release: the take would transcribe. In headless
                // mode there is no recording to transcribe, but the phase
                // must match production so a test can tell "stopped and
                // transcribing" from "discarded silently".
                self.phase = Phase::Transcribing;
                self.broadcast(teletype_core::state::UiState::Transcribing);
            }
            return;
        };

        // Update the pill to "Transcribing" immediately on release; the
        // (blocking) audio finish + resample happens on the worker thread so
        // the UI never stalls.
        self.next_id += 1;
        let session = self.next_id;
        // P0.2: from here on the trace travels with the session id the
        // workers report back with.
        if let Some(key) = self.trace_key.take() {
            if let Some(mut t) = self.traces.remove(&key) {
                t.session = session;
                if t.engine.is_empty() {
                    #[cfg(test)]
                    let engine = if self.headless_flash && self.test_events.is_some() {
                        String::new()
                    } else {
                        self.state()
                            .map(|s| s.settings().selected_speech_model.clone())
                            .unwrap_or_default()
                    };
                    #[cfg(not(test))]
                    let engine = self
                        .state()
                        .map(|s| s.settings().selected_speech_model.clone())
                        .unwrap_or_default();
                    t.engine = engine;
                }
                self.traces.insert(session, t);
            }
        }
        if let Some(ms) = speech_ms {
            self.speech_ms.insert(session, ms);
        }
        // P4: measure the streaming take's trace marks (committed segments +
        // tail audio ms) NOW, on the controller thread, before the worker
        // moves the state. These ride the session id to the pipeline worker,
        // which puts them in the `TraceStageMarks` (the whole-take fallback
        // path keeps 0/0, byte-identical to before).
        if let Some(slot) = &streaming {
            if let Some(t) = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
            {
                if let Some(marks) = t.streaming_marks() {
                    self.streaming_marks.insert(session, marks);
                }
            }
        }
        // HARD-4 row 2: a mid-take device loss is a clean stop (the
        // captured audio still transcribes and pastes), not a cancel. A
        // user-initiated cancel must never flash the device message.
        if cancelled {
            let _ = recording.finish(); // drop the captured audio ASAP
            drop(streaming); // committer worker exits on drop; no decode
            self.phase = Phase::Cancelled;
            // P0.2: a cancelled take never pastes; finish its trace with the
            // reason so the cancellation is visible in the timing data.
            self.finish_trace(session, |t| {
                t.errors.push("cancelled by user".into());
            });
            // F-03: an empty note is the silent discard (a mistap under
            // the visible-recording floor). It ends the take with the same
            // cleanup as any cancel but shows nothing: no `Message`
            // broadcast, no flash, no `Processing` pill. The pill goes
            // straight to idle. A named cancel ("Cancelled") keeps the
            // note.
            if cancel_note.is_empty() {
                self.go_idle();
            } else {
                // The session is over; the model stays resident (D052).
                // (Headless: no managers; `go_idle`'s headless guard skips the
                // same block for the silent discard.)
                #[cfg(test)]
                if self.headless_flash {
                    // Test path: no model managers.
                } else if let Some(state) = self.state() {
                    release_take_session(&state);
                }
                self.broadcast(teletype_core::state::UiState::Message {
                    text: cancel_note.into(),
                });
                self.flash_note(cancel_note);
            }
            return;
        }
        self.phase = Phase::Transcribing;
        self.broadcast(teletype_core::state::UiState::Transcribing);
        self.show(PillState::Processing {
            message: "Transcribing…".into(),
        });

        let controller = self.controller.clone();
        #[cfg(test)]
        let app = self.worker_app();
        #[cfg(not(test))]
        let app = self.app().cloned();
        // Clone the settings values the worker needs. In a headless test
        // with `test_events` set, the session has no `AppState`, so the
        // defaults stand in (the gate drops the take before any of these
        // values matter, and `mock_asr` replaces the engine).
        #[cfg(test)]
        let worker_settings = if self.headless_flash && self.test_events.is_some() {
            commands::Settings::default()
        } else {
            self.state().map(|s| s.settings()).unwrap_or_default()
        };
        #[cfg(not(test))]
        let worker_settings = self.state().map(|s| s.settings()).unwrap_or_default();
        let (speech_model_path, kind) = if let Some(state) = self.state() {
            resolve_speech_model(&state.settings(), &state.models_dir)
        } else {
            (std::path::PathBuf::new(), SpeechEngineKind::Whisper)
        };
        // Parakeet is English-only; "auto" there means English. Guarding here
        // prevents the Russian mis-transcription on short utterances.
        // P3.3: consult the per-app language override for the frontmost app
        // before falling back to the global setting.
        #[cfg(test)]
        let language = if self.app.is_some() {
            self.effective_language(kind)
        } else {
            "auto".to_string()
        };
        #[cfg(not(test))]
        let language = self.effective_language(kind);
        // T8: a non-fatal notice when the resolved language is locked to
        // something the selected engine cannot transcribe. Emitted before
        // the Transcribing state so it rides the same session and never
        // blocks the take.
        if let Some(message) =
            language_mismatch_notice(&language, kind, &worker_settings.selected_speech_model)
        {
            self.broadcast(teletype_core::state::UiState::Message { text: message });
        }
        // Keep a copy for the worker's detected-language plumbing below.
        let language_for_worker = language.clone();
        // P7.5: the boost list travels into the release worker; the session
        // slot is emptied so a failed spawn cannot leak it into a later take.
        let take_boost_terms = std::mem::take(&mut self.take_boost_terms);
        let input_device = worker_settings.input_device.clone();
        // Read once per take: a mid-take toggle applies to the next take.
        // e2e takes are file-fed fixtures, not the user's voice.
        let save_recordings = worker_settings.save_recordings && self.controller.e2e.is_none();
        #[cfg(test)]
        let mock_asr = self.mock_asr.clone();
        #[cfg(test)]
        let test_events = self.test_events.clone();
        // HARD-4 row 2: the device-loss marks travel into the worker (the
        // flag was read and cleared above; the worker flashes the message
        // after the text is delivered and records the marks in the trace).
        let device_lost_ms = if device_lost && !cancelled {
            device_lost_ms
        } else {
            0
        };
        // HARD-4 row 14: the fallback model id, read once before the worker
        // spawns so the closure owns it (the app handle is not Send-safe to
        // move into the thread). `None` when nothing has been recorded yet.
        let last_working_model = self.state().as_ref().and_then(|s| {
            s.last_working_model
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        });

        let worker = thread::Builder::new()
            .name("teletype-transcribe".into())
            .spawn(move || {
                let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                    move || {
                    let mut exit_guard = WorkerExitGuard::new(&controller, session);
                    #[cfg(test)]
                    if let Some(tx) = &test_events {
                        let _ = tx.send(Event::WorkerStarted { session });
                        exit_guard = exit_guard.with_test_events(tx.clone());
                    }
                    let captured = match recording.finish() {
                        Ok(c) => c,
                        Err(e) => {
                            crate::log_entry(
                                crate::LogLevel::Error,
                                format!("recording.finish failed: {e}"),
                            );
                            exit_guard.terminal(Event::Transcribed {
                                session,
                                transcript: Err(MIC_FAILURE_MESSAGE.into()),
                                detected_language: None,
                                asr_start_us: 0,
                                asr_end_us: 0,
                                device_lost: false,
                                device_lost_ms: 0,
                                pcm: None,
                                losses: None,
                                take_audio: None,
                                segment_polished: false,
                                segment_raws: None,
                            });
                            return;
                        }
                    };
                    if let Some(message) = take_failure(&captured) {
                        exit_guard.terminal(Event::Transcribed {
                            session,
                            transcript: Err(message.into()),
                            detected_language: None,
                            asr_start_us: 0,
                            asr_end_us: 0,
                            device_lost: false,
                            device_lost_ms: 0,
                            pcm: None,
                            losses: None,
                            take_audio: None,
                            segment_polished: false,
                            segment_raws: None,
                        });
                        // Test-only mirror: `for_tests` controllers have no
                        // event pump, so the terminal event goes to
                        // `test_events` as well (the whole path is test-only).
                        #[cfg(test)]
                        if let Some(tx) = &test_events {
                            let _ = tx.send(Event::Transcribed {
                                session,
                                transcript: Err(message.into()),
                                detected_language: None,
                                asr_start_us: 0,
                                asr_end_us: 0,
                                device_lost: false,
                                device_lost_ms: 0,
                                pcm: None,
                                losses: None,
                                take_audio: None,
                                segment_polished: false,
                                segment_raws: None,
                            });
                        }
                        return;
                    }
                    // Phase 5.7: per-take quality signals. Compute them right
                    // after `captured` (before the trim) and send
                    // `TakeQuality` BEFORE the transcription work: one FIFO
                    // channel, so it always precedes this session's
                    // `Transcribed`.
                    let flags = vad_flags
                        .as_ref()
                        .map(|c| {
                            c.lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone()
                                .into_flags()
                        })
                        .unwrap_or_default();
                    let bt_hfp = active_device_bt_hfp(&input_device);
                    let quality = take_quality(&captured.samples, &flags, bt_hfp);
                    controller.send(Event::TakeQuality { session, quality });
                    // Phase 5.1: prepend the resampled pre-roll so the
                    // whole-take path (and its trim) sees the pre-roll audio.
                    // The streaming branch does not re-decode `captured`, so
                    // the prepend is harmless there. `peak` stays captured.
                    let asr_samples = merge_preroll(
                        &captured.samples,
                        preroll.as_ref().map(|(raw, rate)| (raw.as_slice(), *rate)),
                    );
                    let take_audio: Option<std::sync::Arc<[f32]>> =
                        save_recordings.then(|| std::sync::Arc::from(&asr_samples[..]));
                    let transcribe_started = std::time::Instant::now();
                    // P0.2: ASR start/end on the same monotonic clock as every
                    // other stage mark.
                    let asr_start_us = now_us();
                    // P4: when the streaming take committed segments mid-take,
                    // the release decodes ONLY the uncommitted tail through the
                    // committer (its own engine handle) and folds the committed
                    // segments in by timestamp. The whole-take decode below is
                    // the fallback for short takes (no mid-take commits) and for
                    // takes without streaming state — byte-identical to before.
                    let streaming_marks: Option<(u32, u32)> = streaming.as_ref().and_then(|slot| {
                        let g = slot
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        g.as_ref().and_then(StreamingTake::streaming_marks)
                    });
                    let streaming_slot = streaming_marks.is_some().then(|| streaming.take()).flatten();
                    if let (Some((committed, tail_ms)), Some(slot)) = (streaming_marks, streaming_slot) {
                        // Take the streaming state out of the slot (the capture
                        // thread's callback now sees `None` and stops feeding).
                        let take = {
                            let mut g = slot
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let Some(take) = g.take() else {
                                crate::log_entry(
                                    crate::LogLevel::Warn,
                                    "streaming take slot missing; taking quiet end",
                                );
                                return;
                            };
                            take
                        };
                        // `finish` waits for the tail (which jumps the committer
                        // worker's queue) and all in-flight segments, returning
                        // the committed segments in order followed by the tail,
                        // plus the take's word-loss accounting (T03).
                        let committed_result = take.finish(language.clone());
                        // T10b: the losses travel with the transcript so the
                        // controller can flash the warning after delivery.
                        // P0: the streaming fold is the take's transcript ONLY
                        // when it actually produced text. A fold that failed
                        // (engine error, or every segment decoded to nothing)
                        // must not end the take: the full take buffer is still
                        // here, so the release falls back to the whole-take
                        // decode below, exactly like a take with no mid-take
                        // commits. Before this, a failed fold ended the take
                        // with "No speech detected" even though the buffer held
                        // real speech.
                        let streaming_ok = committed_result
                            .as_ref()
                            .map(|r| {
                                r.transcripts
                                    .iter()
                                    .any(|t| !t.text.trim().is_empty())
                            })
                            .unwrap_or(false);
                        if !streaming_ok {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                format!(
                                    "streaming fold produced no text ({committed} segments committed, tail {tail_ms} ms); falling back to the full take buffer"
                                ),
                            );
                        }
                        if streaming_ok {
                            let losses = committed_result
                                .as_ref()
                                .ok()
                                .map(|r| r.failures);
                            let segment_polished = committed_result
                                .as_ref()
                                .is_ok_and(|r| r.polished_segments > 0);
                            // The unpolished ASR text of the segments polish
                            // rewrote (a `None` slot is the never-polished
                            // tail). Carried to the release path so a long take
                            // stores its raw text and records a Polish stage,
                            // like any other take.
                            let segment_raws = committed_result
                                .as_ref()
                                .ok()
                                .and_then(|r| {
                                    r.segment_raws
                                        .iter()
                                        .any(|s| s.is_some())
                                        .then(|| r.segment_raws.clone())
                                });
                            // Fold the transcripts by timestamp: overlapping
                            // words at segment boundaries are de-duplicated
                            // (the segmenter's 0.4 s forced-cut overlap is
                            // included in the FOLLOWING audio, so the later
                            // segment's overlapping words are dropped).
                            let mut acc: Option<teletype_speech::Transcript> = None;
                            let outcome = match committed_result {
                                Ok(result) => {
                                    for t in result.transcripts {
                                        acc = Some(match acc {
                                            Some(prev) => {
                                                crate::committer::merge_transcripts(&prev, t)
                                            }
                                            None => t,
                                        });
                                    }
                                    match acc {
                                        Some(t) => Ok((t.text, Some(t.words))),
                                        None => Err(NO_SPEECH_SENTINEL.to_string()),
                                    }
                                }
                                Err(e) => Err(e),
                            };
                            let asr_end_us = now_us();
                            let elapsed_ms = transcribe_started.elapsed().as_millis() as u64;
                            match &outcome {
                                Ok((t, _)) if !t.trim().is_empty() => {
                                    crate::log_entry_ms(
                                        crate::LogLevel::Success,
                                        format!(
                                            "streaming transcription complete: {committed} segments committed mid-take, tail {tail_ms} ms"
                                        ),
                                        elapsed_ms,
                                    );
                                }
                                Ok(_) => {
                                    crate::log_entry_ms(
                                        crate::LogLevel::Info,
                                        "no speech detected (streaming)",
                                        elapsed_ms,
                                    );
                                }
                                Err(e) if is_no_speech_error(e) => {
                                    // The fold found no speech: an expected
                                    // outcome, not a failure. Log at Info so
                                    // a muted take does not read as an error.
                                    crate::log_entry_ms(
                                        crate::LogLevel::Info,
                                        "no speech detected (streaming)",
                                        elapsed_ms,
                                    );
                                }
                                Err(e) => crate::log_entry(
                                    crate::LogLevel::Error,
                                    format!("streaming transcription failed: {e}"),
                                ),
                            }
                            #[cfg(test)]
                            let test_outcome = test_events
                                .is_some()
                                .then(|| outcome.clone());
                            // HARD-4 row 2: a device loss during a streaming
                            // take is noticed the same way as on the
                            // whole-take path: the delivered text stays, and
                            // the pill gets the device-loss message.
                            let device_lost_delivered = device_lost_ms > 0
                                && outcome
                                    .as_ref()
                                    .is_ok_and(|(t, _)| !t.trim().is_empty());
                            if device_lost_delivered {
                                crate::log_entry(
                                    crate::LogLevel::Warn,
                                    format!(
                                        "device lost mid-take (streaming); {device_lost_ms} ms of audio captured and used"
                                    ),
                                );
                                controller.send(Event::FlashNote {
                                    session,
                                    message: DEVICE_LOSS_MESSAGE.to_string(),
                                });
                            }
                            // CMD-PIPELINE-REPAIR C: the streaming terminal
                            // must carry the take's real 16 kHz PCM; the
                            // command decider's VAD gate needs it.
                            let streaming_pcm = trim_silence_vad(&asr_samples);
                            exit_guard.terminal(Event::Transcribed {
                                session,
                                transcript: outcome,
                                detected_language: None,
                                asr_start_us,
                                asr_end_us,
                                device_lost: device_lost_ms > 0,
                                device_lost_ms,
                                pcm: Some(streaming_pcm.to_vec()),
                                losses,
                                take_audio,
                                segment_polished,
                                segment_raws,
                            });
                            #[cfg(test)]
                            if let (Some(tx), Some(transcript)) = (&test_events, test_outcome) {
                                let _ = tx.send(Event::Transcribed {
                                    session,
                                    transcript,
                                    detected_language: None,
                                    asr_start_us,
                                    asr_end_us,
                                    device_lost: device_lost_ms > 0,
                                    device_lost_ms,
                                    pcm: Some(streaming_pcm.to_vec()),
                                    losses: None,
                                    take_audio: None,
                                    segment_polished: false,
                                    segment_raws: None,
                                });
                            }
                            return;
                        }
                        // The fold produced no text: fall through to the
                        // whole-take decode below (the `streaming` slot is
                        // already taken, so the callback stops feeding).
                    }
                    // P1.11: trim leading/trailing silence beyond 150 ms before
                    // the final decode (faster decode, fewer hallucinations).
                    let trimmed = trim_silence_vad(&asr_samples);
                    // F-70: when the VAD finds no speech at all, skip the ASR
                    // decode entirely (Whisper-class models hallucinate filler on
                    // silence). The gate is advisory for faint audio: a take
                    // whose peak sits above the energy floor still decodes, so
                    // whispered speech is not dropped.
                    let speech_probe = vad_speech_probe(&asr_samples);
                    // Headless test path: a scripted ASR outcome replaces the
                    // whole decode, so the no-speech gate must not drop the
                    // take first (the test audio is synthetic and the Silero
                    // probe has nothing to find in it). The gate still runs
                    // for every non-scripted take, exactly as before.
                    #[cfg(test)]
                    let scripted = mock_asr.is_some();
                    #[cfg(not(test))]
                    let scripted = false;
                    if !scripted {
                        if let Some(message) = gate_or_capture_outcome(
                            speech_probe.is_empty(),
                            captured.peak,
                            captured.samples.len(),
                        ) {
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!("skipping decode: {message}"),
                        );
                        exit_guard.terminal(Event::Transcribed {
                            session,
                            transcript: Err(message.into()),
                            detected_language: None,
                            asr_start_us: 0,
                            asr_end_us: 0,
                            device_lost: false,
                            device_lost_ms: 0,
                            pcm: None,
                            losses: None,
                            take_audio: None,
                            segment_polished: false,
                            segment_raws: None,
                        });
                        return;
                        }
                    }
                    if trimmed.len() != captured.samples.len() {
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!(
                                "vad trim: {} -> {} samples",
                                captured.samples.len(),
                                trimmed.len()
                            ),
                        );
                    }
                    // ASRHALL-1: the VAD trim above is a no-op while the silero
                    // engine is broken, so also apply the deterministic energy
                    // trim. It cuts the silent tail that ASR engines hallucinate
                    // over, and only ever trims from the end.
                    let vad_trimmed = trimmed;
                    let keep = teletype_core::audio::trim_trailing_silence(vad_trimmed);
                    let trimmed = &vad_trimmed[..keep];
                    if trimmed.len() != vad_trimmed.len() {
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!(
                                "trailing-silence trim: {} -> {} samples",
                                vad_trimmed.len(),
                                trimmed.len()
                            ),
                        );
                    }
                    let trimmed_captured = teletype_core::audio::Captured {
                        samples: trimmed.to_vec(),
                        duration_secs: trimmed.len() as f64 / 16_000.0,
                        peak: captured.peak,
                    };
                    #[cfg(debug_assertions)]
                    if let Ok(dir) = std::env::var("TELETYPE_DUMP_AUDIO") {
                        let path = format!(
                            "{}/take-{}.wav",
                            dir,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| d.as_millis())
                                .unwrap_or(0)
                        );
                        let _ = crate::recordings::write_wav16(
                            std::path::Path::new(&path),
                            &trimmed_captured.samples,
                            crate::recordings::SAMPLE_RATE,
                        );
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!("TELETYPE_DUMP_AUDIO: wrote {path}"),
                        );
                    }                crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "transcribing audio: {} samples, language {}, engine {:?}",
                            trimmed_captured.samples.len(),
                            language,
                            kind
                        ),
                    );
                    // Headless test path: a scripted ASR outcome replaces the
                    // real engine (no model, no app handle needed). The
                    // device-loss flash below still runs, so a take the
                    // device lost mid-capture delivers its text AND flashes
                    // the device-loss message.
                    #[cfg(test)]
                    if let Some(scripted) = &mock_asr {
                        let outcome = scripted.clone();
                        let asr_end_us = now_us();
                        let elapsed_ms = transcribe_started.elapsed().as_millis() as u64;
                        match &outcome {
                            Ok((t, _)) if !t.trim().is_empty() => {
                                crate::log_entry_ms(
                                    crate::LogLevel::Success,
                                    "transcription complete (scripted)",
                                    elapsed_ms,
                                );
                            }
                            _ => {}
                        }
                        let device_lost_delivered = device_lost_ms > 0
                            && outcome
                                .as_ref()
                                .is_ok_and(|(t, _)| !t.trim().is_empty());
                        if device_lost_delivered {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                format!(
                                    "device lost mid-take; {device_lost_ms} ms of audio captured and used"
                                ),
                            );
                        }
                        // The headless controller's `send` drops the event
                        // (no app), so the device-loss flash is mirrored on
                        // the test events channel, the same way the terminal
                        // event below is mirrored. In production the flash
                        // rides behind the terminal event on the one FIFO
                        // pump channel; here the test drains the terminal
                        // event first (it returns the phase to Idle) and
                        // keeps draining for the flash.
                        if device_lost_delivered {
                            #[cfg(test)]
                            if let Some(tx) = &test_events {
                                let _ = tx.send(Event::FlashNote {
                                    session,
                                    message: DEVICE_LOSS_MESSAGE.to_string(),
                                });
                            }
                            controller.send(Event::FlashNote {
                                session,
                                message: DEVICE_LOSS_MESSAGE.to_string(),
                            });
                        }
                        exit_guard.terminal(Event::Transcribed {
                            session,
                            transcript: outcome,
                            detected_language: None,
                            asr_start_us,
                            asr_end_us,
                            device_lost: device_lost_ms > 0,
                            device_lost_ms,
                            pcm: None,
                            losses: None,
                            take_audio: None,
                            segment_polished: false,
                            segment_raws: None,
                        });
                        return;
                    }
                    // HARD-3 (#14) fault point (decode side): the committer's
                    // `Job::Load` fault leaves the provider UNLOADED, so the
                    // whole-take decode's load fails the way a real engine
                    // load failure does. The controller flashes exactly this
                    // string. Without this probe the headless take (no app
                    // handle, no scripted outcome) would panic at the real
                    // transcribe call below.
                    #[cfg(feature = "fault-injection")]
                    if crate::fault_inject::hit("engine-load-error") {
                        crate::log_entry(
                            crate::LogLevel::Error,
                            "whole-take decode: engine load failed (fault-injected); flashing the error".to_string(),
                        );
                        let outcome = Err("Speech model load failed: fault-injected engine load failure".to_string());
                        let asr_end_us = now_us();
                        let elapsed_ms = transcribe_started.elapsed().as_millis() as u64;
                        crate::log_entry(
                            crate::LogLevel::Error,
                            format!("engine load failed after {elapsed_ms} ms"),
                        );
                        exit_guard.terminal(Event::Transcribed {
                            session,
                            transcript: outcome,
                            detected_language: None,
                            asr_start_us,
                            asr_end_us,
                            device_lost: false,
                            device_lost_ms: 0,
                            pcm: None,
                            losses: None,
                            take_audio: None,
                            segment_polished: false,
                            segment_raws: None,
                        });
                        return;
                    }
                    #[cfg(test)]
                    let outcome = if let Some(scripted) = &mock_asr {
                        // Test path: the scripted ASR outcome replaces the real
                        // engine (no model, no app handle needed).
                        scripted.clone()
                    } else {
                        // HARD-3 (#14) fault point: the engine load fails the
                        // way the real engines do — a `SpeechError` before any
                        // decode. The controller flashes exactly this string.
                        // The whole-take decode is only reached when the
                        // streaming fold produced no text, so a headless take
                        // with mid-take commits never hits this probe; the
                        // committer's `Job::Load` fault point is the
                        // authoritative one (it leaves the provider
                        // unloaded, so this decode's load fails the same
                        // way and the fold falls back here).
                        #[cfg(feature = "fault-injection")]
                        {
                            if crate::fault_inject::hit("engine-load-error") {
                                Err("Speech model load failed: fault-injected engine load failure".to_string())
                            } else if let Some(app) = &app {
                                // Production app handle present (e2e): the real
                                // engine runs.
                                transcribe(
                                    app,
                                    &speech_model_path,
                                    &trimmed_captured,
                                    &language,
                                    kind,
                                    &take_boost_terms,
                                )
                            } else {
                                // Headless test without a scripted outcome: the gate
                                // must have dropped the take before ASR, so this is
                                // unreachable. If it is reached, the test is wrong.
                                panic!("headless test reached the real transcribe call")
                            }
                        }
                        #[cfg(not(feature = "fault-injection"))]
                        {
                            if let Some(app) = &app {
                                // Production app handle present (e2e): the real
                                // engine runs.
                                transcribe(
                                    app,
                                    &speech_model_path,
                                    &trimmed_captured,
                                    &language,
                                    kind,
                                    &take_boost_terms,
                                )
                            } else {
                                // Headless test without a scripted outcome: the gate
                                // must have dropped the take before ASR, so this is
                                // unreachable. If it is reached, the test is wrong.
                                panic!("headless test reached the real transcribe call")
                            }
                        }
                    };
                    #[cfg(not(test))]
                    let outcome = match app.as_ref() {
                        Some(app) => transcribe(
                            app,
                            &speech_model_path,
                            &trimmed_captured,
                            &language,
                            kind,
                            &take_boost_terms,
                        ),
                        None => {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                "no app handle available for transcribe; ending take quietly",
                            );
                            Err("no app handle".to_string())
                        }
                    };
                    let asr_end_us = now_us();
                    // HARD-4 row 14: record the model that just completed a
                    // decode successfully, so a future load failure can fall
                    // back to it. Model id only, never text.
                    if let (Some(app), Ok((t, _))) = (&app, &outcome) {
                        if !t.trim().is_empty() {
                            let state = app.state::<crate::AppState>();
                            *state
                                .last_working_model
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                                Some(state.settings().selected_speech_model.clone());
                        }
                    }
                    // The language the engine detected (auto-detect only), for the
                    // pill chip and the lock-language flow.
                    let detected_language = match &app {
                        Some(app) => detect_language_from_provider(app, kind, language_for_worker.as_str()),
                        None => None,
                    };
                    // Classify the outcome before it travels to the controller:
                    // an engine that found no speech in the audio (the typed
                    // `SpeechError::NoSpeech`, rendered to its display string by
                    // `transcribe`) is an expected outcome, not a failure. The
                    // sentinel is what `transcribed` routes to the quiet end.
                    // A take the engine returned empty or filler-only text for
                    // is classified the same way: there is no speech to deliver.
                    let mut transcript = match outcome {
                        Err(e) if is_no_speech_error(&e) => Err(NO_SPEECH_SENTINEL.to_string()),
                        Ok((t, _words))
                            if t.trim().is_empty() || !has_lexical_content(&t) =>
                        {
                            Err(NO_SPEECH_SENTINEL.to_string())
                        }
                        other => other,
                    };
                    // HARD-4 row 14: the selected model's load or decode
                    // failed. If a different model last completed a decode
                    // and its files are still on disk, try it once. The
                    // fallback must never make a take slower than one extra
                    // load plus one extra decode, and never retries a third
                    // time.
                    let mut fallback_message: Option<String> = None;
                    let mut fallback_used: Option<String> = None;
                    let mut fallback_transcript: Option<(
                        String,
                        Option<Vec<teletype_speech::Word>>,
                    )> = None;
                    if let Some(error) = transcript.as_ref().err() {
                        if let (Some(app), Some(last_working)) =
                            (app.as_ref(), last_working_model.as_deref())
                        {
                            let state = app.state::<crate::AppState>();
                            let selected_id = state.settings().selected_speech_model.clone();
                            let fallback_id =
                                pick_fallback_model(error, &selected_id, Some(last_working));
                            if let Some(fallback_id) = fallback_id {
                                let fallback_entry = teletype_speech::catalog::find(fallback_id);
                                let platform = teletype_speech::catalog::host_platform();
                                if let Some(entry) =
                                    fallback_entry.filter(|e| e.available_on(platform))
                                {
                                    let (fb_path, fb_kind) =
                                        resolve_speech_model_for_id(fallback_id, &state.settings(), &state.models_dir);
                                    if fb_path.exists()
                                        || matches!(
                                            fb_kind,
                                            SpeechEngineKind::Fluid | SpeechEngineKind::AppleSpeech
                                        )
                                    {
                                        crate::log_entry(
                                            crate::LogLevel::Warn,
                                            format!(
                                                "selected model failed to load; trying last working model '{fallback_id}'"
                                            ),
                                        );
                                        let fb_result = transcribe_for_model(
                                            app,
                                            &fb_path,
                                            &trimmed_captured,
                                            &language,
                                            fb_kind,
                                            &take_boost_terms,
                                            fallback_id,
                                        );
                                        match fb_result {
                                            Ok((t, words)) if !t.trim().is_empty() => {
                                                let fb_label = entry.name;
                                                let sel_label = teletype_speech::catalog::find(
                                                    &selected_id,
                                                )
                                                .map(|e| e.name)
                                                .unwrap_or(&selected_id);
                                                crate::log_entry(
                                                    crate::LogLevel::Warn,
                                                    format!(
                                                        "fallback model '{fallback_id}' succeeded; flashing the notice"
                                                    ),
                                                );
                                                fallback_message = Some(format!(
                                                    "Used {fb_label} because {sel_label} could not load."
                                                ));
                                                fallback_transcript = Some((t, words));
                                                fallback_used = Some(fallback_id.to_string());
                                            }
                                            Ok(_) => {
                                                crate::log_entry(
                                                    crate::LogLevel::Warn,
                                                    "fallback model produced no speech; ending as today",
                                                );
                                            }
                                            Err(fb_err) => {
                                                crate::log_entry(
                                                    crate::LogLevel::Error,
                                                    format!("fallback model also failed: {fb_err}"),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let elapsed_ms = transcribe_started.elapsed().as_millis() as u64;
                    match &transcript {
                        Ok((t, _)) if !t.trim().is_empty() => {
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
                        Err(e) if is_no_speech_error(e) => {
                            // No speech in the take: end quietly. This is the
                            // expected outcome for a muted press, not a failure,
                            // so it logs at Info (not Error) and the controller
                            // ends the take without a flash or a history entry.
                            crate::log_entry_ms(
                                crate::LogLevel::Info,
                                "no speech in take: ended quietly",
                                elapsed_ms,
                            );
                        }
                        Err(e) => crate::log_entry(
                            crate::LogLevel::Error,
                            format!("transcription failed: {e}"),
                        ),
                    }
                    // HARD-4 row 14: the fallback decode succeeded. The take
                    // completes normally (paste, history, polish) and the
                    // user is told which model was used. The terminal event
                    // carries the fallback text (the take delivers it) and
                    // the flash message (the pill shows it).
                    if let Some(msg) = &fallback_message {
                        #[cfg(test)]
                        if let Some(tx) = &test_events {
                            let _ = tx.send(Event::FlashNote {
                                session,
                                message: msg.clone(),
                            });
                        }
                        controller.send(Event::FlashNote {
                            session,
                            message: msg.clone(),
                        });
                        if let (Some(app), Some(id)) = (app.as_ref(), fallback_used.as_ref()) {
                            let state = app.state::<crate::AppState>();
                            *state
                                .last_working_model
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                                Some(id.clone());
                        }
                        // The take delivers the fallback text, not the error.
                        if let Some(fb) = fallback_transcript.take() {
                            transcript = Ok(fb);
                        }
                    }
                    #[cfg(test)]
                    let test_outcome = test_events.is_some().then(|| transcript.clone());
                    // HARD-4 row 2: the device stopped responding mid-take.
                    // The captured audio was transcribed and pasted through
                    // the normal stop path; now tell the user which audio
                    // was used. Only on a delivered take: an error or a
                    // no-speech end already carries (or deliberately
                    // withholds) its own message. The trace carries the
                    // flag as a boolean plus the captured duration in ms
                    // (no audio text, no cpal error text).
                    let device_lost_delivered = device_lost_ms > 0
                        && transcript
                            .as_ref()
                            .is_ok_and(|(t, _)| !t.trim().is_empty());
                    if device_lost_delivered {
                        crate::log_entry(
                            crate::LogLevel::Warn,
                            format!(
                                "device lost mid-take; {device_lost_ms} ms of audio captured and used"
                            ),
                        );
                        controller.send(Event::FlashNote {
                            session,
                            message: DEVICE_LOSS_MESSAGE.to_string(),
                        });
                    }
                    exit_guard.terminal(Event::Transcribed {
                        session,
                        transcript,
                        detected_language,
                        asr_start_us,
                        asr_end_us,
                        device_lost: device_lost_ms > 0,
                        device_lost_ms,
                        pcm: Some(trimmed.to_vec()),
                        losses: None,
                        take_audio,
                        segment_polished: false,
                        segment_raws: None,
                    });
                    #[cfg(test)]
                    if let (Some(tx), Some(transcript)) = (&test_events, test_outcome) {
                        // CMD-PIPELINE-REPAIR C: mirror the production
                        // terminal's real PCM, not a synthetic substitute.
                        let _ = tx.send(Event::Transcribed {
                            session,
                            transcript,
                            detected_language: None,
                            asr_start_us,
                            asr_end_us,
                            device_lost: device_lost_ms > 0,
                            device_lost_ms,
                            pcm: Some(trimmed.to_vec()),
                            losses: None,
                            take_audio: None,
                            segment_polished: false,
                            segment_raws: None,
                        });
                    }
                    }
                ));
                if let Err(payload) = attempt {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!(
                            "transcribe worker panicked: {}; the take was lost",
                            panic_text(payload.as_ref())
                        ),
                    );
                }
            });
        if let Err(e) = worker {
            // A failed spawn must still flash the pill (production and
            // test alike).
            crate::log_entry(
                crate::LogLevel::Error,
                format!("couldn't start transcription: {e}"),
            );
            self.flash("Transcription failed");
        }
    }
}
