use crate::dictation::*;

impl Session {
    /// P0.2: finish a session's trace: apply the final mutation, then
    /// append it as one JSON line — always AFTER the user-visible outcome
    /// (paste, refusal, or discard), never before. The write is skipped
    /// without an app handle (unit tests) and is best-effort everywhere
    /// else: a failed write drops a measurement, never a dictation.
    pub(in crate::dictation) fn finish_trace(
        &mut self,
        session: u64,
        f: impl FnOnce(&mut DictationTrace),
    ) {
        let Some(mut trace) = self.traces.remove(&session) else {
            return;
        };
        f(&mut trace);
        // Phase 5.7: one attach point covers every completion path
        // (success, error, cancel, supersede). Drains the entry so a
        // quality event for a finished id cannot linger.
        if let Some(q) = self.take_quality.remove(&session) {
            apply_take_quality(&mut trace, Some(&q));
        }
        // U2: derive the per-stage durations from the `t_*` marks at the
        // single completion attach point, so the derivation lives in exactly
        // one place and a stage that did not run stays absent (never 0).
        trace.derive_stage_ms();
        if self.app.is_some() {
            if let Some(state) = self.state() {
                let dir = state.data_dir.clone();
                FileTraceSink::new(&dir).write_trace(&trace);
            }
        }
    }

    /// Marks `session` as cancelled in the durable sense: the phase can
    /// move on (a newer take can start), but a later `PipelineReady` for
    /// this id must still be discarded. Called on the controller thread
    /// only, so the tag and the phase transition are one atomic step in
    /// the event order.
    pub(in crate::dictation) fn cancel_session(&mut self, session: u64) {
        debug_assert!(
            self.cancelled_sessions.insert(session),
            "session {session} cancelled twice"
        );
        tracing::info!(
            session,
            "session marked cancelled; pending pipeline result will be discarded"
        );
        crate::log_entry(
            crate::LogLevel::Info,
            format!("session {session} cancelled; pending result will be discarded"),
        );
    }

    /// P3.7: confidence-aware phonetic rescoring over the dictionary +
    /// pack terms, wired into the dictation pipeline when the engine reports
    /// per-word confidences (Fluid, sherpa). Engines without word timings
    /// skip this entirely — `correct_with_dictionary` runs with confidence
    /// 1.0 and its phonetic pass stays dormant, so behavior is byte-identical
    /// for Whisper/ggml Parakeet.
    pub(in crate::dictation) fn rescore_with_confidence(
        &mut self,
        raw: String,
        words: &[teletype_speech::Word],
    ) -> String {
        let Some(state) = self.state() else {
            return raw;
        };
        let settings = state.settings();

        // Build RescoreTerms from dictionary words + pack terms.
        let dictionary = if settings.dictionary_enabled {
            (*state.dictionary.get()).clone()
        } else {
            teletype_core::dictionary::Dictionary::default()
        };

        let mut pack_terms = teletype_core::vocab::terms_for(&settings.enabled_packs);
        let user_packs = state.user_packs.get();
        pack_terms.extend(user_packs.terms_for());
        // Same learned-term tier as the text-only correction path: after ASR,
        // never inside decode (D029). Respect the dictionary-enabled dial.
        if settings.dictionary_enabled {
            let app_category = self
                .screen_context
                .as_ref()
                .map(|ctx| ctx.app_category.as_str());
            pack_terms.extend(Self::learned_terms_for_post_asr(
                &state.data_dir,
                app_category,
            ));
        }

        let terms: Vec<teletype_core::rescoring::RescoreTerm> = dictionary
            .words
            .iter()
            .map(|w| teletype_core::rescoring::RescoreTerm {
                text: w.word.clone(),
                aliases: w.aliases.clone(),
                weight: 1.0,
            })
            .chain(
                pack_terms
                    .iter()
                    .map(|pt| teletype_core::rescoring::RescoreTerm {
                        text: pt.canonical.clone(),
                        aliases: vec![pt.alias.clone()],
                        weight: 1.0,
                    }),
            )
            .collect();

        rescore_or_keep(raw, words, &terms)
    }

    pub(in crate::dictation) fn arm_edit_check(&mut self, session: u64) {
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no edit-watch to observe (no platform services).
            return;
        }
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

    /// A 2s terminal message that may be an error: it stays clickable so an
    /// error's action (Open Settings) works. Plain notes use `flash_note`.
    pub(in crate::dictation) fn flash(&mut self, message: &str) {
        self.flash_for(message, Duration::from_millis(2000), FlashKind::Alert);
    }

    /// A 2s plain note ("Cancelled", "Pasted", "Too short"): clicks pass
    /// through the pill to whatever is underneath.
    pub(in crate::dictation) fn flash_note(&mut self, message: &str) {
        self.flash_for(message, Duration::from_millis(2000), FlashKind::Note);
    }

