//! Dictation history: a local log of every completed dictation.
//!
//! Every transcription is saved here (with the app it was dictated into),
//! mirroring Wispr Flow's Dictation tab. When the user dictated somewhere
//! with no text field to receive it, the entry is still kept and can be
//! copied or re-pasted from the UI.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::context::ApplicationContext;
use crate::storage::JsonStore;

/// One completed dictation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationEntry {
    /// Unique id (uuid / timestamp-based).
    pub id: String,
    /// When it was dictated (ms since epoch).
    pub created_at: u64,
    /// The final (transformed, filler-stripped) text.
    pub text: String,
    /// Where it was dictated into (best-effort).
    #[serde(default)]
    pub context: Option<ApplicationContext>,
}

/// The history document: a flat list, newest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationHistory {
    #[serde(default)]
    pub entries: Vec<DictationEntry>,
}

/// Cap on stored entries so the file doesn't grow without bound.
const MAX_ENTRIES: usize = 1000;

impl DictationHistory {
    /// Appends an entry (newest first) and trims to [`MAX_ENTRIES`].
    pub fn push(&mut self, entry: DictationEntry) {
        self.entries.insert(0, entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.truncate(MAX_ENTRIES);
        }
    }

    /// Removes the entry with `id`, if present.
    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }
}

/// Loads or creates the history store at `dir/dictation.json`.
pub fn open_history(dir: &Path) -> (JsonStore<DictationHistory>, DictationHistory) {
    let store = JsonStore::new(dir, "dictation.json");
    let history = store.load(DictationHistory::default());
    (store, history)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, at: u64) -> DictationEntry {
        DictationEntry {
            id: id.into(),
            created_at: at,
            text: "hello".into(),
            context: None,
        }
    }

    #[test]
    fn newest_first_and_trimmed() {
        let mut h = DictationHistory::default();
        for i in 0..MAX_ENTRIES + 5 {
            h.push(entry(&i.to_string(), i as u64));
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        // First pushed (id 0) should have been trimmed; last pushed is first.
        assert_eq!(h.entries[0].id, (MAX_ENTRIES + 4).to_string());
    }

    #[test]
    fn remove_by_id() {
        let mut h = DictationHistory::default();
        h.push(entry("a", 1));
        h.push(entry("b", 2));
        h.remove("a");
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].id, "b");
    }
}
