//! The scratchpad: a private, always-available dictation target.
//!
//! While the scratchpad is "focused" (its window is frontmost), a dictation
//! is appended to the pad instead of being injected into the previous focus.
//! The pad persists across sessions and can be cleared or copied out.

use serde::{Deserialize, Serialize};

/// One line of the scratchpad (one dictation = one entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScratchEntry {
    pub id: String,
    pub created_at: u64,
    pub text: String,
}

/// The scratchpad document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Scratchpad {
    pub entries: Vec<ScratchEntry>,
}

/// Cap so the pad stays a scratch space, not an archive.
const MAX_ENTRIES: usize = 200;

impl Scratchpad {
    /// Appends an entry (newest first, like the history).
    pub fn append(&mut self, text: String) -> Option<ScratchEntry> {
        if text.trim().is_empty() {
            return None;
        }
        let entry = ScratchEntry {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: crate::storage::now_ms(),
            text: text.trim().to_string(),
        };
        self.entries.insert(0, entry.clone());
        if self.entries.len() > MAX_ENTRIES {
            self.entries.truncate(MAX_ENTRIES);
        }
        Some(entry)
    }

    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The whole pad as one text block (oldest → newest, one entry per line).
    pub fn combined(&self) -> String {
        self.entries
            .iter()
            .rev()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_is_newest_first_and_trimmed() {
        let mut pad = Scratchpad::default();
        pad.append("first".into());
        pad.append("second".into());
        assert_eq!(pad.entries[0].text, "second");
        assert_eq!(pad.combined(), "first\nsecond");
    }

    #[test]
    fn blank_entries_are_rejected() {
        let mut pad = Scratchpad::default();
        assert!(pad.append("   ".into()).is_none());
        assert!(pad.entries.is_empty());
    }

    #[test]
    fn clear_and_remove_work() {
        let mut pad = Scratchpad::default();
        let e = pad.append("x".into()).unwrap();
        pad.remove(&e.id);
        assert!(pad.entries.is_empty());
        pad.append("y".into());
        pad.clear();
        assert!(pad.entries.is_empty());
    }
}