    pub(in crate::dictation) fn flash_for(
        &mut self,
        message: &str,
        ttl: Duration,
        kind: FlashKind,
    ) {
        self.next_id += 1;
        let generation = self.next_id;
        self.phase = Phase::Idle;
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no broadcast, no pill, no flash timer. The
            // counter still advances so session ids stay unique.
            self.headless_flashes.push(message.to_string());
            return;
        }
        self.broadcast(teletype_core::state::UiState::Message {
            text: message.into(),
        });
        self.show_with(
            PillState::Processing {
                message: message.into(),
            },
            kind != FlashKind::Note,
        );
        if kind == FlashKind::MicFailure {
            if let Some(app) = self.app() {
                let _ = app.emit_to(crate::overlay::PILL_LABEL, "mic-failure", ());
            }
        }
        let controller = self.controller.clone();
        thread::Builder::new()
            .name("teletype-flash".into())
            .spawn(move || {
                thread::sleep(ttl);
                controller.send(Event::FlashDone { generation });
            })
            .ok();
    }

    pub(in crate::dictation) fn flash_done(&mut self) {
        self.show(PillState::Idle);
        self.broadcast(teletype_core::state::UiState::Idle);
    }

    /// Recovery for a panicking event handler: the take has no terminal
    /// state, so end it like an abandoned worker. An already-idle session
    /// only needs the pill push (the panic may have hit after the phase
    /// changed but before the overlay update).
    pub(in crate::dictation) fn recover_after_panic(&mut self) {
        if self.phase == Phase::Idle {
            #[cfg(test)]
            if self.headless_flash {
                return;
            }
            self.show(PillState::Idle);
        } else {
            // The panic abandoned an in-flight take. Tag its session
            // cancelled BEFORE idling, so a `PipelineReady` the abandoned
            // worker emits later is discarded instead of injected into
            // whatever the user is editing now. Without this the orphaned
            // result passes the `next_id` staleness check (the phase is
            // already Idle) and pastes the dead take's text mid-typing.
            self.cancel_session(self.next_id);
            self.go_idle();
        }
    }

    pub(in crate::dictation) fn go_idle(&mut self) {
        // The pending-edit watch is deliberately NOT cleared here. `go_idle`
        // runs after a successful paste too, and the paste records that same
        // watch moments earlier: clearing it made every learn-from-edits check
        // find nothing, so the loop was inert. A take that never pasted has no
        // watch of its own, and a stale one is consumed (or dropped) by the
        // observation points in `observe_pending_edit`.
        self.phase = Phase::Idle;
        self.hands_free = false;
        self.last_release = None;
        self.vad = None;
        self.recording_started = None;
        self.speech_ms.clear();
        // P4: the streaming state is owned by the release worker (moved out
        // of `self.streaming` in `stop()`); if the take ended without a
        // release (e.g. the mic never opened), drop it here so the
        // committer's worker exits.
        self.streaming = None;
        // P4.3b: same rule as `streaming` (a take that ends without a
        // release must not leak its polish install handle).
        self.polish_app = None;
        // Phase 5.7: a take that ends without a release (or without a
        // `finish_trace`) must not leak its stashed quality entry.
        self.take_quality.clear();
        self.streaming_marks.clear();
        self.injected_at = None;
        // P2.6: a selection captured for a command take is single-use. The
        // take that would consume it ends here (transcribed() takes it when
        // the pipeline starts), so a cancelled take's selection can never
        // leak into a later take.
        self.pending_selection = None;
        // P6.4: the screen context is ephemeral per take — a cancelled or
        // discarded take must not leak its field text into the next one.
        self.screen_context = None;
        // P7.5: likewise the boost list; it is rebuilt from the next take's
        // own context (and the cache makes that cheap).
        self.take_boost_terms.clear();
        // P1.4: join the interim thread now, if one is still running. The
        // final transcribe has finished by the time the session goes idle,
        // so the speech-model lock can no longer be contended. This is the
        // only place the join happens: `stop()` itself must not block on
        // an in-flight interim decode.
        if let Some(handle) = self.live_preview.take() {
            handle.join();
        }
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no model managers, warm-up, pill, or broadcast to
            // touch.
            return;
        }
        // The dictation session is over; the model stays resident (D052).
        if let Some(state) = self.state() {
            release_take_session(&state);
        }
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no engine to warm.
        } else {
            // Item 2: `warm_engine_policy == "always"` promises the engine is
            // ready *before* the next press, not just during a take. Nothing
            // re-loads it once idle, so a take that ends with the model
            // unavailable (a failed or evicted load) would leave the promise
            // broken until the user pressed the hotkey again. Cheap: this is the
            // `ready` short-circuit in `load_speech_model` for every healthy
            // resident engine, so it only does work after a real failure, and it
            // touches neither the pill nor the session ids.
            if self.engine_tier() == EngineWarmTier::Resident {
                self.warm_in_background();
            }
        }
        self.broadcast(teletype_core::state::UiState::Idle);
        self.show(PillState::Idle);
    }

    pub(in crate::dictation) fn show(&self, state: PillState) {
        self.show_with(state, false);
    }

    /// Pushes `state` to the pill and picks its pointer behaviour for that
    /// state (`overlay::interactive_for`): clickable while recording (device
    /// picker, Cancel) and for an `alert` flash, click-through otherwise, so
    /// the transparent window does not swallow clicks meant for the app
    /// underneath while it shows a progress label or a plain note.
    pub(in crate::dictation) fn show_with(&self, state: PillState, alert: bool) {
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no pill window to update.
            return;
        }
        let Some(settings) = self.state().map(|s| s.settings()) else {
            return;
        };
        let position = settings.pill_position.resolve();
        let Some(app) = self.app().cloned() else {
            return;
        };
        let interactive = crate::overlay::interactive_for(&state, alert);
        crate::overlay::update(&app, state, position, settings.always_show_pill);
        crate::overlay::set_interactive(&app, interactive);
    }

    pub(in crate::dictation) fn broadcast(&mut self, state: teletype_core::state::UiState) {
        #[cfg(test)]
        if self.headless_flash {
            // Test path: record what the UI would have seen, then stop
            // before the Tauri app (a headless session has none).
            self.headless_broadcasts.push(state);
            return;
        }
        let Some(app) = self.app().cloned() else {
            return;
        };
        let _ = app.emit("dictation-state", &state);
        crate::tray::show_state(&app, &state);
        if let Some(app_state) = self.state() {
            *app_state
                .dictation_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
        }
    }
}
