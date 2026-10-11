//! ShortcutManager: configurable transformation shortcuts.
//!
//! Matches (keycode, modifiers) against a runtime-updatable map. Matching
//! KeyDown/KeyUp events are consumed by the native tap (the target app never
//! receives them); the corresponding Teletype Action is dispatched
//! asynchronously with a synchronously captured selection.

use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager};
use tauri::Emitter;

use super::normalizer::parse_shortcut;

/// A Teletype Action behind a shortcut.
#[derive(Debug, Clone)]
pub enum Action {
    /// Apply a transform to the current selection.
    Transform(String),
    /// Add the selected word to the dictionary.
    QuickAdd,
}

#[derive(Debug, Clone)]
pub struct PlannedBinding {
    pub flags: u64,
    pub keycode: i64,
    pub action: Action,
    pub label: String,
}

static TARGETS: Mutex<Vec<PlannedBinding>> = Mutex::new(Vec::new());
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Build the current shortcut plan from the transform store + Quick Add
/// setting. Unparseable shortcuts are skipped with a log (callers may fall
/// back to Carbon for those).
pub fn plan(state: &crate::AppState) -> Vec<PlannedBinding> {
    let transforms = state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .transforms
        .clone();
    let quick_add = state.settings().quick_add_hotkey.clone();

    let mut out = Vec::new();
    for t in &transforms {
        if !t.enabled || t.shortcut.trim().is_empty() {
            continue;
        }
        match parse_shortcut(&t.shortcut) {
            Some((flags, kc)) => out.push(PlannedBinding {
                flags,
                keycode: kc,
                action: Action::Transform(t.id.clone()),
                label: format!("{} ({})", t.name, t.shortcut),
            }),
            None => crate::log_entry(
                crate::LogLevel::Warn,
                format!(
                    "transform shortcut for '{}' not in tap (unmapped key): {}",
                    t.name, t.shortcut
                ),
            ),
        }
    }
    if !quick_add.trim().is_empty() {
        if let Some((flags, kc)) = parse_shortcut(&quick_add) {
            out.push(PlannedBinding {
                flags,
                keycode: kc,
                action: Action::QuickAdd,
                label: format!("Quick Add ({quick_add})"),
            });
        }
    }
    out
}

/// Remember the plan for later `dispatch` calls and the ObjC mirror.
pub fn remember(app: &AppHandle, bindings: Vec<PlannedBinding>) {
    let _ = APP.set(app.clone());
    *TARGETS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = bindings;
}

/// Dispatch the action behind `slot` (called from the tap on KeyDown).
/// The selection is read synchronously here on the main thread while the
/// keystroke is already swallowed, so it cannot be corrupted; the slow model
/// work runs off-thread.
pub fn dispatch(slot: u32) {
    let binding: Option<PlannedBinding> = TARGETS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(slot as usize)
        .cloned();
    let Some(binding) = binding else {
        return;
    };
    let Some(app) = APP.get().cloned() else {
        return;
    };
    crate::log_entry(
        crate::LogLevel::Info,
        format!("shortcut FIRED: {}", binding.label),
    );
    let selected = super::text_context::selected_text();
    match binding.action {
        Action::Transform(id) => {
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                let st = app_clone.state::<crate::AppState>();
                let a2 = app_clone.clone();
                match crate::commands::transform_selection_with_text(a2, st, id, selected).await {
                    Ok(result) => crate::log_entry(
                        crate::LogLevel::Success,
                        format!("transform_selection OK: {} chars", result.len()),
                    ),
                    Err(e) => crate::log_entry(
                        crate::LogLevel::Error,
                        format!("transform_selection failed: {e}"),
                    ),
                }
            });
        }
        Action::QuickAdd => {
            // The selection was captured synchronously above while the
            // keystroke is swallowed, so it cannot be corrupted. We do NOT
            // write to the dictionary here: the UI confirms the word (and
            // may edit it) before calling `quick_add_word`. A refusal during
            // a take is surfaced by the UI as a visible message.
            let selected = selected.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
            let _ = app.emit(
                "quick-add-requested",
                &serde_json::json!({ "selection": selected }),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_debug_is_stable() {
        let a = Action::Transform("x".into());
        assert!(format!("{a:?}").contains('x'));
    }
}
