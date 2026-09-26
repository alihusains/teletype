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

/// Generic brand/acronym seeds (ported from the reference implementation's
/// builtin defaults, minus its own brand words). They stop the ASR correction
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

// ---- Import / export (P3.9) ----

/// Version of the custom-words import/export envelope. Bump when the shape
/// changes; older versions are rejected with a clear error.
pub const DICT_EXPORT_VERSION: u32 = 1;

/// The JSON envelope for custom-words import/export:
/// `"{"version": 1, "words": [...]}"`.
///
/// The `words` array holds [`DictionaryWord`] entries in their stored shape
/// (camelCase). The envelope is versioned so a future shape change can be
/// detected and rejected rather than silently mis-parsed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryExport {
    pub version: u32,
    pub words: Vec<DictionaryWord>,
}

/// Serializes `dict` to the versioned export envelope as pretty JSON.
pub fn export_to_json(dict: &Dictionary) -> Result<String, String> {
    let doc = DictionaryExport {
        version: DICT_EXPORT_VERSION,
        words: dict.words.clone(),
    };
    serde_json::to_string_pretty(&doc)
        .map_err(|e| format!("Couldn't serialize dictionary: {e}"))
}

/// Writes `dict` to `path` as the versioned export envelope.
pub fn export_to_file(dict: &Dictionary, path: &str) -> Result<(), String> {
    let json = export_to_json(dict)?;
    std::fs::write(path, json).map_err(|e| format!("Couldn't write {path}: {e}"))
}

/// Outcome of an import: how many words were newly added, how many existing
/// entries were updated (key conflict, import wins), and how many were
/// skipped (invalid shape, empty word, duplicate within the file).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCounts {
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Parses the export envelope from `bytes`, validating the version and each
/// entry. Returns the validated word list. Rejects anything that is not the
/// envelope (wrong version, wrong shape, non-JSON) with a clear error.
pub fn parse_export(bytes: &[u8]) -> Result<Vec<DictionaryWord>, String> {
    let doc: DictionaryExport =
        serde_json::from_slice(bytes).map_err(|e| format!("Not a valid dictionary export: {e}"))?;
    if doc.version != DICT_EXPORT_VERSION {
        return Err(format!(
            "Unsupported dictionary export version {} (expected {})",
            doc.version, DICT_EXPORT_VERSION
        ));
    }
    Ok(doc.words)
}

/// Merges `incoming` words into `dict` (import wins on key conflict). Returns
/// counts. The existing store is never truncated: words not in `incoming`
/// stay, and a bad file never reaches this function (the caller parses first).
pub fn merge_words(dict: &mut Dictionary, incoming: Vec<DictionaryWord>) -> ImportCounts {
    let mut counts = ImportCounts::default();
    // Track words already seen in this import so a duplicate within the file
    // is skipped rather than double-applied.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for w in incoming {
        if w.word.trim().is_empty() {
            counts.skipped += 1;
            continue;
        }
        let key = w.word.to_lowercase();
        if !seen.insert(key.clone()) {
            counts.skipped += 1;
            continue;
        }
        match dict.words.iter().position(|x| x.word.to_lowercase() == key) {
            Some(idx) => {
                // Conflict: import wins. Keep the existing id (so UI refs
                // don't break) but take the imported pronunciation/fuzzy.
                dict.words[idx].pronunciation = w.pronunciation;
                dict.words[idx].fuzzy = w.fuzzy;
                counts.updated += 1;
            }
            None => {
                dict.words.push(w);
                counts.imported += 1;
            }
        }
    }
    if counts.imported > 0 {
        dict.words.sort_by(|a, b| a.word.cmp(&b.word));
    }
    counts
}

