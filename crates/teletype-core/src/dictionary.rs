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
    /// Whether near-misses (edit distance <= 2) may be corrected to this word.
    /// User ta- words keep true (the taught-mishearing feature); builtin
    /// seeds set false: common acronyms sit within distance 2 of ordinary
    /// words ("apt" -> "API", "its" -> "iOS", "cloud" -> "Claude",
    /// "his code" -> "VS Code"), which silently rewrote dictated text.
    #[serde(default = "default_fuzzy")]
    pub fuzzy: bool,
}

fn default_fuzzy() -> bool {
    true
}

impl DictionaryWord {
    pub fn new(word: impl Into<String>, pronunciation: impl Into<String>) -> Self {
        let now = crate::storage::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            word: word.into(),
            pronunciation: pronunciation.into(),
            created_at: now,
            fuzzy: true,
        }
    }
}

/// Version of the built-in word list below. Bump when adding entries: the
/// seed merge runs once per install (tracked by `Dictionary::builtin_version`),
/// so existing dictionaries gain newer builtins without ever overwriting what
/// the user already added or removed.
///
/// History: v1 = first seed (all words accidentally fuzzy);
/// v2 = seeds marked exact-only (`fuzzy: false`), applied retroactively to
/// already-seeded words so installs seeded by v1 get the fix on next start.
pub const BUILTIN_DICTIONARY_VERSION: u32 = 2;

/// Generic brand/acronym seeds (ported from EnviousWispr's builtinDefaults,
/// minus their own brand words). They stop the ASR correction pass and the
/// transforms from "fixing" these common tech terms into wrong spellings.
pub const BUILTIN_DICTIONARY_WORDS: &[&str] = &[
    "API", "CLI", "ChatGPT", "Claude", "EG-1", "GitHub", "iOS", "macOS", "OpenAI", "VS Code",
];

/// The stored dictionary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dictionary {
    pub words: Vec<DictionaryWord>,
    /// Which builtin seed version has already been merged in. 0 (or a missing
    /// field on a pre-seed install) means "not seeded yet".
    pub builtin_version: u32,
}

impl Dictionary {
    /// Merges built-in words that are not already present (case-insensitive)
    /// and records the seed version. Returns how many words were added or
    /// updated (v1-seeded words get `fuzzy` flipped off). The user's own words
    /// are never touched: a builtin the user deleted stays deleted, because
    /// the version flag stops the merge from running again.
    ///
    /// Seeded words are exact-match only (`fuzzy: false`): they are common
    /// tech terms, and the distance-2 correction pass would otherwise rewrite
    /// ordinary speech that happens to sit near them.
    pub fn seed_builtins(&mut self) -> usize {
        if self.builtin_version >= BUILTIN_DICTIONARY_VERSION {
            return 0;
        }
        let mut changed = 0;
        let mut pushed = false;
        for w in BUILTIN_DICTIONARY_WORDS {
            match self
                .words
                .iter()
                .position(|x| x.word.eq_ignore_ascii_case(w))
            {
                // Present (either just added by the user or seeded by v1):
                // migrate to exact-only without altering anything else.
                Some(idx) => {
                    if self.words[idx].fuzzy {
                        self.words[idx].fuzzy = false;
                        changed += 1;
                    }
                }
                None => {
                    let mut word = DictionaryWord::new(*w, "");
                    word.fuzzy = false;
                    self.words.push(word);
                    changed += 1;
                    pushed = true;
                }
            }
        }
        if pushed {
            self.words.sort_by(|a, b| a.word.cmp(&b.word));
        }
        self.builtin_version = BUILTIN_DICTIONARY_VERSION;
        changed
    }

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

    #[test]
    fn seed_builtins_runs_once_and_keeps_user_words() {
        let mut d = Dictionary::default();
        // User already has their own (differently cased) word before seeding.
        d.insert(DictionaryWord::new("github", "")).unwrap();
        assert!(d.seed_builtins() > 0);
        // Case-insensitive dedupe: the builtin "GitHub" was not re-added.
        assert_eq!(
            d.words
                .iter()
                .filter(|w| w.word.eq_ignore_ascii_case("github"))
                .count(),
            1
        );
        // User's casing is preserved.
        assert!(d.find("github").is_some());
        assert!(d.find("macOS").is_some());
        // Second call: version recorded, nothing re-added.
        assert_eq!(d.seed_builtins(), 0);
        // A word the user deletes stays deleted on later runs.
        let id = d.find("macOS").unwrap().id.clone();
        d.remove(&id);
        assert_eq!(d.seed_builtins(), 0);
        assert!(d.find("macOS").is_none());
    }
}
