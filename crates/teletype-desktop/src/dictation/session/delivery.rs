use crate::dictation::*;

impl Session {
    /// T2.1: after injection, wait for the user to finish editing the
    /// inserted text, then diff it once and learn. The check runs on the
    /// controller thread (AX reads are main-thread on macOS) and only if the
    /// watch is still pending — the next dictation's `observe_pending_edit`
    /// remains the fallback.
    /// The single point where a pipeline result becomes a user-visible side
    /// effect. Runs on the controller thread, which is the only thread that
    /// ever calls `insert_text`, the scratchpad append, or the injector.
    ///
    /// Verdict, in order:
    /// 1. session id was tagged cancelled by a `Cancel` event processed
    ///    earlier on this same queue → discard, no injection, no scratchpad.
    /// 2. session is no longer the newest (a newer take superseded it) →
    ///    still inject (the delivery gate re-verifies the target) but the
    ///    pill hand-off belongs to the newer session.
    /// 3. otherwise → inject.
    ///
    /// A `Cancel` processed *after* this returns is allowed: the text has
    /// already landed, and the cancel then transitions the session as
    /// normal. That ordering is the intended semantics, not a bug.
    ///
    /// The `on_inject` hook exists for the concurrency tests: they run the
    /// controller loop without a Tauri app, so the real injection (which
    /// needs the main thread / platform services) is replaced by a
    /// recording closure. Production passes `None` and the real path runs.
    /// The hook is called exactly where the side effect happens, so a test
    /// can assert "injected" vs "discarded" against the event order.
    pub(in crate::dictation) fn pipeline_ready(
        &mut self,
        session: u64,
        payload: PipelinePayload,
        on_inject: Option<&mut dyn FnMut(&PipelinePayload)>,
    ) {
        debug_assert_eq!(payload.session, session);
        // P9.6: record the e2e outcome BEFORE the verdict (the cancelled
        // path returns early below and would skip the post-body hook).
        self.e2e_record(session, &payload);
        let deliver_started = Instant::now();
        if self.cancelled_sessions.remove(&session) {
            // Cancel won the race. The result is discarded; the text is
            // already saved to history and the transcript file by the
            // worker, so it stays recoverable there.
            tracing::info!(
                session,
                "pipeline ready for cancelled session; discarding result, no injection"
            );
            crate::log_entry(
                crate::LogLevel::Info,
                format!(
                    "session {session}: cancelled before the pipeline finished — result discarded (kept in History)"
                ),
            );
            // P0.2: the take ends here without a paste; keep the reason.
            let marks = payload.trace_marks.clone();
            let decider_marks = (
                payload.decider,
                payload.decider_ms,
                payload.tool,
                payload.args_source,
            );
            self.finish_trace(session, |t| {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                t.errors.push(format!(
                    "cancelled (deliver {} ms): discarded; kept in history",
                    deliver_started.elapsed().as_millis() as u64
                ));
            });
            self.flash_note("Cancelled");
            // The worker's bookkeeping is complete, so the pill reset for a
            // superseded session still goes through the normal path.
            if self.next_id != session && self.phase != Phase::Idle {
                self.go_idle();
            }
            return;
        }
        if self.next_id != session {
            crate::log_entry(
                crate::LogLevel::Info,
                format!(
                    "pipeline ready for superseded session {session}; injecting, newer session owns the pill"
                ),
            );
        }
        // DEV-ONLY artificial delay between the verdict and the side
        // effect, so the "cancel after ready" ordering can be exercised by
        // hand. 0 unless the env var says otherwise.
        let inject_delay = dev_delay_ms(PIPELINE_INJECT_DELAY_ENV);
        if !inject_delay.is_zero() {
            tracing::debug!(
                ?inject_delay,
                session,
                "dev: artificial delay before injection"
            );
            thread::sleep(inject_delay);
        }
        if let Some(on_inject) = on_inject {
            // Test path: the controller made the "inject" verdict. Record
            // it instead of touching platform services, then finish the
            // session exactly like the production path does.
            let marks = payload.trace_marks.clone();
            on_inject(&payload);
            // P0.2: same marks and paste stamp as production; the write is
            // skipped (no app handle in tests — see `finish_trace`).
            let audio_ms = self.speech_ms.get(&session).copied().unwrap_or(0) as u32;
            // CMD-PIPELINE-REPAIR B: restamp the route the take actually ran.
            let restamp_command = payload.decider.is_some() || payload.tool.is_some();
            let decider_marks = (
                payload.decider,
                payload.decider_ms,
                payload.tool,
                payload.args_source,
            );
            if let Some(t) = self.traces.get_mut(&session) {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                if restamp_command {
                    t.take_kind = Some(TakeKind::Command.label().to_string());
                }
                t.audio_ms = audio_ms;
                t.mark(|s, now| s.t_paste_sent = Some(now));
                // T10b: the test path mirrors the production trace mutation
                // (the production path pushes the counts in `transcribed`).
                if let Some(losses) = &payload.losses {
                    t.errors.push(format!(
                        "word loss: {} segment(s) failed, tail_failed={}",
                        losses.failed_segments, losses.tail_failed
                    ));
                }
            }
            // T10b: the text was delivered; now tell the user some of it was
            // lost (the production path below does the same; the test path
            // returns before reaching it, so both copy the flash).
            if let Some(losses) = &payload.losses {
                if let Some(message) = loss_warning(losses.failed_segments, losses.tail_failed) {
                    self.flash_for(message, Duration::from_millis(4000), FlashKind::Note);
                }
            }
            self.phase = Phase::Idle;
            self.controller.send(Event::PipelineDone { session });
            return;
        }
        let Some(state) = self.state() else {
            return;
        };
        let settings = state.settings();
        let e2e_forced = self.e2e_forced_mode(&payload);
        let platform: &dyn teletype_core::platform::Platform = state.platform.as_ref();
        // Route to the Notetaker when it's enabled and frontmost; otherwise
        // inject into the previous focus.
        let to_scratchpad = settings.scratchpad_enabled
            && payload
                .context
                .application_name
                .eq_ignore_ascii_case("Teletype");
        // P0.2: when delivery is refused nothing lands, so the trace gets
        // the reason instead of a paste mark.
        let mut refused: Option<String> = None;
        // Where this take's text landed, recorded on the trace tail. Only
        // `Some("inject")` earns the paste mark: the Notetaker consumes the
        // text without touching the clipboard or the focused app, so a note
        // take must not look like a pasted one.
        let mut delivered: Option<&'static str> = None;
        if to_scratchpad {
            match crate::commands::append_note(&state, &payload.final_text) {
                Some(note) => {
                    delivered = Some("note");
                    tracing::debug!(
                        "dictation routed to note {} ({} chars)",
                        note.id,
                        note.body.chars().count()
                    );
                }
                None => {
                    tracing::warn!("dictation could not be routed to a note: blank text");
                }
            }
        } else if let Some(msg) = match e2e_forced {
            Some(_) => None, // P9.6: the forced mode owns the cascade
            None => delivery_refusal(
                &payload.record_target,
                platform,
                payload.context.application_type,
                &payload.final_text,
            ),
        } {
            // Refused delivery: no Tier 1, no Tier 2. The text is already in
            // history and the transcript file, and the clipboard was never
            // touched, so nothing needs restoring. The edit-watch record is
            // skipped for the same reason: nothing landed, so there is
            // nothing to learn a correction from.
            crate::log_entry(crate::LogLevel::Info, format!("delivery: {msg}"));
            let marks = payload.trace_marks.clone();
            let decider_marks = (
                payload.decider.clone(),
                payload.decider_ms,
                payload.tool.clone(),
                payload.args_source.clone(),
            );
            let audio_ms = self.speech_ms.get(&session).copied().unwrap_or(0) as u32;
            self.finish_trace(session, |t| {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                t.audio_ms = audio_ms;
                t.errors.push(format!(
                    "delivery refused (deliver {} ms): {msg}",
                    deliver_started.elapsed().as_millis() as u64
                ));
            });
            let Some(app) = self.app().cloned() else {
                return;
            };
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
            // P0.2: nothing landed, so there is no paste mark; the reason is
            // recorded on the trace below.
            refused = Some(format!("delivery refused: {msg}"));
        } else {
            // P7.2: capture the paste anchor BEFORE the insertion. The
            // surroundings read is the pre-insertion caret context, which is
            // exactly what `PasteAnchor` wants (≤ 40 chars each side of the
            // insertion point). A failed read yields no anchor — the
            // observation is then dropped, never guessed.
            let surroundings = focused_surroundings();
            let anchor = crate::edit_watch::build_anchor(
                surroundings.as_ref().map(|(b, _)| b.as_str()),
                surroundings.as_ref().map(|(_, a)| a.as_str()),
                &payload.final_text,
            );
            // Smart insertion, resolved once for both delivery tiers: the
            // direct write and the clipboard paste must land the same
            // characters, or the text changes shape depending on which
            // route the injector happened to pick.
            //
            // Falls back to the dictated text unchanged when the field
            // cannot be read. An unreadable field is not an error worth
            // surfacing — it just means there is no context to fit the
            // text into.
            let final_text = if settings.smart_insertion {
                #[cfg(target_os = "windows")]
                {
                    // P8.5: Windows smart spacing. The core smart_insert::merge
                    // handles whitespace and capitalisation; CJK spacing is
                    // the Windows-specific rule (CJK scripts do not separate
                    // words with spaces), so it is applied here via the pure
                    // `should_add_space` predicate against the UIA caret read.
                    let before = crate::uia::text_before_caret(400).unwrap_or_default();
                    let merged = teletype_core::smart_insert::merge(
                        &teletype_core::smart_insert::Surroundings {
                            before: &before,
                            after: "",
                        },
                        &payload.final_text,
                    );
                    let cjk_gap = crate::commands::should_add_space(&before)
                        && !merged.starts_with(char::is_whitespace);
                    if cjk_gap {
                        let mut out = String::with_capacity(merged.len() + 1);
                        out.push(' ');
                        out.push_str(&merged);
                        out
                    } else {
                        merged
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    match focused_surroundings() {
                        Some((before, after)) => {
                            let merged = teletype_core::smart_insert::merge(
                                &teletype_core::smart_insert::Surroundings {
                                    before: &before,
                                    after: &after,
                                },
                                &payload.final_text,
                            );
                            if merged != payload.final_text {
                                tracing::debug!(
                                    "smart insertion: {} -> {}",
                                    payload.final_text,
                                    merged
                                );
                            }
                            merged
                        }
                        None => payload.final_text.clone(),
                    }
                }
            } else {
                payload.final_text.clone()
            };
            // P8.1: route through the core delivery cascade. The cascade
            // handles the full sequence: clipboard paste + verify, typed
            // fallback, set-value fallback, and the held-for-paste-last
            // outcome. The per-app policy table decides the starting mode.
            // macOS: real clipboard guard; other platforms: compile-safe stub
            // via `delivery_io::macos_io`'s non-mac twin (`Arc<()>`).
            // Item 3: `keep_text_on_clipboard` decides whether the dictation
            // is left on the pasteboard as an extra item after the restore.
            let app_id = platform
                .active_application()
                .map(|a| a.application_id)
                .filter(|id| !id.is_empty());
            #[cfg(target_os = "macos")]
            let io = crate::delivery_io::macos_io_with_target(
                std::sync::Arc::new(crate::clipboard::MacClipboardGuard::new()),
                settings.keep_text_on_clipboard,
                if e2e_forced.is_some() {
                    None
                } else {
                    payload.record_target.clone()
                },
            );
            #[cfg(target_os = "windows")]
            let io = crate::delivery_io::windows_io(settings.keep_text_on_clipboard);
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let io = crate::delivery_io::macos_io(
                std::sync::Arc::new(()),
                settings.keep_text_on_clipboard,
            );
            let policy = DeliveryPolicy::builtin();
            let outcome = match &e2e_forced {
                Some(mode) => {
                    // P9.6: the forced mode is applied through the core
                    // cascade's per-app policy override (exact bundle-id
                    // match, like the checked-in policy table). `hold`
                    // cannot be expressed in the table, so it
                    // short-circuits to the held outcome: every route is
                    // forced to fail and the text is kept for Paste-Last.
                    match mode {
                        crate::e2e::ForcedMode::Hold => {
                            teletype_core::delivery::DeliveryOutcome::HeldForPasteLast
                        }
                        _ => {
                            let policy = mode.apply(&policy, app_id.as_deref().unwrap_or(""));
                            delivery::deliver(&final_text, app_id.as_deref(), &policy, &io)
                        }
                    }
                }
                None => delivery::deliver(&final_text, app_id.as_deref(), &policy, &io),
            };
            // Map outcomes onto existing UI/trace behaviour.
            let delivery_held = matches!(outcome, DeliveryOutcome::HeldForPasteLast);
            let delivery_refused = match &outcome {
                DeliveryOutcome::Refused { reason } => Some(reason.clone()),
                _ => None,
            };
            // F-16 / F-17 / F-19: the paste-shortcut label in the user-facing
            // message. macOS uses Cmd+V; Windows uses Ctrl+V.
            #[cfg(target_os = "macos")]
            let paste_shortcut_label = "Cmd+V";
            #[cfg(not(target_os = "macos"))]
            let paste_shortcut_label = "Ctrl+V";
            match &outcome {
                DeliveryOutcome::PastedVerified => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: pasted verified ({} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::PastedUnverified => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: pasted (unverified, {} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::Typed => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: typed fallback ({} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::SetValue => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: set-value fallback ({} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::HeldForPasteLast => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        "inject: all delivery routes failed; held for Paste-Last",
                    );
                    let Some(app) = self.app().cloned() else {
                        return;
                    };
                    let _ = app.emit_to(
                        "pill",
                        "pill-skip",
                        &serde_json::json!({ "message": "Delivery failed. Text kept for Paste-Last" }),
                    );
                    crate::tray::show_skip(&app, "Delivery failed. Text kept for Paste-Last");
                }
                DeliveryOutcome::Refused { reason } => {
                    crate::log_entry(crate::LogLevel::Warn, format!("inject refused: {reason}"));
                    let Some(app) = self.app().cloned() else {
                        return;
                    };
                    let _ = app.emit_to(
                        "pill",
                        "pill-skip",
                        &serde_json::json!({ "message": format!("Not pasted: {reason}") }),
                    );
                    crate::tray::show_skip(&app, &format!("Not pasted: {reason}"));
                }
                DeliveryOutcome::ClipboardOnly => {
                    // F-16 / F-19: the text is on the clipboard. Tell the
                    // user to press the paste shortcut.
                    let msg = format!("Copied. Press {paste_shortcut_label} to paste.");
                    crate::log_entry(crate::LogLevel::Info, "inject: clipboard only");
                    let Some(app) = self.app().cloned() else {
                        return;
                    };
                    let _ = app.emit_to(
                        "pill",
                        "pill-skip",
                        &serde_json::json!({ "message": msg.clone() }),
                    );
                    crate::tray::show_skip(&app, &msg);
                }
                DeliveryOutcome::AccessibilityDenied => {
                    // F-17: the text is on the clipboard, and the message
                    // names the cause (Accessibility is not trusted).
                    let msg = format!(
                        "Auto-paste needs Accessibility. Text copied: press {paste_shortcut_label}."
                    );
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        "inject: accessibility not trusted; text on clipboard",
                    );
                    let Some(app) = self.app().cloned() else {
                        return;
                    };
                    let _ = app.emit_to(
                        "pill",
                        "pill-skip",
                        &serde_json::json!({ "message": msg.clone() }),
                    );
                    crate::tray::show_skip(&app, &msg);
                }
            }
            // P9.6: stamp the delivery label on the recorded e2e outcome.
            let delivery_label = match &outcome {
                DeliveryOutcome::PastedVerified => "clipboard(pasted,verified)".to_string(),
                DeliveryOutcome::PastedUnverified => "clipboard(pasted,unverified)".to_string(),
                DeliveryOutcome::Typed => "type".to_string(),
                DeliveryOutcome::SetValue => "setvalue".to_string(),
                DeliveryOutcome::HeldForPasteLast => "hold".to_string(),
                DeliveryOutcome::Refused { reason } => format!("refused({reason})"),
                DeliveryOutcome::ClipboardOnly => "clipboard(kept)".to_string(),
                DeliveryOutcome::AccessibilityDenied => "clipboard(kept,ax-denied)".to_string(),
            };
            self.e2e_set_delivery(session, &delivery_label);
            // Post-insertion card: only a delivery that actually landed shows
            // what polish changed. Held and refused takes already flash their
            // own message, and a diff card there would describe text the user
            // never received.
            if !delivery_held && delivery_refused.is_none() {
                let original = self.original_after_filler_removal(&payload);
                self.pending_polish_diff = polish_diff(
                    &payload.raw_text,
                    &payload.final_text,
                    &payload.trace_marks,
                    (!payload.polish_profile_reason.is_empty())
                        .then_some(payload.polish_profile_reason.as_str()),
                    Some(payload.polish_profile.key()),
                    payload.polish_ran,
                    payload.polish_model.clone(),
                    original,
                )
                .map(|diff| (session, diff));
            }
            // P8.4: store the last final text for Paste-Last. Every
            // successful AND failed delivery updates it; only a cancelled
            // take (handled earlier) does not.
            if let Some(state) = self.state() {
                *state
                    .last_text
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(final_text.clone());
            }
            // The delivery-refused path (gate, terminal newline) already
            // handled its own toast above. The cascade-level refusal and
            // hold also set `refused` so the trace gets the reason.
            if let Some(reason) = &delivery_refused {
                refused = Some(format!("delivery refused: {reason}"));
            } else if delivery_held {
                refused = Some("delivery held for Paste-Last (all routes failed)".into());
            }
            // Watch what landed so the personalization loop can diff it
            // against what the user changes it into on the next dictation.
            // See `edit_watch` for why the observation point is the
            // following dictation.
            let target = payload.context.application_name.clone();
            // P7.2: record this take into the learning store (only a
            // successful paste is recorded, never a refusal or a cancel) and
            // arm the anchored watch. Store failures disable learning for
            // the session and never block the paste.
            let dictation_id = self.record_dictation_row(&payload, &final_text);
            if let Some(state) = self.state() {
                state
                    .edit_watch
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record_watch(
                        final_text.clone(),
                        anchor,
                        dictation_id,
                        crate::edit_watch::now_ms(),
                    );
                state
                    .edit_watch
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .set_target(target);
            }
            delivered = Some("inject");
        }
        // P0.2: the verdict was made and acted on — stamp and write the
        // trace AFTER the user-visible side effect, never before. The
        // write is one best-effort append; delivery already happened, so a
        // slow or failed write cannot cost the user their text. A refused
        // delivery gets the reason instead of a paste mark.
        let marks = payload.trace_marks;
        let decider_marks = (
            payload.decider.clone(),
            payload.decider_ms,
            payload.tool.clone(),
            payload.args_source.clone(),
        );
        // CMD-PIPELINE-REPAIR B: restamp the route the take actually ran.
        let restamp_command = payload.decider.is_some() || payload.tool.is_some();
        let audio_ms = self.speech_ms.get(&session).copied().unwrap_or(0) as u32;
        match refused {
            Some(reason) => self.finish_trace(session, |t| {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                if restamp_command {
                    t.take_kind = Some(TakeKind::Command.label().to_string());
                }
                t.audio_ms = audio_ms;
                t.errors.push(reason);
            }),
            None => self.finish_trace(session, |t| {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                if restamp_command {
                    t.take_kind = Some(TakeKind::Command.label().to_string());
                }
                t.audio_ms = audio_ms;
                t.delivered_to = delivered.map(str::to_string);
                if matches!(delivered, Some("inject")) {
                    t.mark(|s, now| s.t_paste_sent = Some(now));
                }
            }),
        }
        // T10b: the text was delivered; now tell the user some of it was
        // lost. The warning flashes 4 s (longer than the usual 2 s: it
        // describes a real loss, not a transient state) and only when the
        // committer actually lost words — a clean take gets no warning.
        if let Some(losses) = &payload.losses {
            if let Some(message) = loss_warning(losses.failed_segments, losses.tail_failed) {
                self.flash_for(message, Duration::from_millis(4000), FlashKind::Note);
            }
        }
        // Verdict made and acted on: tell the controller the session is
        // over. The worker must never send this itself.
        self.controller.send(Event::PipelineDone { session });
    }

    /// P9.6: the forced delivery mode for this take, if the e2e slot has
    /// one set for the target app (exact bundle-id match, like the policy
    /// table). `None` outside `--e2e` or when no override applies.
    pub(in crate::dictation) fn e2e_forced_mode(
        &self,
        payload: &PipelinePayload,
    ) -> Option<crate::e2e::ForcedMode> {
        let slot = self.controller.e2e.as_ref()?;
        let id = payload.context.application_id.as_str();
        if id.is_empty() {
            return None;
        }
        slot.force_for(Some(id))
    }

    /// P9.6: records the take's outcome in the e2e slot (no-op outside
    /// `--e2e`). Called at the top of `pipeline_ready`, before the verdict,
    /// so a cancelled take still reports its transcript and error. The
    /// delivery label is updated in place once the injection runs.
    pub(in crate::dictation) fn e2e_record(&self, session: u64, payload: &PipelinePayload) {
        let Some(slot) = self.controller.e2e.as_ref() else {
            return;
        };
        let cancelled = self.cancelled_sessions.contains(&session);
        let command = payload
            .command
            .as_ref()
            .map(|c| format!("{} ({})", c.tool, c.arguments));
        let error = if cancelled {
            Some("cancelled before the pipeline finished".into())
        } else {
            None
        };
        slot.record(crate::e2e::make_result(
            session,
            payload.final_text.clone(),
            command,
            "pending".into(),
            error,
        ));
    }

    /// P9.6: sets the delivery label on the recorded e2e outcome once the
    /// injection has run (no-op outside `--e2e`).
    pub(in crate::dictation) fn e2e_set_delivery(&self, session: u64, delivery: &str) {
        let Some(slot) = self.controller.e2e.as_ref() else {
            return;
        };
        slot.set_delivery(session, delivery);
    }
}
