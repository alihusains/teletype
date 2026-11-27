use crate::dictation::*;

impl Session {
    pub(in crate::dictation) fn state(&self) -> Option<tauri::State<'_, AppState>> {
        self.app.as_ref().map(|app| app.state::<AppState>())
    }

    /// The `polish-diff` card's "original" side: the take's raw ASR text after
    /// the same deterministic filler-removal pass the pipeline ran. Mirrors
    /// `pipeline.rs` (same core remover, dictionary terms and AutoText triggers
    /// protected, take language and strength) so the UI compares like with
    /// like. A store that cannot be read degrades to an empty list rather than
    /// blocking the take; with no app handle (headless tests) the raw text is
    /// returned unchanged.
    pub(in crate::dictation) fn original_after_filler_removal(
        &self,
        payload: &PipelinePayload,
    ) -> String {
        let Some(state) = self.state() else {
            return payload.raw_text.clone();
        };
        let settings = state.settings();
        let dictionary_terms: Vec<String> = state
            .dictionary
            .get()
            .known_words()
            .into_iter()
            .map(|w| w.to_lowercase())
            .collect();
        let autotext_triggers: Vec<String> = state
            .autotext
            .get()
            .entries
            .iter()
            .map(|e| e.trigger.to_lowercase())
            .collect();
        let language = state.profile.get().language.clone();
        teletype_core::fillers::remove_filler_words(
            &payload.raw_text,
            &settings.filler_words,
            &dictionary_terms,
            &autotext_triggers,
            &language,
            settings.filler_strength.to_core(),
        )
    }

    /// P6.4: read the focused field's surroundings once at hotkey-down and
    /// classify the frontmost app. `None` on any failure (no AX permission,
    /// no focused field, secure field, no app handle in tests) — the take
    /// proceeds with empty context, exactly as before this capture existed.
    pub(in crate::dictation) fn capture_screen_context(
        &self,
    ) -> Option<teletype_core::screen_context::ScreenContext> {
        // Headless tests have no app handle; skip the capture entirely.
        #[cfg(test)]
        if self.headless_flash {
            return None;
        }
        let state = self.state()?;
        let app = state.platform.active_application()?;
        // Focused-field text: macOS reads the AX tree (`crate::ax_text`);
        // other platforms have no reader yet (P8.5 wires UIA for the
        // delivery path separately) — documented `None` fallback: the
        // take proceeds with book-level context only.
        #[cfg(target_os = "macos")]
        let (before, after, selection) = match crate::ax_text::read_focused_context(400, 100)? {
            ctx if ctx.secure => (None, None, None),
            ctx => (Some(ctx.before), Some(ctx.after), ctx.selection),
        };
        #[cfg(not(target_os = "macos"))]
        let (before, after, selection) = (None, None, None);
        // A secure field's text must never enter the context, even though
        // the read happened before the subrole was known (see
        // `FocusedContext::secure`): drop the text, keep the app info.
        // (Handled inside the macos branch above.)
        let category = teletype_core::app_category::app_category(
            if app.application_id.is_empty() {
                None
            } else {
                Some(app.application_id.as_str())
            },
            app.window_title.as_deref(),
        );
        Some(teletype_core::screen_context::ScreenContext {
            app_id: if app.application_id.is_empty() {
                None
            } else {
                Some(app.application_id)
            },
            window_title: app.window_title,
            focused_before: before,
            focused_after: after,
            selection,
            // T13: the bounded AX walk fills this; empty until wired.
            window_terms: Vec::new(),
            app_category: category,
        })
    }

    /// The production app handle. Unit tests never reach a path that needs
    /// it (the cancel / pipeline-ready / go-idle paths are state-free).
    pub(in crate::dictation) fn app(&self) -> Option<&AppHandle> {
        self.app.as_ref()
    }

    /// Test-only: the app handle for the release worker, or `None` when the
    /// session is headless and driven by a test event channel (the worker's
    /// `transcribe` call is replaced by `mock_asr`, so the app is unused).
    #[cfg(test)]
    pub(in crate::dictation) fn worker_app(&self) -> Option<AppHandle> {
        if self.headless_flash && self.test_events.is_some() {
            None
        } else {
            self.app().cloned()
        }
    }

    /// The ASR language for the frontmost app (P3.3): the per-app override
    /// wins over the global setting. `"auto"` is passed through so Whisper
    /// can still auto-detect.
    pub(in crate::dictation) fn effective_language(&self, kind: SpeechEngineKind) -> String {
        let Some(state) = self.state() else {
            return "auto".to_string();
        };
        let settings = state.settings();
        let app_ctx = state.platform.active_application().unwrap_or_default();
        let profile = teletype_core::personalization::UserProfile {
            language: settings.language.clone(),
            app_language_overrides: settings.app_language_overrides.clone(),
            ..Default::default()
        };
        let resolved = teletype_core::personalization::resolve_language(&profile, &app_ctx);
        // The catalog row's english_only flag is the single source of truth
        // for "can this engine transcribe this language": an English-only
        // row (Parakeet TDT ggml, Core ML Ultra, sherpa) resolves a locked
        // non-English language to "en"; Whisper rows pass it through. A
        // pinned model that is not in the catalog falls back to the
        // kind-based list, so a custom build never loses the check.
        let english_only = teletype_speech::catalog::find(&settings.selected_speech_model)
            .map(|m| m.english_only)
            .unwrap_or_else(|| crate::dictation::is_english_only(kind));
        effective_language(resolved, english_only)
    }

    pub(in crate::dictation) fn handle(&mut self, event: Event) {
        // Test-only: capture the pipeline payload before `dispatch` moves
        // the event. A headless session records it instead of injecting
        // (no app handle, no platform); the staleness guard in
        // `pipeline_ready` has already run, so what lands here is exactly
        // what production would have injected.
        #[cfg(test)]
        let headless_pipeline = matches!(
            &event,
            Event::PipelineReady { session, .. }
                if self.headless_flash && self.next_id == *session
        );
        self.dispatch(event);
        #[cfg(test)]
        if headless_pipeline {
            self.headless_pipeline_count += 1;
        }
        // Take-scoped OS bindings are re-evaluated after every event rather
        // than at each transition: the phase is the only input, there are a
        // dozen ways to leave a take (cancel, discard, error, supersede), and
        // a missing disarm would leave a bare `Escape` intercepted in every
        // app for the rest of the session. The sync is cheap and idempotent
        // (it compares against `cancel_hotkey_armed` before touching the OS).
        self.sync_cancel_hotkey();
    }

    pub(in crate::dictation) fn dispatch(&mut self, event: Event) {
        match event {
            Event::HotkeyDown => {
                self.hotkey_down_event();
                self.e2e_after_event();
            }
            Event::HotkeyUp => self.hotkey_up_event(),
            Event::HotkeyBounceSettled { seq, held_ms } => self.hotkey_bounce_settled(seq, held_ms),
            Event::CommandHotkeyDown => {
                self.command_hotkey_down_event();
                self.e2e_after_event();
            }
            Event::CommandHotkeyUp => self.hotkey_up_event(),
            Event::E2eFeed { frame } => self.e2e_feed(&frame),
            Event::PasteLast => self.paste_last_event(),
            Event::CommandToggle => self.command_toggle_event(),
            Event::HotkeyInterrupted => self.input(Input::HotkeyInterrupted),
            Event::Toggle => self.input(Input::Toggle),
            Event::Cancel => self.input(Input::Cancel),
            // The trailing resync in `handle` is the whole handler: a
            // settings save may have changed the cancel binding, and the
            // resync compares the live binding against what is registered.
            Event::ResyncBindings => {}
            // Delete-all-learning-data closed the handle; the file is gone,
            // so the cache must not keep a dead connection (F-84).
            Event::CloseLearningStore => {
                self.close_learning_store();
            }
            // Test-only: the release worker started; nothing to do. The
            // test pumps this to learn the worker is alive.
            #[cfg(test)]
            Event::WorkerStarted { .. } => {}
            // The expiry is keyed to its own offer: a newer offer replaced
            // the state, so this timer must leave it alone (F-84).
            Event::OfferUndoExpired { offer_id } => {
                if let Some(app) = self.app() {
                    crate::tray::clear_undo_offer_if(app, offer_id);
                }
            }
            Event::StartupWiring => self.startup_wiring(),
            Event::Transcribed {
                session,
                transcript,
                detected_language,
                asr_start_us,
                asr_end_us,
                device_lost,
                device_lost_ms,
                pcm,
                losses,
                take_audio,
                segment_polished,
                segment_raws,
            } => {
                self.transcribed(
                    session,
                    transcript,
                    detected_language,
                    asr_start_us,
                    asr_end_us,
                    device_lost,
                    device_lost_ms,
                    pcm,
                    losses,
                    take_audio,
                    segment_polished,
                    segment_raws,
                );
            }
            // HARD-4 row 2: the device-loss notice rides behind the take's
            // terminal event (one FIFO channel), so the session id is still
            // current here; a stale or cancelled session is dropped. The
            // take's text is already delivered by the time this lands, and
            // the pill is free to show the notice (the terminal event
            // returned the phase to Idle).
            Event::FlashNote { session, message } => {
                if self.next_id >= session {
                    self.flash_note(&message);
                }
            }
            Event::PipelineReady { session, payload } => {
                self.pipeline_ready(session, payload, None)
            }
            Event::PipelineDone { session } => {
                debug_assert!(
                    !self.cancelled_sessions.contains(&session),
                    "PipelineDone for cancelled session {session}"
                );
                if self.next_id == session {
                    self.go_idle();
                    // The card is emitted after the idle reset so the same
                    // transition cannot wipe it; the pill receives idle then
                    // the diff, in that order.
                    if let Some((diff_session, diff)) = self.pending_polish_diff.take() {
                        if diff_session == session {
                            // The idle reset just hid the pill (always_show
                            // off), so bring it back for the card first.
                            if let Some(app) = self.app() {
                                let card = crate::overlay::show_result(app);
                                let payload = PolishDiffCard { diff: &diff, card };
                                let _ = app.emit_to("pill", "polish-diff", &payload);
                            }
                        }
                    }
                    // T2.1: the inserted text is now editable. Check for an
                    // edit after the user has had a moment to make one,
                    // without waiting for the next dictation.
                    self.injected_at = Some(Instant::now());
                    self.arm_edit_check(session);
                } else if self.phase != Phase::Idle {
                    // A newer session superseded this one while its pipeline
                    // was still running. Its pill state ("Transforming…")
                    // must not survive: the newer session owns the pill now,
                    // so reset to idle. The superseded take still injects
                    // and records history on its own worker thread; only the
                    // UI hand-off is dropped.
                    crate::log_entry(
                        crate::LogLevel::Info,
                        "pipeline done for superseded session; resetting pill",
                    );
                    self.go_idle();
                    // A newer session owns the pill. A card for this take
                    // would arrive mid-recording, so it is dropped instead.
                    self.pending_polish_diff = None;
                }
            }
            Event::PipelineWorkerDone { session } => {
                // The worker finished its bookkeeping. If this session is no
                // longer the newest (a newer dictation started while the
                // pipeline was running), the pill is still showing this
                // session's "Transforming…" state. Reset it now, on the main
                // thread, so the event reliably reaches the pill webview.
                // For the newest session, `PipelineDone` (sent right after
                // this) will call `go_idle()` which does the same reset —
                // this is a no-op in that case because the phase is already
                // Idle by the time `PipelineDone` is processed (the two
                // events are sent back-to-back and the controller channel
                // is FIFO).
                if self.next_id != session && self.phase != Phase::Idle {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        "worker done for superseded session; resetting pill",
                    );
                    self.go_idle();
                }
            }
            Event::WorkerAbandoned { session } => {
                // The take can never complete: its worker died before the
                // hand-off. If it is still the newest session, the pill is
                // showing its busy state and nothing else will reset it.
                crate::log_entry(
                    crate::LogLevel::Error,
                    format!("worker abandoned session {session}; resetting the pill"),
                );
                if self.next_id == session {
                    self.go_idle();
                }
            }
            Event::CheckEdit { session } => {
                // Only the most recent finished take is worth checking; a
                // newer session means this check raced a newer dictation.
                if self.next_id == session {
                    self.observe_pending_edit();
                }
            }
            Event::PrewarmTick => {
                self.prewarm_tick();
                self.poll_learning_observer();
            }
            Event::TakeQuality { session, quality } => {
                // Phase 5.7: stash by session id; `finish_trace` drains
                // and attaches it. An id with no pending pipeline (e.g.
                // the take was cancelled) is drained there or in `go_idle`.
                self.take_quality.insert(session, quality);
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
                        // The durable half of the cancellation: tag the
                        // session id so a `PipelineReady` that arrives later
                        // on this same FIFO queue is discarded by the
                        // controller instead of injecting.
                        self.cancel_session(self.next_id);
                        // The session is over; the model stays resident (D052).
                        #[cfg(test)]
                        if self.headless_flash {
                            // Test path: no model managers.
                        } else if let Some(state) = self.state() {
                            release_take_session(&state);
                        }
                        self.flash_note("Cancelled");
                    }
                    // Nothing, Start, GoHandsFree: no-op for a cancel click.
                    _ => {}
                }
            }
            Event::WarmupDone { session: token } => {
                // F-11: the warm token, not the take's session id. Only the
                // newest warm-up may repaint the pill; a stale token from a
                // superseded warm is dropped. The take's own session id
                // (`next_id`) is untouched by the warm-up, so the `VadFired`
                // guard still matches and hands-free auto-stop survives a
                // cold take.
                if self.warm_token == token && self.phase == Phase::Listening {
                    #[cfg(test)]
                    if self.headless_flash {
                        // Test path: no pill to repaint; the phase and
                        // session-id accounting are what the test asserts.
                        return;
                    }
                    self.show(PillState::Recording {
                        started_at_ms: self.started_at_ms,
                        command: self.take_kind == TakeKind::Command,
                    });
                }
            }
            Event::WarmupFailed {
                session: token,
                reason,
            } => {
                // The model failed to load. End warming with an error state
                // so the pill does not sit on "Getting model ready…" until
                // the take fails with a generic flash. Only the newest
                // warm-up may act (a stale failure from a superseded warm
                // must not repaint a newer pill).
                if self.warm_token == token && self.phase == Phase::Listening {
                    let msg = format!("Couldn't load the speech model ({reason})");
                    crate::log_entry(crate::LogLevel::Error, msg.clone());
                    self.stop_with_note(true, "");
                    self.flash(&msg);
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
                    #[cfg(test)]
                    if self.headless_flash {
                        // Test path: a VAD auto-stop is a clean stop
                        // (transcribe), so the take enters Transcribing.
                        // The production path goes through `stop(false)`,
                        // which in headless mode would also land in
                        // Transcribing, but the direct assignment keeps the
                        // test path independent of the `stop`/`stop_with_note`
                        // headless logic.
                        self.phase = Phase::Transcribing;
                        return;
                    }
                    self.stop(false);
                }
            }
        }
    }

    /// Phase 5.1: re-evaluate the prewarm arm state. `app_active` is the
    /// "main" or "pill" webview being focused; `desired = should_arm(mode,
    /// app_active)`; arm when idle+desired, disarm when armed+not desired.
    /// `FrontEndState::Fallback` is terminal for the session: the backend
    /// rejected pause/play, so stop trying (per-take rebuild continues).
    pub(in crate::dictation) fn prewarm_tick(&mut self) {
        let Some(mode) = self.prewarm_mode() else {
            return;
        };
        // Fallback is terminal for the session: the backend rejected
        // pause/play, so stop trying (per-take rebuild continues).
        if self
            .front_end
            .as_ref()
            .is_some_and(|fe| fe.state() == FrontEndState::Fallback)
        {
            return;
        }
        let app_active = self.app_focused();
        let desired = should_arm(mode, app_active);
        let Some(fe) = self.front_end.as_mut() else {
            return;
        };
        let armed = fe.state() == FrontEndState::Armed;
        if !armed && desired {
            if let Err(e) = fe.arm() {
                // arm() already recorded Fallback + warned on failure.
                crate::log_entry(crate::LogLevel::Warn, format!("prewarm arm: {e}"));
            }
        } else if armed && !desired {
            if let Err(e) = fe.disarm() {
                crate::log_entry(crate::LogLevel::Warn, format!("prewarm disarm: {e}"));
            }
        }
    }

    /// P9.6: after a hotkey-down event, hand the e2e hook its seam: if a
    /// file was queued for the next take and the take is now recording,
    /// the hook spawns the feeder that plays it in. No-op outside `--e2e`
    /// (no slot) or when the phase is not Listening (the press was eaten).
    pub(in crate::dictation) fn e2e_after_event(&self) {
        if self.phase != Phase::Listening {
            return;
        }
        let Some(slot) = &self.controller.e2e else {
            return;
        };
        let Some(app) = self.app() else {
            return;
        };
        crate::e2e::on_take_start(app, slot);
    }

    /// The parsed `instant_start` mode for the prewarm front end, or `None`
    /// when the feature is off (mode Off or no front end built).
    pub(in crate::dictation) fn prewarm_mode(&self) -> Option<PrewarmMode> {
        self.front_end.as_ref()?;
        let state = self.state()?;
        Some(match state.settings().instant_start.as_str() {
            "always" => PrewarmMode::Always,
            "while_app_active" => PrewarmMode::WhileAppActive,
            _ => PrewarmMode::Off,
        })
    }

    /// Whether the "main" or "pill" webview is focused (the app_active
    /// input to `should_arm`). `false` when either window lookup fails.
    pub(in crate::dictation) fn app_focused(&self) -> bool {
        let Some(app) = self.app() else {
            return false;
        };
        ["main", "pill"].iter().any(|label| {
            app.get_webview_window(label)
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false)
        })
    }

    /// Live settings, or `None` when this session has no app handle (a
    /// headless `for_tests` session) or when `AppState` is not managed yet
    /// (the startup window described on [`Event::StartupWiring`]). The
    /// binding and warm-policy reads go through here instead of `self.state()`
    /// so that window can't panic.
    pub(in crate::dictation) fn live_settings(&self) -> Option<commands::Settings> {
        let app = self.app.as_ref()?;
        let state = app.try_state::<AppState>()?;
        Some(state.settings())
    }

    /// Item 2: the tier implied by the *current* setting. Never cached, so a
    /// change in the settings screen applies to the next take. Falls back to
    /// [`EngineWarmTier::OnHotkeyDown`] (the behaviour the app has always
    /// shipped) when the settings aren't reachable.
    pub(in crate::dictation) fn engine_tier(&self) -> EngineWarmTier {
        self.live_settings()
            .map(|s| engine_warm_tier(s.warm_engine_policy))
            .unwrap_or(EngineWarmTier::OnHotkeyDown)
    }

    /// A take whose result can still be escaped: recording, or anything after
    /// the release that is still heading for an insertion. `Phase::is_busy`
    /// covers the second half; `Listening` is added because that is the state
    /// the cancel hotkey exists for.
    pub(in crate::dictation) fn take_active(&self) -> bool {
        matches!(
            self.phase,
            Phase::Listening
                | Phase::Stopping
                | Phase::Transcribing
                | Phase::Transforming
                | Phase::Inserting
        )
    }

    /// Item 1: makes the OS cancel binding match "a take is alive *and* the
    /// setting names a key". Runs after every event, so it is the single
    /// choke point: no start/stop/cancel path can forget it, and it costs no
    /// syscall while nothing changes.
    ///
    /// Arming is per-take on purpose. The shipped default binding is a bare
    /// `Escape`, and a Carbon global hotkey removes the keystroke from the
    /// normal event flow for every app, so registering it at startup would
    /// break Escape everywhere (dialogs, find fields, fullscreen) for the
    /// whole session. See [`Controller::register_cancel_hotkey`].
    pub(in crate::dictation) fn sync_cancel_hotkey(&mut self) {
        let desired = if self.take_active() {
            self.live_settings()
                .map(|s| s.cancel_hotkey.trim().to_string())
                .filter(|binding| !binding.is_empty())
        } else {
            None
        };
        if desired == self.cancel_hotkey_armed {
            return;
        }
        // Headless session: nothing to register, but keep the bookkeeping
        // consistent so the comparison above stays a no-op.
        let Some(app) = self.app.as_ref() else {
            self.cancel_hotkey_armed = desired;
            return;
        };
        if let Some(stale) = self.cancel_hotkey_armed.take() {
            self.controller.unregister_cancel_hotkey(app, &stale);
        }
        if let Some(binding) = desired {
            self.controller.register_cancel_hotkey(app, &binding);
            self.cancel_hotkey_armed = Some(binding);
        }
    }

    /// Item 2: the one wiring step that has no take to hang off. Deferred to
    /// the first idle event because the controller is spawned before
    /// `AppState` is managed, so settings aren't readable yet (see
    /// [`Event::StartupWiring`]).
    ///
    /// Only the `always` policy does anything: it means "resident", which
    /// needs a load that no hotkey press will trigger if the user never
    /// dictates again. The other tiers are already covered by the on-press
    /// warm in `start()`.
    pub(in crate::dictation) fn startup_wiring(&mut self) {
        if self.engine_tier() == EngineWarmTier::Resident {
            self.warm_in_background();
        }
    }

    /// P0.2: make sure the in-flight take has a trace, stamping hotkey-down
    /// when the trace is created. Idempotent: a take that already has one
    /// (e.g. double-tap switching to hands-free mid-take) keeps its marks.
    ///
    /// CMD-PIPELINE-REPAIR B: a take already re-keyed to its session id
    /// (`trace_key` cleared by `stop_with_note`) must not mint a second trace
    /// when the chord unwind's redundant edges arrive while it transcribes.
    pub(in crate::dictation) fn ensure_trace(&mut self) {
        if self.trace_key.is_none() && self.phase != Phase::Transcribing {
            let key = self.next_id;
            let mut trace = DictationTrace::new(key, "");
            trace.mark(|s, now| s.t_hotkey_down = Some(now));
            self.traces.insert(key, trace);
            self.trace_key = Some(key);
        }
    }

    /// A.3.3: stamp the command-decider trace fields from the pipeline
    /// payload. No-op when the take never ran the command route (all four
    /// are `None`).
    pub(in crate::dictation) fn apply_decider_marks(
        t: &mut teletype_core::trace::DictationTrace,
        decider: Option<String>,
        decider_ms: Option<u32>,
        tool: Option<String>,
        args_source: Option<String>,
    ) {
        if decider.is_some() || decider_ms.is_some() || tool.is_some() || args_source.is_some() {
            t.decider = decider;
            t.decider_ms = decider_ms;
            t.tool = tool;
            t.args_source = args_source;
        }
    }

    /// P2.9: append one command-take lifecycle event to the trace stored
    /// under `key`. Controller-side stages (`armed`, `started`) are pushed
    /// directly; worker-side stages (`classified=`, `executed`,
    /// `refused`) travel back in `TraceStageMarks` and are merged by
    /// `apply_stage_marks`. Missing traces are ignored — instrumentation
    /// never fails a take.
    pub(in crate::dictation) fn trace_command_event(&mut self, key: u64, event: impl Into<String>) {
        if let Some(t) = self.traces.get_mut(&key) {
            t.command_mode.push(event.into());
        }
    }

    /// CMD-2: write the route this take is on into its trace. Called at
    /// `start` and again whenever a later stage re-routes the take, so the
    /// value a finished trace carries is the route the pipeline ran, not the
    /// route the first keypress suggested.
    pub(in crate::dictation) fn trace_take_kind(&mut self, key: u64, kind: TakeKind) {
        if let Some(t) = self.traces.get_mut(&key) {
            t.take_kind = Some(kind.label().to_string());
        }
    }
}
