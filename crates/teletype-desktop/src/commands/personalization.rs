//! Personalization commands: profile, preferences, style profiles and language overrides.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use std::collections::BTreeMap;

use super::*;
use crate::AppState;
use tauri::{AppHandle, State};
use teletype_core::context::AppType;
use teletype_core::dictionary::{Dictionary, LearnedChange};
use teletype_core::personalization::{Preference, UserProfile};
use teletype_core::style::StyleProfile;

// ---- Personalization ----

/// The most recent take's learning, kept for the undo offer (tray menu item
/// and main-window toast) until it is undone or expires.
///
/// Carries the dictionary writes alongside the preference ids so undo can put
/// both stores back: removing the preferences alone used to leave a learned
/// word behind, which looked like the undo had failed.
#[derive(Debug, Clone, Default)]
pub struct LearnedOffer {
    pub preference_ids: Vec<String>,
    /// Exactly the dictionary writes this take made (never `Unchanged`).
    pub dictionary_changes: Vec<LearnedChange>,
    /// Lexicon terms this take created (Phase 7). Only terms the take added
    /// are listed, so undo never tombstones a term the user already had.
    pub lexicon_terms: Vec<String>,
}

impl LearnedOffer {
    /// Whether `ids` names this offer (order-insensitive, exact set), so a
    /// stale toast cannot undo a newer take's learning.
    pub fn matches(&self, ids: &[String]) -> bool {
        !self.preference_ids.is_empty()
            && self.preference_ids.len() == ids.len()
            && self.preference_ids.iter().all(|id| ids.contains(id))
    }
}

/// What an undo changed in each store.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UndoOutcome {
    pub preferences: usize,
    pub words: usize,
    /// Lexicon terms tombstoned (already-missing terms are not counted).
    pub lexicon: usize,
}

/// Records one observed edit of AI-polished text (T2.1): extracts candidate
/// signals (greeting / sign-off / terminology) and applies them to the
/// learned preferences, respecting the profile's learning gates. Returns the
/// ids of the preferences that changed; `Ok(vec![])` when nothing learned.
#[tauri::command]
pub async fn record_dictation_edit(
    state: State<'_, AppState>,
    ai_output: String,
    final_text: String,
) -> CommandResult<Vec<String>> {
    if ai_output.trim().is_empty() || final_text.trim().is_empty() || ai_output == final_text {
        return Ok(Vec::new());
    }

    let app_ctx = state.platform.active_application().unwrap_or_default();
    let mut profile = (*state.profile.get()).clone();
    let mut dictionary = (*state.dictionary.get()).clone();
    let (changed, dictionary_changes) = record_edit_and_learn(
        &mut profile,
        &mut dictionary,
        &ai_output,
        &final_text,
        &app_ctx,
    );
    if !changed.is_empty() {
        state.profile.update(|p| *p = profile)?;
    }
    // Persist the same dictionary snapshot the learning mutated. The old code
    // dropped that clone and saved a fresh copy of the unmodified state, so
    // this command never actually wrote a learned word.
    if !dictionary_changes.is_empty() {
        state.dictionary.update(|d| *d = dictionary)?;
    }
    Ok(changed)
}

/// The profile+dictionary half of [`record_dictation_edit`], pure so the
/// command and its tests share one path.
pub(crate) fn record_edit_and_learn(
    profile: &mut UserProfile,
    dictionary: &mut Dictionary,
    ai_output: &str,
    final_text: &str,
    app: &teletype_core::context::ApplicationContext,
) -> (Vec<String>, Vec<LearnedChange>) {
    let changed = record_edit_core(profile, ai_output, final_text, app);
    let words = learn_terminology_to_dictionary(dictionary, profile, ai_output, final_text, app);
    (changed, words)
}

/// The pure gate+extract+apply core of [`record_dictation_edit`], exposed to
/// the dictation controller's edit watcher so an observed edit and a
/// UI-reported edit go through exactly the same gates.
///
/// This is the seam that makes the personalization loop actually reachable:
/// before it existed, `record_dictation_edit` was a Tauri command with no
/// production caller, so nothing ever learned from the user's edits.
pub(crate) fn record_edit_core_public(
    profile: &mut UserProfile,
    ai_output: &str,
    final_text: &str,
    app: &teletype_core::context::ApplicationContext,
) -> Vec<String> {
    record_edit_core(profile, ai_output, final_text, app)
}

/// Learns terminology corrections observed in `ai_output` → `final_text` into
/// the user dictionary (the self-learning dictionary, D006/T5.1 / item 5 & 7).
///
/// Root cause of the "zero learned words" bug: the personalization loop
/// recorded terminology swaps into the `UserProfile` preferences but never
/// touched the `Dictionary`, so the Dictionary tab always showed 0 learned
/// words even though the model was learning. This is the missing link: every
/// terminology signal (a word the ASR misheard that the user corrected) is
/// also written to the dictionary with provenance, exactly like the reference
/// implementation's learned-alias path.
///
/// Returns the dictionary writes it made (empty when gated, when the judge
/// rejects the pair, or when the word already knew the alias). Respects the
/// same `learn_terminology` gate as the preference path so a user who turned
/// terminology learning off sees no dictionary growth either.
pub(crate) fn learn_terminology_to_dictionary(
    dictionary: &mut Dictionary,
    profile: &UserProfile,
    ai_output: &str,
    final_text: &str,
    app: &teletype_core::context::ApplicationContext,
) -> Vec<LearnedChange> {
    if !profile.learn_from_edits || !profile.learn_terminology {
        return Vec::new();
    }
    if ai_output.trim().is_empty() || final_text.trim().is_empty() || ai_output == final_text {
        return Vec::new();
    }
    let signals =
        teletype_core::personalization::learn::extract_signals(ai_output, final_text, app);
    let mut changes = Vec::new();
    for s in signals {
        let Some(rest) = s.key.strip_prefix("term:") else {
            continue;
        };
        let Some((from, _)) = rest.split_once('→') else {
            continue;
        };
        let Some(to) = s.canonical_word.as_deref() else {
            continue;
        };
        if !is_learnable_terminology(from, to) {
            continue;
        }
        match dictionary.insert_learned(from, to) {
            LearnedChange::Unchanged => {}
            change => changes.push(change),
        }
    }
    changes
}

/// Judge guard for the dictionary write path: only a mishearing may become a
/// learned word, never a style edit, a rewrite, or a structural span.
///
/// The Phase 7 judge calls a span `Mishearing` only when `to` is
/// proper-noun-like and neither side is an everyday word. That standard suits
/// the lexicon, but this path is the older D006 dictionary: its extractor has
/// already established a targeted same-shape swap and vetoed synonym pairs,
/// and its canonical example — "margets" → "Markets" — has an everyday word
/// on the `to` side, which the judge reports as `Skip { EverydayWord }`.
/// Accepting only `Mishearing` would silently disable the learning this guard
/// is meant to protect. So: reject structural skips (paths, URLs, numbers,
/// code, markdown) and style/rewrite/ambiguous judgements, but keep a
/// word-list skip when the pair is still phonetically close — a person's name
/// or a business term is exactly what the dictionary is for.
pub(crate) fn is_learnable_terminology(from: &str, to: &str) -> bool {
    use teletype_core::learning::judge::{
        judge_replace, phon_sim, JudgeContext, Judgement, SkipReason,
    };

    /// The judge's own mishearing band floor.
    const MIN_PHONETIC: f32 = 0.6;

    let ctx = JudgeContext {
        // The extractor already found a targeted swap; treat that as the
        // sub-0.9 evidence the judge expects before it drops the
        // proper-noun requirement.
        min_conf: Some(0.8),
        ..Default::default()
    };
    match judge_replace(from, to, &ctx) {
        Judgement::Mishearing { .. } => true,
        Judgement::Skip {
            reason:
                SkipReason::EverydayWord | SkipReason::CommonFirstName | SkipReason::EverydayAcronym,
        } => phon_sim(from, to) >= MIN_PHONETIC,
        _ => false,
    }
}

