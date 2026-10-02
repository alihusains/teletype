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
    /// Set when the word entered the dictionary through the personalization
    /// loop (a learned terminology correction) rather than a user teaching it.
    /// The value is the misheard form that was first observed, e.g.
    /// "margets" for a learned "Markets". Shown as a "learned" badge in the
    /// Dictionary UI (T5.1, decision D006).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned_from: Option<String>,
    /// When the word was learned (epoch ms); set together with `learned_from`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learned_at: Option<u64>,
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
            learned_from: None,
            learned_at: None,
        }
    }

    /// Marks this word as learned from the misheard form `from` (D006).
    pub fn mark_learned(&mut self, from: impl Into<String>) {
        self.learned_from = Some(from.into());
        self.learned_at = Some(crate::storage::now_ms());
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

    /// Adds `to` as a learned word (D006/T5.1) and records `from` as the
    /// misheard form it was learned from. If the word already exists, its
    /// provenance is backfilled only when it has none (a user-taught word is
    /// never relabeled). Returns true when the dictionary changed.
    pub fn insert_learned(&mut self, from: &str, to: &str) -> bool {
        let to = to.trim();
        if to.is_empty() {
            return false;
        }
        if self.find(to).is_some() {
            let changed = self
                .words
                .iter_mut()
                .find(|w| w.word.eq_ignore_ascii_case(to))
                .filter(|w| w.learned_from.is_none())
                .map(|w| {
                    w.learned_from = Some(from.to_string());
                    w.learned_at = Some(crate::storage::now_ms());
                })
                .is_some();
            changed
        } else {
            let mut word = DictionaryWord::new(to, "");
            word.mark_learned(from);
            self.words.push(word);
            self.words.sort_by(|a, b| a.word.cmp(&b.word));
            true
        }
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
    serde_json::to_string_pretty(&doc).map_err(|e| format!("Couldn't serialize dictionary: {e}"))
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
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {path}: {e}"))?;
    let words = parse_export(&bytes)?;
    Ok(merge_words(dict, words))
}

// ── Word checking ────────────────────────────────────────────────────────────

/// Decides whether a listed dictionary word was actually spoken, given the
/// surrounding context. Replaces the hard edit-distance threshold with a
/// calibrated probability and a tunable cutoff, so the wrong-swap rate is a
/// dial instead of a cliff.
///
/// The default implementation ([`EditDistanceChecker`]) reproduces the
/// historical `edit_distance <= 2 && similarity >= 0.80` rule. A future
/// trained model (Kev-class, jev-rs, or a calibrated Rust classifier) drops
/// in behind this trait without touching the pipeline.
pub trait WordChecker: Send + Sync {
    /// Returns a score in `0.0..=1.0`: how likely it is that the speaker
    /// said `listed_word` here.
    ///
    /// - `listed_word`: the canonical dictionary word (lowercase, folded).
    /// - `as_written`: the token/phrase as ASR produced it (lowercase, folded).
    /// - `fuzzy`: whether the entry allows near-miss correction (user-taught
    ///   words are `true`; builtin seeds are `false`).
    ///
    /// The caller compares the result against a cutoff (see
    /// [`EditDistanceChecker::cutoff`]).
    fn probability(&self, listed_word: &str, as_written: &str, fuzzy: bool) -> f32;

    /// The score at or above which a swap is approved.
    fn cutoff(&self) -> f32;
}

/// The default checker: Levenshtein edit distance with a similarity floor.
///
/// Score mapping (preserves existing behavior exactly):
/// - exact match → `1.0`
/// - fuzzy entry, `dist <= 2`, similarity `>= 0.80` → `1.0 - dist / max_len`
///   (range `0.80..=1.0`)
/// - everything else → `0.0`
///
/// Cutoff: `0.80` (the historical similarity floor).
pub struct EditDistanceChecker;

impl EditDistanceChecker {
    pub const fn new() -> Self {
        Self
    }
}

/// The default word checker, available as a shared constant for pipeline
/// construction. Zero-cost: a unit struct with no state.
pub static EDIT_DISTANCE_CHECKER: EditDistanceChecker = EditDistanceChecker::new();

impl Default for EditDistanceChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl WordChecker for EditDistanceChecker {
    fn probability(&self, listed_word: &str, as_written: &str, fuzzy: bool) -> f32 {
        if listed_word == as_written {
            return 1.0;
        }
        if !fuzzy {
            return 0.0;
        }
        let len_diff = (listed_word.len() as i32 - as_written.len() as i32).unsigned_abs();
        if len_diff > 2 {
            return 0.0;
        }
        let dist = crate::pipeline::edit_distance_public(as_written, listed_word);
        if dist > 2 {
            return 0.0;
        }
        let max_len = as_written.len().max(listed_word.len());
        let sim = 1.0f32 - dist as f32 / max_len as f32;
        if sim >= 0.80 {
            sim
        } else {
            0.0
        }
    }

    fn cutoff(&self) -> f32 {
        0.80
    }
}

/// A single labelled observation of a word-check decision, collected from
/// user overrides. This is the training data a future model needs: every
/// time the user re-edits a word the checker swapped (or the app auto-corrects
/// one), that is a label.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordCheckLabel {
    pub id: String,
    /// The canonical dictionary word (e.g. "Teletype").
    pub listed_word: String,
    /// The token/phrase as ASR produced it (e.g. "teletype" or "tea type").
    pub as_written: String,
    /// The surrounding sentence as ASR wrote it.
    pub context: String,
    /// `true` if the user confirmed the swap was correct; `false` if the
    /// user reverted it (wrong swap).
    pub approved: bool,
    pub timestamp: u64,
}

