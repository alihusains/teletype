//! History commands: dictation history, transcripts, insights and usage stats.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use super::*;
use crate::AppState;
use tauri::{AppHandle, State};
use teletype_core::insights;

// ---- Dictation history ----

/// A history entry as returned to the UI (camelCase).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub created_at: u64,
    pub text: String,
    pub app_name: String,
    pub app_type: String,
}

fn entry_to_view(e: &teletype_core::history::DictationEntry) -> HistoryEntry {
    let ctx = e.context.clone().unwrap_or_default();
    HistoryEntry {
        id: e.id.clone(),
        created_at: e.created_at,
        text: e.text.clone(),
        app_name: ctx.application_name.clone(),
        app_type: format!("{:?}", ctx.application_type),
    }
}

#[tauri::command]
pub async fn list_dictation_history(
    state: State<'_, AppState>,
) -> CommandResult<Vec<HistoryEntry>> {
    let history = state.history.get();
    // Older builds recorded no-speech takes with empty text. They are not
    // dictations and render as blank cards, so hide them on read.
    Ok(history
        .entries
        .iter()
        .filter(|e| !e.text.trim().is_empty())
        .map(entry_to_view)
        .collect())
}

#[tauri::command]
pub async fn delete_dictation_entry(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    // In-memory removal for immediate reads; the disk write goes through the
    // persistence actor (A4).
    let mut next = (*state.history.get()).clone();
    next.remove(&id);
    state.history.set_snapshot(next);
    state.persist.history_remove(&id);
    Ok(())
}

/// The folder day-wise transcript files are written to (resolved).
#[tauri::command]
pub async fn get_transcripts_dir(state: State<'_, AppState>) -> CommandResult<String> {
    let dir = state.transcripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    Ok(dir.to_string_lossy().to_string())
}

/// Reveals the transcripts folder in the system file manager (Finder / Explorer).
#[tauri::command]
pub async fn reveal_transcripts_dir(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let dir = state.transcripts_dir();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    let _ = app;
    Ok(())
}

// ---- Insights ----

#[tauri::command]
pub async fn get_insights(
    state: State<'_, AppState>,
    range: Option<String>,
    custom_from: Option<u64>,
    custom_to: Option<u64>,
) -> CommandResult<insights::Insights> {
    let now = teletype_core::storage::now_ms();
    // Resolve the window. Named ranges are relative to now; "custom" uses
    // explicit epoch-ms bounds (the UI clamps them to first activity).
    let window: Option<(u64, u64)> = match range.as_deref() {
        Some("today") => {
            let start = teletype_core::stats::day_start_ms(now);
            Some((start, now))
        }
        Some("week") => Some((now.saturating_sub(7 * 86_400_000), now)),
        Some("month") => Some((now.saturating_sub(30 * 86_400_000), now)),
        Some("year") => Some((now.saturating_sub(365 * 86_400_000), now)),
        Some("custom") => {
            let from = custom_from.unwrap_or(0);
            let to = custom_to.unwrap_or(now);
            Some((from.min(to), to.max(from)))
        }
        _ => None, // "lifetime"
    };

    // Load full history from the writer (no 1000-row cap) and merge any rows
    // the writer has not flushed yet. The merged history is the input for
    // every range: `compute_for_window` applies the window itself, while the
    // habit views, records and milestones must see all of it.
    let full_db = teletype_core::persist::read_history(&state.config_dir, None)?;
    let mem = state.history.get().entries.clone();
    // Legacy blank rows from no-speech takes are not dictations; keep them
    // out of every aggregate without deleting the stored rows.
    let full: Vec<teletype_core::history::DictationEntry> = merge_history(mem, full_db)
        .into_iter()
        .filter(|e| !e.text.trim().is_empty())
        .collect();
    let full_hist = teletype_core::history::DictationHistory { entries: full };

    let mut insights = insights::compute_for_window(&full_hist, window, now);
    let in_window = |at: u64| window.is_none_or(|(from, to)| at >= from && at <= to);

    // Filler/AutoText counters are the persisted all-time totals (they carry
    // no timestamps and cannot be sliced to a range).
    {
        let usage = state.usage.get();
        let settings = state.settings();
        insights.filler_removed = if settings.remove_filler_words {
            usage.filler_counts.values().sum()
        } else {
            0
        };
        insights.top_fillers = if settings.remove_filler_words {
            insights::ranked_from_counts(&usage.filler_counts)
        } else {
            Vec::new()
        };
        insights.autotext_expansions = usage.autotext_counts.values().sum();
        insights.top_autotext = insights::ranked_from_counts(&usage.autotext_counts);
    }

    // Earliest activity across the sources the UI can clamp a custom range
    // to: dictations, learned dictionary words, learned preferences.
    // `first_activity_at` is all-time so it never moves when the user
    // switches range filters.
    let mut first_activity: Option<u64> = full_hist.entries.iter().map(|e| e.created_at).min();
    {
        let dict = state.dictionary.get();
        insights.dictionary_words = dict.words.len() as u32;
        let learned: Vec<u64> = dict
            .words
            .iter()
            .filter(|w| w.learned_from.is_some())
            .filter_map(|w| w.learned_at)
            .collect();
        insights.learned_words = learned.len() as u32;
        insights.dictionary_learned = learned.iter().filter(|t| in_window(**t)).count() as u32;
        if let Some(m) = learned.iter().copied().min() {
            first_activity = Some(first_activity.map_or(m, |f| f.min(m)));
        }
    }
    {
        let profile = state.profile.get();
        let learned_prefs: Vec<u64> = profile
            .preferences
            .iter()
            .filter(|p| !p.explicit)
            .map(|p| p.created_at)
            .collect();
        insights.personalization_corrections =
            learned_prefs.iter().filter(|t| in_window(**t)).count() as u32;
        if let Some(m) = learned_prefs.iter().copied().min() {
            first_activity = Some(first_activity.map_or(m, |f| f.min(m)));
        }
    }
    if first_activity == Some(u64::MAX) {
        first_activity = None;
    }
    insights.first_activity_at = first_activity;

    // The learning section (learned terms, edit rate, frequent corrections).
    // Opening the store creates it, so a fresh install is left untouched
    // until the first dictation writes one. Capped at 300 recent rows: the
    // top-fix pass runs a phonetic similarity over every changed span, and
    // this command re-runs whenever the Insights window regains focus.
    let learning_path = state.data_dir.join("learning").join("learning.sqlite");
    if learning_path.exists() {
        match teletype_core::learning::store::LearningStore::open(&learning_path) {
            Ok(store) => {
                let dictations = store.recent_dictations(300).unwrap_or_default();
                let lexicon = store.lexicon_all().unwrap_or_default();
                let accuracy = store.personal_accuracy_history(10).unwrap_or_default();
                insights.learning =
                    insights::compute_learning(&dictations, &accuracy, &lexicon, now);
            }
            Err(e) => crate::log_entry(
                crate::LogLevel::Warn,
                format!("insights: learning store unavailable: {e}"),
            ),
        }
    }

    // P1-16: surface the polish/transform state so the user knows why
    // nothing was rewritten.
    if state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .is_none()
    {
        insights.polish_status = Some(
            "No polish model loaded. Dictations are not being rewritten. \
             Pick a model in Settings > Models."
                .into(),
        );
    }
    Ok(insights)
}

