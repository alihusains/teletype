//! Personalization commands: profile, preferences, style profiles and language overrides.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use tauri::State;
use teletype_core::personalization::{Preference, UserProfile};
use teletype_core::style::StyleProfile;
use crate::AppState;
use super::*;

// ---- Personalization ----

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
    let changed = record_edit_core(&mut profile, &ai_output, &final_text, &app_ctx);
    if !changed.is_empty() {
        state.profile.update(|p| *p = profile.clone())?;
    }
    // Self-learning dictionary: terminology corrections also land in the
    // Dictionary (item 5/7). The profile lock is still held, so read the
    // gates from it before the dictionary write.
    let dict_changed = {
        let mut dictionary = (*state.dictionary.get()).clone();
        learn_terminology_to_dictionary(
            &mut dictionary,
            &profile,
            &ai_output,
            &final_text,
            &app_ctx,
        )
    };
    if dict_changed {
        let dictionary = (*state.dictionary.get()).clone();
        state.dictionary.update(|d| *d = dictionary)?;
    }
    Ok(changed)
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
/// Returns true when the dictionary changed. Respects the same `learn_terminology`
/// gate as the preference path so a user who turned terminology learning off
/// sees no dictionary growth either.
pub(crate) fn learn_terminology_to_dictionary(
    dictionary: &mut teletype_core::dictionary::Dictionary,
    profile: &UserProfile,
    ai_output: &str,
    final_text: &str,
    app: &teletype_core::context::ApplicationContext,
) -> bool {
    if !profile.learn_from_edits || !profile.learn_terminology {
        return false;
    }
    if ai_output.trim().is_empty() || final_text.trim().is_empty() || ai_output == final_text {
        return false;
    }
    let signals =
        teletype_core::personalization::learn::extract_signals(ai_output, final_text, app);
    let mut changed = false;
    for s in signals {
        if let (Some(from), Some(to)) = (
            s.key
                .strip_prefix("term:")
                .and_then(|rest| rest.split('→').next()),
            s.canonical_word.as_deref(),
        ) {
            if dictionary.insert_learned(from, to) {
                changed = true;
            }
        }
    }
    changed
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

/// Removes just-learned preferences by id (the undo pill). Returns how many
/// were actually removed; unknown ids are skipped, so a double-click or a
/// pill/ backend race cannot error.
#[tauri::command]
pub async fn undo_learned(state: State<'_, AppState>, ids: Vec<String>) -> CommandResult<usize> {
    let mut profile = (*state.profile.get()).clone();
    let mut removed = 0;
    for id in &ids {
        if profile.remove(id) {
            removed += 1;
        }
    }
    if removed > 0 {
        state.profile.update(|p| *p = profile)?;
    }
    Ok(removed)
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
