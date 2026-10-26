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
    /// Sound-alikes the user taught by hand (e.g. "Teletype" also heard as
    /// "tel-uh-type"). Distinct from `learned_from`, which is the single
    /// misheard form the personalization loop first observed.
    ///
    /// Always serialised: the IPC consumer (DictionaryScreen) calls
    /// `w.aliases.length` directly, and with `skip_serializing_if =
    /// "Vec::is_empty"` the key was omitted for every word without sound-alikes,
    /// arriving as `undefined` and crashing the screen with
    /// "undefined is not an object (evaluating 's.aliases.length')".
    #[serde(default)]
    pub aliases: Vec<String>,
    /// How often a dictation was corrected to this word. Shown as a small
    /// "used N times" count on the word row, the way the reference app shows
    /// usage on its custom terms.
    #[serde(default)]
    pub usage_count: u32,
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
            aliases: Vec::new(),
            usage_count: 0,
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
    "API", "CLI", "ChatGPT", "Claude", "GitHub", "iOS", "macOS", "OpenAI", "VS Code",
];

// ── Multi-word join guards (word-corrector port, plan row 6) ────────────────
//
// The fuzzy fixer joins tokens before matching, which can swallow a sentence
// boundary ("still open. ai" -> "still OpenAI") or a standalone function
// word ("a pi" -> "API"). These predicates gate every multi-word span claim
// back against the ORIGINAL text. Kept here (not in `pipeline.rs`) so they
// are unit-testable without the rest of the pipeline.

/// Clause or sentence punctuation marks that end a phrase: a claimed span
/// must not match across one of these in the original text.
const JOIN_PUNCTUATION: &[char] = &['.', ',', ';', ':', '!', '?'];

/// Standalone English function words a multi-word span must not absorb
/// when the claimed canonical term does not itself contain one.
const JOIN_STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "in", "on", "at", "to", "for", "and", "or", "is", "are",
];

/// Canonical terms that legitimately contain a stopword and are therefore
/// never blocked by [`span_joins_through_stopword`], whatever the source
/// looks like ("Bank of America" keeps its "of").
const JOIN_STOPWORD_TERM_ALLOWLIST: &[&str] =
    &["bank of america", "state of the art", "visa versa"];

/// True when the original text of the span `[start, end)` crosses a clause
/// or sentence boundary: any of the span's tokens carries clause/sentence
/// punctuation ([`JOIN_PUNCTUATION`]) or a line break on its *trailing* edge
/// (the boundary with the next token, or the end of the span).
///
/// `tokens` is the whitespace-split token stream of `text` (the same
/// `text.split_whitespace()` collection the fixer walks) and must agree with
/// `text`; on any disagreement the span is refused (returned `true`) rather
/// than silently allowed. Punctuation is checked on the original token text
/// because the fixer strips it before matching: "open. ai" only looks like
/// "open ai" after stripping, and the "." that separates the two tokens
/// lives on the trailing edge of "open". A single token's own punctuation
/// never counts (only spans of two or more tokens are gated).
pub fn slice_crosses_punctuation(tokens: &[&str], start: usize, end: usize, text: &str) -> bool {
    if end.saturating_sub(start) < 2 || start >= tokens.len() || end > tokens.len() {
        return false;
    }

    // Verify the token stream against the original text by replaying the
    // whitespace split left to right. `pos` ends at the byte offset just
    // past `tokens[i]`.
    let mut pos = 0;
    for t in tokens.iter().take(end) {
        while pos < text.len() && text.as_bytes()[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos + t.len() > text.len() || &text[pos..pos + t.len()] != *t {
            return true; // token stream and text disagree; refuse the span
        }
        pos += t.len();
    }

    // A span crosses a boundary when any of its tokens ends in clause/
    // sentence punctuation or a line break. The trailing edge of the last
    // token is included: "open. ai" -> "OpenAI" is blocked by the "." that
    // sits between "open" and "ai", even though it is glued to "open".
    tokens[start..end].iter().any(|t| {
        t.chars().any(|c| c.is_whitespace() && c != ' ' || JOIN_PUNCTUATION.contains(&c))
    })
}

/// True when the span `[start, end)` over `tokens` would absorb a standalone
/// English stopword ([`JOIN_STOPWORDS`]) that is one of the span's own
/// tokens, and the claimed canonical term does not legitimately contain it.
///
/// A term that contains a stopword as one of its own words ("Bank of
/// America", user-taught "state of the art") is its own authority and is
/// never blocked; the allowlist pins the canonical forms we expect to see
/// in the wild regardless of casing.
pub fn span_joins_through_stopword(
    tokens: &[&str],
    start: usize,
    end: usize,
    canonical: &str,
) -> bool {
    if end.saturating_sub(start) < 2 || start >= tokens.len() || end > tokens.len() {
        return false;
    }
    let canonical_lower = canonical.to_lowercase();
    if JOIN_STOPWORD_TERM_ALLOWLIST.contains(&canonical_lower.as_str()) {
        return false;
    }
    // A canonical term that contains a stopword as one of its own words is
    // its own authority ("state of the art" taught by the user).
    if canonical_lower
        .split_whitespace()
        .any(|w| JOIN_STOPWORDS.contains(&w))
    {
        return false;
    }
    for t in &tokens[start..end] {
        let bare: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
        if JOIN_STOPWORDS.contains(&bare.to_lowercase().as_str()) {
            return true;
        }
    }
    false
}

/// The stored dictionary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Dictionary {
    pub words: Vec<DictionaryWord>,
    /// Which builtin seed version has already been merged in. 0 (or a missing
    /// field on a pre-seed install) means "not seeded yet".
    pub builtin_version: u32,
}

/// What one [`Dictionary::insert_learned`] call changed.
///
/// Returned instead of a bare `bool` so the undo offer can revert exactly the
/// write a take made: remove a word learning created, clear provenance it
/// backfilled, or drop only the alias it added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LearnedChange {
    /// Learning created the word.
    Created { id: String },
    /// An existing word gained its first learned provenance. `alias_added`
    /// is false when the user had already taught the same misheard form.
    Backfilled {
        id: String,
        from: String,
        alias_added: bool,
    },
    /// An already-learned word gained another misheard form as an alias.
    AliasAdded { id: String, from: String },
    /// Nothing changed.
    Unchanged,
}

