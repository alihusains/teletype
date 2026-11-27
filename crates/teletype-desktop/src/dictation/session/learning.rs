use crate::dictation::*;

impl Session {
    /// Reads back the previous dictation's field and, if the user changed it,
    /// feeds the difference into the personalization loop.
    ///
    /// Every guard here fails closed. A wrong signal teaches the model a
    /// preference the user never expressed, so an unreadable field, a field we
    /// no longer recognise, or a diff we cannot explain all result in learning
    /// nothing.
    pub(in crate::dictation) fn observe_pending_edit(&mut self) {
        // P7.2: the next dictation is one of the five observer triggers. The
        // learning-store observation runs first, with its own guards (anchor
        // + locate distance), and is independent of the UI-hint path below.
        self.observe_learning_store();

        let Some(app) = self.app().cloned() else {
            return;
        };
        let pending: Option<crate::edit_watch::PendingEdit> = {
            let state = app.state::<AppState>();
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.take()
        };
        let Some(pending) = pending else {
            crate::log_entry(
                crate::LogLevel::Info,
                "edit watch: nothing pending to observe",
            );
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
        crate::log_entry(
            crate::LogLevel::Info,
            format!("edit watch: field read ({} chars)", value.chars().count()),
        );

        let diff = crate::edit_watch::diff(&value, &pending.inserted);
        // `diff` decides whether the user's text is the field itself (in
        // place) or `inserted + tail` (append). Inferring it from lengths
        // drops every correction that shortens the text.
        let Some(user_text) = diff.edited_text(&pending.inserted) else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("edit watch: no learnable edit ({diff:?})"),
            );
            return;
        };
        let app_ctx = state.platform.active_application().unwrap_or_default();

        // The pending take's row carries the terms that were on screen when
        // it was recorded — the only trustworthy confirmation available when
        // we observe this edit (a new take's context may have moved on).
        // No row, no id or no terms leaves the empty slice, so every judge
        // and dictionary guard keeps its strict behaviour.
        let screen_terms: Vec<String> = pending
            .dictation_id
            .and_then(|id| {
                self.learning_store()
                    .and_then(|store| store.dictation_by_id(id).ok().flatten())
            })
            .map(|row| row.screen_terms)
            .unwrap_or_default();

