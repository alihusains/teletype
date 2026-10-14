//! History commands: dictation history, transcripts, insights and usage stats.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use tauri::{AppHandle, State};
use teletype_core::insights;
use crate::AppState;
use super::*;

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
    Ok(history.entries.iter().map(entry_to_view).collect())
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
    // Clone under the lock, then release it before computing.
    //
    // `insights::compute` is quadratic in the number of stored entries (the
    // `subsumed` n-gram sweep does a `contains` per candidate pair): measured
    // at 0.67 s for 100 entries and 64 s for the 1000-entry cap. The dictation
    // worker takes this same mutex before it injects text
    // (`dictation.rs` history push -> `insert_text`), so holding it across the
    // computation delayed the user's words landing in their document by up to
    // a minute, possibly after they had switched windows.
    let filtered: Vec<teletype_core::history::DictationEntry> = {
        let history = state.history.get();
        let filtered = match window {
            Some((from, to)) => history
                .entries
                .iter()
                .filter(|r| r.created_at >= from && r.created_at <= to)
                .cloned()
                .collect(),
            None => history.entries.clone(),
        };
        filtered
    };
    let filtered_history = teletype_core::history::DictationHistory { entries: filtered };
    let mut insights = insights::compute(&filtered_history, now);
    // Range-scoped usage. The persisted UsageStats are all-time aggregate
    // counters with no timestamps, so range views are recomputed from the
    // range-filtered history (the same counting functions the pipeline uses
    // at record time). "Lifetime" keeps the persisted counters so
    // pre-history counts are not lost.
    let (settings, autotext) = {
        let settings = state.settings();
        let autotext = state.autotext.get();
        (settings, autotext)
    };
    // Usage counters (filler removed, AutoText expansions) are recorded at
    // event time — typed AutoText expansions happen in other apps and never
    // create a dictation history entry, so they cannot be recomputed from the
    // filtered history. The persisted counters are all-time only (no
    // timestamps), so any bounded window is a lower bound; show the true
    // lifetime total rather than undercounting.
    {
        let usage = state.usage.get();
        insights.filler_removed = if settings.remove_filler_words {
            usage.filler_counts.values().sum()
        } else {
            0
        };
        insights.autotext_expansions = usage.autotext_counts.values().sum();
    }
    if window.is_some() {
        drop(autotext);
    }
    // Earliest activity across the sources the UI can clamp a custom range
    // to: dictations, learned dictionary words, learned preferences.
    let mut first_activity: Option<u64> =
        filtered_history.entries.iter().map(|e| e.created_at).min();
    // Self-learning visibility: how many words the dictionary holds and how
    // many of them the app learned from the user's own edits.
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
        if window.is_some() {
            if let Some((from, to)) = window {
                insights.dictionary_learned =
                    learned.iter().filter(|t| **t >= from && **t <= to).count() as u32;
            }
        }
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
        if window.is_some() {
            if let Some((from, to)) = window {
                insights.personalization_corrections = learned_prefs
                    .iter()
                    .filter(|t| **t >= from && **t <= to)
                    .count() as u32;
            }
        }
        if let Some(m) = learned_prefs.iter().copied().min() {
            first_activity = Some(first_activity.map_or(m, |f| f.min(m)));
        }
    }
    if first_activity == Some(u64::MAX) {
        first_activity = None;
    }
    insights.first_activity_at = first_activity;
    // P1-16: surface the polish/transform state so the user knows why
    // nothing was rewritten.
    if state.inference.get().is_none() {
        insights.polish_status = Some(
            "No polish model loaded. Dictations are not being rewritten. \
             Pick a model in Settings > Models."
                .into(),
        );
    }
    Ok(insights)
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
