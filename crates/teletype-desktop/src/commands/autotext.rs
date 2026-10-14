//! AutoText commands: user entries and the read-only system table.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use tauri::State;
use teletype_core::autotext::{self, AutoTextEntry};
use crate::AppState;
use super::*;

// ---- AutoText ----

#[tauri::command]
pub async fn list_autotext(state: State<'_, AppState>) -> CommandResult<Vec<AutoTextEntry>> {
    Ok(state.autotext.get().entries.clone())
}

/// The built-in System AutoText entries (read-only; not user-editable).
#[tauri::command]
pub async fn list_system_autotext() -> CommandResult<Vec<AutoTextEntry>> {
    Ok(teletype_core::autotext::system::entries().to_vec())
}

#[tauri::command]
pub async fn create_autotext(
    state: State<'_, AppState>,
    entry: AutoTextEntry,
) -> CommandResult<AutoTextEntry> {
    autotext::validate_trigger(&entry.trigger).map_err(|e| e.to_string())?;
    autotext::validate_snippet(&entry.snippet)?;
    let mut store = (*state.autotext.get()).clone();
    store.insert(entry.clone())?;
    state.autotext.update(|s| *s = store)?;
    Ok(entry)
}

#[tauri::command]
pub async fn update_autotext(
    state: State<'_, AppState>,
    entry: AutoTextEntry,
) -> CommandResult<AutoTextEntry> {
    autotext::validate_trigger(&entry.trigger).map_err(|e| e.to_string())?;
    autotext::validate_snippet(&entry.snippet)?;
    let mut store = (*state.autotext.get()).clone();
    store.update(entry.clone())?;
    state.autotext.update(|s| *s = store)?;
    Ok(entry)
}

#[tauri::command]
pub async fn delete_autotext(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut store = (*state.autotext.get()).clone();
    if !store.remove(&id) {
        return Err(CommandError::not_found("Entry not found"));
    }
    state.autotext.update(|s| *s = store)?;
    Ok(())
}
