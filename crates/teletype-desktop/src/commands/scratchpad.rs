//! Notetaker commands. The store type, persistence and file name are
//! unchanged from the scratchpad era (`Repo<Notes>` at `scratchpad.json`,
//! loaded with legacy migration); only the commands and the wire shape
//! changed.

use super::*;
use crate::AppState;
use tauri::State;
use teletype_core::scratchpad::{Note, NoteSource};

/// A note as the UI lists it: the full fields the editor needs plus a short
/// body preview for the list pane's "last line" text.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteListItem {
    pub id: String,
    pub title: String,
    pub body: String,
    pub preview: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub source: NoteSource,
}

fn to_item(n: &Note) -> NoteListItem {
    let preview = n
        .body
        .chars()
        .take(160)
        .collect::<String>()
        .trim_end()
        .to_string();
    NoteListItem {
        id: n.id.clone(),
        title: n.title.clone(),
        body: n.body.clone(),
        preview,
        created_at: n.created_at,
        updated_at: n.updated_at,
        source: n.source,
    }
}

/// All notes, newest-edited first.
#[tauri::command]
pub async fn list_notes(state: State<'_, AppState>) -> CommandResult<Vec<NoteListItem>> {
    let notes = state.scratchpad.get();
    let mut items: Vec<NoteListItem> = notes.notes.iter().map(to_item).collect();
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(items)
}

/// The note the editor pane should show, or `None`.
#[tauri::command]
pub async fn get_open_note(state: State<'_, AppState>) -> CommandResult<Option<Note>> {
    let notes = state.scratchpad.get();
    Ok(notes
        .open_note_id
        .as_deref()
        .and_then(|id| notes.note(id))
        .cloned())
}

/// Creates a blank note, opens it, and returns it.
#[tauri::command]
pub async fn create_note(state: State<'_, AppState>) -> CommandResult<Note> {
    let ts = crate::edit_watch::now_ms() as u64;
    let arc = state.scratchpad.update(|n| {
        n.create_note(ts);
    })?;
    let n = &*arc;
    Ok(n.note(
        n.open_note_id
            .as_deref()
            .expect("create_note sets the open note"),
    )
    .cloned()
    .expect("create_note sets the open note"))
}

/// Opens a note (the dictation target follows it).
#[tauri::command]
pub async fn open_note(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.scratchpad.update(|n| n.set_open_note(Some(id)))?;
    Ok(())
}

/// Saves a title/body edit on an existing note, or renames it when the body
/// is empty. Returns the saved note.
#[tauri::command]
pub async fn save_note(
    state: State<'_, AppState>,
    id: String,
    title: String,
    body: String,
) -> CommandResult<Note> {
    let ts = crate::edit_watch::now_ms() as u64;
    state.scratchpad.update(|n| {
        if body.trim().is_empty() {
            n.rename_note(&id, title, ts);
        } else {
            n.save_note(&id, title, body, ts);
        }
    })?;
    let n = state.scratchpad.get();
    Ok(n.note(&id).cloned().ok_or_else(|| {
        "note not found, or the edit would erase it (blank title and body)".to_string()
    })?)
}

#[tauri::command]
pub async fn delete_note(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    state.scratchpad.update(|n| {
        n.delete_note(&id);
    })?;
    Ok(())
}

/// Appends dictated text to the open note (or creates one when none is
/// open). Called by the dictation controller while the Teletype window is
/// frontmost and "route dictation here" is on.
pub fn append_note(state: &AppState, text: &str) -> Option<Note> {
    let ts = crate::edit_watch::now_ms() as u64;
    if state
        .scratchpad
        .update(|n| {
            n.append_to_open_note(text.to_string(), ts);
        })
        .is_err()
    {
        return None;
    }
    let notes = state.scratchpad.get();
    notes
        .open_note_id
        .as_deref()
        .and_then(|id| notes.note(id))
        .cloned()
}