/// The pure gate+extract+apply core of [`record_dictation_edit`], factored
/// out so it can be unit-tested without a Tauri `State`.
fn record_edit_core(
    profile: &mut UserProfile,
    ai_output: &str,
    final_text: &str,
    app: &teletype_core::context::ApplicationContext,
) -> Vec<String> {
    use teletype_core::personalization::learn::{apply_signals, extract_signals};
    use teletype_core::personalization::PreferenceScope;

    if ai_output.trim().is_empty() || final_text.trim().is_empty() || ai_output == final_text {
        return Vec::new();
    }
    if !profile.learn_from_edits {
        return Vec::new();
    }
    let signals: Vec<_> = extract_signals(ai_output, final_text, app)
        .into_iter()
        .filter(|s| {
            (matches!(s.scope, PreferenceScope::Global) || profile.learn_app_specific)
                && (!s.key.starts_with("term:") || profile.learn_terminology)
        })
        .collect();
    apply_signals(profile, &signals)
}

#[tauri::command]
pub async fn get_profile(state: State<'_, AppState>) -> CommandResult<UserProfile> {
    Ok((*state.profile.get()).clone())
}

#[tauri::command]
pub async fn add_preference(
    state: State<'_, AppState>,
    preference: Preference,
) -> CommandResult<Preference> {
    let mut profile = (*state.profile.get()).clone();
    profile.add(preference.clone());
    state.profile.update(|p| *p = profile)?;
    Ok(preference)
}

/// Replaces one preference's description and phrase (T5.6). Preserves the
/// learned preference's evidence (count and provenance) and `created_at`.
#[tauri::command]
pub async fn update_preference(
    state: State<'_, AppState>,
    id: String,
    description: String,
    phrase: String,
) -> CommandResult<()> {
    let mut profile = (*state.profile.get()).clone();
    if !profile.update(&id, &description, &phrase) {
        return Err(CommandError::not_found("Preference not found"));
    }
    state.profile.update(|p| *p = profile)?;
    Ok(())
}

#[tauri::command]
pub async fn remove_preference(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut profile = (*state.profile.get()).clone();
    if !profile.remove(&id) {
        return Err(CommandError::not_found("Preference not found"));
    }
    state.profile.update(|p| *p = profile)?;
    Ok(())
}

#[tauri::command]
pub async fn clear_learned(state: State<'_, AppState>) -> CommandResult<usize> {
    let mut profile = (*state.profile.get()).clone();
    let n = profile.clear_learned();
    state.profile.update(|p| *p = profile)?;
    Ok(n)
}

