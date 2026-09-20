//! The user dictionary: custom words and pronunciation hints.
//!
//! Custom words keep proper nouns, product names and jargon from being
//! "corrected" away by transforms and validators. Pronunciation hints
//! influence how the app speaks about words (and can be passed to the ASR
//! engine when the backend supports hotwords).

use serde::{Deserialize, Serialize};

/// One dictionary word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryWord {
    pub id: String,
    /// The canonical spelling, e.g. "Teletype".
    pub word: String,
    /// Optional pronunciation hint (phonetic spelling), e.g. "tel-uh-type".
    pub pronunciation: String,
    pub created_at: u64,
}

impl DictionaryWord {
    pub fn new(word: impl Into<String>, pronunciation: impl Into<String>) -> Self {
        let now = crate::storage::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            word: word.into(),
            pronunciation: pronunciation.into(),
            created_at: now,
        }
    }
}

/// The stored dictionary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dictionary {
    pub words: Vec<DictionaryWord>,
}

impl Dictionary {
    pub fn get(&self, id: &str) -> Option<&DictionaryWord> {
        self.words.iter().find(|w| w.id == id)
    }

    pub fn find(&self, word: &str) -> Option<&DictionaryWord> {
        self.words
            .iter()
            .find(|w| w.word.eq_ignore_ascii_case(word))
    }

    pub fn insert(&mut self, word: DictionaryWord) -> Result<(), String> {
        if self.find(&word.word).is_some() {
            return Err("Word already exists".into());
        }
        self.words.push(word);
        self.words.sort_by(|a, b| a.word.cmp(&b.word));
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.words.len();
        self.words.retain(|w| w.id != id);
        self.words.len() != before
    }

    /// Case-insensitive set of all known words, for validators.
    pub fn known_words(&self) -> std::collections::HashSet<String> {
        self.words.iter().map(|w| w.word.to_lowercase()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_find_remove_roundtrip() {
        let mut d = Dictionary::default();
        let w = DictionaryWord::new("Teletype", "tel-uh-type");
        let id = w.id.clone();
        d.insert(w).unwrap();
        assert!(d.find("teletype").is_some());
        assert!(d.insert(DictionaryWord::new("teletype", "")).is_err());
        assert!(d.remove(&id));
        assert!(d.find("Teletype").is_none());
    }

    #[test]
    fn known_words_are_lowercased() {
        let mut d = Dictionary::default();
        d.insert(DictionaryWord::new("IC Markets", "")).unwrap();
        assert!(d.known_words().contains("ic markets"));
    }
}
