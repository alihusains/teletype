//! T1: record what the user did with a polished take.
//!
//! Feedback is local only and append-only. This command validates the request
//! and writes one row; it never changes Polish behavior. T5 consumes the store.

use teletype_core::learning::feedback::{FeedbackKind, FeedbackStore, PolishFeedback};

use crate::{commands::CommandError, AppState};

/// The feedback file lives beside the learning store in the app data dir.
pub(crate) fn feedback_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("polish_feedback.jsonl")
}

/// Distinct terms the user marked "Never change this", in file order.
///
/// A missing or unreadable store means no protected terms; feedback is an
/// optimization and must never break a dictation.
pub(crate) fn never_change_terms(data_dir: &std::path::Path) -> Vec<String> {
    let store = FeedbackStore::new(feedback_path(data_dir));
    let Ok(entries) = store.load() else {
        return Vec::new();
    };
    let mut terms: Vec<String> = Vec::new();
    for entry in entries {
        if entry.feedback.kind != FeedbackKind::NeverChange {
            continue;
        }
        let Some(term) = entry.feedback.term else {
            continue;
        };
        if !term.trim().is_empty() && !terms.contains(&term) {
            terms.push(term);
        }
    }
    terms
}

pub(crate) fn validate_feedback(feedback: &PolishFeedback) -> crate::commands::CommandResult<()> {
    if feedback.take_id == 0 {
        return Err(CommandError::user("Unknown take for polish feedback"));
    }
    if feedback.kind == FeedbackKind::NeverChange {
        match feedback.term.as_deref().map(str::trim) {
            Some(term) if !term.is_empty() => {
                if term.chars().count() > 120 {
                    return Err(CommandError::user("Protected term is too long"));
                }
            }
            _ => return Err(CommandError::user("Choose a word or phrase to protect")),
        }
    }
    if let Some(span) = &feedback.span {
        if span.original.trim().is_empty() && span.replacement.trim().is_empty() {
            return Err(CommandError::user("Empty polish span"));
        }
    }
    Ok(())
}

#[tauri::command]
pub fn record_polish_feedback(
    state: tauri::State<'_, AppState>,
    feedback: PolishFeedback,
) -> crate::commands::CommandResult<()> {
    validate_feedback(&feedback)?;
    let store = FeedbackStore::new(feedback_path(&state.data_dir));
    let wrote = store
        .record(&feedback, teletype_core::storage::now_ms())
        .map_err(|e| CommandError::internal(format!("Could not save polish feedback: {e}")))?;
    // A deduped click is a no-op: never feed the bandit twice for one row.
    if wrote {
        let kind = feedback.kind;
        let app_id = feedback.app_id.clone().unwrap_or_default();
        let hint = feedback.profile;
        let outcome = teletype_core::learning::profile_bandit::feedback_outcome(kind);
        state
            .update_bandit(|bandit| {
                let Some(outcome) = outcome else {
                    return;
                };
                let profile = hint
                    .or_else(|| bandit.active_change(&app_id).map(|c| c.to))
                    .unwrap_or_else(|| {
                        let settings = state.settings();
                        let app_ctx = state.platform.active_application().unwrap_or_default();
                        teletype_core::profile::resolve_profile(
                            &app_ctx,
                            &settings.polish_app_profiles,
                            settings.polish_profile,
                            0,
                        )
                    });
                bandit.record_outcome(&app_id, profile, Some(outcome));
            })
            .map_err(|e| {
                CommandError::internal(format!("Could not update profile learning: {e}"))
            })?;
    }
    Ok(())
}

/// Reverts an automatic profile change. The bandit is the runtime source of
/// truth (`choose` ignores an undone change), so settings are deliberately left
/// untouched: an automatic change never writes a per-app override in the first
/// place, and touching settings here could clobber a user's explicit choice.
#[tauri::command]
pub fn undo_profile_learning(
    state: tauri::State<'_, AppState>,
    app_key: String,
) -> crate::commands::CommandResult<()> {
    state
        .update_bandit(|bandit| {
            bandit.undo_change(&app_key);
        })
        .map_err(|e| CommandError::internal(format!("Could not undo profile learning: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use teletype_core::learning::feedback::SpanFeedback;
    use teletype_core::profile::PolishProfile;

    fn feedback(kind: FeedbackKind) -> PolishFeedback {
        PolishFeedback {
            take_id: 3,
            kind,
            term: None,
            span: None,
            app_id: Some("com.apple.TextEdit".into()),
            model: Some("test-model".into()),
            profile: Some(PolishProfile::Natural),
        }
    }

    fn store(name: &str) -> (tempfile::TempDir, FeedbackStore) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{name}.jsonl"));
        (dir, FeedbackStore::new(path))
    }

    #[test]
    fn valid_feedback_writes_one_row() {
        let (_dir, store) = store("valid");
        let mut row = feedback(FeedbackKind::ThumbUp);
        row.span = Some(SpanFeedback {
            original: "Shivon".into(),
            replacement: "Siobhan".into(),
        });
        assert!(store.record(&row, 1).unwrap());
        assert_eq!(store.load().unwrap().len(), 1);
    }

    #[test]
    fn never_change_requires_a_term() {
        let mut row = feedback(FeedbackKind::NeverChange);
        row.term = Some("   ".into());
        let err = validate_feedback(&row).unwrap_err();
        assert_eq!(err.code, "E_REQUEST");
        assert_eq!(err.message, "Choose a word or phrase to protect");
    }

    #[test]
    fn unknown_take_is_rejected_before_writing() {
        let row = PolishFeedback {
            take_id: 0,
            ..feedback(FeedbackKind::ThumbUp)
        };
        let err = validate_feedback(&row).unwrap_err();
        assert_eq!(err.code, "E_REQUEST");
        assert_eq!(err.message, "Unknown take for polish feedback");
    }

    #[test]
    fn empty_span_is_rejected() {
        let mut row = feedback(FeedbackKind::ThumbUp);
        row.span = Some(SpanFeedback {
            original: " ".into(),
            replacement: "".into(),
        });
        let err = validate_feedback(&row).unwrap_err();
        assert_eq!(err.code, "E_REQUEST");
        assert_eq!(err.message, "Empty polish span");
    }

    #[test]
    fn valid_feedback_validates() {
        let mut row = feedback(FeedbackKind::NeverChange);
        row.term = Some("Siobhan".into());
        assert!(validate_feedback(&row).is_ok());
    }

    #[test]
    fn never_change_terms_are_distinct_and_keep_their_spelling() {
        let dir = tempfile::tempdir().unwrap();
        let store = FeedbackStore::new(feedback_path(dir.path()));
        for (take_id, term) in [(1, "Siobhan"), (2, "Siobhan"), (3, "PyTorch")] {
            let mut row = feedback(FeedbackKind::NeverChange);
            row.take_id = take_id;
            row.term = Some(term.into());
            assert!(store.record(&row, take_id).unwrap());
        }
        let mut row = feedback(FeedbackKind::ThumbUp);
        row.take_id = 4;
        row.term = Some("NotProtected".into());
        assert!(store.record(&row, 4).unwrap());

        assert_eq!(never_change_terms(dir.path()), vec!["Siobhan", "PyTorch"]);
    }

    #[test]
    fn never_change_terms_with_no_store_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(never_change_terms(dir.path()).is_empty());
    }
}