/// Undoes just-learned preferences by id (the undo pill). Reverts the
/// dictionary writes that learning made in the same take, and only applies to
/// the live offer, so a stale toast cannot undo newer learning. Unknown or
/// stale ids are a no-op, so a double-click cannot error.
#[tauri::command]
pub async fn undo_learned(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> CommandResult<usize> {
    let Some(outcome) = undo_offer_from_state(&state, &ids)? else {
        return Ok(0);
    };
    crate::tray::clear_undo_offer(&app);
    Ok(outcome.preferences)
}

/// Applies an offer to clones of both stores. Pure so the atomicity rule
/// (one undo reverts the preference *and* its dictionary write) is testable
/// without a Tauri `State`.
pub(crate) fn undo_offer_in(
    profile: &mut UserProfile,
    dictionary: &mut Dictionary,
    offer: &LearnedOffer,
    learning_store: Option<&teletype_core::learning::store::LearningStore>,
) -> UndoOutcome {
    let mut outcome = UndoOutcome::default();
    for id in &offer.preference_ids {
        if profile.remove(id) {
            outcome.preferences += 1;
        }
    }
    for change in &offer.dictionary_changes {
        if dictionary.revert_learned(change) {
            outcome.words += 1;
        }
    }
    if let Some(store) = learning_store {
        for term in &offer.lexicon_terms {
            // Only a live row is worth counting; a term that vanished or was
            // already tombstoned by another path is a no-op, not an undo.
            let live = matches!(
                store.lexicon_by_term(term),
                Ok(Some(row)) if !row.tombstoned
            );
            if live && teletype_core::learning::promote::undo(store, term).is_ok() {
                outcome.lexicon += 1;
            }
        }
    }
    outcome
}

/// Undoes the live offer when `preference_ids` names it. Returns `None` when
/// there is no live offer or the ids belong to an older one (nothing was
/// touched). The caller owns clearing the tray/UI offer.
pub(crate) fn undo_offer_from_state(
    state: &AppState,
    preference_ids: &[String],
) -> Result<Option<UndoOutcome>, String> {
    let offer = state
        .last_learned
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let Some(offer) = offer.filter(|offer| offer.matches(preference_ids)) else {
        return Ok(None);
    };
    let mut profile = (*state.profile.get()).clone();
    let mut dictionary = (*state.dictionary.get()).clone();
    // Open the lexicon before touching anything so a broken store surfaces as
    // an error with both snapshots untouched, rather than a half undo.
    let learning_store = if offer.lexicon_terms.is_empty() {
        None
    } else {
        let path = state.data_dir.join("learning").join("learning.sqlite");
        Some(
            teletype_core::learning::store::LearningStore::open(&path)
                .map_err(|e| format!("open learning store: {e}"))?,
        )
    };
    let outcome = undo_offer_in(
        &mut profile,
        &mut dictionary,
        &offer,
        learning_store.as_ref(),
    );
    // Both snapshots are written only after the clones succeeded; if a write
    // fails the error is surfaced rather than silently leaving half an undo.
    if outcome.preferences > 0 {
        state.profile.update(|p| *p = profile)?;
    }
    if outcome.words > 0 {
        state.dictionary.update(|d| *d = dictionary)?;
    }
    Ok(Some(outcome))
}

#[tauri::command]
pub async fn set_profile_settings(
    state: State<'_, AppState>,
    learn_from_edits: bool,
    learn_app_specific: bool,
    learn_terminology: bool,
) -> CommandResult<()> {
    let mut profile = (*state.profile.get()).clone();
    profile.learn_from_edits = learn_from_edits;
    profile.learn_app_specific = learn_app_specific;
    profile.learn_terminology = learn_terminology;
    state.profile.update(|p| *p = profile)?;
    Ok(())
}

/// Master "Personalization: Off" switch (T5.9). Disabling empties the
/// preference packet and stops learning; the stored preferences survive, so
/// re-enabling restores them.
#[tauri::command]
pub async fn set_personalization_enabled(
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<()> {
    let mut profile = (*state.profile.get()).clone();
    profile.set_personalization_enabled(enabled);
    state.profile.update(|p| *p = profile)?;
    Ok(())
}

/// Sets the S1-mini control axes. Values are the wire strings from the model
/// card (e.g. "semi-formal"); an unknown value falls back to that axis's
/// default so a stale UI can never send an off-set control token.
#[tauri::command]
pub async fn set_s1_control(
    state: State<'_, AppState>,
    styling: String,
    structure: String,
    context: String,
) -> CommandResult<()> {
    use teletype_core::transforms::prompt::{S1Context, S1Control, S1Structure, S1Styling};
    fn parse_styling(s: &str) -> S1Styling {
        match s {
            "casual" => S1Styling::Casual,
            "semi-casual" => S1Styling::SemiCasual,
            "formal" => S1Styling::Formal,
            _ => S1Styling::SemiFormal,
        }
    }
    fn parse_structure(s: &str) -> S1Structure {
        if s == "prose" {
            S1Structure::Prose
        } else {
            S1Structure::Lists
        }
    }
    fn parse_context(s: &str) -> S1Context {
        if s == "email" {
            S1Context::Email
        } else {
            S1Context::General
        }
    }
    let mut profile = (*state.profile.get()).clone();
    profile.s1_control = S1Control {
        styling: parse_styling(&styling),
        structure: parse_structure(&structure),
        context: parse_context(&context),
    };
    state.profile.update(|p| *p = profile)?;
    Ok(())
}

// ---- Per-app language overrides (P3.3) ----

/// Sets the ASR language override for one app key. Pass an empty `lang` to
/// remove an existing override. The override is keyed by the normalized app
/// key (lowercased bundle id, or lowercased app name when no id is exposed).
#[tauri::command]
pub async fn set_app_language_override(
    state: State<'_, AppState>,
    app_key: String,
    lang: String,
) -> CommandResult<()> {
    let key = app_key.trim().to_ascii_lowercase();
    if key.is_empty() {
        return Err(CommandError::user("app key must not be empty"));
    }
    let mut settings = state.settings();
    let lang = lang.trim().to_string();
    if lang.is_empty() {
        settings.app_language_overrides.remove(&key);
    } else {
        settings.app_language_overrides.insert(key, lang);
    }
    state.replace_settings(settings)?;
    Ok(())
}

/// Returns the full map of per-app language overrides.
#[tauri::command]
pub async fn get_app_language_overrides(
    state: State<'_, AppState>,
) -> CommandResult<std::collections::BTreeMap<String, String>> {
    Ok(state.settings().app_language_overrides)
}

// ---- Style profiles ----

#[tauri::command]
pub async fn list_style_profiles(state: State<'_, AppState>) -> CommandResult<Vec<StyleProfile>> {
    Ok(state.styles.get().profiles.clone())
}

#[tauri::command]
pub async fn create_style_profile(
    state: State<'_, AppState>,
    profile: StyleProfile,
) -> CommandResult<StyleProfile> {
    let mut store = (*state.styles.get()).clone();
    store.insert(profile.clone())?;
    state.styles.update(|s| *s = store)?;
    Ok(profile)
}

#[tauri::command]
pub async fn update_style_profile(
    state: State<'_, AppState>,
    profile: StyleProfile,
) -> CommandResult<StyleProfile> {
    let mut store = (*state.styles.get()).clone();
    store.update(profile.clone())?;
    state.styles.update(|s| *s = store)?;
    Ok(profile)
}

#[tauri::command]
pub async fn delete_style_profile(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut store = (*state.styles.get()).clone();
    store.remove(&id)?;
    // If the deleted profile was active, clear the selection.
    if settings_active_style_is(&state, &id) {
        let mut settings = state.settings();
        settings.active_style_profile = String::new();
        state.replace_settings(settings)?;
    } else {
        state.styles.update(|s| *s = store)?;
    }
    Ok(())
}

fn settings_active_style_is(state: &AppState, id: &str) -> bool {
    state.settings().active_style_profile == id
}

#[tauri::command]
pub async fn set_active_style_profile(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    // "" means none; any other id must exist.
    if !id.is_empty() {
        let store = state.styles.get();
        if store.get(&id).is_none() {
            return Err(CommandError::not_found("Profile not found"));
        }
    }
    let mut settings = state.settings();
    settings.active_style_profile = id;
    state.replace_settings(settings)
}

#[tauri::command]
pub async fn reset_style_profiles(state: State<'_, AppState>) -> CommandResult<usize> {
    let mut store = (*state.styles.get()).clone();
    let n = store.reset_built_ins();
    state.styles.update(|s| *s = store)?;
    Ok(n)
}

/// Sets the per-app style override: the style profile that applies
/// automatically when the given app is frontmost and the user has not
/// explicitly picked a style for this dictation. An empty `style_id`
/// removes the override. Keys are normalized (trimmed, lowercased).
#[tauri::command]
pub async fn set_app_style_override(
    state: State<'_, AppState>,
    app_key: String,
    style_id: String,
) -> CommandResult<()> {
    let mut store = (*state.styles.get()).clone();
    store.set_app_style_override(&app_key, &style_id)?;
    state.styles.update(|s| *s = store)?;
    Ok(())
}

#[tauri::command]
pub async fn get_app_style_overrides(
    state: State<'_, AppState>,
) -> CommandResult<std::collections::BTreeMap<String, String>> {
    Ok(state.styles.get().app_style_overrides.clone())
}

// ---- Style resolution (Unit H: make style measurable) ----

/// One rung of the cascade that decides which writing style applies to a
/// dictation. The Style screen lists every rung with what it says, so the
/// level that actually decides is visible instead of guessed at.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleLayer {
    /// 1..=5, highest priority first.
    pub rank: u8,
    /// Stable key: "explicit" | "app_override" | "category_default" | "global" | "none".
    pub key: String,
    /// Human label for the UI, e.g. "This app's override".
    pub label: String,
    /// Resolved style id, "" when this layer decides nothing.
    pub style_id: String,
    /// Style name, "" when unresolved.
    pub style_name: String,
    /// True for the layer that actually wins.
    pub winner: bool,
    /// Why this layer did or did not win, one short sentence.
    pub note: String,
}

/// The whole cascade for the frontmost app, plus what it ends up telling the
/// model. Returned as one struct rather than a bare `Vec` so the payload can
/// name the app the layers were computed against.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleResolution {
    /// Display name of the frontmost app; "" when the platform says nothing.
    pub frontmost_app: String,
    /// Stable category key: "email" | "chat" | "social" | "coding" |
    /// "document" | "browser" | "terminal" | "unknown".
    pub frontmost_app_type: String,
    /// Rank order, highest priority first.
    pub layers: Vec<StyleLayer>,
    /// The winning style's phrases — what the model is actually told.
    pub effective_phrases: Vec<String>,
    /// What the selected polish model can be offered at all. The Style tab
    /// renders from this instead of from hard-coded model ids.
    pub capability: StyleCapability,
}

/// What the Style tab is allowed to show and switch on, for one polish model.
///
/// `teletype_core::style::model_style` owns the mapping; this only adds the
/// display name from the model index and flattens the three questions the
/// screen asks, so no id comparison survives in the UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleCapability {
    pub model_id: String,
    /// Name from the model index, falling back to the id, then to plain copy
    /// when nothing is selected.
    pub model_name: String,
    /// "fixedBehaviour" | "controlLine" | "stylePhrases".
    pub channel: &'static str,
    /// Editing the styles list changes what the model writes.
    pub styles_apply: bool,
    /// The Tone / Structure / Context controls reach this model.
    pub reads_control_line: bool,
    /// Two styles can produce two different polishes, so Compare is honest.
    pub compare_meaningful: bool,
}

