//! AutoTextManager: typed-trigger expansion from observed text.
//!
//! The native tap NEVER consumes ordinary input; it only forwards resulting
//! Unicode characters. This manager keeps a lightweight recent-text buffer,
//! matches configured triggers (e.g. `/home-address`) against resulting text
//! rather than physical keycodes, and replaces the trigger with its
//! configured replacement through [`TextContext`]/[`TextInserter`]. The
//! trigger may appear briefly before replacement (observe-then-replace).
//!
//! Independent from `ShortcutManager`: the only shared pieces are the tap and
//! the text abstraction.

use std::sync::Mutex;

use tauri::Manager;

use super::normalizer::ObservedKind;
use super::text_context;

/// Replacement timing, configurable via settings (`autotext_timing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    /// Expand the moment the buffer ends with a trigger.
    Immediate,
    /// Expand when a delimiter (space/tab/enter) completes a trigger.
    Delimiter,
}

impl Timing {
    pub fn from_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("immediate") {
            Self::Immediate
        } else {
            Self::Delimiter
        }
    }
}

/// Recent typed text (resulting characters). Triggers are at most 64 chars;
/// the cap keeps a tail so a trigger is never split off.
const BUFFER_CAP: usize = 128;

static BUFFER: Mutex<String> = Mutex::new(String::new());

fn buffer() -> std::sync::MutexGuard<'static, String> {
    BUFFER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Feed one observed event from the tap. Fast: buffer bookkeeping inline, slow
/// replacement work spawned off-thread. `text` is the resulting Unicode for
/// `Text`/`Delimiter` kinds.
pub fn observe(app: &tauri::AppHandle, kind: ObservedKind, text: &str) {
    let state = app.state::<crate::AppState>();
    if !state.settings().typing_autotext_enabled {
        return;
    }
    match kind {
        ObservedKind::Backspace => {
            buffer().pop();
            return;
        }
        ObservedKind::Break => {
            buffer().clear();
            return;
        }
        ObservedKind::Delimiter => {
            let candidate = std::mem::take(&mut *buffer());
            if candidate.trim().is_empty() {
                return;
            }
            let timing = Timing::from_str(&state.settings().autotext_timing);
            // Delimiter mode expands here; immediate mode already expanded at
            // trigger completion, so the delimiter only clears (buffer empty).
            if timing == Timing::Delimiter {
                maybe_expand(app, &candidate, true);
            }
        }
        ObservedKind::Text => {
            {
                let mut buf = buffer();
                buf.push_str(text);
                if buf.len() > BUFFER_CAP {
                    let start = buf.len() - BUFFER_CAP;
                    let tail = buf[start..].to_string();
                    *buf = tail;
                }
            }
            let timing = Timing::from_str(&state.settings().autotext_timing);
            if timing == Timing::Immediate {
                let snapshot = buffer().clone();
                maybe_expand(app, &snapshot, false);
                // On success maybe_expand clears the buffer; on no-match the
                // buffer stays for the next keystroke.
            }
        }
    }
}

