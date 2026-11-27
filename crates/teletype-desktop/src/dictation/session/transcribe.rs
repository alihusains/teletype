use crate::dictation::*;

impl Session {
    #[allow(clippy::too_many_arguments)] // the take's full context; each field is consumed once
    pub(in crate::dictation) fn transcribed(
        &mut self,
        session: u64,
        transcript: Result<(String, Option<Vec<teletype_speech::Word>>), String>,
        detected_language: Option<String>,
        asr_start_us: u64,
        asr_end_us: u64,
        device_lost: bool,
        device_lost_ms: u32,
        pcm: Option<Vec<f32>>,
        losses: Option<crate::committer::CommitterFailureReport>,
        take_audio: Option<std::sync::Arc<[f32]>>,
        segment_polished: bool,
        segment_raws: Option<Vec<Option<String>>>,
    ) {
        if self.next_id != session {
            return; // stale
        }
        // P0.2: fold the worker's ASR marks into the session trace. A
        // zero start means the worker never ran the engine (audio finish
        // failed), so there is nothing to merge.
        if asr_start_us > 0 {
            if let Some(t) = self.traces.get_mut(&session) {
                t.t_asr_start = Some(asr_start_us);
                t.t_asr_end = Some(asr_end_us);
            }
        }
        // HARD-4 row 2: the device-loss marks ride the trace (the boolean
        // plus the captured duration in ms; no audio text, no cpal error
        // text).
        if device_lost {
            if let Some(t) = self.traces.get_mut(&session) {
                t.errors.push(format!(
                    "device lost mid-take: {device_lost_ms} ms of audio captured and used"
                ));
            }
        }
        if self.phase == Phase::Cancelled {
            // Belt and braces: a cancel that raced the worker must never
            // transcribe, pipeline, or inject.
            crate::log_entry(
                crate::LogLevel::Info,
                "dropping transcript for cancelled session",
            );
            // P0.2: the take ends here without a paste; keep the reason.
            self.finish_trace(session, |t| {
                t.errors.push("transcript arrived after cancel".into());
            });
            self.go_idle();
            return;
        }
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no pill, no platform calls, no pipeline worker.
            // The phase and session-id accounting still run so a later
            // take starts clean. A no-speech take (the sentinel, or an
            // empty / filler-only transcript) ends the take quietly
            // (idle, no flash), exactly like the production no-speech
            // gate. A real transcription failure flashes the pill,
            // exactly like production. A successful one runs the real
            // pipeline (with no inference provider, so only the
            // deterministic passes run) and applies the F-62 empty-text
            // floor: a take whose final text is empty but whose post-ITN
            // or raw ASR text has lexical content is delivered that text;
            // a filler-only take ends quietly.
            if let Err(msg) = &transcript {
                if is_no_speech_error(msg) {
                    // No speech in the take: end quietly, exactly like the
                    // production no-speech gate. No flash, no broadcast.
                    self.finish_trace(session, |t| {
                        t.errors.push("no speech detected".to_string());
                    });
                    self.phase = Phase::Idle;
                    self.next_id += 1;
                    return;
                }
                // A real transcription failure: flash the pill, exactly
                // like production.
                self.finish_trace(session, |t| {
                    t.errors.push(format!("transcription failed: {msg}"));
                });
                self.flash(msg);
                self.phase = Phase::Idle;
                self.next_id += 1;
                return;
            }
            let (raw, _words) = transcript.as_ref().unwrap().clone();
            // No speech in the take: the engine returned empty or
            // filler-only text. End quietly, exactly like the production
            // no-speech gate.
            if raw.trim().is_empty() || !has_lexical_content(&raw) {
                self.finish_trace(session, |t| {
                    t.errors.push("no speech detected".to_string());
                });
                self.headless_final_texts.push(String::new());
                self.phase = Phase::Idle;
                self.next_id += 1;
                return;
            }
            let headless_result = headless_pipeline_run(&raw);
            // F-62 floor ladder: the pipeline's final text, then the
            // post-ITN text (filler pass + ITN, before the transform),
            // then the raw ASR text if it has lexical content. If none of
            // the three has lexical content, the take is filler-only (or
            // empty) and ends quietly: no delivery.
            let final_text = if has_lexical_content(&headless_result.final_text) {
                Some(headless_result.final_text.clone())
            } else if has_lexical_content(&headless_result.post_itn) {
                Some(headless_result.post_itn.clone())
            } else {
                // F-62: the filler pass emptied the take and the post-ITN
                // text has no lexical content. The take is filler-only (or
                // empty) and must end quietly: no delivery, no clipboard
                // write, no paste.
                None
            };
            let Some(final_text) = final_text else {
                // Quiet no-speech end: record the sentinel so a test can
                // tell "quiet end" from "never reached the ladder".
                self.headless_final_texts.push(String::new());
                self.phase = Phase::Idle;
                self.next_id += 1;
                return;
            };
            self.headless_final_texts.push(final_text);
            self.phase = Phase::Idle;
            self.next_id += 1;
            return;
        }
        // This is the reliable moment to observe an edit to the *previous*
        // dictation: the user has spoken again, so they have finished editing
        // the last one. See `edit_watch` for why it is not done inline.
        // (The post-injection check in `arm_edit_check` usually handles this
        // first; this is the fallback for a take the user edits late.)
        self.observe_pending_edit();
        // T10b: the take's word-loss counts (T03) are recorded on the
        // session's core trace — counts only, never the lost words — so the
        // finished trace JSONL line carries them for debugging. When the
        // take has losses the trace is kept (not finalized here): the
        // warning flashes first, and the trace is finished right after,
        // so the counts land in the same JSONL line as the delivery marks.
        // (The test path returns before `finish_trace`; it reads the kept
        // trace directly.)
        if let Some(losses) = &losses {
            if let Some(t) = self.traces.get_mut(&session) {
                t.errors.push(format!(
                    "word loss: {} segment(s) failed, tail_failed={}",
                    losses.failed_segments, losses.tail_failed
                ));
            }
        }
        match transcript {
            Ok((raw, words)) => {
                // No speech in the take: the engine returned empty or
                // filler-only text (the worker classified it the same way).
                // End exactly like the F-70 silent path: no error flash, no
                // pill text, no paste, no history entry.
                if raw.trim().is_empty() || !has_lexical_content(&raw) {
                    self.finish_trace(session, |t| {
                        t.errors.push("no speech detected".to_string());
                    });
                    self.go_idle();
                    return;
                }
                // P3.7: when the engine reports per-word confidences
                // (Fluid, sherpa), run confidence-aware phonetic rescoring
                // over the dictionary + pack terms before the pipeline.
                // Engines without word timings (Whisper, ggml Parakeet) pass
                // `None` and skip rescoring — the pipeline's
                // `correct_with_dictionary` runs with confidence 1.0, so its
                // phonetic stay dormant and behavior is byte-identical.
                let text = if let Some(words) = &words {
                    self.rescore_with_confidence(raw, words)
                } else {
                    raw
                };
                // If the language is English and the model misidentified the
                // speech as Russian (Cyrillic), transliterate back to Latin.
                let text = ensure_latin_if_english(
                    &text,
                    &effective_language(
                        self.state()
                            .map(|s| s.settings().language.clone())
                            .unwrap_or_default(),
                        false,
                    ),
                );
                self.phase = Phase::Transforming;
                self.broadcast(teletype_core::state::UiState::Transforming);
                self.show(PillState::Processing {
                    message: "Transforming…".into(),
                });

                // Read the measured speaking time before `self` is moved into
                // the worker closure.
                let speech_ms_for_session = self.speech_ms.get(&session).copied();
                // The speech model that served this take (the trace records
                // it at release), stored on the history entry by name so a
                // later catalog rename cannot change an old card.
                let asr_model = self
                    .traces
                    .get(&session)
                    .map(|t| t.engine.clone())
                    .filter(|e| !e.is_empty())
                    .unwrap_or_else(|| {
                        self.state()
                            .map(|s| s.settings().selected_speech_model.clone())
                            .unwrap_or_default()
                    });
                let asr_model = asr_model_label(&asr_model);
                // P4: the streaming take's trace marks (committed segments +
                // tail audio ms), measured in `stop()` and consumed here. The
                // whole-take fallback path keeps 0/0 (byte-identical to before).
                let streaming_marks = self.streaming_marks.remove(&session);
                // P2.2: the take kind travels into the worker by value. The
                // worker reads it (not a re-fetched snapshot) so an armed
                // command take is forced through command extraction even if
                // auto-classification would call it dictation.
                //
                // P2.5: a plain dictation take that opens with the wake
                // phrase is re-routed here, before the worker spawns, so the
                // rest of the pipeline (force_command, pill, trace) treats
                // it as a command take.
                let mut text = text;
                let mut take_kind = self.take_kind;
                if take_kind == TakeKind::Dictation
                    && self
                        .state()
                        .map(|s| s.settings().command_wake_phrase)
                        .unwrap_or(false)
                {
                    if let Some(stripped) = strip_wake_phrase(&text) {
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!("wake phrase routed as command: {stripped}"),
                        );
                        text = stripped;
                        take_kind = TakeKind::Command;
                        // P2.9: the wake phrase is what armed this take; the
                        // recording itself started much earlier (visible in
                        // its hotkey-down / first-audio marks), so only the
                        // `armed` event is recorded here. `stop()` has
                        // already re-keyed the trace to this session id.
                        self.trace_command_event(session, "armed");
                    }
                }
                // P2.6: a Command take with selected text becomes a
                // Transform take: the spoken transcript is the instruction,
                // the selection is the target, run through the normal
                // transform engine as a one-shot ad-hoc transform. Consumed
                // here so a stale selection can never leak into a later
                // take (go_idle also clears it on early ends).
                let pending_selection = self.pending_selection.take();
                let (text, take_kind, selection_transform) = match (&take_kind, pending_selection) {
                    (TakeKind::Command, Some(selected)) => {
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!(
                                "selection command: {text:?} on {} chars",
                                selected.chars().count()
                            ),
                        );
                        let t = teletype_core::transforms::TransformDefinition::new(
                            "Voice command",
                            "one-shot instruction spoken over selected text",
                            text,
                        );
                        (selected, TakeKind::Transform, Some(t))
                    }
                    _ => (text, take_kind, None),
                };
                // CMD-2: the trace must state the route the pipeline ran, not
                // the route the first keypress suggested, so stamp it after
                // every re-route above (wake phrase, selection transform).
                self.trace_take_kind(session, take_kind);
                // Same for the recorded paste target: the gate compares it
                // against the live target after the pipeline finishes.
                let record_target = self.record_target.clone();
                let Some(app) = self.app().cloned() else {
                    return;
                };
                let controller = self.controller.clone();
                // P1.3: the bookkeeping handle. The pipeline worker ships
                // `PipelineReady` (the injection) FIRST, then hands the
                // usage/history/dictionary-usage writes to the bookkeeper
                // so no disk write sits on the release→paste path.
                let bookkeeper = crate::bookkeeper::Bookkeeper::spawn(app.clone());
                // T01: the take's on-screen names (extracted terms only,
                // never raw field text) reach the polish prompt, so a
                // dictated name not in the dictionary can be spelled as it
                // appears on screen. Copied out before the worker spawn:
                // the `move` closure below owns the rest of the take's
                // context.
                //
                // DUAL USE of this one field: the same terms feed the prompt,
                // the polish guard and the screen-term rescore, so a word the
                // user marked "Never change this" is protected by all three.
                let screen_terms = {
                    let state = app.state::<AppState>();
                    let mut terms = self
                        .screen_context
                        .as_ref()
                        .map(teletype_core::screen_context::ephemeral_terms)
                        .unwrap_or_default();
                    for term in
                        crate::commands::polish_feedback::never_change_terms(&state.data_dir)
                    {
                        if !terms.contains(&term) {
                            terms.push(term);
                        }
                    }
                    terms
                };
                // Learned lexicon terms are also applied after decode, through
                // the existing dictionary/pack correction tier. The category
                // keeps app-scoped terms from leaking into other apps.
                let post_asr_app_category = self
                    .screen_context
                    .as_ref()
                    .map(|ctx| ctx.app_category.as_str());
                let worker = thread::Builder::new()
                    .name("teletype-pipeline".into())
                    .spawn(move || {
                        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                            move || {
                            // A panic anywhere below (transform, usage counting,
                            // command execution) must not leave the pill stuck in
                            // "Transforming…": the guard reports the abandoned
                            // session so the controller can reset it.
                            let mut exit_guard = WorkerExitGuard::new(&controller, session);
                            let pipeline_started = std::time::Instant::now();
                            // A.3.3: the take audio for the command decider.
                            let pcm: &[f32] = pcm.as_deref().unwrap_or(&[]);
                            // The take audio rides to the bookkeeper with the
                            // history entry; command takes leave it here.
                            let mut take_audio = take_audio;
                            let state = app.state::<AppState>();
                            // A2: per-stage timings for this take. The controller
                            // finishes the trace with the `deliver` span and
                            // stores it in `AppState.traces`.
                            let mut trace = dictation_stages::DictationTrace::new(session);
                            if let Some(ms) = speech_ms_for_session {
                                trace.record("capture", ms, format!("{ms} ms speaking"));
                            }
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
                            // hangs for the full LLM latency. The inference engine
                            // is accessed via ArcSwap: we clone the Arc and drop
                            // the lock immediately; model swap atomically installs
                            // a new Arc while the old one lives until in-flight
                            // dictations finish.
                            let snap_started = std::time::Instant::now();
                            let (autotext, transforms, mut profile, styles, dictionary) = {
                                let a = (*state.autotext.get()).clone();
                                let t = (*state.transforms.get()).clone();
                                let mut p = (*state.profile.get()).clone();
                                if let Some(detected) = &detected_language {
                                    p.language = detected.clone();
                                }
                                let s = (*state.styles.get()).clone();
                                let d = if settings.dictionary_enabled {
                                    (*state.dictionary.get()).clone()
                                } else {
                                    teletype_core::dictionary::Dictionary::default()
                                };
                                (a, t, p, s, d)
                            };
                            trace.record(
                                "snapshots",
                                snap_started.elapsed().as_millis() as u64,
                                "store snapshots",
                            );
                            // P3.3: a per-app language override for the frontmost
                            // app wins over the (possibly detected) global
                            // language.
                            let ctx_started = std::time::Instant::now();
                            let app_ctx = state.platform.active_application().unwrap_or_default();
                            let active_style = settings.active_style_profile.clone();
                            let explicit_style = String::new();
                            // P1.8: clone the engine Arc and drop the lock BEFORE
                            // the LLM call. The pipeline holds the Arc snapshot
                            // for its whole run, so a model swap (which takes the
                            // lock to install a new Arc) never blocks on an
                            // in-flight generation, and the UI/model-list
                            // commands never block on the generation.
                            let inference: Option<Arc<dyn teletype_core::llm::InferenceProvider>> =
                                state
                                    .inference
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .clone();
                            let decision = state
                                .decision
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            // A.3.3: snapshot the selected command decider while
                            // the state borrow is live (the pipeline worker runs
                            // off the main thread, so the Arc is cloned now).
                            let decider = crate::command_mode::select_decider(
                                &state.command_decider,
                                match detected_language.as_deref() {
                                    Some(l) => l,
                                    None => &settings.language,
                                },
                            );

                            let transform_model = inference
                                .as_deref()
                                .map(|p| format!("{} ({})", p.model_name(), p.model_id()))
                                .unwrap_or_else(|| "no LLM".to_string());
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
                            trace.record(
                                "context",
                                ctx_started.elapsed().as_millis() as u64,
                                app_ctx.application_name.clone(),
                            );
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
                            // preview. P1.9: the sink emits `{seq, delta}` token
                            // events, not the whole accumulated string — the IPC
                            // cost per take is O(N) bytes, not O(N²). The pill
                            // concatenates the deltas in `seq` order.
                            // P0.2: the first streamed token is the closest
                            // observable point to "polish started producing".
                            let polish_first_token_us =
                                std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                            let first_token_mark = polish_first_token_us.clone();
                            let sink_app = app.clone();
                            let mut seq: u64 = 0;
                            let mut token_sink: Box<dyn FnMut(&str)> = Box::new(move |tok| {
                                if first_token_mark.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                                    first_token_mark
                                        .store(now_us(), std::sync::atomic::Ordering::Relaxed);
                                }
                                let _ = sink_app.emit_to(
                                    "pill",
                                    "transform-token",
                                    serde_json::json!({ "seq": seq, "delta": tok }),
                                );
                                seq += 1;
                            });
                            // Vocabulary packs: precompute the enabled packs'
                            // terms once per dictation for the lowest-priority
                            // fuzzy correction tier.
                            let mut pack_terms =
                                teletype_core::vocab::terms_for(&settings.enabled_packs);
                            // User-created packs (item 15) feed the same tier.
                            {
                                let user_terms = state.user_packs.get().terms_for();
                                pack_terms.extend(user_terms);
                            }
                            // Learned terms bias the final text after ASR, not
                            // the decode itself (D029). They use the same
                            // low-priority fuzzy tier as vocabulary packs.
                            pack_terms.extend(Self::learned_terms_for_post_asr(
                                &state.data_dir,
                                post_asr_app_category,
                            ));
                            // Dictionary strictness is a real dial, so the floor
                            // has to reach the checker. "standard" resolves to
                            // None and keeps the pipeline's own default rather than
                            // a number restated here, so the two cannot drift.
                            let strictness_floor =
                                crate::commands::match_strictness_floor(settings.match_strictness);
                            let strictness_checker = teletype_core::dictionary::FloorChecker::new(
                                strictness_floor
                                    .unwrap_or(teletype_core::dictionary::EditDistanceChecker::FLOOR),
                            );
                            let word_checker: &dyn teletype_core::dictionary::WordChecker =
                                match strictness_floor {
                                    // The default floor is the shared zero-cost
                                    // constant, so leaving strictness alone
                                    // behaves exactly as it did before this setting
                                    // existed.
                                    None => &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
                                    Some(_) => &strictness_checker,
                                };
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
                            // A.3.3: decider trace events collected before the
                            // shared `trace_marks` is built below.
                            let mut trace_marks_pre: Vec<String> = Vec::new();
                            let mut decider_label: Option<String> = None;
                            let mut decider_ms: Option<u32> = None;
                            let mut tool_label: Option<String> = None;
                            let mut args_source: Option<String> = None;
                            // Resolve the take's polish profile before the
                            // pipeline reads the instruction channel. An
                            // explicit per-app override always wins; only when
                            // there is none may the local bandit choose a
                            // learned profile. The global default is NOT an
                            // explicit override.
                            let raw_word_count = input.text.split_whitespace().count();
                            let recommended_profile = teletype_core::profile::resolve_profile(
                                &app_ctx,
                                &settings.polish_app_profiles,
                                settings.polish_profile,
                                raw_word_count,
                            );
                            let app_key = if app_ctx.application_id.is_empty() {
                                app_ctx.application_name.clone()
                            } else {
                                app_ctx.application_id.clone()
                            };
                            let has_explicit_override = [
                                app_ctx.application_id.as_str(),
                                app_ctx.application_name.as_str(),
                                teletype_core::profile::category_key(app_ctx.application_type),
                            ]
                            .iter()
                            .any(|k| {
                                settings
                                    .polish_app_profiles
                                    .contains_key(&teletype_core::profile::normalize_key(k))
                            });
                            let take_profile = if has_explicit_override {
                                recommended_profile
                            } else {
                                state
                                    .bandit
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .choose(
                                        &app_key,
                                        recommended_profile,
                                        teletype_core::storage::now_ms(),
                                    )
                            };
                            let polish_prompt_override =
                                teletype_core::profile::encode_instruction(
                                    take_profile,
                                    settings.polish_prompt_override.as_deref(),
                                );
                            let mut pipeline = Pipeline {
                                platform,
                                autotext: &autotext,
                                transforms: &transforms,
                                profile: &profile,
                                polish_prompt_override,
                                inference: inference.as_deref(),
                                decision: decision.as_deref(),
                                force_command: take_kind == TakeKind::Command,
                                dictionary: &dictionary,
                                styles: &styles,
                                active_style: &active_style,
                                explicit_style: explicit_style.as_str(),
                                auto_apply: settings.auto_apply_transform,
                                restore_clipboard: settings.restore_clipboard,
                                remove_filler_words: settings.remove_filler_words,
                                filler_words: settings.filler_words.clone(),
                                filler_strength: settings.filler_strength.to_core(),
                                restore_emoji: settings.restore_emoji,
                                spoken_emoji: settings.spoken_emoji,
                                spoken_punctuation: settings.spoken_punctuation,
                                system_autotext: &system_entries,
                                token_sink: Some(&mut token_sink),
                                polish_gate_enabled: settings.polish_gate_enabled,
                                polish_skip_when_clean: settings.polish_skip_when_clean,
                                polish_gate_threshold_words: settings.polish_gate_threshold_words,
                                pack_terms: &pack_terms,
                                word_checker,
                                list_style: settings.list_style.resolve(),
                                polish_max_change_percent: settings.polish_max_change_percent,
                                screen_terms: &screen_terms,
                                date_format: settings.date_format.as_str(),
                            };
                            // P0.2: polish start is the moment the text pipeline
                            // (which ends in the LLM pass) begins.
                            let t_polish_start_us = now_us();
                            // P2.6: selection-command takes run the spoken
                            // instruction as a one-shot transform over the
                            // selection; everything else uses auto-apply.
                            let mut result = pipeline.run(input, selection_transform.as_ref());
                            // The take's raw ASR text: the unpolished segments
                            // joined, when the take was polished segment by
                            // segment, else the engine's output as usual.
                            let segment_raw = segment_raws
                                .as_ref()
                                .and_then(|raws| {
                                    let joined = raws
                                        .iter()
                                        .filter_map(|s| s.as_deref())
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    (!joined.is_empty()).then_some(joined)
                                });
                            // A segment-polished take: the pipeline saw the
                            // segments already rewritten, so its own stages
                            // start from that text. Record the per-segment
                            // polish as a Polish stage of (joined unpolished
                            // ASR, joined polished) before the pipeline's
                            // stages, so the take's fillers and polish edits
                            // show in the Dictation tab like any other take.
                            // When the spans cannot spell the final text the
                            // bookkeeper's compose returns None and the entry
                            // keeps the existing safe fallback (no diff).
                            if let Some(before) = &segment_raw {
                                if before != &result.raw_input {
                                    result.stages.insert(
                                        0,
                                        teletype_core::pipeline::StageText {
                                            kind: teletype_core::pipeline::EditKind::Polish,
                                            before: before.clone(),
                                            after: result.raw_input.clone(),
                                        },
                                    );
                                }
                            }
                            // A.3.3: a command take decided by the decider
                            // replaces the provider's extraction. The decider
                            // ran only on command takes (the selection never
                            // reaches it — a Command take with selected text is
                            // re-keyed to Transform above), and its output is a
                            // superset of the provider's (free-form args filled
                            // from the transcript). `no_action` maps to `None`:
                            // the transcript is delivered as plain dictation,
                            // exactly like a failed extraction.
                            if take_kind == TakeKind::Command {
                                match decider {
                                    Some(d) => match d.decide(pcm, &result.raw_input) {
                                        Ok(decision) => {
                                            let action = decision.to_command_action();
                                            crate::log_entry(
                                                crate::LogLevel::Info,
                                                format!(
                                                    "command decider {}: tool={} args_source={} ({} ms)",
                                                    d.kind().label(),
                                                    decision.tool,
                                                    decision.args_source,
                                                    decision.decider_ms,
                                                ),
                                            );
                                            let tool = decision.tool.clone();
                                            result.command = action;
                                            decider_label = Some(d.kind().label().to_string());
                                            decider_ms = Some(decision.decider_ms);
                                            tool_label = Some(tool);
                                            args_source = Some(decision.args_source);
                                            trace_marks_pre.push(format!(
                                                "decider={} ({} ms)",
                                                d.kind().label(),
                                                decision.decider_ms,
                                            ));
                                        }
                                        Err(e) => {
                                            crate::log_entry(
                                                crate::LogLevel::Warn,
                                                format!(
                                                    "command decider error, falling back to the provider extraction: {e}"
                                                ),
                                            );
                                            // The provider's extraction (already in
                                            // `result.command`) stands; the trace
                                            // records the fallback.
                                            decider_label = Some(d.kind().label().to_string());
                                            trace_marks_pre.push(format!(
                                                "decider={} (error: {e})",
                                                d.kind().label(),
                                            ));
                                        }
                                    },
                                    None => {
                                        // No decider selected (non-English or no
                                        // slot): the provider's extraction stands.
                                        decider_label = Some("needle-fallback".into());
                                        trace_marks_pre.push("decider=needle-fallback".into());
                                    }
                                }
                            }
                            // P0.2: stage marks for this worker. The text
                            // pipeline (dictionary, ITN, polish gate, LLM) has
                            // no finer internal boundaries observable from here,
                            // so polish start/end bracket the same `run()` call
                            // the text-pipeline mark closes. When no transform
                            // ran, the polish marks stay empty and the reason is
                            // recorded instead.
                            let t_text_pipeline_end = now_us();
                            let first_token =
                                polish_first_token_us.load(std::sync::atomic::Ordering::Relaxed);
                            let polished = result.transform.as_ref().is_some_and(|t| t.transformed);
                            let polish_skipped_reason = result
                                .transform
                                .as_ref()
                                .and_then(|t| t.metrics.skip_reason.as_ref())
                                .map(|r| r.to_string())
                                .or_else(|| {
                                    if result.transform_skipped_no_model {
                                        Some("no LLM model loaded".to_string())
                                    } else if result.transform.is_none() {
                                        Some("no transform ran".to_string())
                                    } else {
                                        None
                                    }
                                });
                            // `polish_skipped_reason` is None exactly when a
                            // transform ran without a gate/model skip, so it
                            // is the honest "polish actually ran" evidence for
                            // the post-insertion card. The id comes from the
                            // engine Arc snapshotted for this take.
                            let polish_ran = polish_skipped_reason.is_none();
                            let polish_model = inference
                                .as_deref()
                                .map(|p| p.model_id().to_string());
                            let mut trace_marks = TraceStageMarks {
                                t_polish_start: Some(t_polish_start_us),
                                t_polish_first_token: match first_token {
                                    0 => None,
                                    v => Some(v),
                                },
                                t_polish_end: polished.then_some(t_text_pipeline_end),
                                t_text_pipeline_end: Some(t_text_pipeline_end),
                                polish_skipped_reason,
                                // P4: the streaming take's real trace marks
                                // (committed segments + tail audio ms), measured
                                // in `stop()` and carried by the session id. The
                                // whole-take fallback path keeps 0/0 (byte-
                                // identical to before P4).
                                committed_segments: streaming_marks.map(|m| m.0).unwrap_or(0),
                                tail_audio_ms: streaming_marks.map(|m| m.1).unwrap_or(0),
                                command_events: std::mem::take(&mut trace_marks_pre),
                            };
                            // P2.9: record the classification for every take
                            // that entered the command route — a dedicated
                            // command take (even when classification says
                            // dictation and the text falls back to injection)
                            // and a dictation take the pipeline turned into a
                            // command. `executed` / `refused(<reason>)` are
                            // appended at the execution site below, so the
                            // trace keeps the lifecycle order.
                            if take_kind == TakeKind::Command || result.command.is_some() {
                                trace_marks.command_events.push(format!(
                                    "classified={}",
                                    result
                                        .intent
                                        .map(teletype_core::llm::Intent::label)
                                        .unwrap_or("none")
                                ));
                            }

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

                            // Emit pipeline trace for the live visualizer.
                            let trace = serde_json::json!({
                                "raw_input": result.raw_input,
                                "intent": result.intent.map(|i| i.label().to_string()),
                                "command": result.command.as_ref().map(|c| {
                                    serde_json::json!({ "tool": c.tool, "arguments": c.arguments })
                                }),
                                "transformed": result.transformed,
                                "final_text": result.final_text,
                                "pipeline_ms": pipeline_ms,
                                "llm_ms": llm_ms,
                            });
                            tracing::info!(?trace, "pipeline-trace emitted");
                            let _ = app.emit("pipeline-trace", &trace);

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

                            // The pill must leave "Transforming…" on every exit
                            // path. The reset is sent through the controller
                            // channel (PipelineWorkerDone) so it is handled on
                            // the MAIN thread, where `self.show()` reliably
                            // delivers the event to the pill webview. A direct
                            // `emit_to` from this worker thread can be lost when
                            // the main thread is blocked on an IPC call that
                            // waits for a lock this worker still holds — that
                            // was the stuck-pill bug.
                            controller.send(Event::PipelineWorkerDone { session });
                            // DEV-ONLY artificial delay: widens the window
                            // between "bookkeeping done" and "controller may
                            // inject" so the cancel-vs-ready race can be
                            // exercised by hand. 0 unless the env var says
                            // otherwise; it is a diagnostic, not a feature.
                            let ready_delay = dev_delay_ms(PIPELINE_READY_DELAY_ENV);
                            if !ready_delay.is_zero() {
                                tracing::debug!(
                                    ?ready_delay,
                                    session,
                                    "dev: artificial delay before PipelineReady"
                                );
                                thread::sleep(ready_delay);
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
                            // UiState::Message broadcast. Silent skips (too
                            // short, gate, intent, validator fallback, a valid
                            // empty answer) return None: they stay in the log
                            // and the trace, but never flash on the pill or tray
                            // (F-13).
                            if let Some(msg) = result
                                .transform
                                .as_ref()
                                .and_then(|t| t.metrics.skip_reason.as_ref())
                                .and_then(teletype_core::state::skip_message)
                            {
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
                                // The pill renders this broadcast as a note, and
                                // a failure ("Polish failed: model load failed")
                                // carries Open Settings. The last Rust push was a
                                // click-through progress label, so make the pill
                                // clickable like any alert flash; the next push
                                // ("Inserting…", idle) sets it back.
                                crate::overlay::set_interactive(&app, true);
                            }

                            // A take with no speech produces empty final text.
                            // It is not a dictation: recording it puts a blank
                            // card in the Dictation screen and counts a phantom
                            // take in Insights.
                            if !result.final_text.trim().is_empty() {
                                // P1.3: usage counts are computed here (cheap,
                                // in-memory) but the store write is handed to the
                                // bookkeeper, so no disk write sits on the
                                // release→paste path. The bookkeeper coalesces
                                // bursts of rapid takes into one save per store.
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
                                    bookkeeper.usage(filler_counts, autotext_counts);
                                }

                                drop(dictionary);
                                drop(styles);
                                drop(profile);
                                drop(transforms);
                                drop(autotext);

                                // P1.3: history + transcript file + dictionary usage
                                // are handed to the bookkeeper. The entry is built
                                // here (the context was captured at pipeline run
                                // time, the app focused while dictating) but the
                                // disk writes happen on the bookkeeper thread,
                                // coalesced, and only AFTER `PipelineReady` has
                                // been shipped for the injection.
                                {
                                    let created_at = teletype_core::storage::now_ms();
                                    // "Polished by" only when polish actually
                                    // changed the text: the validator can return
                                    // a near-identical output as transformed.
                                    // Segment polish rewrote a long take before
                                    // the pipeline saw it, so that counts too.
                                    let polish_changed =
                                        result.polish_change.is_some() || segment_polished;
                                    // The LLM pass's own wall time, not the
                                    // whole text pipeline around it. Unknown for
                                    // polish that ran per segment mid-take.
                                    let polish_ms = result
                                        .polish_change
                                        .as_ref()
                                        .and(result.transform.as_ref())
                                        .map(|t| t.metrics.latency_ms as u64);
                                    // The take's raw ASR text: the unpolished
                                    // segments joined for a segment-polished
                                    // take, the engine's output otherwise.
                                    let raw_text =
                                        segment_raw.clone().or_else(|| Some(result.raw_input.clone()));
                                    let entry = teletype_core::history::DictationEntry {
                                        id: uuid::Uuid::new_v4().to_string(),
                                        created_at: teletype_core::storage::now_ms(),
                                        text: result.final_text.clone(),
                                        context: Some(result.context.clone()),
                                        // Measured, not assumed: how long the mic
                                        // was actually open for this take.
                                        duration_ms: speech_ms_for_session,
                                        raw_text,
                                        asr_model: Some(asr_model.clone()),
                                        polish_model: polish_changed
                                            .then(|| crate::commands::polish_model_label(&settings))
                                            .flatten(),
                                        polish_ms,
                                        word_count: Some(
                                            result.final_text.split_whitespace().count() as u32,
                                        ),
                                        // Set by the bookkeeper once the WAV is on
                                        // disk; edits are composed there too, off
                                        // the paste path.
                                        ..Default::default()
                                    };
                                    let dir = state.transcripts_dir();
                                    let app_name = result.context.application_name.clone();
                                    let transcript_text = result.final_text.clone();
                                    bookkeeper.history(
                                        entry,
                                        crate::bookkeeper::TakeExtras {
                                            stages: std::mem::take(&mut result.stages),
                                            audio: take_audio.take(),
                                        },
                                        dir,
                                        created_at,
                                        transcript_text,
                                        app_name,
                                    );
                                    if settings.dictionary_enabled {
                                        bookkeeper.dictionary_usage(result.final_text.clone());
                                    }
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
                                match state.update_settings_with(|settings| {
                                    settings.language = detected.clone();
                                }) {
                                    Ok((new, old)) if new.language != old.language => {
                                        let _ = app.emit("settings-changed", &());
                                    }
                                    Ok(_) => {}
                                    Err(error) => crate::log_entry(
                                        crate::LogLevel::Warn,
                                        format!("could not persist detected language: {error}"),
                                    ),
                                }
                            }
                            // Route to the scratchpad when it's enabled and
                            // frontmost; otherwise inject into the previous focus.
                            // The last bookkeeping step, and the hand-off. The
                            // worker does NOT perform the user-visible side
                            // effect (scratchpad append or text injection): it
                            // ships the prepared payload to the controller with
                            // `PipelineReady` and waits for the controller's
                            // verdict. A `Cancel` that reaches the controller
                            // queue before this event marks the session
                            // cancelled, so the verdict discards the result;
                            // one that reaches it after the verdict is simply
                            // too late. The worker never re-checks a shared
                            // flag — the FIFO order on the controller channel
                            // IS the cancellation decision.
                            // If the intent was Command and a structured action
                            // was extracted, execute it instead of injecting text.
                            // The Command Mode master switch gates execution:
                            // when off, a refused command falls through to the
                            // plain dictation path below (like a failed
                            // extraction), with the refusal traced.
                            let command_action = if settings.command_mode_enabled {
                                result.command.as_ref()
                            } else if result.command.is_some() {
                                crate::log_entry(crate::LogLevel::Info, "command mode disabled: delivering transcript as text");
                                trace_marks
                                    .command_events
                                    .push("refused(command mode disabled)".into());
                                None
                            } else {
                                None
                            };
                            if let Some(cmd) = command_action {
                                // Argument *keys* only. This line reaches the
                                // in-app log and the log file, and the values of
                                // send_message / type_text are dictated text; the
                                // log must never carry a transcript.
                                let arg_keys = cmd
                                    .arguments
                                    .as_object()
                                    .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                                    .unwrap_or_default();
                                crate::log_entry(
                                    crate::LogLevel::Info,
                                    format!("command extracted: {} ({arg_keys})", cmd.tool),
                                );
                                let exec_result =
                                    crate::command_executor::execute(cmd, &result.context);
                                // PRIV-1: the executor's result strings
                                // interpolate the action's argument values
                                // (dictated text). The log and the persisted
                                // trace must carry lengths, not values, so
                                // both are redacted with the argument
                                // values before they leave this scope. The
                                // pill emit below keeps the raw message:
                                // it is a transient on-screen note for the
                                // same user, not a persistence boundary.
                                let arg_values = cmd
                                    .arguments
                                    .as_object()
                                    .map(|o| {
                                        o.values()
                                            .filter_map(|v| v.as_str().map(str::to_owned))
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                match &exec_result {
                                    Ok(msg) => {
                                        crate::log_entry(
                                            crate::LogLevel::Success,
                                            format!(
                                                "command executed: {}",
                                                redact_command_message(msg, &arg_values)
                                            ),
                                        );
                                        // Show feedback in the pill so the user knows what happened.
                                        let _ = app.emit_to(
                                            "pill",
                                            "pill-skip",
                                            &serde_json::json!({ "message": format!("✓ {msg}") }),
                                        );
                                        // P2.9: the command route completed.
                                        trace_marks.command_events.push("executed".into());
                                    }
                                    Err(e) => {
                                        crate::log_entry(
                                            crate::LogLevel::Error,
                                            format!(
                                                "command execution failed: {}",
                                                redact_command_message(e, &arg_values)
                                            ),
                                        );
                                        let _ = app.emit_to(
                                            "pill",
                                            "pill-skip",
                                            &serde_json::json!({ "message": format!("✗ {e}") }),
                                        );
                                        // P2.9: refused, with the executor's
                                        // reason. PRIV-1: redacted, like the
                                        // log above; the reason rides into
                                        // the persisted trace.
                                        trace_marks.command_events.push(format!(
                                            "refused({})",
                                            redact_command_message(e, &arg_values)
                                        ));
                                    }
                                }
                                // Still record in history for audit. P1.3:
                                // handed to the bookkeeper like the dictation
                                // path, so the write is coalesced and off the
                                // critical path. No plain-text transcript line
                                // for a command (it is not dictated speech).
                                let created_at = teletype_core::storage::now_ms();
                                let entry = teletype_core::history::DictationEntry {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    created_at,
                                    // The spoken words only, not the marker.
                                    word_count: Some(
                                        result.raw_input.split_whitespace().count() as u32,
                                    ),
                                    text: format!("[command] {}", result.raw_input),
                                    context: Some(result.context.clone()),
                                    duration_ms: speech_ms_for_session,
                                    asr_model: Some(asr_model.clone()),
                                    ..Default::default()
                                };
                                bookkeeper.history(
                                    entry,
                                    crate::bookkeeper::TakeExtras::default(),
                                    state.transcripts_dir(),
                                    created_at,
                                    String::new(),
                                    result.context.application_name.clone(),
                                );
                                exit_guard.terminal(Event::PipelineReady {
                                    session,
                                    payload: PipelinePayload {
                                        session,
                                        raw_text: String::new(),
                                        final_text: String::new(),
                                        polish_change: None,
                                        context: result.context,
                                        record_target,
                                        detected_language,
                                        command: Some(cmd.clone()),
                                        trace_marks: trace_marks.clone(),
                                        decider: decider_label.clone(),
                                        decider_ms,
                                        tool: tool_label.clone(),
                                        args_source: args_source.clone(),
                                        losses: None,
                                        polish_profile: result.polish_profile,
                                        polish_profile_reason: result.polish_profile_reason,
                                        polish_ran,
                                        polish_model: polish_model.clone(),
                                    },
                                });
                            } else {
                                exit_guard.terminal(Event::PipelineReady {
                                    session,
                                    payload: PipelinePayload {
                                        session,
                                        raw_text: result.raw_input,
                                        final_text: result.final_text,
                                        polish_change: result.polish_change,
                                        context: result.context,
                                        record_target,
                                        detected_language,
                                        command: None,
                                        trace_marks,
                                        decider: decider_label,
                                        decider_ms,
                                        tool: tool_label,
                                        args_source,
                                        losses,
                                        polish_profile: result.polish_profile,
                                        polish_profile_reason: result.polish_profile_reason,
                                        polish_ran,
                                        polish_model,
                                    },
                                });
                            }
                            // The controller answers with `PipelineDone` once it
                            // has made the verdict and acted on it. Do NOT send
                            // it here: that would re-enter the done path and run
                            // the pill reset / edit-check twice (the
                            // repeat-paste bug, in UI form).
                            }
                        ));
                        if let Err(payload) = attempt {
                            crate::log_entry(
                                crate::LogLevel::Error,
                                format!(
                                    "pipeline worker panicked: {}; the take was lost",
                                    panic_text(payload.as_ref())
                                ),
                            );
                        }
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
                if is_no_speech_error(&e) {
                    // No speech in the take: the worker already logged this
                    // at Info. End exactly like the F-70 silent path: no
                    // error flash, no pill text, no paste, no history entry.
                    // The trace records the outcome (counts only) so a
                    // muted press is still visible in the session trace.
                    self.finish_trace(session, |t| {
                        t.errors.push("no speech detected".to_string());
                    });
                    self.go_idle();
                    return;
                }
                crate::log_entry(crate::LogLevel::Error, format!("transcription failed: {e}"));
                // P0.2: no transcript means no pipeline and no paste; the
                // trace ends here with the failure recorded.
                let msg = e.clone();
                self.finish_trace(session, |t| {
                    t.errors.push(format!("transcription failed: {msg}"));
                });
                if e == MIC_FAILURE_MESSAGE {
                    // T10a: a zero-sample take gets the permission-aware
                    // message. The pill's `mic-failure` action opens the OS
                    // microphone permission pane, which is the fix for a
                    // denied permission; a granted-but-silent mic keeps the
                    // existing copy.
                    let permission = mic_permission_status();
                    let message = zero_sample_take_message(permission);
                    self.flash_for(message, Duration::from_millis(6000), FlashKind::MicFailure);
                } else {
                    self.flash(&e);
                }
            }
        }
    }
}
