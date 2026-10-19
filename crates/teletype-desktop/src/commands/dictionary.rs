//! Dictionary commands: words, packs, user packs and quick-add.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.

use super::*;
use crate::AppState;
use tauri::State;
use teletype_core::dictionary::DictionaryWord;

// ---- Dictionary ----

#[tauri::command]
pub async fn list_dictionary(state: State<'_, AppState>) -> CommandResult<Vec<DictionaryWord>> {
    Ok(state.dictionary.get().words.clone())
}

#[tauri::command]
pub async fn add_dictionary_word(
    state: State<'_, AppState>,
    word: DictionaryWord,
) -> CommandResult<DictionaryWord> {
    if word.word.trim().is_empty() {
        return Err(CommandError::user("Word can't be empty"));
    }
    let mut dict = (*state.dictionary.get()).clone();
    dict.insert(word.clone())?;
    state.dictionary.update(|d| *d = dict)?;
    Ok(word)
}

#[tauri::command]
pub async fn remove_dictionary_word(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let mut dict = (*state.dictionary.get()).clone();
    if !dict.remove(&id) {
        return Err(CommandError::not_found("Word not found"));
    }
    state.dictionary.update(|d| *d = dict)?;
    Ok(())
}

/// Replaces a dictionary word in place (edit flow: word, pronunciation,
/// aliases, fuzzy). The client sends the full row; the id is preserved.
#[tauri::command]
pub async fn update_dictionary_word(
    state: State<'_, AppState>,
    word: DictionaryWord,
) -> CommandResult<()> {
    if word.word.trim().is_empty() {
        return Err(CommandError::user("Word can't be empty"));
    }
    let mut dict = (*state.dictionary.get()).clone();
    if dict
        .find(&word.word)
        .is_some_and(|existing| existing.id != word.id)
    {
        return Err(CommandError::user("Word already exists"));
    }
    if dict.update(word).is_none() {
        return Err(CommandError::not_found("Word not found"));
    }
    state.dictionary.update(|d| *d = dict)?;
    Ok(())
}

/// Records that a dictation contained the given words, bumping the usage
/// counter of every dictionary word (or alias) that appears. Called from the
/// dictation pipeline after each take.
#[tauri::command]
pub async fn record_dictionary_usage(
    state: State<'_, AppState>,
    text: String,
) -> CommandResult<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let mut dict = (*state.dictionary.get()).clone();
    if dict.record_usage(&text) > 0 {
        state.dictionary.update(|d| *d = dict)?;
    }
    Ok(())
}

/// Exports the custom-words dictionary to `path` as a versioned JSON
/// envelope (`{"version":1,"words":[...]}`).
#[tauri::command]
pub async fn export_custom_words(state: State<'_, AppState>, path: String) -> CommandResult<()> {
    let dict = state.dictionary.get();
    teletype_core::dictionary::export_to_file(&dict, &path).map_err(CommandError::internal)
}

#[tauri::command]
pub async fn import_custom_words(
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<teletype_core::dictionary::ImportCounts> {
    let mut dict = (*state.dictionary.get()).clone();
    let counts = teletype_core::dictionary::import_from_file(&mut dict, &path)?;
    state.dictionary.update(|d| *d = dict)?;
    Ok(counts)
}

// ---- Vocabulary packs ----

/// One vocabulary pack as seen by the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub term_count: usize,
    pub enabled: bool,
}

/// Lists the built-in vocabulary packs with their enabled state.
#[tauri::command]
pub async fn list_packs(state: State<'_, AppState>) -> CommandResult<Vec<PackInfo>> {
    let settings = state.settings();
    Ok(teletype_core::vocab::all()
        .iter()
        .map(|p| PackInfo {
            id: p.id.to_string(),
            name: p.name.to_string(),
            description: p.description.to_string(),
            term_count: p.term_count(),
            enabled: settings.enabled_packs.iter().any(|id| *id == p.id),
        })
        .collect())
}

/// Enables or disables one vocabulary pack, persisting the change.
#[tauri::command]
pub async fn set_pack_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CommandResult<()> {
    if teletype_core::vocab::find(&id).is_none() {
        return Err(CommandError::not_found("Unknown vocabulary pack"));
    }
    let mut settings = state.settings();
    let present = settings.enabled_packs.contains(&id);
    if enabled && !present {
        settings.enabled_packs.push(id);
    } else if !enabled && present {
        settings.enabled_packs.retain(|x| x != &id);
    }
    state.replace_settings(settings)?;
    Ok(())
}

/// One term inside a vocabulary pack, for the pack detail view.
#[derive(serde::Serialize)]
pub struct PackTermInfo {
    pub canonical: String,
    pub mishearings: Vec<String>,
}