/// Adds `alias` to `word` unless it is already present (case-insensitive).
fn add_alias(word: &mut DictionaryWord, alias: &str) -> bool {
    if word.aliases.iter().any(|a| a.eq_ignore_ascii_case(alias)) {
        return false;
    }
    word.aliases.push(alias.to_string());
    true
}

/// Removes `alias` from `word` if present (case-insensitive).
fn remove_alias(word: &mut DictionaryWord, alias: &str) -> bool {
    let before = word.aliases.len();
    word.aliases.retain(|a| !a.eq_ignore_ascii_case(alias));
    word.aliases.len() != before
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

    /// Adds `to` as a learned word (D006/T5.1), records `from` as the
    /// misheard form it was learned from, and registers `from` as an alias so
    /// the next take corrects the same mishearing through the exact/alias
    /// rescoring pass.
    ///
    /// If the word already exists, provenance is backfilled only when it has
    /// none (a user-taught word is never relabeled), but a new misheard form
    /// is still added as an alias. The returned [`LearnedChange`] describes
    /// exactly what changed, so the undo offer can revert it.
    pub fn insert_learned(&mut self, from: &str, to: &str) -> LearnedChange {
        let to = to.trim();
        let from = from.trim();
        if to.is_empty() || from.is_empty() {
            return LearnedChange::Unchanged;
        }
        if let Some(idx) = self
            .words
            .iter()
            .position(|w| w.word.eq_ignore_ascii_case(to))
        {
            let id = self.words[idx].id.clone();
            let alias_added = add_alias(&mut self.words[idx], from);
            if self.words[idx].learned_from.is_none() {
                self.words[idx].learned_from = Some(from.to_string());
                self.words[idx].learned_at = Some(crate::storage::now_ms());
                return LearnedChange::Backfilled {
                    id,
                    from: from.to_string(),
                    alias_added,
                };
            }
            if alias_added {
                return LearnedChange::AliasAdded {
                    id,
                    from: from.to_string(),
                };
            }
            return LearnedChange::Unchanged;
        }
        let mut word = DictionaryWord::new(to, "");
        word.mark_learned(from);
        add_alias(&mut word, from);
        let id = word.id.clone();
        self.words.push(word);
        self.words.sort_by(|a, b| a.word.cmp(&b.word));
        LearnedChange::Created { id }
    }

    /// Reverts one [`insert_learned`](Self::insert_learned) step: removes the
    /// word learning created, or clears the provenance/alias it added to a
    /// word that already existed. Never deletes a word the user already had.
    /// Returns true when the dictionary changed.
    pub fn revert_learned(&mut self, change: &LearnedChange) -> bool {
        match change {
            LearnedChange::Created { id } => self.remove(id),
            LearnedChange::Backfilled {
                id,
                from,
                alias_added,
            } => {
                let Some(word) = self.words.iter_mut().find(|w| w.id == *id) else {
                    return false;
                };
                let mut changed = false;
                if word.learned_from.is_some() {
                    word.learned_from = None;
                    word.learned_at = None;
                    changed = true;
                }
                if *alias_added && remove_alias(word, from) {
                    changed = true;
                }
                changed
            }
            LearnedChange::AliasAdded { id, from } => {
                let Some(word) = self.words.iter_mut().find(|w| w.id == *id) else {
                    return false;
                };
                remove_alias(word, from)
            }
            LearnedChange::Unchanged => false,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.words.len();
        self.words.retain(|w| w.id != id);
        self.words.len() != before
    }

    /// Replaces the word `id` with `word` (the UI sends the full edited row).
    /// Returns `None` when no word has that id.
    pub fn update(&mut self, word: DictionaryWord) -> Option<()> {
        let pos = self.words.iter().position(|w| w.id == word.id)?;
        // Preserve the id: the client echoes it back, but a hand-built
        // caller must not be able to rename a word's identity.
        let id = self.words[pos].id.clone();
        self.words[pos] = word;
        self.words[pos].id = id;
        Some(())
    }

    /// Bumps the usage counter for every word whose canonical spelling or one
    /// of its aliases appears in `heard` (case-insensitive; multi-word
    /// entries like "VS Code" match as a phrase anywhere in the text).
    /// Returns how many words were counted.
    pub fn record_usage(&mut self, heard: &str) -> usize {
        let heard_lower = heard.to_lowercase();
        let mut hits = 0;
        for w in self.words.iter_mut() {
            let matched = heard_lower.contains(&w.word.to_lowercase())
                || w.aliases
                    .iter()
                    .any(|a| heard_lower.contains(&a.to_lowercase()));
            if matched {
                w.usage_count = w.usage_count.saturating_add(1);
                hits += 1;
            }
        }
        hits
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

// ── File import: formats + preview (Phase 7.6) ─────────────────────────────

/// Detects which dictionary import format `bytes` is in, returning one of
/// `"json"`, `"csv"`, `"tsv"`, or `"text"`.
///
/// Content sniffing wins over the extension: a JSON envelope is recognized by
/// its leading `{`/`[`, and a delimited table by a `,` or `\t` on the first
/// line. Only when the content is inconclusive (no delimiter, not JSON) does
/// the file extension in `path_hint` decide, and the default is `"text"`.
pub fn detect_dictionary_format(bytes: &[u8], path_hint: &str) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim_start();
    // JSON envelope: leading `{` or `[`.
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return "json".to_string();
    }
    // Delimited table: a comma or tab on the first non-blank line.
    if let Some(first_line) = trimmed.lines().next() {
        if first_line.contains(',') {
            return "csv".to_string();
        }
        if first_line.contains('\t') {
            return "tsv".to_string();
        }
    }
    // Extension fallback.
    let lower = path_hint.to_ascii_lowercase();
    if lower.ends_with(".json") {
        "json".to_string()
    } else if lower.ends_with(".csv") {
        "csv".to_string()
    } else if lower.ends_with(".tsv") {
        "tsv".to_string()
    } else {
        "text".to_string()
    }
}

/// A non-mutating preview of what importing `bytes` into a dictionary would
/// do. Computed by parsing the file without touching the store, so the UI can
/// show the user what will happen before they confirm.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileImportPreview {
    /// The detected format: `"json" | "csv" | "tsv" | "text"`.
    pub format: String,
    /// The parsed words, in file order (duplicates within the file removed).
    pub words: Vec<DictionaryWord>,
    /// Words not already in the dictionary (case-insensitive term match).
    pub would_import: usize,
    /// Words matching an existing dictionary entry (case-insensitive).
    pub would_update: usize,
    /// Invalid rows (empty term) plus in-file duplicates, counted once each.
    pub would_skip: usize,
    /// Up to the first 10 skipped terms/rows, for the preview UI.
    pub skipped_examples: Vec<String>,
}