fn style_capability(model_id: &str) -> StyleCapability {
    use teletype_core::style::StyleChannel;
    let style = teletype_core::style::model_style(model_id);
    let channel = match style.channel {
        StyleChannel::FixedBehaviour => "fixedBehaviour",
        StyleChannel::ControlLine => "controlLine",
        StyleChannel::StylePhrases => "stylePhrases",
    };
    let model_name = if model_id.is_empty() {
        "No model selected".to_string()
    } else {
        teletype_inference::catalog::find(model_id)
            .map(|entry| entry.name.to_string())
            .unwrap_or_else(|| model_id.to_string())
    };
    StyleCapability {
        model_id: model_id.to_string(),
        model_name,
        channel,
        styles_apply: style.styles_apply(),
        reads_control_line: style.reads_control_line(),
        compare_meaningful: style.compare_is_meaningful(),
    }
}

/// The category of apps sharing one automatic style, phrased so it can open a
/// sentence, or `None` for categories with no automatic style. Mirrors
/// `teletype_core::style::default_style_for_app`; it only words the result, it
/// never re-decides it.
fn category_group(app_type: &AppType) -> Option<&'static str> {
    match app_type {
        AppType::Email => Some("Email and documents"),
        AppType::Document => Some("Documents"),
        AppType::Chat => Some("Chat apps"),
        AppType::Social => Some("Social apps"),
        AppType::Coding => Some("Coding apps"),
        AppType::Browser | AppType::Terminal | AppType::Unknown => None,
    }
}

/// The stable lowercase key for an app category (the `AppType` serde form).
fn app_type_key(app_type: &AppType) -> &'static str {
    match app_type {
        AppType::Email => "email",
        AppType::Chat => "chat",
        AppType::Social => "social",
        AppType::Coding => "coding",
        AppType::Document => "document",
        AppType::Browser => "browser",
        AppType::Terminal => "terminal",
        AppType::Unknown => "unknown",
    }
}

/// How to refer to the frontmost app in a sentence: the platform's name when it
/// gives one, else the bundle id, else "this app".
fn app_reference(name: &str, id: &str) -> String {
    let name = name.trim();
    if !name.is_empty() {
        return name.to_string();
    }
    let id = id.trim();
    if !id.is_empty() {
        return id.to_string();
    }
    "this app".to_string()
}

/// A style's display name for a sentence, or a plain description when the id no
/// longer names an existing style. Never returns a raw id.
fn style_word(names: impl Fn(&str) -> String, id: &str) -> String {
    if id.is_empty() {
        return "no style".to_string();
    }
    let name = names(id);
    let name = name.trim();
    if name.is_empty() {
        // A stored id whose style has since been deleted: honest, and not an id.
        return "a style that no longer exists".to_string();
    }
    name.to_string()
}

/// Builds the cascade rows for one app.
///
/// Pure on purpose: `tauri::State` cannot be constructed in a unit test (see
/// `.agents/brain/learnings/2026-10-11-tauri-state-cannot-be-constructed-in-tests.md`),
/// so the command is a thin wrapper that feeds real state into this function
/// and the tests drive this function directly.
///
/// The resolved id comes from the SAME two functions the pipeline calls
/// (`default_style_for_app` then `resolve_style_id`, with the bundle id tried
/// before the display name), so this reporting layer cannot drift from the
/// resolution that actually runs.
fn build_layers(
    app_name: &str,
    app_id: &str,
    app_type: &AppType,
    overrides: &BTreeMap<String, String>,
    active: &str,
    explicit: &str,
    names: impl Fn(&str) -> String,
) -> Vec<StyleLayer> {
    let app_keys = [app_id, app_name];
    let app_ref = app_reference(app_name, app_id);

    // 1. What each rung says, using the production functions.
    let category_default = teletype_core::style::default_style_for_app(app_type, active);
    // Asking `resolve_style_id` with nothing below it is how the override rung
    // reports its own answer without re-implementing the lookup.
    let override_id = teletype_core::style::resolve_style_id(overrides, &app_keys, "", "");
    let resolved =
        teletype_core::style::resolve_style_id(overrides, &app_keys, &category_default, explicit);

    // 2. Which rung decides. A category default that merely echoes the global
    // pick (Browser / Terminal / Unknown fall through, and a matching manual
    // pick) is reported as the global pick deciding, because that is the level
    // the user would have to change to change the outcome.
    let winner_key = if !explicit.trim().is_empty() {
        "explicit"
    } else if !override_id.is_empty() {
        "app_override"
    } else if !category_default.is_empty() && category_default != active {
        "category_default"
    } else if !active.is_empty() {
        "global"
    } else {
        "none"
    };
    let decided_by = match winner_key {
        "explicit" => "your pick for this dictation",
        "app_override" => "this app's own style",
        "category_default" => "the automatic style for this kind of app",
        _ => "",
    };

    let mut layers = Vec::with_capacity(5);

    layers.push(StyleLayer {
        rank: 1,
        key: "explicit".into(),
        label: "This dictation's pick".into(),
        style_id: explicit.to_string(),
        style_name: if explicit.is_empty() {
            String::new()
        } else {
            names(explicit)
        },
        winner: winner_key == "explicit",
        note: if explicit.is_empty() {
            "Only applies when you choose a style for one dictation. Nothing is picked now."
                .to_string()
        } else {
            "You picked this style for this dictation, so it outranks every level below."
                .to_string()
        },
    });

    let override_note = if override_id.is_empty() {
        format!("No override set for {app_ref}.")
    } else if winner_key == "app_override" {
        format!("Set for {app_ref}, so it outranks the automatic and global picks.")
    } else {
        format!("Set for {app_ref}, but {decided_by} outranks it.")
    };
    layers.push(StyleLayer {
        rank: 2,
        key: "app_override".into(),
        label: "This app's override".into(),
        style_id: override_id.clone(),
        style_name: if override_id.is_empty() {
            String::new()
        } else {
            names(&override_id)
        },
        winner: winner_key == "app_override",
        note: override_note,
    });

    let category_note = match category_group(app_type) {
        Some(group) => {
            let word = style_word(&names, &category_default);
            if winner_key == "category_default" {
                format!("{group} use {word} automatically.")
            } else if category_default == active {
                format!("{group} use {word} automatically, the same as the global pick.")
            } else {
                format!("{group} would use {word}, but {decided_by} outranks it.")
            }
        }
        None => "This kind of app has no automatic style, so it falls through to the global pick."
            .to_string(),
    };
    layers.push(StyleLayer {
        rank: 3,
        key: "category_default".into(),
        label: "Automatic for this kind of app".into(),
        style_id: category_default.clone(),
        style_name: if category_default.is_empty() {
            String::new()
        } else {
            names(&category_default)
        },
        winner: winner_key == "category_default",
        note: category_note,
    });

    let global_note = if active.is_empty() {
        "No global style is set.".to_string()
    } else if winner_key == "global" {
        "Your global pick decides, because nothing above it applies to this app.".to_string()
    } else {
        format!(
            "Your global pick is {}, but {decided_by} outranks it.",
            style_word(&names, active)
        )
    };
    layers.push(StyleLayer {
        rank: 4,
        key: "global".into(),
        label: "Global pick".into(),
        style_id: active.to_string(),
        style_name: if active.is_empty() {
            String::new()
        } else {
            names(active)
        },
        winner: winner_key == "global",
        note: global_note,
    });

    if resolved.is_empty() {
        layers.push(StyleLayer {
            rank: 5,
            key: "none".into(),
            label: "No style".into(),
            style_id: String::new(),
            style_name: String::new(),
            winner: true,
            note: "Nothing sets a style, so your text is polished with no style phrases."
                .to_string(),
        });
    }

    debug_assert_eq!(
        layers
            .iter()
            .find(|l| l.winner)
            .map(|l| l.style_id.as_str())
            .unwrap_or(""),
        resolved.as_str(),
        "the winning layer must be what the pipeline resolves"
    );

    layers
}