/// Reads the file at `path`, parses the export envelope, merges into `dict`,
/// and returns the counts. On any parse/IO error the existing store is left
/// untouched and the error is returned.
pub fn import_from_file(dict: &mut Dictionary, path: &str) -> Result<ImportCounts, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("Couldn't read {path}: {e}"))?;
    let words = parse_export(&bytes)?;
    Ok(merge_words(dict, words))
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

    // ---- Import / export (P3.9) ----

    #[test]
    fn export_import_roundtrip_is_lossless() {
        let mut d = Dictionary::default();
        d.insert(DictionaryWord::new("Teletype", "tel-uh-type")).unwrap();
        d.insert(DictionaryWord::new("IC Markets", "")).unwrap();
        let before: Vec<(String, String, bool)> = d
            .words
            .iter()
            .map(|w| (w.word.clone(), w.pronunciation.clone(), w.fuzzy))
            .collect();

        let json = export_to_json(&d).unwrap();
        // Envelope shape check.
        assert!(json.contains("\"version\": 1"), "missing version: {json}");
        assert!(json.contains("\"words\":"), "missing words: {json}");

        let mut d2 = Dictionary::default();
        let counts = import_from_bytes(&mut d2, &json.as_bytes()).unwrap();
        assert_eq!(counts.imported, 2);
        assert_eq!(counts.updated, 0);
        assert_eq!(counts.skipped, 0);

        let after: Vec<(String, String, bool)> = d2
            .words
            .iter()
            .map(|w| (w.word.clone(), w.pronunciation.clone(), w.fuzzy))
            .collect();
        assert_eq!(before, after, "round-trip must be lossless");
    }

    #[test]
    fn import_corrupt_file_leaves_store_intact() {
        let mut d = Dictionary::default();
        d.insert(DictionaryWord::new("KeepMe", "")).unwrap();
        let before_len = d.words.len();

        // Non-JSON.
        let err = import_from_bytes(&mut d, b"{not json").unwrap_err();
        assert!(err.contains("Not a valid dictionary export"), "got: {err}");
        assert_eq!(d.words.len(), before_len);

        // Valid JSON but wrong shape (no version field → defaults to 0 → rejected).
        let err = import_from_bytes(&mut d, b"{\"words\":[]}").unwrap_err();
        assert!(err.contains("version"), "got: {err}");
        assert_eq!(d.words.len(), before_len);

        // Right shape but wrong version.
        let err =
            import_from_bytes(&mut d, b"{\"version\":99,\"words\":[]}").unwrap_err();
        assert!(err.contains("version 99"), "got: {err}");
        assert_eq!(d.words.len(), before_len);
    }

    #[test]
    fn import_merge_counts_and_conflicts() {
        let mut d = Dictionary::default();
        // Existing word that will be updated by the import.
        d.insert(DictionaryWord::new("Teletype", "old-pron")).unwrap();
        // Existing word not in the import (stays).
        d.insert(DictionaryWord::new("Stays", "")).unwrap();

        let json = serde_json::json!({
            "version": 1,
            "words": [
                {"id":"x1","word":"Teletype","pronunciation":"new-pron","createdAt":1,"fuzzy":true},
                {"id":"x2","word":"BrandNew","pronunciation":"","createdAt":1,"fuzzy":true},
                {"id":"x3","word":"","pronunciation":"","createdAt":1,"fuzzy":true},
                {"id":"x4","word":"BrandNew","pronunciation":"dup","createdAt":1,"fuzzy":true}
            ]
        })
        .to_string();

        let counts = import_from_bytes(&mut d, json.as_bytes()).unwrap();
        assert_eq!(counts.imported, 1, "only BrandNew is new");
        assert_eq!(counts.updated, 1, "Teletype conflicts → updated");
        assert_eq!(counts.skipped, 2, "empty word + in-file dup");

        // Conflict: import wins on pronunciation.
        assert_eq!(d.find("teletype").unwrap().pronunciation, "new-pron");
        // New word added.
        assert!(d.find("brandnew").is_some());
        // Untouched word stays.
        assert!(d.find("stays").is_some());
    }

    /// Helper: import from in-memory bytes (same code path as the file
    /// variant minus the fs::read).
    fn import_from_bytes(
        dict: &mut Dictionary,
        bytes: &[u8],
    ) -> Result<ImportCounts, String> {
        let words = parse_export(bytes)?;
        Ok(merge_words(dict, words))
    }
}
