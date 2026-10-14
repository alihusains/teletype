//! Scratchpad commands.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use tauri::State;
use teletype_core::scratchpad::ScratchEntry;
use crate::AppState;
use super::*;

// ---- Scratchpad ----

#[tauri::command]
pub async fn list_scratchpad(state: State<'_, AppState>) -> CommandResult<Vec<ScratchEntry>> {
    Ok(state.scratchpad.get().entries.clone())
}

#[tauri::command]
pub async fn append_scratchpad(state: State<'_, AppState>, text: String) -> CommandResult<()> {
    state.scratchpad.update(|pad| { pad.append(text); })?;
    Ok(())
}

#[tauri::command]
pub async fn delete_scratchpad_entry(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.scratchpad.update(|pad| pad.remove(&id))?;
    Ok(())
}

#[tauri::command]
pub async fn clear_scratchpad(state: State<'_, AppState>) -> CommandResult<()> {
    state.scratchpad.update(|pad| pad.clear())?;
    Ok(())
}

#[tauri::command]
pub async fn get_scratchpad_text(state: State<'_, AppState>) -> CommandResult<String> {
    let pad = state.scratchpad.get();
    Ok(pad.combined())
}