/// The cap on `skipped_examples` retained in a preview.
const PREVIEW_SKIP_EXAMPLE_CAP: usize = 10;

/// Splits an alias string on `;` or `|`, trimming and dropping empty pieces.
fn split_aliases(raw: &str) -> Vec<String> {
    raw.split([';', '|'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Builds a `DictionaryWord` from parsed fields, minting a fresh id and
/// timestamp the same way `DictionaryWord::new` does.
fn build_word(word: &str, pronunciation: &str, aliases: Vec<String>) -> DictionaryWord {
    let mut w = DictionaryWord::new(word, pronunciation);
    w.aliases = aliases;
    w
}

/// Parses the first row of a table to decide whether it is a header.
///
/// Returns `Some((term_idx, aliases_idx, pronunciation_idx))` when any cell
/// (case-insensitive, trimmed) is `word`/`term`/`phrase` — the term column is
/// that cell, the aliases column is a cell named `aliases`/`alts` when
/// present, and the pronunciation column a cell named
/// `pronunciation`/`phonetic` when present. Returns `None` for a positional
/// (no-header) table.
fn detect_table_header(row: &[String]) -> Option<(usize, Option<usize>, Option<usize>)> {
    let term_idx = row.iter().position(|c| {
        matches!(
            c.trim().to_ascii_lowercase().as_str(),
            "word" | "term" | "phrase"
        )
    })?;
    let aliases_idx = row
        .iter()
        .position(|c| matches!(c.trim().to_ascii_lowercase().as_str(), "aliases" | "alts"));
    let pronunciation_idx = row.iter().position(|c| {
        matches!(
            c.trim().to_ascii_lowercase().as_str(),
            "pronunciation" | "phonetic"
        )
    });
    Some((term_idx, aliases_idx, pronunciation_idx))
}

/// Splits a delimited line into cells with minimal RFC-4180 quote handling:
/// fields may be wrapped in `"` with `""` as the escaped quote; the delimiter
/// only separates fields outside a quoted region.
fn split_delimited(line: &str, delim: char) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == delim {
            cells.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    cells.push(cur);
    cells
}

/// Parses a `csv`/`tsv` table into words. `delim` is `,` or `\t`.
///
/// Rows with an empty term are skipped and their raw text retained in
/// `skipped` (capped at [`PREVIEW_SKIP_EXAMPLE_CAP`]).
struct TableParse {
    words: Vec<DictionaryWord>,
    skipped: Vec<String>,
}

/// Parses a `csv`/`tsv` table into words. `delim` is `,` or `\t`.
///
/// Rows with an empty term are skipped and their raw text retained in
/// `skipped` (capped at [`PREVIEW_SKIP_EXAMPLE_CAP`]).
fn parse_table(lines: &[&str], delim: char) -> TableParse {
    let rows: Vec<Vec<String>> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| split_delimited(l, delim))
        .collect();
    let mut words = Vec::new();
    let mut skipped = Vec::new();
    if rows.is_empty() {
        return TableParse { words, skipped };
    }
    // Header detection on the first row. A header row is consumed (start = 1)
    // and the named columns used; a positional table has no header (start = 0)
    // and columns 0/1/2 are term/aliases/pronunciation.
    let (start, term_idx, aliases_idx, pronunciation_idx) = match detect_table_header(&rows[0]) {
        Some((t, a, p)) => (1, t, a, p),
        None => (0usize, 0usize, Some(1), Some(2)),
    };
    for row in &rows[start..] {
        if row.len() <= term_idx {
            continue;
        }
        let term = row[term_idx].trim();
        if term.is_empty() {
            if skipped.len() < PREVIEW_SKIP_EXAMPLE_CAP {
                skipped.push(row.join(", "));
            }
            continue;
        }
        let aliases_raw = aliases_idx
            .filter(|&i| i < row.len())
            .map(|i| row[i].as_str())
            .unwrap_or("");
        let pronunciation = pronunciation_idx
            .filter(|&i| i < row.len())
            .map(|i| row[i].trim())
            .unwrap_or("");
        let aliases = split_aliases(aliases_raw);
        words.push(build_word(term, pronunciation, aliases));
    }
    TableParse { words, skipped }
}

/// Parses a plain-text list: one term per line. Blank lines and `#` comment
/// lines are skipped.
///
/// **Term/alias split (documented choice):** the term is everything up to a
/// run of **two or more** consecutive whitespace characters; everything after
/// that run is the alias string (split on `;` or `|`). A single space keeps
/// the term whole, so multi-word terms like `VS Code` survive intact. A
/// pronunciation is not supported in this format and stays empty. A line
/// whose term is empty is recorded in `skipped`.
fn parse_text(lines: &[&str]) -> TableParse {
    let mut words = Vec::new();
    let mut skipped = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Split on the first run of 2+ whitespace: the term is the left part,
        // the alias string the right part. A single space never splits.
        let (term, rest) = match find_double_space_run(trimmed) {
            Some(pos) => (trimmed[..pos].trim(), trimmed[pos..].trim()),
            None => (trimmed, ""),
        };
        if term.is_empty() {
            if skipped.len() < PREVIEW_SKIP_EXAMPLE_CAP {
                skipped.push(trimmed.to_string());
            }
            continue;
        }
        let aliases = split_aliases(rest);
        words.push(build_word(term, "", aliases));
    }
    TableParse { words, skipped }
}

/// Returns the byte index of the first run of two or more consecutive
/// whitespace characters in `s`, or `None` if there is none. Used to separate
/// a multi-word term from its alias string in the plain-text format.
fn find_double_space_run(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut run = 0usize;
    let mut start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_whitespace() {
            if run == 0 {
                start = i;
            }
            run += 1;
            if run >= 2 {
                return Some(start);
            }
        } else {
            run = 0;
        }
    }
    None
}