impl WordCheckLabel {
    pub fn new(
        listed_word: impl Into<String>,
        as_written: impl Into<String>,
        context: impl Into<String>,
        approved: bool,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            listed_word: listed_word.into(),
            as_written: as_written.into(),
            context: context.into(),
            approved,
            timestamp: crate::storage::now_ms(),
        }
    }
}

/// A local log of word-check decisions and user overrides. This is the
/// training data a future Kev-class model needs: every observation is a
/// `(listed_word, as_written, context, approved)` tuple.
///
/// Stored as a JSONL file (one label per line) so it can be inspected,
/// exported, or fed to a trainer without a database.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WordCheckLog {
    pub labels: Vec<WordCheckLabel>,
}

impl WordCheckLog {
    /// Appends a label and returns the updated log.
    pub fn record(&mut self, label: WordCheckLabel) {
        self.labels.push(label);
    }

    /// Loads the log from a JSONL file. Missing file → empty log.
    pub fn load(path: &str) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let labels: Vec<WordCheckLabel> = content
                    .lines()
                    .filter(|l| !l.is_empty())
                    .filter_map(|l| serde_json::from_str(l).ok())
                    .collect();
                Self { labels }
            }
            Err(_) => Self::default(),
        }
    }

    /// Persists the log to a JSONL file. Creates parent directories.
    pub fn save(&self, path: &str) -> Result<(), String> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        let mut out = String::new();
        for label in &self.labels {
            out.push_str(&serde_json::to_string(label).map_err(|e| e.to_string())?);
            out.push('\n');
        }
        std::fs::write(path, out).map_err(|e| format!("could not write {path}: {e}"))
    }

    /// Number of labels collected so far.
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
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

    // ---- Import / export (P3.9) ----

    #[test]
    fn export_import_roundtrip_is_lossless() {
        let mut d = Dictionary::default();
        d.insert(DictionaryWord::new("Teletype", "tel-uh-type"))
            .unwrap();
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
        let counts = import_from_bytes(&mut d2, json.as_bytes()).unwrap();
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
        let err = import_from_bytes(&mut d, b"{\"version\":99,\"words\":[]}").unwrap_err();
        assert!(err.contains("version 99"), "got: {err}");
        assert_eq!(d.words.len(), before_len);
    }

    #[test]
    fn import_merge_counts_and_conflicts() {
        let mut d = Dictionary::default();
        // Existing word that will be updated by the import.
        d.insert(DictionaryWord::new("Teletype", "old-pron"))
            .unwrap();
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
    fn import_from_bytes(dict: &mut Dictionary, bytes: &[u8]) -> Result<ImportCounts, String> {
        let words = parse_export(bytes)?;
        Ok(merge_words(dict, words))
    }
}

#[test]
fn insert_learned_adds_word_with_provenance() {
    let mut d = Dictionary::default();
    assert!(d.insert_learned("margets", "Markets"));
    let w = d.find("Markets").expect("learned word missing");
    assert_eq!(w.learned_from.as_deref(), Some("margets"));
    assert!(w.learned_at.is_some());
    assert!(w.fuzzy);
}

#[test]
fn insert_learned_backfills_existing_word_without_overwriting() {
    let mut d = Dictionary::default();
    let mut taught = DictionaryWord::new("Markets", "");
    taught.learned_from = Some("taught already".into());
    d.insert(taught).unwrap();
    let changed = d.insert_learned("margets", "Markets");
    assert!(!changed);
    let w = d.find("Markets").unwrap();
    assert_eq!(w.learned_from.as_deref(), Some("taught already"));
    assert!(w.learned_at.is_none());
}

#[test]
fn insert_learned_backfills_word_without_provenance() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Markets", "")).unwrap();
    assert!(d.insert_learned("margets", "Markets"));
    let w = d.find("Markets").unwrap();
    assert_eq!(w.learned_from.as_deref(), Some("margets"));
    assert!(w.learned_at.is_some());
}

#[test]
fn insert_learned_rejects_empty_target() {
    let mut d = Dictionary::default();
    assert!(!d.insert_learned("margets", "  "));
    assert!(d.words.is_empty());
}

#[test]
fn learned_provenance_survives_json_roundtrip() {
    let mut d = Dictionary::default();
    d.insert_learned("margets", "Markets");
    let json = serde_json::to_string(&d).unwrap();
    let back: Dictionary = serde_json::from_str(&json).unwrap();
    let w = back.find("Markets").unwrap();
    assert_eq!(w.learned_from.as_deref(), Some("margets"));
    assert!(w.learned_at.is_some());
}

#[test]
fn legacy_json_without_provenance_still_loads() {
    let d: Dictionary =
            serde_json::from_str(r#"{"words":[{"id":"1","word":"API","pronunciation":"","createdAt":0,"fuzzy":false}],"builtinVersion":2}"#)
                .unwrap();
    assert_eq!(d.words.len(), 1);
    assert!(d.words[0].learned_from.is_none());
    assert!(d.words[0].learned_at.is_none());
}