/// Reports the style cascade for the app that is frontmost right now. Read-only:
/// it touches no settings and no store, so calling it cannot change what the
/// next dictation does.
#[tauri::command]
pub async fn style_resolution(state: State<'_, AppState>) -> CommandResult<StyleResolution> {
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let store = (*state.styles.get()).clone();
    let settings = state.settings();
    let active = settings.active_style_profile;
    let selected_model = settings.selected_llm_model;
    // No per-dictation selection exists outside a dictation, so rung 1 is
    // reported empty. See `explicit` in `build_layers` for how it would win.
    let layers = build_layers(
        &app_ctx.application_name,
        &app_ctx.application_id,
        &app_ctx.application_type,
        &store.app_style_overrides,
        &active,
        "",
        |id| store.get(id).map(|p| p.name.clone()).unwrap_or_default(),
    );
    let resolved = layers
        .iter()
        .find(|l| l.winner)
        .map(|l| l.style_id.clone())
        .unwrap_or_default();
    Ok(StyleResolution {
        frontmost_app: app_reference(&app_ctx.application_name, &app_ctx.application_id),
        frontmost_app_type: app_type_key(&app_ctx.application_type).to_string(),
        effective_phrases: store.active_phrases(&resolved),
        layers,
        capability: style_capability(&selected_model),
    })
}

/// The prompt preferences for one previewed style: the personalization packet's
/// own style lines, then this style's phrases appended and deduped, exactly as
/// `teletype_core::pipeline` injects them. Pure, so the dedupe contract is
/// testable without a loaded model.
fn merge_style_phrases(packet: &[String], style_phrases: &[String]) -> Vec<String> {
    let mut merged = packet.to_vec();
    for phrase in style_phrases {
        if !merged.contains(phrase) {
            merged.push(phrase.clone());
        }
    }
    merged
}