/// Merges in-memory history rows with the full database, deduplicating by id.
/// In-memory rows win (they may not be flushed to the writer yet). Result is
/// sorted newest first.
fn merge_history(
    mem: Vec<teletype_core::history::DictationEntry>,
    db: Vec<teletype_core::history::DictationEntry>,
) -> Vec<teletype_core::history::DictationEntry> {
    let mut map: std::collections::HashMap<String, teletype_core::history::DictationEntry> =
        db.into_iter().map(|e| (e.id.clone(), e)).collect();
    for e in mem {
        map.insert(e.id.clone(), e);
    }
    let mut out: Vec<_> = map.into_values().collect();
    out.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    out
}

// ---- Usage stats (filler words removed, AutoText used) ----

#[tauri::command]
pub async fn get_usage_stats(
    state: State<'_, AppState>,
) -> CommandResult<teletype_core::usage::UsageStats> {
    let usage = state.usage.get();
    let autotext = state.autotext.get();
    let settings = state.settings();
    let mut usage = (*usage).clone();
    // Only show counters for things that still exist / are active.
    usage.prune_autotext(&autotext);
    if settings.remove_filler_words {
        usage.prune_fillers(&settings.filler_words);
    } else {
        usage.filler_counts.clear();
    }
    Ok(usage)
}

#[tauri::command]
pub async fn reset_usage_stats(state: State<'_, AppState>) -> CommandResult<()> {
    // In-memory reset for immediate reads; the disk write goes through the
    // persistence actor (A4).
    state
        .usage
        .set_snapshot(teletype_core::usage::UsageStats::default());
    state.persist.usage_reset();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use teletype_core::history::DictationEntry;

    fn test_entry(id: &str, at: u64) -> DictationEntry {
        DictationEntry {
            id: id.into(),
            created_at: at,
            text: format!("text {id}"),
            context: None,
            duration_ms: None,
        }
    }

    #[test]
    fn merge_history_dedupes_by_id_and_sorts_newest_first() {
        let db = vec![
            test_entry("a", 100),
            test_entry("b", 200),
            test_entry("c", 300),
        ];
        let mem = vec![test_entry("b", 250), test_entry("d", 400)];
        let merged = merge_history(mem, db);
        assert_eq!(merged.len(), 4);
        assert_eq!(merged[0].id, "d");
        assert_eq!(merged[1].id, "c");
        assert_eq!(merged[2].id, "b");
        assert_eq!(merged[2].created_at, 250); // mem wins
        assert_eq!(merged[3].id, "a");
    }

    #[test]
    fn merge_history_empty_mem_returns_db_sorted() {
        let db = vec![test_entry("a", 100), test_entry("b", 200)];
        let merged = merge_history(vec![], db);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].id, "b");
        assert_eq!(merged[1].id, "a");
    }
}