/// If `candidate` ends with a configured trigger (longest wins, whole-token),
/// replace it in the focused field.
fn maybe_expand(app: &tauri::AppHandle, candidate: &str, had_delimiter: bool) {
    let state = app.state::<crate::AppState>();
    let Some(matched) = find_trigger(&state, candidate) else {
        return;
    };
    // Buffer consumed by this expansion attempt either way: no retry loops.
    buffer().clear();
    let app_ctx = state.platform.active_application().unwrap_or_default();
    if text_context::is_self_app(&app_ctx.application_id, &app_ctx.application_name) {
        return;
    }
    let replacement =
        teletype_core::autotext::placeholders::expand_placeholders(&matched.replacement);
    // Recorded on success below: the Insights/AutoText usage counters are
    // pipeline-only otherwise, so typed expansions would never count.
    let trigger_key = matched.trigger.clone();
    let trigger_units = text_context::utf16_len(&matched.trigger);
    let trigger_chars = matched.trigger.chars().count();
    let app_clone = app.clone();
    let usage_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = tauri::async_runtime::spawn_blocking(move || {
            replace_trigger(
                &app_clone,
                trigger_units,
                trigger_chars,
                had_delimiter,
                &replacement,
            )
        })
        .await;
        match outcome {
            Ok(true) => {
                crate::log_entry(
                    crate::LogLevel::Success,
                    format!("autotext expanded '{}'", matched.trigger),
                );
                // Count the typed expansion so Insights/AutoText usage stats
                // match what the user actually sees. Spoken expansions are
                // counted by the dictation pipeline; this is the typed path.
                let state = usage_app.state::<crate::AppState>();
                let mut usage = state
                    .usage
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *usage.autotext_counts.entry(trigger_key).or_insert(0) += 1;
                if let Err(e) = state.usage_store.save(&usage) {
                    tracing::warn!("Failed to save usage stats: {e}");
                }
            }
            _ => tracing::warn!("autotext: replacement failed"),
        }
    });
}

/// Jennings-Smith: longest enabled trigger that ends `candidate` on a token
/// boundary and applies to the frontmost app.
struct MatchedTrigger {
    trigger: String,
    replacement: String,
}

fn find_trigger(state: &crate::AppState, candidate: &str) -> Option<MatchedTrigger> {
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let store = state
        .autotext
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let lower = candidate.to_ascii_lowercase();
    let mut best: Option<MatchedTrigger> = None;
    for entry in store.entries.iter().filter(|e| e.enabled) {
        if !entry.applies_to(&app_ctx) {
            continue;
        }
        let trigger = entry.trigger.clone();
        if trigger.is_empty() {
            continue;
        }
        let t_lower = trigger.to_ascii_lowercase();
        if !lower.ends_with(&t_lower) {
            continue;
        }
        // Whole-token rule (same as voice `expand`): start or whitespace
        // before the trigger.
        let before = candidate.len() - trigger.len();
        let boundary_ok = before == 0
            || candidate[..before]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_whitespace());
        if !boundary_ok {
            continue;
        }
        let longer = best
            .as_ref()
            .is_none_or(|b: &MatchedTrigger| trigger.len() > b.trigger.len());
        if longer {
            best = Some(MatchedTrigger {
                trigger,
                replacement: entry.replacement.clone(),
            });
        }
    }
    best
}

/// Replace the trigger (plus delimiter when one completed it) in the focused
/// field: AX direct write first, clipboard+paste fallback with
/// preserve/restore. Returns true when the replacement landed or was queued.
fn replace_trigger(
    app: &tauri::AppHandle,
    trigger_units: usize,
    trigger_chars: usize,
    had_delimiter: bool,
    replacement: &str,
) -> bool {
    // Delimiter mode: the delimiter (1 UTF-16 unit, 1 char) sits between the
    // trigger and the caret, so the replace range covers both.
    let units = trigger_units + usize::from(had_delimiter);
    if text_context::replace_trigger_ax(units, replacement) {
        return true;
    }
    let state = app.state::<crate::AppState>();
    let backspaces = trigger_chars + usize::from(had_delimiter);
    text_context::backspace_then_paste(backspaces, replacement, &state.injector)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_parses_both_modes() {
        assert_eq!(Timing::from_str("immediate"), Timing::Immediate);
        assert_eq!(Timing::from_str("Immediate"), Timing::Immediate);
        assert_eq!(Timing::from_str("delimiter"), Timing::Delimiter);
        assert_eq!(Timing::from_str(""), Timing::Delimiter);
        assert_eq!(Timing::from_str("other"), Timing::Delimiter);
    }

    #[test]
    fn buffer_cap_keeps_a_trigger_sized_tail() {
        assert!(BUFFER_CAP >= 64, "must hold the longest trigger");
    }
}