/// Computes a non-mutating preview of importing `bytes` into `dict`.
///
/// The dictionary is read but never modified. The format is detected from the
/// content (falling back to `path_hint`), the file is parsed, in-file
/// duplicates (case-insensitive term) are removed, and the words are split
/// into `would_import` (new) and `would_update` (already present,
/// case-insensitive) against the current dictionary. `would_skip` counts
/// invalid rows (empty term) and in-file duplicates, with up to
/// [`PREVIEW_SKIP_EXAMPLE_CAP`] examples retained.
///
/// Empty input is a valid preview with zero words, not an error. A malformed
/// table (a header that names no usable term column) is an `Err`.
pub fn preview_dictionary_import(
    dict: &Dictionary,
    bytes: &[u8],
    path_hint: &str,
) -> Result<FileImportPreview, String> {
    let format = detect_dictionary_format(bytes, path_hint);
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().collect();

    // Parse per format into (words, skipped_rows). JSON has no row-level
    // skips; a malformed envelope is an Err in the same clear style as
    // `parse_export`.
    let (words, mut skipped) = match format.as_str() {
        "json" => (parse_export(bytes)?, Vec::new()),
        "csv" => {
            let t = parse_table(&lines, ',');
            (t.words, t.skipped)
        }
        "tsv" => {
            let t = parse_table(&lines, '\t');
            (t.words, t.skipped)
        }
        _ => {
            let t = parse_text(&lines);
            (t.words, t.skipped)
        }
    };

    // Deduplicate within the file (case-insensitive), counting duplicates as
    // skipped. `skipped` holds the first 10 invalid-row examples; duplicate
    // examples are appended up to the same cap.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut deduped: Vec<DictionaryWord> = Vec::new();
    let mut dup_count = 0usize;
    for w in words {
        let key = w.word.to_lowercase();
        if seen.insert(key) {
            deduped.push(w);
        } else {
            dup_count += 1;
            if skipped.len() < PREVIEW_SKIP_EXAMPLE_CAP {
                skipped.push(format!("duplicate: {}", w.word));
            }
        }
    }

    // Classify against the current dictionary (case-insensitive term match).
    let mut would_import = 0usize;
    let mut would_update = 0usize;
    for w in &deduped {
        if dict.find(&w.word).is_some() {
            would_update += 1;
        } else {
            would_import += 1;
        }
    }

    let invalid_count = skipped
        .iter()
        .filter(|s| !s.starts_with("duplicate:"))
        .count();
    let would_skip = invalid_count + dup_count;

    Ok(FileImportPreview {
        format,
        words: deduped,
        would_import,
        would_update,
        would_skip,
        skipped_examples: skipped,
    })
}

/// Applies a previously computed [`FileImportPreview`] by merging its words
/// into `dict`. Delegates to [`merge_words`], which re-checks every conflict
/// at apply time (a word that was new at preview time but added to the
/// dictionary before apply is treated as an update, not a duplicate import).
///
/// Returns the actual counts, which may differ from the preview if the
/// dictionary changed in between.
pub fn apply_dictionary_import(dict: &mut Dictionary, preview: FileImportPreview) -> ImportCounts {
    merge_words(dict, preview.words)
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
    /// The historical similarity floor. Named so the adjustable
    /// [`FloorChecker`] and the tests can refer to one number.
    pub const FLOOR: f32 = 0.80;

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
        probability_at(listed_word, as_written, fuzzy, Self::FLOOR)
    }

    fn cutoff(&self) -> f32 {
        Self::FLOOR
    }
}

/// The same edit-distance checker with a caller-chosen similarity floor.
///
/// Exists so the dictionary strictness setting has a real effect: the floor is
/// a dial the user controls, not a constant buried in this file. Constructed
/// per dictation from the setting, so it holds no shared mutable state.
///
/// [`EditDistanceChecker`] stays the zero-cost default and is unchanged at
/// 0.80; `strictness` of "standard" routes to it rather than restating the
/// number, so the default cannot drift away from what the code actually does.
pub struct FloorChecker {
    floor: f32,
}

impl FloorChecker {
    /// The similarity floor. Clamped to `(0, 1]` so a bad settings value
    /// cannot make every word match, or stop the checker matching anything.
    pub fn new(floor: f32) -> Self {
        Self {
            floor: if floor.is_finite() && floor > 0.0 && floor <= 1.0 {
                floor
            } else {
                EditDistanceChecker::FLOOR
            },
        }
    }
}

impl WordChecker for FloorChecker {
    fn probability(&self, listed_word: &str, as_written: &str, fuzzy: bool) -> f32 {
        probability_at(listed_word, as_written, fuzzy, self.floor)
    }

    fn cutoff(&self) -> f32 {
        self.floor
    }
}