        // Phase 7.4: the same observed edit feeds the learning store's
        // lexicon, which the per-take boost list reads back (inert while
        // boosting is off; `commands::record_dictation_edit` is the D006
        // dictionary path that currently makes a learned term stick). Gated exactly
        // like the preference/dictionary write, and every span is re-judged,
        // so style edits and synonym swaps can never become vocabulary.
        let lexicon_terms = {
            let gates_on = {
                let profile = state.profile.get();
                profile.learn_from_edits && profile.learn_terminology
            };
            if gates_on {
                match self.learning_store() {
                    Some(store) => match teletype_core::learning::promote::consider_observed_edit(
                        store,
                        &pending.inserted,
                        &user_text,
                        &teletype_core::learning::judge::JudgeContext {
                            allow_lowercase_terms: true,
                            screen_terms: screen_terms.clone(),
                            ..Default::default()
                        },
                        teletype_core::storage::now_ms(),
                    ) {
                        Ok(out) => {
                            if !out.promotions.is_empty() {
                                crate::log_entry(
                                    crate::LogLevel::Info,
                                    format!(
                                        "edit watch: {} mishearing outcome(s), created terms {:?}",
                                        out.promotions.len(),
                                        out.created_terms
                                    ),
                                );
                            }
                            out.created_terms
                        }
                        Err(e) => {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                format!("edit watch: lexicon promotion failed: {e}"),
                            );
                            Vec::new()
                        }
                    },
                    None => Vec::new(),
                }
            } else {
                Vec::new()
            }
        };

        // T10d: the same observed edit feeds the learned style rules the
        // dictation packet resolution reads back (`consider_style` is the
        // production caller T09 left unwired). Gated exactly like the
        // preference write; a store failure only skips the style half.
        {
            let profile = (*state.profile.get()).clone();
            if profile.learn_from_edits {
                let category = self
                    .screen_context
                    .as_ref()
                    .map(|ctx| ctx.app_category.as_str())
                    .unwrap_or("other");
                let signals =
                    teletype_core::personalization::learn::extract_signals_with_screen_terms(
                        &pending.inserted,
                        &user_text,
                        &app_ctx,
                        &screen_terms,
                    );
                for obs in teletype_core::personalization::learn::style_observations(&signals) {
                    if let Some(store) = self.learning_store() {
                        if let Err(e) = teletype_core::learning::promote::consider_style(
                            store,
                            category,
                            obs.rule,
                            &obs.value,
                            obs.agrees,
                            teletype_core::storage::now_ms(),
                        ) {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                format!("edit watch: style observation failed: {e}"),
                            );
                        }
                    }
                }
            }
        }

        let (changed, dictionary_changes, learned_words, dictionary_save_failed, failed_word) = {
            let mut profile = (*state.profile.get()).clone();
            let changed = commands::record_edit_core_public(
                &mut profile,
                &pending.inserted,
                &user_text,
                &app_ctx,
            );
            if !changed.is_empty() {
                if let Err(e) = state.profile.update(|p| *p = profile.clone()) {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!("edit watch: profile save failed: {e}"),
                    );
                }
            }
            // Pair ids with their user-facing descriptions for the undo
            // pill; looked up here while the profile is locked.
            // T5.1 (D006): a learned terminology correction is also recorded
            // on the dictionary word (for the "learned" badge), so carry its
            // from/to pair alongside the id and description.
            let changed: Vec<_> = changed
                .into_iter()
                .filter_map(|id| {
                    let pref = profile.preferences.iter().find(|p| p.id == id)?;
                    let (id, description) = (pref.id.clone(), pref.description.clone());
                    let learned = pref.learned_correction();
                    Some((id, description, learned))
                })
                .collect::<Vec<_>>();
            // Keep exactly what the dictionary write changed, so undo can put
            // it back: remove a word learning created, clear provenance it
            // backfilled, or drop only the alias it added. Alongside the
            // change, keep the canonical word and its user-facing kind so a
            // single-word learn can be shown on the pill without a second
            // dictionary lookup.
            let mut learned_words: Vec<(LearnedChange, String, commands::LearnedWordKind)> =
                Vec::new();
            let mut dictionary_save_failed = false;
            let mut failed_word = String::new();
            if changed.iter().any(|(_, _, l)| l.is_some()) {
                let mut dict = (*state.dictionary.get()).clone();
                for (_, _, learned) in &changed {
                    if let Some((from, to)) = learned {
                        // Same judge guard the command path applies: a style
                        // or structural edit must not become a learned word.
                        if !teletype_core::personalization::is_learnable_terminology_with_screen_terms(
                            from,
                            to,
                            &screen_terms,
                        ) {
                            continue;
                        }
                        // F-20: a pair the user said "no" to twice is not
                        // learned through this path either (checked here,
                        // at the write, before the word is created).
                        if let Some(store) = self.learning_store() {
                            match teletype_core::learning::learn_memory::is_blocked(
                                store,
                                from,
                                to,
                                teletype_core::storage::now_ms(),
                            ) {
                                Ok(true) => continue,
                                Ok(false) => {}
                                Err(e) => {
                                    crate::log_entry(
                                        crate::LogLevel::Warn,
                                        format!("edit watch: learn-memory check failed: {e}"),
                                    );
                                }
                            }
                        }
                        if failed_word.is_empty() {
                            failed_word = to.to_string();
                        }
                        match dict.insert_learned(from, to) {
                            LearnedChange::Unchanged => {}
                            change => {
                                if let Some(kind) = commands::learned_kind(&change) {
                                    learned_words.push((change, to.to_string(), kind));
                                }
                            }
                        }
                    }
                }
                if !learned_words.is_empty() {
                    if let Err(e) = state.dictionary.update(|d| *d = dict) {
                        // A failed write must not become a success offer: there
                        // is nothing to undo and nothing was saved. Surface it
                        // on the pill instead of toasting a learned word.
                        dictionary_save_failed = true;
                        crate::log_entry(
                            crate::LogLevel::Error,
                            format!("edit watch: dictionary save failed: {e}"),
                        );
                    }
                }
            }
            let dictionary_changes: Vec<LearnedChange> = learned_words
                .iter()
                .map(|(change, _, _)| change.clone())
                .collect();
            let changed = changed
                .into_iter()
                .map(|(id, description, _)| (id, description))
                .collect::<Vec<_>>();
            (
                changed,
                dictionary_changes,
                learned_words,
                dictionary_save_failed,
                failed_word,
            )
        };
        // A dictionary word or lexicon term can be learned without any
        // preference changing (the Phase 7 path). It still deserves a visible
        // notice and an undo offer; preference offers keep their exact
        // message and ids.
        let learned_something =
            !changed.is_empty() || !dictionary_changes.is_empty() || !lexicon_terms.is_empty();
        let notice_plan = commands::learned_notice_plan(
            learned_words.len(),
            dictionary_save_failed,
            self.phase == Phase::Idle,
        );
        if notice_plan == commands::LearnedNoticePlan::PillError {
            // Nothing was saved, so there is nothing to undo: no offer, no
            // tray item and no success toast. If the slot is free and this
            // was a single-word learn, the failure surfaces on the pill.
            if learned_words.len() == 1 && self.phase == Phase::Idle {
                let card = crate::overlay::show_learned_error(&app);
                let _ = app.emit_to(
                    crate::overlay::PILL_LABEL,
                    "learned-word-error",
                    &commands::LearnedWordSaveError {
                        card,
                        word: failed_word.clone(),
                    },
                );
            }
        } else if learned_something && notice_plan != commands::LearnedNoticePlan::None {
            if !changed.is_empty() {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("edit watch: learned {} preference(s)", changed.len()),
                );
            } else {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!(
                        "edit watch: learned {} word(s) and {} term(s) without a preference",
                        dictionary_changes.len(),
                        lexicon_terms.len()
                    ),
                );
            }
            // The pill tells an honest one-word story with its own Undo and
            // countdown. Multi-word learns, preference/lexicon-only learns,
            // and learns while the pill is busy keep the main-window toast,
            // whose Undo reverts the whole offer.
            let pill_admitted = notice_plan == commands::LearnedNoticePlan::Pill;
            let (message, ids) = if !changed.is_empty() {
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
                (message, ids)
            } else {
                // No preference changed, so there are no ids to carry; the
                // learned term itself is the honest, human-readable message.
                let first = lexicon_terms.first().cloned().unwrap_or_else(|| {
                    dictionary_changes
                        .first()
                        .map(|change| match change {
                            LearnedChange::Backfilled { from, .. }
                            | LearnedChange::AliasAdded { from, .. } => from.clone(),
                            LearnedChange::Created { .. } | LearnedChange::Unchanged => {
                                "a word".to_string()
                            }
                        })
                        .unwrap_or_else(|| "a word".to_string())
                });
                (format!("Learned: {first}"), Vec::new())
            };
            let offer_id = teletype_core::storage::now_ms();
            // Word-only offers have no preference ids; the pill/toast carries
            // an offer-id sentinel so a stale Undo cannot undo newer learning.
            let event_ids = if ids.is_empty() {
                vec![format!("offer:{offer_id}")]
            } else {
                ids.clone()
            };
            *state
                .last_learned
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                Some(commands::LearnedOffer {
                    id: offer_id,
                    preference_ids: ids.clone(),
                    dictionary_changes,
                    lexicon_terms,
                });
            crate::tray::offer_undo(&app, &message, offer_id);
            // Both clocks follow the same setting: the tray offer expires on
            // this timer, and the pill reads the same value for its dwell.
            // A change applies to the next learn offer, not a live one.
            let undo_secs = commands::clamped_learn_undo_secs(state.settings().learn_undo_secs);
            let expiry_controller = self.controller.clone();
            std::thread::Builder::new()
                .name("teletype-undo-expiry".into())
                .spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(undo_secs));
                    expiry_controller.send(Event::OfferUndoExpired { offer_id });
                })
                .ok();
            if pill_admitted {
                let (_, word, kind) = &learned_words[0];
                let subtitle = match kind {
                    commands::LearnedWordKind::Added => "Added to dictionary",
                    commands::LearnedWordKind::Updated => "Updated in dictionary",
                };
                // show_learned allocates the card id before the event is
                // emitted, so a size report with that id can show the window.
                let card = crate::overlay::show_learned(&app);
                let _ = app.emit_to(
                    crate::overlay::PILL_LABEL,
                    "learned-word",
                    &commands::LearnedWordNotice {
                        card,
                        offer_id,
                        word: word.clone(),
                        kind: *kind,
                    },
                );
                crate::overlay::announce_learned(&app, word, subtitle);
                // No toast: two live Undo buttons for one offer would race.
            } else {
                let _ = app.emit(
                    "learned-preference",
                    &serde_json::json!({ "message": message, "ids": event_ids }),
                );
            }
        }
    }

    // ── P7.2: learning-store recording + anchored observation ──────────

    /// Maximum learned terms injected into the post-ASR correction paths.
    /// Native ASR boost stays off (D029); this is the measured-safe way for
    /// personalization to affect output after decode.
    const MAX_LEARNED_POST_ASR_TERMS: usize = 64;

    /// Builds `PackTerm`s from the ranked lexicon for post-ASR correction.
    /// Only learned rows are included; manual words already live in the
    /// dictionary and contacts/packs have their own paths. App-scoped rows
    /// only apply in their own category, and every failure yields an empty
    /// list because correction is an optimization, never a blocker.
    pub(in crate::dictation) fn learned_terms_for_post_asr(
        data_dir: &std::path::Path,
        app_category: Option<&str>,
    ) -> Vec<teletype_core::vocab::PackTerm> {
        use teletype_core::learning::store::LearningStore;
        use teletype_core::vocab::PackTerm;

        let path = data_dir.join("learning").join("learning.sqlite");
        let Ok(store) = LearningStore::open(&path) else {
            return Vec::new();
        };
        let Ok(rows) = store.lexicon_terms_for_boost(Self::MAX_LEARNED_POST_ASR_TERMS as i64)
        else {
            return Vec::new();
        };
        rows.into_iter()
            .filter(|row| row.source == "learned" && !row.tombstoned)
            .filter(|row| match row.app_scope.as_deref() {
                Some(scope) => Some(scope) == app_category,
                None => true,
            })
            .flat_map(|row| {
                let aliases = row.aliases.clone();
                let canonical = row.term.clone();
                aliases
                    .into_iter()
                    .take(2)
                    .map(move |alias| PackTerm {
                        canonical: canonical.clone(),
                        alias,
                    })
                    .collect::<Vec<_>>()
            })
            .take(Self::MAX_LEARNED_POST_ASR_TERMS)
            .collect()
    }

    /// P7.5: builds the take's per-engine boost list from the lexicon scored
    /// for the focused app category plus the terms on screen. The one-entry
    /// cache is keyed by everything scoring depends on, so an unchanged
    /// context reuses the previous list. Every failure yields an empty list:
    /// boosting is an optimization and must never block a take.
    pub(in crate::dictation) fn build_boost_terms(&mut self) -> Vec<teletype_speech::BoostTerm> {
        use teletype_core::learning::boost::{
            boost_list, lexicon_fingerprint, screen_terms_hash, BoostCacheKey, BoostContext,
        };

        let kind = match self.state() {
            Some(state) => resolve_speech_model(&state.settings(), &state.models_dir).1,
            None => return Vec::new(),
        };
        let n = boost_capacity(kind);
        if n == 0 {
            return Vec::new();
        }
        let (app_category, screen_terms) = match &self.screen_context {
            Some(ctx) => (
                Some(ctx.app_category.as_str().to_string()),
                teletype_core::screen_context::ephemeral_terms(ctx),
            ),
            None => (None, Vec::new()),
        };
        let rows = match self.learning_store() {
            Some(store) => match store.lexicon_all() {
                Ok(rows) => rows,
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("boost terms: lexicon read failed: {e}"),
                    );
                    return Vec::new();
                }
            },
            None => return Vec::new(),
        };
        if rows.is_empty() && screen_terms.is_empty() {
            return Vec::new();
        }
        let key = BoostCacheKey {
            lexicon_version: lexicon_fingerprint(&rows),
            app_category: app_category.clone(),
            screen_terms_hash: screen_terms_hash(&screen_terms),
        };
        let now_ms = teletype_core::storage::now_ms() as i64;
        let core_terms = self.boost_cache.get_or_build(&key, || {
            boost_list(
                &rows,
                &BoostContext {
                    app_category: app_category.as_deref(),
                    screen_terms: &screen_terms,
                    now_ms,
                },
                n,
            )
        });
        to_engine_terms(core_terms)
    }

    /// Opens the learning store lazily on first use. The file lives at
    /// `<app_data>/learning/learning.sqlite` (the same location
    /// `commands.rs` resolves for the insights UI). A failure logs once and
    /// disables learning for the session: nothing here may ever block or
    /// fail a paste.
    ///
    /// Dropping the cache is `close_learning_store` (the "Delete all
    /// learning data" path must close the handle before deleting the file).
    pub(in crate::dictation) fn learning_store(
        &mut self,
    ) -> Option<&teletype_core::learning::store::LearningStore> {
        if self.learning_store.is_none() {
            let state = self.state()?;
            let dir = state.data_dir.clone();
            let path = dir.join("learning").join("learning.sqlite");
            match teletype_core::learning::store::LearningStore::open(&path) {
                Ok(store) => self.learning_store = Some(store),
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("learning store: open failed, disabled for session: {e}"),
                    );
                }
            }
        }
        self.learning_store.as_ref()
    }

    /// Drops the cached learning store handle. The next `learning_store()`
    /// call reopens a fresh one, so the app can learn again after a delete.
    /// Called on the controller thread only (the `CloseLearningStore` event
    /// and the event loop); the delete command sends that event because the
    /// cache lives on the worker, not on the `Controller` handle.
    pub(in crate::dictation) fn close_learning_store(&mut self) {
        self.learning_store = None;
    }

    /// Records one successful paste as a `DictationRow` and returns the row
    /// id. Only called from the paste site in `pipeline_ready` (refused and
    /// cancelled takes never reach it). Returns `None` when the store is
    /// unavailable or the insert failed — the paste itself is unaffected;
    /// the watch then arms without a store id and the observation is a no-op.
    pub(in crate::dictation) fn record_dictation_row(
        &mut self,
        payload: &PipelinePayload,
        final_text: &str,
    ) -> Option<i64> {
        // Collect everything we need from self before taking the store
        // borrow, to avoid a double borrow.
        let state = self.state()?;
        let settings = state.settings();
        // app_id + category: prefer the P6.4 screen context captured at
        // hotkey-down; fall back to the context the pipeline saw.
        let (app_id, app_category) = match &self.screen_context {
            Some(ctx) => (ctx.app_id.clone().unwrap_or_default(), ctx.app_category),
            None => (
                payload.context.application_id.clone(),
                teletype_core::app_category::app_category(
                    if payload.context.application_id.is_empty() {
                        None
                    } else {
                        Some(payload.context.application_id.as_str())
                    },
                    payload.context.window_title.as_deref(),
                ),
            ),
        };
        let engine = settings.selected_speech_model.clone();
        let lang = effective_language(settings.language.clone(), false);
        let polish_model = if settings.selected_llm_model.is_empty() {
            None
        } else {
            Some(settings.selected_llm_model.clone())
        };
        // Captured before the mutable store borrow below. These are the terms
        // the judge and the dictionary guard will need when this take's edit
        // is observed on the next dictation.
        let screen_terms = self
            .screen_context
            .as_ref()
            .map(teletype_core::screen_context::ephemeral_terms)
            .unwrap_or_default();
        let store = self.learning_store()?;
        // `raw_asr`: the transcript exactly as ASR produced it, carried on the
        // payload since the polish-diff card needed the pre-pipeline text.
        // Earlier builds recorded the post-pipeline text here as a compromise.
        // `words_json`: word timings are not carried onto the controller
        // either (they are consumed by the confidence rescorer on the
        // worker), so v1 records an empty list.
        let row = teletype_core::learning::store::DictationRow {
            id: 0,
            ts: teletype_core::storage::now_ms(),
            app_id,
            app_category: app_category.as_str().to_string(),
            engine,
            lang,
            audio_path: None, // audio retention is a later unit
            raw_asr: payload.raw_text.clone(),
            words_json: "[]".to_string(),
            pasted: final_text.to_string(),
            observed: None,
            edit_ops_json: None,
            polish_model,
            polish_profile: payload.polish_profile,
            screen_terms,
            // The controller does not know whether polish ran (that
            // knowledge stays on the worker's `PipelineResult`); the
            // `polish_skipped_reason` trace mark carries the truth for the
            // trace. Record `false` (polish attempted) as the v1 default.
            polish_skipped: false,
        };
        match store.record_dictation(&row) {
            Ok(id) => {
                // U3 settle-by-survival: on a take where polish changed words,
                // record the model's own before/after pairs so the sweep can
                // judge them by survival in 30 minutes. The pair comes from
                // the polish stage, never from raw-ASR-to-final: that diff
                // also carries dictionary correction, filler removal, ITN and
                // AutoText expansion, and a deterministic pass must never be
                // counted (or auto-applied) as a learned correction.
                //
                // T05a: each edit is attributed to the stage that made it
                // (Polish), using the same closed-set accounting the U4
                // polish guard uses. Stages that run after polish (emoji
                // restore, spoken-emoji formatting) are not in this pair,
                // so their changes are excluded by construction.
                if let Some((before, after)) = payload.polish_change.as_ref() {
                    let edits = teletype_core::learning::settle::edits_from_change(before, after);
                    if !edits.is_empty() {
                        if let Err(e) = store.set_settle_edits(
                            id,
                            &teletype_core::learning::settle::encode_edits(&edits),
                        ) {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                format!("learning store: set_settle_edits failed: {e}"),
                            );
                        }
                    }
                }
                Some(id)
            }
            Err(e) => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("learning store: record_dictation failed: {e}"),
                );
                None
            }
        }
    }

    /// Reads the field back and, if the pasted span is still locatable,
    /// writes `observed` + `edit_ops_json` to the store row. Triggered by
    /// the next dictation (the reliable moment: the user has finished
    /// editing or moved on). Every guard fails closed: unreadable field,
    /// unlocatable span, missing anchor or store id — drop, never guess.
    pub(in crate::dictation) fn observe_learning_store(&mut self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let Some(state) = self.state() else {
            return;
        };
        let (pending, observer, last_good) = {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let p = watch.pending().cloned();
            let o = watch.observer_mut().cloned();
            let lg = watch.last_good().cloned();
            (p, o, lg)
        };
        let (Some(pending), Some(mut observer)) = (pending, observer) else {
            return;
        };
        // Open a fresh sink handle on the same file (cheap on an existing
        // file); the session's own handle stays put.
        let sink: Option<crate::edit_watch::StoreSink> = self.state().and_then(|state| {
            let dir = state.data_dir.clone();
            let path = dir.join("learning").join("learning.sqlite");
            crate::edit_watch::StoreSink::open(&path)
        });
        let outcome = crate::edit_watch::observe_once(
            &mut observer,
            teletype_core::learning::observer::ObserverEvent::NextDictationStarted,
            crate::edit_watch::now_ms(),
            &pending,
            &read_focused_field,
            sink.as_ref()
                .map(|s| s as &dyn crate::edit_watch::LearningSink),
            last_good.as_ref(),
        );
        // Write the (final) observer back and KEEP the pending edit: the
        // caller `observe_pending_edit` takes it right after this call for
        // the preference/dictionary path. Clearing here made that path
        // unreachable for every real paste. A final observer never observes
        // twice, so the store layer cannot double-record.
        {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.set_observer(Some(observer));
        }
        if let Some(outcome) = outcome {
            match outcome {
                crate::edit_watch::ObserveOutcome::NotLocated => crate::log_entry(
                    crate::LogLevel::Info,
                    "learning observe: span not located, dropped",
                ),
                crate::edit_watch::ObserveOutcome::FieldUnreadable => crate::log_entry(
                    crate::LogLevel::Info,
                    "learning observe: field unreadable, dropped",
                ),
                _ => {}
            }
        }
        // The settle sweep rides this same lazy moment: rows recorded more
        // than 30 minutes ago are judged against what the field held, once.
        // The bandit is locked only for the sweep + save, never across the
        // long transcription/polish work above.
        if let Some(sink) = sink {
            let now_ms = crate::edit_watch::now_ms().max(0) as u64;
            let _ = state.update_bandit(|bandit| {
                sink.settle_sweep(now_ms, bandit);
            });
        }
    }

    /// The periodic half of the observer (20 s idle / 3 min cap), driven by
    /// the existing 2 s prewarm tick — no second timer. No-op when no watch
    /// is armed.
    pub(in crate::dictation) fn poll_learning_observer(&mut self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let Some(state) = self.state() else {
            return;
        };
        let (pending, observer, last_good) = {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // Keep the flush fallback fresh. The field is read here only while a
            // watch is armed, which is a bounded window after a dictation; the text
            // is held in memory and dropped with the pending edit.
            if let crate::edit_watch::FieldRead::Readable(text) = read_focused_field() {
                watch.note_field_read(&text, crate::edit_watch::now_ms());
            }
            let p = watch.pending().cloned();
            let o = watch.observer_mut().cloned();
            let lg = watch.last_good().cloned();
            (p, o, lg)
        };
        let (Some(pending), Some(mut observer)) = (pending, observer) else {
            return;
        };
        let sink: Option<crate::edit_watch::StoreSink> = self.state().and_then(|state| {
            let dir = state.data_dir.clone();
            let path = dir.join("learning").join("learning.sqlite");
            crate::edit_watch::StoreSink::open(&path)
        });
        let outcome = crate::edit_watch::poll_observer(
            &mut observer,
            crate::edit_watch::now_ms(),
            &pending,
            &read_focused_field,
            sink.as_ref()
                .map(|s| s as &dyn crate::edit_watch::LearningSink),
            last_good.as_ref(),
        );
        // Write the (possibly updated) observer back. The pending edit stays
        // for the next dictation's preference/dictionary path: the poll's
        // final observer never fires again, and `observe_pending_edit` is
        // the only consumer that clears it.
        {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.set_observer(Some(observer));
            if matches!(outcome, Some(crate::edit_watch::ObserveOutcome::NotLocated)) {
                crate::log_entry(
                    crate::LogLevel::Info,
                    "learning observe (poll): span not located, dropped",
                );
            }
        }
    }
}