/// The words in a single vocabulary pack (canonical spelling + the known
/// mis-hearings the corrector maps back to it). Lets the UI drill into a pack
/// and show its full word list, matching the reference app's pack detail.
#[tauri::command]
pub async fn list_pack_terms(id: String) -> CommandResult<Vec<PackTermInfo>> {
    let pack =
        teletype_core::vocab::find(&id).ok_or_else(|| format!("Unknown vocabulary pack: {id}"))?;
    Ok(pack
        .terms
        .iter()
        .map(|(canonical, mishearings)| PackTermInfo {
            canonical: (*canonical).to_string(),
            mishearings: mishearings.iter().map(|m| (*m).to_string()).collect(),
        })
        .collect())
}

// ---- User-created vocabulary packs (item 15) ----

use teletype_core::userpacks::UserPackStore;

/// Lists the user's own packs (company, tools, …) with their enabled state
/// and word counts, appended after the built-in packs in the UI.
#[tauri::command]
pub async fn list_user_packs(state: State<'_, AppState>) -> CommandResult<Vec<PackInfo>> {
    let store = state.user_packs.get();
    Ok(store
        .packs
        .iter()
        .map(|p| PackInfo {
            id: p.id.clone(),
            name: p.name.clone(),
            description: p.description.clone(),
            term_count: p.terms.len(),
            enabled: p.enabled,
        })
        .collect())
}

/// Creates an empty user pack.
#[tauri::command]
pub async fn create_user_pack(
    state: State<'_, AppState>,
    name: String,
    description: String,
) -> CommandResult<PackInfo> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(CommandError::user("Pack name can't be empty"));
    }
    let pack = UserPackStore::new_pack(&name, description.trim());
    {
        let mut store = (*state.user_packs.get()).clone();
        store.insert(pack.clone())?;
        state.user_packs.replace(store)?;
    }
    Ok(PackInfo {
        id: pack.id,
        name: pack.name,
        description: pack.description,
        term_count: 0,
        enabled: pack.enabled,
    })
}

/// Deletes a user pack and all its words.
#[tauri::command]
pub async fn delete_user_pack(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    {
        let mut store = (*state.user_packs.get()).clone();
        if !store.remove(&id) {
            return Err(CommandError::not_found("Pack not found"));
        }
        state.user_packs.replace(store)?;
    }
    Ok(())
}

/// Toggles a user pack on/off (built-in packs keep using `set_pack_enabled`).
#[tauri::command]
pub async fn set_user_pack_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> CommandResult<()> {
    {
        let mut store = (*state.user_packs.get()).clone();
        let Some(pack) = store.packs.iter_mut().find(|p| p.id == id) else {
            return Err(CommandError::not_found("Unknown user pack"));
        };
        pack.enabled = enabled;
        state.user_packs.replace(store)?;
    }
    Ok(())
}

/// Adds a word (with optional mis-hearings) to a user pack.
#[tauri::command]
pub async fn add_user_pack_word(
    state: State<'_, AppState>,
    id: String,
    word: String,
    mishearings: Vec<String>,
) -> CommandResult<()> {
    {
        let mut store = (*state.user_packs.get()).clone();
        store.add_term(&id, &word, &mishearings)?;
        state.user_packs.replace(store)?;
    }
    Ok(())
}

/// Removes one word from a user pack (the per-word "x" chip, item 6).
#[tauri::command]
pub async fn remove_user_pack_word(
    state: State<'_, AppState>,
    id: String,
    word: String,
) -> CommandResult<()> {
    {
        let mut store = (*state.user_packs.get()).clone();
        if !store.remove_term(&id, &word) {
            return Err(CommandError::not_found("Word not found in pack"));
        }
        state.user_packs.replace(store)?;
    }
    Ok(())
}

/// Quick Add (item 8): the text currently selected in the focused app is
/// added to the dictionary, so a word Teletype keeps mangling is fixed with
/// one keystroke — highlight the word, press the keybinding, done. The read
/// happens on the main thread because AX calls must run there (same rule as
/// the transform shortcuts).
#[tauri::command]
pub async fn quick_add_selected_word(state: State<'_, AppState>) -> CommandResult<DictionaryWord> {
    let selected = crate::text_selection::selected_text();
    let Some(selected) = selected
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return Err(CommandError::user(
            "No text is selected. Highlight the word first.",
        ));
    };
    // Keep only the first token: quick-add is for a single word, and a stray
    // fragment of a sentence would poison the dictionary with a phrase.
    let word = selected.split_whitespace().next().unwrap_or("").to_string();
    if word.is_empty() {
        return Err(CommandError::user(
            "No text is selected. Highlight the word first.",
        ));
    }
    let mut dict = (*state.dictionary.get()).clone();
    if dict.find(&word).is_some() {
        return Err(CommandError::user(
            "That word is already in your dictionary.",
        ));
    }
    let new_word = DictionaryWord::new(word, "");
    dict.insert(new_word.clone())?;
    state.dictionary.update(|d| *d = dict)?;
    Ok(new_word)
}