/// The shared scoring rule, parameterised by its similarity floor.
///
/// Extracted so [`EditDistanceChecker`] and [`FloorChecker`] cannot drift apart
/// in how they score; only the floor differs.
fn probability_at(listed_word: &str, as_written: &str, fuzzy: bool, floor: f32) -> f32 {
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
    if max_len == 0 {
        return 0.0;
    }
    let sim = 1.0f32 - dist as f32 / max_len as f32;
    if sim >= floor {
        sim
    } else {
        0.0
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
    fn the_default_floor_is_unchanged_at_eighty_percent() {
        // Guards the claim in the docs: "standard" must behave exactly as it
        // did before match_strictness existed.
        assert_eq!(EDIT_DISTANCE_CHECKER.cutoff(), 0.80);
        assert_eq!(EditDistanceChecker::FLOOR, 0.80);
    }

    #[test]
    fn floor_checker_scoring_matches_the_default_at_the_default_floor() {
        let a = &EDIT_DISTANCE_CHECKER;
        let b = FloorChecker::new(EditDistanceChecker::FLOOR);
        for (listed, written) in [
            ("kubernetes", "kubernetes"),
            ("kubernetes", "kubernates"),
            ("kubernetes", "kubernetess"),
            ("postgres", "postgresql"),
            ("vaild", "valid"),
            ("abc", "xyz"),
        ] {
            assert_eq!(
                a.probability(listed, written, true),
                b.probability(listed, written, true),
                "scoring diverged at the default floor for {listed} vs {written}"
            );
        }
    }

    #[test]
    fn a_stricter_floor_rejects_matches_the_default_accepts() {
        // "kubernetess" -> "kubernetes" is a near miss the default approves.
        // Strict must refuse it; loose must not be looser than the default.
        let default = &EDIT_DISTANCE_CHECKER;
        let strict = FloorChecker::new(0.92);
        assert!(default.probability("kubernetes", "kubernetess", true) > 0.0);
        assert_eq!(strict.probability("kubernetes", "kubernetess", true), 0.0);
        assert_eq!(strict.cutoff(), 0.92);
    }

    #[test]
    fn an_exact_match_survives_any_floor() {
        // An exact match is not a fuzzy decision, so strictness must not be
        // able to lose a word the user actually said.
        for floor in [0.0, 0.5, 0.8, 0.92, 1.0] {
            let c = FloorChecker::new(floor);
            assert_eq!(c.probability("kubernetes", "kubernetes", true), 1.0);
        }
    }

    #[test]
    fn a_nonsense_floor_falls_back_instead_of_breaking_matching() {
        // A corrupt settings value must not make every word match (floor 0) or
        // stop any word matching (floor 99).
        for bad in [0.0, -1.0, 1.5, f32::NAN, f32::INFINITY] {
            assert_eq!(
                FloorChecker::new(bad).cutoff(),
                EditDistanceChecker::FLOOR,
                "bad floor {bad} should have fallen back"
            );
        }
    }

    #[test]
    fn fuzzy_off_is_unaffected_by_the_floor() {
        let c = FloorChecker::new(0.5);
        assert_eq!(c.probability("kubernetes", "kubernetess", false), 0.0);
    }

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
    let change = d.insert_learned("margets", "Markets");
    let w = d.find("Markets").expect("learned word missing");
    assert!(
        matches!(change, LearnedChange::Created { ref id, .. } if *id == w.id),
        "expected Created for the new word, got {change:?}"
    );
    assert_eq!(w.learned_from.as_deref(), Some("margets"));
    assert!(w.learned_at.is_some());
    assert!(w.fuzzy);
}

#[test]
fn insert_learned_records_the_misheard_form_as_an_alias() {
    // Provenance alone is only a badge. The alias is what makes the next
    // take correct the same mishearing, so learning must write both.
    let mut d = Dictionary::default();
    d.insert_learned("margets", "Markets");
    let w = d.find("Markets").unwrap();
    assert_eq!(w.aliases, vec!["margets".to_string()]);
}

#[test]
fn insert_learned_dedupes_a_repeated_mishearing() {
    let mut d = Dictionary::default();
    d.insert_learned("margets", "Markets");
    let change = d.insert_learned("Margets", "Markets");
    assert_eq!(change, LearnedChange::Unchanged);
    assert_eq!(
        d.find("Markets").unwrap().aliases,
        vec!["margets".to_string()]
    );
}

#[test]
fn insert_learned_adds_a_second_mishearing_as_an_alias() {
    let mut d = Dictionary::default();
    d.insert_learned("margets", "Markets");
    let change = d.insert_learned("mark its", "Markets");
    assert!(
        matches!(change, LearnedChange::AliasAdded { .. }),
        "expected AliasAdded, got {change:?}"
    );
    let w = d.find("Markets").unwrap();
    assert_eq!(
        w.aliases,
        vec!["margets".to_string(), "mark its".to_string()]
    );
    // The first mishearing keeps the provenance: a later sighting must not
    // rewrite where the word was first learned from.
    assert_eq!(w.learned_from.as_deref(), Some("margets"));
}

#[test]
fn insert_learned_backfills_existing_word_without_overwriting() {
    let mut d = Dictionary::default();
    let mut taught = DictionaryWord::new("Markets", "");
    taught.learned_from = Some("taught already".into());
    d.insert(taught).unwrap();
    let change = d.insert_learned("margets", "Markets");
    assert!(
        matches!(change, LearnedChange::AliasAdded { .. }),
        "expected AliasAdded, got {change:?}"
    );
    let w = d.find("Markets").unwrap();
    assert_eq!(w.learned_from.as_deref(), Some("taught already"));
    assert!(w.learned_at.is_none());
    assert_eq!(w.aliases, vec!["margets".to_string()]);
}

#[test]
fn insert_learned_backfills_word_without_provenance() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Markets", "")).unwrap();
    let change = d.insert_learned("margets", "Markets");
    assert!(
        matches!(
            change,
            LearnedChange::Backfilled {
                alias_added: true,
                ..
            }
        ),
        "expected Backfilled, got {change:?}"
    );
    let w = d.find("Markets").unwrap();
    assert_eq!(w.learned_from.as_deref(), Some("margets"));
    assert!(w.learned_at.is_some());
    assert_eq!(w.aliases, vec!["margets".to_string()]);
}