/// Runs the polish transform over `input` with one style's phrases injected the
/// way production does, WITHOUT touching any setting. This is how the Style
/// screen measures a style: the same sentence, two styles, on screen.
///
/// Mirrors `commands::transforms::test_transform` (the same lock, clone, packet
/// and engine path) and `pipeline`'s phrase injection. Like `test_transform` it
/// calls the engine inline, so a preview blocks this async task for as long as
/// a local model takes (order of 20 s per pane).
#[tauri::command]
pub async fn preview_style_profile(
    state: State<'_, AppState>,
    profile_id: String,
    input: String,
) -> CommandResult<String> {
    if input.trim().is_empty() {
        return Err(CommandError::user(
            "Write a sentence to compare first, then pick the styles.",
        ));
    }
    // The transform production would use: the enabled auto-apply one, else the
    // built-in Polish definition.
    let transforms = (*state.transforms.get()).clone();
    let transform = transforms
        .auto_apply()
        .cloned()
        .or_else(|| transforms.get("builtin-polish").cloned())
        .ok_or_else(|| {
            CommandError::user("No polish step is available. Reset steps under Transforms.")
        })?;
    let engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or_else(|| {
            CommandError::user("No polish model is loaded. Pick one under Models, then try again.")
        })?;
    let provider = engine.as_ref();
    let store = (*state.styles.get()).clone();
    let profile = (*state.profile.get()).clone();
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let packet = teletype_core::personalization::packet::resolve(&profile, &app_ctx);
    let preferences = merge_style_phrases(&packet.style, &store.active_phrases(&profile_id));
    let prompt_ctx = teletype_core::transforms::prompt::PromptContext {
        app: Some(app_ctx),
        preferences,
        preferred_terms: packet.terms,
        user_instruction: None,
        language: profile.language.clone(),
        s1_control: profile.s1_control,
        list_style: state.settings().list_style.resolve(),
        polish_max_change_percent: state.settings().polish_max_change_percent,
        screen_terms: Vec::new(),
    };
    let result = teletype_core::transforms::engine::run_transform_blocking(
        provider,
        &transform,
        &input,
        &prompt_ctx,
        &mut None,
    );
    Ok(result.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use teletype_core::dictionary::{Dictionary, DictionaryWord};
    use teletype_core::personalization::learn::{apply_signals, Signal};
    use teletype_core::personalization::PreferenceScope;

    fn term_signal(from: &str, to: &str) -> Signal {
        Signal {
            key: format!("term:{from}→{to}"),
            description: format!("Prefer '{to}' instead of '{from}'"),
            phrase: format!("use '{to}' instead of '{from}'"),
            scope: PreferenceScope::Global,
            canonical_word: Some(to.to_string()),
        }
    }

    #[test]
    fn learn_terminology_writes_the_word_and_its_alias() {
        let app = teletype_core::context::ApplicationContext::unknown();
        let profile = UserProfile::default();
        let mut dict = Dictionary::default();
        let changes = learn_terminology_to_dictionary(
            &mut dict,
            &profile,
            "I work at ic margets.",
            "I work at IC Markets.",
            &app,
        );
        assert_eq!(changes.len(), 1, "{changes:?}");
        let w = dict.find("Markets").expect("learned word missing");
        assert_eq!(w.learned_from.as_deref(), Some("margets"));
        assert_eq!(w.aliases, vec!["margets".to_string()]);
    }

    #[test]
    fn learn_terminology_accepts_a_phonetic_mishearing() {
        assert!(is_learnable_terminology("margets", "Markets"));
        assert!(is_learnable_terminology("kubernates", "kubernetes"));
        assert!(is_learnable_terminology("pie torch", "PyTorch"));
    }

    #[test]
    fn learn_terminology_accepts_a_shortened_name_correction() {
        // The names.wav fixture transcribes "Siobhan O'Rourke" as "Shavano
        // Rourke". The user's correction "Shivon" is one character shorter,
        // which is exactly the shape the old caller lost by inferring
        // append-vs-in-place from the replacement's length.
        let app = teletype_core::context::ApplicationContext::unknown();
        let profile = UserProfile::default();
        let mut dict = Dictionary::default();
        let changes = learn_terminology_to_dictionary(
            &mut dict,
            &profile,
            "Shavano Rourke, Epha, Py Torch, Kubernetes.",
            "Shivon Rourke, Epha, Py Torch, Kubernetes.",
            &app,
        );
        assert_eq!(changes.len(), 1, "{changes:?}");
        let w = dict.find("Shivon").expect("learned word missing");
        assert_eq!(w.learned_from.as_deref(), Some("Shavano"));
    }

    #[test]
    fn learn_terminology_rejects_non_mishearings() {
        // Structural skips: numbers and paths are never terminology.
        assert!(!is_learnable_terminology("2024", "2025"));
        assert!(!is_learnable_terminology("src/main.rs", "src/main2.rs"));
        // A casing edit is style, not a new term.
        assert!(!is_learnable_terminology("markets", "Markets"));
        // Unrelated words are not mishearings.
        assert!(!is_learnable_terminology("xylophone", "zzzqqq"));
    }

    #[test]
    fn record_edit_and_learn_persists_the_learned_word() {
        // Regression: `record_dictation_edit` used to mutate a clone of the
        // dictionary and then save a fresh clone of the old state, so the
        // learned word was silently dropped by the command path.
        let app = teletype_core::context::ApplicationContext::unknown();
        let mut profile = UserProfile::default();
        let mut dict = Dictionary::default();
        let (changed, words) = record_edit_and_learn(
            &mut profile,
            &mut dict,
            "I work at ic margets.",
            "I work at IC Markets.",
            &app,
        );
        assert!(!changed.is_empty());
        assert_eq!(words.len(), 1, "{words:?}");
        assert!(profile
            .preferences
            .iter()
            .any(|p| p.learned_to.as_deref() == Some("Markets")));
        let w = dict
            .find("Markets")
            .expect("the command path must persist the learned word");
        assert_eq!(w.learned_from.as_deref(), Some("margets"));
    }

    #[test]
    fn undo_offer_reverts_the_preference_and_the_word_it_created() {
        let mut profile = UserProfile::default();
        let ids = apply_signals(&mut profile, &[term_signal("margets", "Markets")]);
        assert_eq!(ids.len(), 1);
        let mut dict = Dictionary::default();
        let change = dict.insert_learned("margets", "Markets");
        let offer = LearnedOffer {
            preference_ids: ids,
            dictionary_changes: vec![change],
            lexicon_terms: Vec::new(),
        };

        let outcome = undo_offer_in(&mut profile, &mut dict, &offer, None);
        assert_eq!(outcome.preferences, 1);
        assert_eq!(outcome.words, 1);
        assert!(profile.preferences.is_empty());
        assert!(dict.find("Markets").is_none());
    }

    #[test]
    fn undo_offer_leaves_a_pre_existing_word_in_place() {
        let mut dict = Dictionary::default();
        dict.insert(DictionaryWord::new("Markets", "")).unwrap();
        let change = dict.insert_learned("margets", "Markets");
        let mut profile = UserProfile::default();
        let offer = LearnedOffer {
            preference_ids: Vec::new(),
            dictionary_changes: vec![change],
            lexicon_terms: Vec::new(),
        };

        let outcome = undo_offer_in(&mut profile, &mut dict, &offer, None);
        assert_eq!(outcome.words, 1);
        let w = dict
            .find("Markets")
            .expect("pre-existing word must survive");
        assert!(w.learned_from.is_none());
        assert!(w.aliases.is_empty());
    }

    fn lexicon_row(term: &str) -> teletype_core::learning::store::LexiconRow {
        teletype_core::learning::store::LexiconRow {
            term: term.into(),
            aliases: vec!["pie torch".into()],
            source: "learned".into(),
            weight: 1.0,
            hits: 0,
            false_alarms: 0,
            created: 1,
            last_used: None,
            tombstoned: false,
            app_scope: None,
            learned_from: None,
        }
    }

    #[test]
    fn undo_offer_tombstones_the_lexicon_term_it_created() {
        let dir = tempfile::tempdir().unwrap();
        let store = teletype_core::learning::store::LearningStore::open(
            &dir.path().join("learning.sqlite"),
        )
        .unwrap();
        store.upsert_lexicon(&lexicon_row("PyTorch")).unwrap();
        let mut profile = UserProfile::default();
        let mut dict = Dictionary::default();
        let offer = LearnedOffer {
            preference_ids: Vec::new(),
            dictionary_changes: Vec::new(),
            lexicon_terms: vec!["PyTorch".into()],
        };

        let outcome = undo_offer_in(&mut profile, &mut dict, &offer, Some(&store));
        assert_eq!(outcome.lexicon, 1);
        let row = store.lexicon_by_term("PyTorch").unwrap().unwrap();
        assert!(row.tombstoned, "undo must stop the term from boosting");
    }

    #[test]
    fn undo_offer_without_a_store_still_reverts_the_rest() {
        let mut profile = UserProfile::default();
        let ids = apply_signals(&mut profile, &[term_signal("margets", "Markets")]);
        let mut dict = Dictionary::default();
        let change = dict.insert_learned("margets", "Markets");
        let offer = LearnedOffer {
            preference_ids: ids,
            dictionary_changes: vec![change],
            lexicon_terms: vec!["Markets".into()],
        };

        let outcome = undo_offer_in(&mut profile, &mut dict, &offer, None);
        assert_eq!(outcome.preferences, 1);
        assert_eq!(outcome.words, 1);
        assert_eq!(
            outcome.lexicon, 0,
            "no store means no lexicon to undo, not a failure"
        );
    }

    #[test]
    fn undo_offer_does_not_count_a_missing_lexicon_term() {
        let dir = tempfile::tempdir().unwrap();
        let store = teletype_core::learning::store::LearningStore::open(
            &dir.path().join("learning.sqlite"),
        )
        .unwrap();
        let mut profile = UserProfile::default();
        let mut dict = Dictionary::default();
        let offer = LearnedOffer {
            preference_ids: Vec::new(),
            dictionary_changes: Vec::new(),
            lexicon_terms: vec!["NotThere".into()],
        };

        let outcome = undo_offer_in(&mut profile, &mut dict, &offer, Some(&store));
        assert_eq!(outcome.lexicon, 0);
        assert!(store.lexicon_by_term("NotThere").unwrap().is_none());
    }

    #[test]
    fn learned_offer_matches_only_its_own_ids() {
        let offer = LearnedOffer {
            preference_ids: vec!["a".into(), "b".into()],
            dictionary_changes: Vec::new(),
            lexicon_terms: Vec::new(),
        };
        assert!(offer.matches(&["b".into(), "a".into()]));
        // A stale toast (older offer) must not undo a newer take's writes.
        assert!(!offer.matches(&["a".into()]));
        assert!(!offer.matches(&["a".into(), "c".into()]));
        assert!(!offer.matches(&[]));
    }

    #[test]
    fn record_edit_identical_texts_learn_nothing() {
        let mut profile = UserProfile::default();
        let app = teletype_core::context::normalize("com.google.gmail", "Gmail");
        let changed = record_edit_core(&mut profile, "hi john", "hi john", &app);
        assert!(changed.is_empty());
        assert!(profile.preferences.is_empty());
        // Empty inputs are a no-op too.
        let changed = record_edit_core(&mut profile, "", "", &app);
        assert!(changed.is_empty());
    }

    #[test]
    fn record_edit_signoff_swap_creates_preference() {
        let mut profile = UserProfile::default();
        let app = teletype_core::context::normalize("com.google.gmail", "Gmail");
        // "Best" -> "Regards" is a closing swap (last line of each text). It
        // also trips the position-aligned terminology heuristic ("Best" vs
        // "Regards" differ by 3 chars, 4+ chars each, trailing context
        // matches), so the edit yields one sign-off and one term signal.
        let changed = record_edit_core(
            &mut profile,
            "Hi team,\nPlease review the report.\nBest",
            "Hi team,\nPlease review the report.\nRegards",
            &app,
        );
        assert!(changed.iter().all(|id| profile.get(id).is_some()));
        let signoffs: Vec<_> = profile
            .preferences
            .iter()
            .filter(|p| p.description.contains("sign-off"))
            .collect();
        assert_eq!(signoffs.len(), 1);
        assert!(signoffs[0].description.contains("'Regards'"));
    }

    #[test]
    fn record_edit_respects_learning_gates() {
        let app = teletype_core::context::normalize("com.google.gmail", "Gmail");
        // Unknown app: extract_signals keys every signal Global, so the
        // app-specific gate cannot filter them out (the scope gate is
        // exercised by the sign-off test on a known app instead).
        let unknown = teletype_core::context::ApplicationContext::unknown();
        let ai = "Dear John,\nPlease send the report.\nBest";
        let fin = "Hi John,\nPlease send the document.\nRegards";

        // All gates off: nothing learned.
        let mut profile = UserProfile {
            learn_from_edits: false,
            ..Default::default()
        };
        assert!(record_edit_core(&mut profile, ai, fin, &app).is_empty());

        // Terminology off, app-specific on (unknown app → Global scope): the
        // greeting and sign-off land, the term signal is gated out.
        let mut profile = UserProfile {
            learn_terminology: false,
            ..Default::default()
        };
        let changed = record_edit_core(&mut profile, ai, fin, &unknown);
        assert_eq!(changed.len(), 2);
        assert!(profile
            .preferences
            .iter()
            .all(|p| !p.description.contains("document")));

        // Terminology off AND app-specific off, on a known app (Gmail): all
        // signals are app-scoped, so nothing learns.
        let mut profile = UserProfile {
            learn_app_specific: false,
            ..Default::default()
        };
        profile.learn_terminology = false;
        assert!(record_edit_core(&mut profile, ai, fin, &app).is_empty());

        // Terminology on, app-specific off: the greeting and sign-off are
        // app-scoped (Gmail) and gated out. The terminology signal is
        // app-scoped too (the scope is chosen per-context, not per-key), so
        // with app-specific learning off nothing lands.
        let mut profile = UserProfile {
            learn_app_specific: false,
            ..Default::default()
        };
        let changed = record_edit_core(&mut profile, ai, fin, &app);
        assert!(changed.is_empty());
        assert!(profile.preferences.is_empty());

        // Terminology off: greeting + sign-off land, term does not.
        let mut profile = UserProfile {
            learn_terminology: false,
            ..Default::default()
        };
        let changed = record_edit_core(&mut profile, ai, fin, &unknown);
        assert_eq!(changed.len(), 2);
        assert!(profile
            .preferences
            .iter()
            .all(|p| !p.description.contains("document")));

        // All gates on: greeting + sign-off land. "report"→"document" is a
        // synonym swap (both common words), so BUG-001's judge vetoes it —
        // it must NOT be learned as a terminology preference.
        let mut profile = UserProfile::default();
        let changed = record_edit_core(&mut profile, ai, fin, &unknown);
        assert_eq!(changed.len(), 2);
        assert_eq!(profile.preferences.len(), 2);
        assert!(profile
            .preferences
            .iter()
            .all(|p| !p.description.contains("document")));
    }

    // ---- Style cascade reporting (Unit H) ----

    use teletype_core::context::AppType;
    use teletype_core::style::StyleProfileStore;

    /// Names styles the way the command does, from the built-in store.
    fn built_in_names(store: &StyleProfileStore) -> impl Fn(&str) -> String + '_ {
        move |id: &str| store.get(id).map(|p| p.name.clone()).unwrap_or_default()
    }

    fn winner_of(layers: &[StyleLayer]) -> &StyleLayer {
        let winners: Vec<_> = layers.iter().filter(|l| l.winner).collect();
        assert_eq!(winners.len(), 1, "exactly one layer may decide");
        winners[0]
    }

    #[test]
    fn style_layers_are_ranked_and_have_exactly_one_winner() {
        let store = StyleProfileStore::with_built_ins();
        let layers = build_layers(
            "Gmail",
            "com.google.gmail",
            &AppType::Email,
            &store.app_style_overrides,
            "style-concise",
            "",
            built_in_names(&store),
        );
        let ranks: Vec<u8> = layers.iter().map(|l| l.rank).collect();
        assert_eq!(ranks, vec![1, 2, 3, 4], "ascending rank, no none-row");
        assert!(ranks.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(winner_of(&layers).key, "category_default");
        for layer in &layers {
            assert!(!layer.label.is_empty());
            assert!(!layer.note.is_empty());
        }
    }

    #[test]
    fn unresolved_style_adds_a_none_row_that_wins() {
        let store = StyleProfileStore::with_built_ins();
        let layers = build_layers(
            "Terminal",
            "com.apple.Terminal",
            &AppType::Terminal,
            &store.app_style_overrides,
            "",
            "",
            built_in_names(&store),
        );
        assert_eq!(layers.len(), 5);
        assert_eq!(layers[4].key, "none");
        assert_eq!(layers[4].rank, 5);
        let winner = winner_of(&layers);
        assert_eq!(winner.key, "none");
        assert!(winner.style_id.is_empty());
        assert!(
            winner.note.contains("Nothing sets a style"),
            "{}",
            winner.note
        );
        // The four reporting rungs are still present, all empty.
        assert!(layers[..4].iter().all(|l| !l.winner));
    }

    #[test]
    fn style_app_override_row_beats_category_default_and_global() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("com.google.gmail", "style-casual")
            .unwrap();
        let layers = build_layers(
            "Gmail",
            "com.google.gmail",
            &AppType::Email,
            &store.app_style_overrides,
            "style-concise",
            "",
            built_in_names(&store),
        );
        let winner = winner_of(&layers);
        assert_eq!(winner.key, "app_override");
        assert_eq!(winner.style_id, "style-casual");
        assert_eq!(winner.style_name, "Casual");
        // The outranked rungs still say what they would have said.
        let find = |key: &str| layers.iter().find(|l| l.key == key).unwrap();
        assert_eq!(find("category_default").style_id, "style-professional");
        assert!(!find("category_default").winner);
        assert_eq!(find("global").style_id, "style-concise");
        assert!(!find("global").winner);
        assert!(
            find("global").note.contains("outranks"),
            "{}",
            find("global").note
        );
        assert!(
            !find("global").note.contains("style-concise"),
            "a note must never leak a raw id: {}",
            find("global").note
        );
    }

    #[test]
    fn style_explicit_pick_beats_the_app_override_row() {
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("com.google.gmail", "style-casual")
            .unwrap();
        let layers = build_layers(
            "Gmail",
            "com.google.gmail",
            &AppType::Email,
            &store.app_style_overrides,
            "style-concise",
            "style-professional",
            built_in_names(&store),
        );
        let winner = winner_of(&layers);
        assert_eq!(winner.key, "explicit");
        assert_eq!(winner.style_id, "style-professional");
    }

    /// The regression `style.rs::a_bundle_id_override_resolves_against_a_bundle_id`
    /// exists for, asserted through the reporting layer: the Settings UI tells
    /// people to type a bundle id, so the bundle-id form must light up the
    /// override row, and so must the display-name form for platforms that
    /// report no id at all.
    #[test]
    fn style_override_row_resolves_from_either_key_form() {
        // Stored as a bundle id, platform reports both.
        let mut store = StyleProfileStore::with_built_ins();
        store
            .set_app_style_override("com.slackmac.Slack", "style-casual")
            .unwrap();
        let layers = build_layers(
            "Slack",
            "com.slackmac.Slack",
            &AppType::Chat,
            &store.app_style_overrides,
            "style-concise",
            "",
            built_in_names(&store),
        );
        let winner = winner_of(&layers);
        assert_eq!(winner.key, "app_override");
        assert_eq!(winner.style_id, "style-casual");

        // Stored as the visible name, platform reports no bundle id.
        let mut by_name = StyleProfileStore::with_built_ins();
        by_name
            .set_app_style_override("Slack", "style-professional")
            .unwrap();
        let layers = build_layers(
            "Slack",
            "",
            &AppType::Chat,
            &by_name.app_style_overrides,
            "style-concise",
            "",
            built_in_names(&by_name),
        );
        let winner = winner_of(&layers);
        assert_eq!(winner.key, "app_override");
        assert_eq!(winner.style_id, "style-professional");
        assert_eq!(winner.style_name, "Professional");
    }

    #[test]
    fn style_fall_through_category_reports_the_global_pick_as_winner() {
        let store = StyleProfileStore::with_built_ins();
        let layers = build_layers(
            "Safari",
            "com.apple.Safari",
            &AppType::Browser,
            &store.app_style_overrides,
            "style-casual",
            "",
            built_in_names(&store),
        );
        let winner = winner_of(&layers);
        assert_eq!(winner.key, "global");
        // Browser has no automatic style, so rung 3 falls through by design.
        let cat = layers.iter().find(|l| l.key == "category_default").unwrap();
        assert!(!cat.winner);
        assert!(cat.note.contains("no automatic style"), "{}", cat.note);
    }

    #[test]
    fn style_category_default_that_matches_global_is_reported_as_global() {
        let store = StyleProfileStore::with_built_ins();
        // Email maps to Professional, and Professional is also the global pick:
        // changing the global pick would not change the outcome, so the global
        // row is the one marked in use.
        let layers = build_layers(
            "Gmail",
            "com.google.gmail",
            &AppType::Email,
            &store.app_style_overrides,
            "style-professional",
            "",
            built_in_names(&store),
        );
        assert_eq!(winner_of(&layers).key, "global");
        let cat = layers.iter().find(|l| l.key == "category_default").unwrap();
        assert_eq!(cat.style_id, "style-professional");
        assert!(cat.note.contains("same as the global pick"), "{}", cat.note);
    }

    #[test]
    fn notes_never_leak_a_raw_style_id() {
        let store = StyleProfileStore::with_built_ins();
        for (name, id, kind) in [
            ("Gmail", "com.google.gmail", AppType::Email),
            ("Slack", "com.slackmac.Slack", AppType::Chat),
            ("Xcode", "com.apple.dt.Xcode", AppType::Coding),
            ("Terminal", "com.apple.Terminal", AppType::Terminal),
        ] {
            for active in ["", "style-concise", "style-casual", "gone"] {
                let layers = build_layers(
                    name,
                    id,
                    &kind,
                    &store.app_style_overrides,
                    active,
                    "",
                    built_in_names(&store),
                );
                for layer in &layers {
                    for leaked in [
                        "style-concise",
                        "style-professional",
                        "style-casual",
                        "gone",
                    ] {
                        assert!(
                            !layer.note.contains(leaked),
                            "{} note leaks {leaked}: {}",
                            layer.key,
                            layer.note
                        );
                    }
                    if !layer.style_id.is_empty() && store.get(&layer.style_id).is_some() {
                        assert!(
                            !layer.style_name.is_empty(),
                            "{}: a resolvable id must carry its name ({:?})",
                            layer.key,
                            layer
                        );
                    }
                    if layer.style_id.is_empty() {
                        assert!(layer.style_name.is_empty(), "{}: {:?}", layer.key, layer);
                    }
                }
                winner_of(&layers);
            }
        }
    }

    #[test]
    fn style_preview_of_an_unknown_style_merges_no_phrases() {
        let store = StyleProfileStore::with_built_ins();
        let packet = vec!["professional".to_string(), "friendly".to_string()];
        let merged = merge_style_phrases(&packet, &store.active_phrases("no-such-style"));
        assert_eq!(merged, packet);
        assert!(store.active_phrases("no-such-style").is_empty());
    }

    #[test]
    fn style_preview_dedupes_a_phrase_the_packet_already_has() {
        let store = StyleProfileStore::with_built_ins();
        let phrases = store.active_phrases("style-casual");
        assert_eq!(phrases.len(), 2, "built-in casual phrases changed");
        let packet = vec![phrases[0].clone(), "professional".to_string()];
        let merged = merge_style_phrases(&packet, &phrases);
        assert_eq!(
            merged.iter().filter(|p| **p == phrases[0]).count(),
            1,
            "the packet's copy must survive exactly once: {merged:?}"
        );
        assert!(merged.contains(&phrases[1]));
        // Packet order first, then the style's additions, as pipeline appends.
        assert_eq!(merged[0], phrases[0]);
        assert_eq!(merged[1], "professional");
        // Inputs are not mutated, so a preview can never write state back.
        assert_eq!(packet.len(), 2);
        assert_eq!(phrases.len(), 2);
    }

    /// Pins the wire contract the Style screen reads: camelCase keys, exactly
    /// these names. A rename here breaks the UI silently.
    #[test]
    fn style_resolution_wire_shape_is_camel_case() {
        let store = StyleProfileStore::with_built_ins();
        let layers = build_layers(
            "Gmail",
            "com.google.gmail",
            &AppType::Email,
            &store.app_style_overrides,
            "",
            "",
            built_in_names(&store),
        );
        let resolution = StyleResolution {
            frontmost_app: "Gmail".into(),
            frontmost_app_type: app_type_key(&AppType::Email).to_string(),
            effective_phrases: store.active_phrases("style-professional"),
            layers,
            capability: style_capability("speakoflow-mini"),
        };
        let json = serde_json::to_value(&resolution).unwrap();
        let mut top: Vec<String> = json
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        top.sort();
        assert_eq!(
            top,
            vec![
                "capability",
                "effectivePhrases",
                "frontmostApp",
                "frontmostAppType",
                "layers"
            ]
        );
        assert_eq!(
            json["capability"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .len(),
            6
        );
        assert_eq!(json["capability"]["channel"], "fixedBehaviour");
        assert_eq!(json["capability"]["stylesApply"], false);
        assert_eq!(json["capability"]["modelName"], "SpeakoFlow Mini");
        assert_eq!(json["frontmostApp"], "Gmail");
        assert_eq!(json["frontmostAppType"], "email");
        assert_eq!(json["effectivePhrases"][0], "use a professional tone");
        let row = &json["layers"][2];
        let mut fields: Vec<String> = row.as_object().unwrap().keys().cloned().collect();
        fields.sort();
        assert_eq!(
            fields,
            vec![
                "key",
                "label",
                "note",
                "rank",
                "styleId",
                "styleName",
                "winner"
            ]
        );
        assert_eq!(row["key"], "category_default");
        assert_eq!(row["styleId"], "style-professional");
        assert_eq!(row["styleName"], "Professional");
        assert_eq!(row["winner"], true);
    }

    /// The Style tab's three questions, answered per model rather than by id
    /// comparisons in the screen. SpeakoFlow Mini is the shipped default and it
    /// honours neither styles nor controls, which the old hard-coded list missed.
    #[test]
    fn capability_tracks_the_model_index() {
        let default_model = style_capability("speakoflow-mini");
        assert_eq!(default_model.channel, "fixedBehaviour");
        assert!(!default_model.styles_apply);
        assert!(!default_model.reads_control_line);
        assert!(!default_model.compare_meaningful);

        let s1 = style_capability("s1-mini");
        assert_eq!(s1.channel, "controlLine");
        assert!(s1.reads_control_line);
        assert!(!s1.styles_apply);

        let qwen = style_capability("qwen3.5-2b");
        assert_eq!(qwen.channel, "controlLine");
        assert!(qwen.reads_control_line);
        assert!(qwen.styles_apply);

        let generic = style_capability("fast");
        assert_eq!(generic.channel, "stylePhrases");
        assert!(generic.styles_apply);
        assert!(!generic.reads_control_line);
        assert!(generic.compare_meaningful);

        // A model the index does not know still gets a usable name, and no
        // selection at all says so instead of naming a random model.
        assert_eq!(
            style_capability("some-other-model").model_name,
            "some-other-model"
        );
        assert_eq!(style_capability("").model_name, "No model selected");
    }
}
