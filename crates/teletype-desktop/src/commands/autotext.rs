//! AutoText commands: user entries and the read-only system table.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use super::*;
use crate::AppState;
use tauri::State;
use teletype_core::autotext::{self, AutoTextEntry};

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

/// Exports every user snippet to `path` as a versioned JSON envelope
/// (`{"version":1,"entries":[...]}`) and returns how many were written, so the
/// UI reports the file's real content rather than stale screen state.
#[tauri::command]
pub async fn export_autotext(state: State<'_, AppState>, path: String) -> CommandResult<usize> {
    let store = state.autotext.get();
    teletype_core::autotext::export_to_file(&store, &path).map_err(CommandError::internal)?;
    Ok(store.entries.len())
}

/// Imports snippets from `path`. Existing triggers are updated in place, new
/// ones are added, and invalid rows are skipped (see `merge_entries`). The
/// store is left untouched if the file cannot be parsed.
#[tauri::command]
pub async fn import_autotext(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<teletype_core::autotext::AutoTextImportCounts> {
    let mut store = (*state.autotext.get()).clone();
    let counts = teletype_core::autotext::import_from_file(&mut store, &path)?;
    state.autotext.update(|s| *s = store)?;
    Ok(counts)
}