#[test]
fn insert_learned_rejects_empty_target() {
    let mut d = Dictionary::default();
    assert_eq!(d.insert_learned("margets", "  "), LearnedChange::Unchanged);
    assert!(d.words.is_empty());
}

#[test]
fn revert_learned_removes_the_word_learning_created() {
    let mut d = Dictionary::default();
    let change = d.insert_learned("margets", "Markets");
    assert!(d.revert_learned(&change));
    assert!(d.find("Markets").is_none());
    // A second undo of the same change is a no-op, not a panic or a delete
    // of some other word.
    assert!(!d.revert_learned(&change));
}

#[test]
fn revert_learned_clears_provenance_and_the_alias_it_added() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Markets", "")).unwrap();
    let change = d.insert_learned("margets", "Markets");
    assert!(d.revert_learned(&change));
    let w = d.find("Markets").expect("the word predated the learning");
    assert!(w.learned_from.is_none());
    assert!(w.learned_at.is_none());
    assert!(w.aliases.is_empty());
}

#[test]
fn revert_learned_keeps_a_user_taught_alias() {
    let mut d = Dictionary::default();
    let mut w = DictionaryWord::new("Markets", "");
    w.aliases = vec!["margets".to_string()];
    d.insert(w).unwrap();
    let change = d.insert_learned("margets", "Markets");
    assert!(
        matches!(
            change,
            LearnedChange::Backfilled {
                alias_added: false,
                ..
            }
        ),
        "the alias already existed, got {change:?}"
    );
    assert!(d.revert_learned(&change));
    let w = d.find("Markets").unwrap();
    assert!(w.learned_from.is_none());
    // Undo of a learning step must never delete what the user taught.
    assert_eq!(w.aliases, vec!["margets".to_string()]);
}

#[test]
fn revert_learned_removes_only_the_alias_it_added() {
    let mut d = Dictionary::default();
    d.insert_learned("margets", "Markets");
    let change = d.insert_learned("mark its", "Markets");
    assert!(d.revert_learned(&change));
    let w = d.find("Markets").unwrap();
    assert_eq!(w.aliases, vec!["margets".to_string()]);
    assert_eq!(w.learned_from.as_deref(), Some("margets"));
    assert!(w.learned_at.is_some());
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

#[test]
fn update_replaces_word_and_preserves_id() {
    let mut d = Dictionary::default();
    let mut w = DictionaryWord::new("Teletype", "");
    w.aliases.push("tel-uh-type".into());
    d.insert(w.clone()).unwrap();
    let id = w.id.clone();
    let mut edited = w;
    edited.word = "Teletype Studio".into();
    // edited.id is still the stored id (the client echoes it back); the
    // update must succeed and keep exactly that id.
    assert_eq!(edited.id, id);
    assert!(d.update(edited.clone()).is_some());
    let found = d.find("Teletype Studio").unwrap();
    assert_eq!(found.id, id, "update must keep the original id");
    assert_eq!(found.aliases, vec!["tel-uh-type".to_string()]);

    // A hostile client that tries to rename the word's identity is rejected:
    // no word carries the attacker id, so the update is a no-op.
    let mut hostile = edited;
    hostile.id = "attacker-id".into();
    assert!(d.update(hostile).is_none());
    assert!(d.get("attacker-id").is_none());
    assert_eq!(d.find("Teletype Studio").unwrap().id, id);
}

#[test]
fn update_missing_word_is_false() {
    let mut d = Dictionary::default();
    let w = DictionaryWord::new("Ghost", "");
    assert!(d.update(w).is_none());
}

#[test]
fn legacy_json_without_aliases_or_usage_still_loads() {
    let d: Dictionary =
            serde_json::from_str(r#"{"words":[{"id":"1","word":"API","pronunciation":"","createdAt":0,"fuzzy":false}],"builtinVersion":2}"#)
                .unwrap();
    assert!(d.words[0].aliases.is_empty());
    assert_eq!(d.words[0].usage_count, 0);
}

#[test]
fn aliases_and_usage_survive_json_roundtrip() {
    let mut d = Dictionary::default();
    let mut w = DictionaryWord::new("Claude", "");
    w.aliases = vec!["clod".into(), "clawed".into()];
    w.usage_count = 7;
    d.insert(w).unwrap();
    let json = serde_json::to_string(&d).unwrap();
    let back: Dictionary = serde_json::from_str(&json).unwrap();
    let w = back.find("Claude").unwrap();
    assert_eq!(w.aliases, vec!["clod".to_string(), "clawed".to_string()]);
    assert_eq!(w.usage_count, 7);
}

#[test]
fn record_usage_counts_word_and_alias_hits() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Teletype", "")).unwrap();
    let mut alias_word = DictionaryWord::new("Claude", "");
    alias_word.aliases = vec!["clod".into()];
    d.insert(alias_word).unwrap();
    d.insert(DictionaryWord::new("Unused", "")).unwrap();

    assert_eq!(d.record_usage("I love teletype and clod"), 2);
    assert_eq!(d.find("Teletype").unwrap().usage_count, 1);
    assert_eq!(d.find("Claude").unwrap().usage_count, 1);
    assert_eq!(d.find("Unused").unwrap().usage_count, 0);

    // Idempotent per call: a second take counts again.
    assert_eq!(d.record_usage("teletype again"), 1);
    assert_eq!(d.find("Teletype").unwrap().usage_count, 2);
}

#[test]
fn record_usage_is_case_insensitive_and_punctuation_aware() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("VS Code", "")).unwrap();
    assert_eq!(d.record_usage("use VS Code, please!"), 1);
    assert_eq!(d.find("VS Code").unwrap().usage_count, 1);
}

// ---- Phase 7.6: file import formats + preview ----

#[test]
fn detect_format_json_by_content() {
    assert_eq!(
        detect_dictionary_format(b"{\"version\":1,\"words\":[]}", "anything.xyz"),
        "json"
    );
    assert_eq!(detect_dictionary_format(b"  {", "x"), "json");
}

#[test]
fn detect_format_csv_and_tsv_by_content() {
    assert_eq!(
        detect_dictionary_format(b"term,aliases\nFoo,bar", "f.xyz"),
        "csv"
    );
    assert_eq!(
        detect_dictionary_format(b"term\taliases\nFoo\tbar", "f.xyz"),
        "tsv"
    );
}

#[test]
fn detect_format_text_default_and_extension_fallback() {
    // No delimiter, not JSON → text regardless of a non-matching hint.
    assert_eq!(
        detect_dictionary_format(b"just a word\nanother", "f.xyz"),
        "text"
    );
    // Extension fallback when content is inconclusive (no delimiter, not JSON).
    assert_eq!(detect_dictionary_format(b"just a word", "f.csv"), "csv");
    assert_eq!(detect_dictionary_format(b"just a word", "f.tsv"), "tsv");
    assert_eq!(detect_dictionary_format(b"just a word", "f.json"), "json");
    // Default is text.
    assert_eq!(detect_dictionary_format(b"word", "f.bin"), "text");
    assert_eq!(detect_dictionary_format(b"", "f.bin"), "text");
}

#[test]
fn preview_csv_positional_no_header() {
    let d = Dictionary::default();
    let bytes = b"Foo,bar;baz\nQux,\n";
    let p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p.format, "csv");
    assert_eq!(p.words.len(), 2);
    assert_eq!(p.words[0].word, "Foo");
    assert_eq!(p.words[0].aliases, vec!["bar", "baz"]);
    assert_eq!(p.words[1].word, "Qux");
    assert_eq!(p.words[1].aliases, Vec::<String>::new());
    assert_eq!(p.would_import, 2);
    assert_eq!(p.would_update, 0);
    assert_eq!(p.would_skip, 0);
}

#[test]
fn preview_csv_with_header_named_columns() {
    let d = Dictionary::default();
    let bytes = b"word,aliases,pronunciation\nFoo,bar|baz,tel-uh-type\n";
    let p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p.format, "csv");
    assert_eq!(p.words.len(), 1);
    assert_eq!(p.words[0].word, "Foo");
    assert_eq!(p.words[0].aliases, vec!["bar", "baz"]);
    assert_eq!(p.words[0].pronunciation, "tel-uh-type");
    assert_eq!(p.would_import, 1);
}

#[test]
fn preview_csv_header_reordered_columns() {
    // term column is "phrase", aliases under "alts", pronunciation under "phonetic".
    let d = Dictionary::default();
    let bytes = b"pronunciation,phrase,alts\nphon,My Term,a;b\n";
    let p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p.words.len(), 1);
    assert_eq!(p.words[0].word, "My Term");
    assert_eq!(p.words[0].aliases, vec!["a", "b"]);
    assert_eq!(p.words[0].pronunciation, "phon");
}

#[test]
fn preview_tsv_positional() {
    let d = Dictionary::default();
    let bytes = b"Foo\tbar;baz\nQux\t\n";
    let p = preview_dictionary_import(&d, bytes, "f.tsv").unwrap();
    assert_eq!(p.format, "tsv");
    assert_eq!(p.words.len(), 2);
    assert_eq!(p.words[0].word, "Foo");
    assert_eq!(p.words[0].aliases, vec!["bar", "baz"]);
}

#[test]
fn preview_csv_quoted_cell_with_comma_and_escaped_quote() {
    let d = Dictionary::default();
    // Term has an embedded comma (quoted); aliases cell has an escaped quote.
    let bytes = b"\"My, Term\",\"a\"\"b\"|c\n";
    let p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p.words.len(), 1);
    assert_eq!(p.words[0].word, "My, Term");
    assert_eq!(p.words[0].aliases, vec!["a\"b", "c"]);
}

#[test]
fn preview_text_with_comments_blanks_trailing_spaces() {
    let d = Dictionary::default();
    let bytes = b"# a comment\n\nTeletype   \n   \nVS Code\n";
    let p = preview_dictionary_import(&d, bytes, "f.txt").unwrap();
    assert_eq!(p.format, "text");
    assert_eq!(p.words.len(), 2);
    assert_eq!(p.words[0].word, "Teletype");
    assert_eq!(p.words[1].word, "VS Code");
    assert_eq!(p.would_skip, 0);
}

#[test]
fn preview_text_unicode_terms_preserved() {
    let d = Dictionary::default();
    let bytes = "café\nnaïve\nZoë\n".as_bytes();
    let p = preview_dictionary_import(&d, bytes, "f.txt").unwrap();
    assert_eq!(p.words.len(), 3);
    assert_eq!(p.words[0].word, "café");
    assert_eq!(p.words[1].word, "naïve");
    assert_eq!(p.words[2].word, "Zoë");
}

#[test]
fn preview_text_alias_after_double_whitespace_run() {
    // Documented behavior: term/alias split on a run of 2+ whitespace.
    // Single spaces keep the term whole (multi-word terms survive).
    let d = Dictionary::default();
    let bytes = b"Markets  alt1;alt2\n";
    let p = preview_dictionary_import(&d, bytes, "f.txt").unwrap();
    assert_eq!(p.words.len(), 1);
    assert_eq!(p.words[0].word, "Markets");
    assert_eq!(p.words[0].aliases, vec!["alt1", "alt2"]);

    // Single-space multi-word term: no 2+ run → whole line is the term.
    let bytes2 = b"VS Code\n";
    let p2 = preview_dictionary_import(&d, bytes2, "f.txt").unwrap();
    assert_eq!(p2.words.len(), 1);
    assert_eq!(p2.words[0].word, "VS Code");
    assert_eq!(p2.words[0].aliases, Vec::<String>::new());
}

#[test]
fn preview_alias_splitting_semicolon_and_pipe() {
    let d = Dictionary::default();
    // CSV: comma separates term from the aliases cell; the aliases cell
    // "x;y|z" is split on ; and |.
    let csv = b"a,x;y|z\n";
    let p = preview_dictionary_import(&d, csv, "f.csv").unwrap();
    assert_eq!(p.words[0].word, "a");
    assert_eq!(p.words[0].aliases, vec!["x", "y", "z"]);
}

#[test]
fn preview_duplicate_rows_counted_once() {
    let d = Dictionary::default();
    // "Foo" appears three times (case-insensitive) → 1 word, 2 skipped dups.
    let bytes = b"Foo,1\nfoo,2\nFOO,3\n";
    let p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p.words.len(), 1);
    assert_eq!(p.words[0].word, "Foo");
    assert_eq!(p.words[0].aliases, vec!["1"]);
    assert_eq!(p.would_skip, 2);
    assert_eq!(p.would_import, 1);
}

#[test]
fn preview_does_not_mutate_dictionary() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Existing", "old")).unwrap();
    let before_len = d.words.len();
    let before: Vec<(String, String)> = d
        .words
        .iter()
        .map(|w| (w.word.clone(), w.pronunciation.clone()))
        .collect();

    let bytes = b"BrandNew,alias1\nExisting,newpron\n";
    let _p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();

    assert_eq!(d.words.len(), before_len, "preview must not add words");
    let after: Vec<(String, String)> = d
        .words
        .iter()
        .map(|w| (w.word.clone(), w.pronunciation.clone()))
        .collect();
    assert_eq!(before, after, "preview must not change existing words");
}

#[test]
fn preview_counts_new_update_skip_and_examples() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Existing", "")).unwrap();

    // "Existing" → update; "NewWord" → import; empty-term row → skip;
    // "Dup" twice → 1 import + 1 dup skip.
    let bytes = b"Existing\nNewWord\n\nDup\nDup\n";
    let p = preview_dictionary_import(&d, bytes, "f.txt").unwrap();
    assert_eq!(p.would_import, 2, "NewWord + Dup");
    assert_eq!(p.would_update, 1, "Existing");
    assert_eq!(p.would_skip, 1, "one in-file duplicate of Dup");
    // skipped_examples should include the duplicate.
    assert!(
        p.skipped_examples.iter().any(|s| s.contains("Dup")),
        "expected a duplicate example, got {:?}",
        p.skipped_examples
    );
}

#[test]
fn preview_empty_term_rows_skipped_with_examples() {
    let d = Dictionary::default();
    // A row whose term cell is empty is skipped and recorded.
    let bytes = b",orphan-alias\nGood,ok\n";
    let p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p.words.len(), 1);
    assert_eq!(p.words[0].word, "Good");
    assert_eq!(p.would_skip, 1);
    assert_eq!(p.skipped_examples.len(), 1);
}

#[test]
fn preview_empty_input_is_zero_words_not_error() {
    let d = Dictionary::default();
    for (bytes, hint) in [
        (b"" as &[u8], "f.csv"),
        (b"\n\n" as &[u8], "f.txt"),
        (b"   \n" as &[u8], "f.csv"),
    ] {
        let p = preview_dictionary_import(&d, bytes, hint).unwrap();
        assert!(p.words.is_empty(), "hint {hint}");
        assert_eq!(p.would_import, 0);
        assert_eq!(p.would_update, 0);
        assert_eq!(p.would_skip, 0);
    }
}

#[test]
fn apply_merges_imported_and_updated() {
    let mut d = Dictionary::default();
    d.insert(DictionaryWord::new("Existing", "old-pron"))
        .unwrap();

    // Positional CSV: col0=term, col1=aliases, col2=pronunciation.
    // "Existing" conflicts (update, import wins on pronunciation);
    // "BrandNew" is new (import).
    let bytes = b"Existing,,new-pron\nBrandNew,al,\n";
    let p = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p.would_update, 1);
    assert_eq!(p.would_import, 1);

    let counts = apply_dictionary_import(&mut d, p);
    assert_eq!(counts.imported, 1);
    assert_eq!(counts.updated, 1);
    // Conflict: import wins on pronunciation.
    assert_eq!(d.find("existing").unwrap().pronunciation, "new-pron");
    assert!(d.find("brandnew").is_some());
}

#[test]
fn second_apply_is_idempotent_at_dictionary_level() {
    let mut d = Dictionary::default();
    let bytes = b"Alpha,a1\nBeta,b1\n";
    let p1 = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    let c1 = apply_dictionary_import(&mut d, p1);
    assert_eq!(c1.imported, 2);
    assert_eq!(c1.updated, 0);

    // Re-apply the same preview: both words now exist → updates, no new.
    let p2 = preview_dictionary_import(&d, bytes, "f.csv").unwrap();
    assert_eq!(p2.would_import, 0);
    assert_eq!(p2.would_update, 2);
    let c2 = apply_dictionary_import(&mut d, p2);
    assert_eq!(c2.imported, 0);
    assert_eq!(c2.updated, 2);
    // Dictionary still has exactly the two words.
    assert_eq!(d.words.len(), 2);
}

#[test]
fn preview_json_path_still_works() {
    let d = Dictionary::default();
    let json = serde_json::json!({
        "version": 1,
        "words": [
            {"id":"j1","word":"JsonWord","pronunciation":"jp","createdAt":1,"fuzzy":true}
        ]
    })
    .to_string();
    let p = preview_dictionary_import(&d, json.as_bytes(), "f.json").unwrap();
    assert_eq!(p.format, "json");
    assert_eq!(p.words.len(), 1);
    assert_eq!(p.words[0].word, "JsonWord");
    assert_eq!(p.would_import, 1);
}

#[test]
fn preview_json_wrong_version_is_err() {
    let d = Dictionary::default();
    let json = r#"{"version":99,"words":[]}"#;
    let err = preview_dictionary_import(&d, json.as_bytes(), "f.json").unwrap_err();
    assert!(err.contains("version"), "got: {err}");
}

#[test]
fn preview_malformed_json_is_err() {
    let d = Dictionary::default();
    let err = preview_dictionary_import(&d, b"{not json", "f.json").unwrap_err();
    assert!(err.contains("Not a valid dictionary export"), "got: {err}");
}
