//! AutoText: deterministic text expansion. No LLM involved.
//!
//! A trigger like `/email` expands to the user's configured value. Values are
//! **protected** around AI transforms: they are replaced with
//! `{{AUTOTEXT_n}}` placeholders before inference and restored verbatim after,
//! so a model can never alter an email address, phone number or signature.

pub mod disambiguate;
pub mod expand;
pub mod placeholders;
pub mod protect;
pub mod system;

use serde::{Deserialize, Serialize};

/// Where an entry applies.
///
/// Serialized as a plain string: `"everywhere"` for the default, or the app
/// id (e.g. `"com.google.gmail"`) to scope it to that application. This keeps
/// the frontend trivial — a scope is just a text field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", try_from = "String", into = "String")]
pub enum AutoTextScope {
    #[default]
    Everywhere,
    /// Only in this application (matched against `ApplicationContext::application_id`,
    /// case-insensitively, or against the lowercased app name).
    Application(String),
}

impl TryFrom<String> for AutoTextScope {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let v = value.trim();
        if v.is_empty() || v.eq_ignore_ascii_case("everywhere") {
            Ok(AutoTextScope::Everywhere)
        } else {
            Ok(AutoTextScope::Application(v.to_string()))
        }
    }
}

impl From<AutoTextScope> for String {
    fn from(scope: AutoTextScope) -> Self {
        match scope {
            AutoTextScope::Everywhere => "everywhere".to_string(),
            AutoTextScope::Application(id) => id,
        }
    }
}

fn default_enabled() -> bool {
    true
}

/// One AutoText entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTextEntry {
    #[serde(default)]
    pub id: String,
    /// The trigger, including the leading `/`.
    pub trigger: String,
    /// The exact replacement text. Sensitive — never log it.
    pub replacement: String,
    #[serde(default)]
    pub description: String,
    /// Defaults to enabled, not to `false`: an entry written before this
    /// field existed was usable, and defaulting it to disabled would make the
    /// user's whole library look like it had silently vanished.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Defaults to `Everywhere`, which is what an entry written before scopes
    /// existed meant. Left required on purpose in every other struct: a
    /// missing `trigger` or `replacement` is real corruption, and failing
    /// loudly is better than expanding a trigger to nothing.
    #[serde(default)]
    pub scope: AutoTextScope,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    /// Optional spoken phrase (e.g. "my email"). When set, saying this phrase
    /// in a dictation expands to `replacement`. Empty means typed-only.
    #[serde(default)]
    pub snippet: String,
    /// True when this is a built-in System AutoText entry (shipped with
    /// Teletype, not user-editable). Custom entries override System entries
    /// with the same trigger/phrase. Defaults to false for stored entries.
    #[serde(default)]
    pub system: bool,
}

impl AutoTextEntry {
    pub fn new(trigger: impl Into<String>, replacement: impl Into<String>) -> Self {
        let now = crate::storage::now_ms();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            trigger: trigger.into(),
            replacement: replacement.into(),
            description: String::new(),
            enabled: true,
            scope: AutoTextScope::default(),
            created_at: now,
            updated_at: now,
            snippet: String::new(),
            system: false,
        }
    }

    /// The spoken phrase to match, trimmed. Empty when unset.
    pub fn snippet_phrase(&self) -> &str {
        self.snippet.trim()
    }

    pub fn applies_to(&self, app: &crate::context::ApplicationContext) -> bool {
        if !self.enabled {
            return false;
        }
        match &self.scope {
            AutoTextScope::Everywhere => true,
            AutoTextScope::Application(app_id) => {
                let want = app_id.to_ascii_lowercase();
                let id = app.application_id.to_ascii_lowercase();
                let name = app.application_name.to_ascii_lowercase();
                // Match on exact id, exact name, or name as a component of the id.
                id == want
                    || name == want
                    || want.split('.').any(|part| !part.is_empty() && name == part)
            }
        }
    }
}

/// Trigger validation rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerError {
    Empty,
    MustStartWithSlash,
    TooLong { max: usize },
    InvalidCharacter { c: char },
}

impl std::fmt::Display for TriggerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TriggerError::Empty => write!(f, "Trigger must not be empty"),
            TriggerError::MustStartWithSlash => write!(f, "Trigger must start with /"),
            TriggerError::TooLong { max } => write!(f, "Trigger too long (max {max} chars)"),
            TriggerError::InvalidCharacter { c } => write!(f, "Trigger may not contain '{c}'"),
        }
    }
}

pub const MAX_TRIGGER_LEN: usize = 64;

/// Validates a trigger: `/` + letters/digits/`_`/`-`, at most 64 chars.
pub fn validate_trigger(trigger: &str) -> Result<(), TriggerError> {
    if trigger.is_empty() {
        return Err(TriggerError::Empty);
    }
    if !trigger.starts_with('/') {
        return Err(TriggerError::MustStartWithSlash);
    }
    if trigger.len() > MAX_TRIGGER_LEN {
        return Err(TriggerError::TooLong {
            max: MAX_TRIGGER_LEN,
        });
    }
    for c in trigger.chars().skip(1) {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(TriggerError::InvalidCharacter { c });
        }
    }
    Ok(())
}

pub const MAX_SNIPPET_LEN: usize = 128;

/// Validates a spoken snippet phrase (e.g. "my email").
///
/// A snippet is a free-form phrase of 1..=8 words of letters/digits/apostrophes,
/// with no leading `/` (that's the typed-trigger namespace) and no slashes.
/// Empty is allowed (means "no snippet").
pub fn validate_snippet(snippet: &str) -> Result<(), String> {
    let s = snippet.trim();
    if s.is_empty() {
        return Ok(());
    }
    if s.contains('/') {
        return Err("Snippet may not contain '/'".into());
    }
    if s.len() > MAX_SNIPPET_LEN {
        return Err(format!("Snippet too long (max {MAX_SNIPPET_LEN} chars)"));
    }
    let words: Vec<&str> = s
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\''))
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return Err("Snippet must contain at least one word".into());
    }
    if words.len() > 8 {
        return Err("Snippet must be at most 8 words".into());
    }
    for w in &words {
        if !w.chars().all(|c| c.is_alphanumeric() || c == '\'') {
            return Err(format!("Snippet word '{w}' has invalid characters"));
        }
    }
    Ok(())
}

/// The stored collection of entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTextStore {
    pub entries: Vec<AutoTextEntry>,
}

impl AutoTextStore {
    /// Adds `entry`, rejecting duplicate triggers (case-insensitive).
    pub fn insert(&mut self, entry: AutoTextEntry) -> Result<(), String> {
        let trigger = entry.trigger.to_ascii_lowercase();
        if self
            .entries
            .iter()
            .any(|e| e.trigger.to_ascii_lowercase() == trigger)
        {
            return Err(format!("A trigger '{trigger}' already exists"));
        }
        self.entries.push(entry);
        Ok(())
    }

    pub fn update(&mut self, entry: AutoTextEntry) -> Result<(), String> {
        let trigger = entry.trigger.to_ascii_lowercase();
        let pos = self
            .entries
            .iter()
            .position(|e| e.id == entry.id)
            .ok_or("Entry not found")?;
        if self.entries[pos].trigger.to_ascii_lowercase() != trigger {
            let dup = self
                .entries
                .iter()
                .any(|o| o.id != entry.id && o.trigger.to_ascii_lowercase() == trigger);
            if dup {
                return Err(format!("A trigger '{trigger}' already exists"));
            }
        }
        let mut updated = entry;
        updated.updated_at = crate::storage::now_ms();
        self.entries[pos] = updated;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&AutoTextEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn find_by_trigger(&self, trigger: &str) -> Option<&AutoTextEntry> {
        let t = trigger.to_ascii_lowercase();
        self.entries
            .iter()
            .find(|e| e.trigger.to_ascii_lowercase() == t)
    }

    /// Entries that have a spoken snippet and apply in `app`, longest phrase
    /// first (so "my company email" wins over "my email").
    pub fn snippets_for(&self, app: &crate::context::ApplicationContext) -> Vec<&AutoTextEntry> {
        let mut v: Vec<&AutoTextEntry> = self
            .entries
            .iter()
            .filter(|e| e.applies_to(app) && !e.snippet.trim().is_empty())
            .collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.snippet.trim().len()));
        v
    }
}

// ---- Import / export ----

/// Version of the AutoText import/export envelope. Bump when the shape
/// changes; older versions are rejected with a clear error.
pub const AUTOTEXT_EXPORT_VERSION: u32 = 1;

/// The JSON envelope for AutoText import/export:
/// `{"version": 1, "entries": [...]}` in the stored camelCase shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTextExport {
    pub version: u32,
    pub entries: Vec<AutoTextEntry>,
}

/// Outcome of an import: how many entries were newly added, how many existing
/// triggers were updated (the import wins), and how many rows were skipped.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoTextImportCounts {
    pub imported: usize,
    pub updated: usize,
    pub skipped: usize,
}

/// Serializes the store to the versioned export envelope as pretty JSON.
pub fn export_to_json(store: &AutoTextStore) -> Result<String, String> {
    let doc = AutoTextExport {
        version: AUTOTEXT_EXPORT_VERSION,
        entries: store.entries.clone(),
    };
    serde_json::to_string_pretty(&doc).map_err(|e| format!("Couldn't serialize AutoText: {e}"))
}

/// Writes the store to `path` as the versioned export envelope.
pub fn export_to_file(store: &AutoTextStore, path: &str) -> Result<(), String> {
    let json = export_to_json(store)?;
    std::fs::write(path, json).map_err(|e| format!("Couldn't write {path}: {e}"))
}

/// Parses the export envelope from `bytes`, validating the version and the
/// envelope shape. A bad file is an error, so the caller never mutates the
/// store on corrupt input.
pub fn parse_export(bytes: &[u8]) -> Result<Vec<AutoTextEntry>, String> {
    let doc: AutoTextExport =
        serde_json::from_slice(bytes).map_err(|e| format!("Not a valid AutoText export: {e}"))?;
    if doc.version != AUTOTEXT_EXPORT_VERSION {
        return Err(format!(
            "Unsupported AutoText export version {} (expected {})",
            doc.version, AUTOTEXT_EXPORT_VERSION
        ));
    }
    Ok(doc.entries)
}

/// Merges `incoming` entries into `store` (import wins on a trigger conflict,
/// case-insensitive). Rows with an invalid trigger or snippet, an empty
/// replacement, or the built-in `system` flag are skipped, as are duplicates
/// within the file. Existing entries not in the file stay untouched.
pub fn merge_entries(
    store: &mut AutoTextStore,
    incoming: Vec<AutoTextEntry>,
) -> AutoTextImportCounts {
    let mut counts = AutoTextImportCounts::default();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let now = crate::storage::now_ms();
    for mut entry in incoming {
        entry.trigger = entry.trigger.trim().to_string();
        entry.snippet = entry.snippet.trim().to_string();
        entry.replacement = entry.replacement.trim().to_string();
        let usable = !entry.system
            && validate_trigger(&entry.trigger).is_ok()
            && validate_snippet(&entry.snippet).is_ok()
            && !entry.replacement.is_empty();
        // Validate first: an invalid row must not shadow a valid one with the
        // same trigger later in the file.
        if !usable || !seen.insert(entry.trigger.to_ascii_lowercase()) {
            counts.skipped += 1;
            continue;
        }
        entry.system = false;
        entry.updated_at = now;
        if let Some(pos) = store
            .entries
            .iter()
            .position(|e| e.trigger.eq_ignore_ascii_case(&entry.trigger))
        {
            // Conflict: import wins on content, but the stored identity and
            // creation time stay so open editors and usage counters keep
            // pointing at the same row.
            entry.id = store.entries[pos].id.clone();
            entry.created_at = store.entries[pos].created_at;
            store.entries[pos] = entry;
            counts.updated += 1;
        } else {
            if entry.id.trim().is_empty() || store.entries.iter().any(|e| e.id == entry.id) {
                entry.id = uuid::Uuid::new_v4().to_string();
            }
            if entry.created_at == 0 {
                entry.created_at = now;
            }
            store.entries.push(entry);
            counts.imported += 1;
        }
    }
    counts
}

/// Reads the file at `path`, parses the export envelope, merges into `store`,
/// and returns the counts. On any parse/IO error the store is left untouched.
pub fn import_from_file(
    store: &mut AutoTextStore,
    path: &str,
) -> Result<AutoTextImportCounts, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {path}: {e}"))?;
    let entries = parse_export(&bytes)?;
    Ok(merge_entries(store, entries))
}

/// Returns the 1-based end index (in `text`) where the snippet phrase
/// `phrase` (lowercased, whitespace-normalized) starts at `start_idx`,
/// matching case-insensitively on whole words. The character after the match
/// must be a non-alphanumeric boundary. Returns `None` if it doesn't match.
pub fn match_snippet_at(text: &str, start_idx: usize, phrase: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let phrase_lower = phrase.to_lowercase();
    let words: Vec<String> = phrase_lower.split_whitespace().map(String::from).collect();
    if words.is_empty() {
        return None;
    }

    let is_boundary_before = |i: usize| i == 0 || !chars[i - 1].is_alphanumeric();

    // The first word must start at `start_idx` (which must itself be a
    // word boundary in the text).
    if !is_boundary_before(start_idx) {
        return None;
    }

    let mut i = start_idx;
    for (wi, word) in words.iter().enumerate() {
        // Between words, skip whitespace and an optional comma. The first word
        // must start exactly at start_idx (no skip), preserving the boundary.
        if wi > 0 {
            while i < chars.len() && (chars[i].is_whitespace() || chars[i] == ',') {
                i += 1;
            }
        }
        let word_chars: Vec<char> = word.chars().collect();
        let wlen = word_chars.len();
        if i + wlen > chars.len() {
            return None;
        }
        let mut ok = true;
        for k in 0..wlen {
            let tc = chars[i + k];
            if !tc.is_alphanumeric() && tc != '\'' {
                ok = false;
                break;
            }
            let want = word_chars[k];
            if tc.to_ascii_lowercase() != want {
                ok = false;
                break;
            }
        }
        if !ok {
            return None;
        }
        i += wlen;
        // Every word (including the last) must end at a hard boundary:
        // whitespace, a punctuation mark, or end-of-text. This prevents a
        // snippet from matching as a prefix of a longer phrase ("my email"
        // must not fire inside "my email address").
        if i < chars.len() && chars[i].is_alphanumeric() {
            return None;
        }
    }
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ApplicationContext;

    fn ctx(id: &str, name: &str) -> ApplicationContext {
        ApplicationContext {
            application_id: id.into(),
            application_name: name.into(),
            ..Default::default()
        }
    }

    #[test]
    fn match_snippet_at_boundary() {
        // Exact phrase followed by whitespace/punct/end -> match.
        assert_eq!(match_snippet_at("my email please", 0, "my email"), Some(8));
        assert_eq!(match_snippet_at("my email.", 0, "my email"), Some(8));
        assert_eq!(match_snippet_at("my email", 0, "my email"), Some(8));
        // Not at a word boundary -> no match.
        assert_eq!(match_snippet_at("xmy email", 1, "my email"), None);
        // A longer word containing the phrase's last word -> no match
        // ("emailish" is not "email").
        assert_eq!(match_snippet_at("my emailish", 0, "my email"), None);
    }

    #[test]
    fn validate_triggers() {
        assert!(validate_trigger("/email").is_ok());
        assert!(validate_trigger("/my-signature_2").is_ok());
        assert_eq!(
            validate_trigger("email"),
            Err(TriggerError::MustStartWithSlash)
        );
        assert_eq!(validate_trigger(""), Err(TriggerError::Empty));
        assert_eq!(
            validate_trigger("/a b"),
            Err(TriggerError::InvalidCharacter { c: ' ' })
        );
        assert!(matches!(
            validate_trigger(&format!("/{}", "x".repeat(70))),
            Err(TriggerError::TooLong { .. })
        ));
    }

    #[test]
    fn duplicate_triggers_rejected() {
        let mut store = AutoTextStore::default();
        store.insert(AutoTextEntry::new("/email", "a@b.c")).unwrap();
        assert!(store.insert(AutoTextEntry::new("/EMAIL", "x@y.z")).is_err());
        // Updating the same entry to its own trigger is fine.
        let entry = store.entries[0].clone();
        store.update(entry).unwrap();
    }

    #[test]
    fn scope_everywhere_applies_everywhere() {
        let entry = AutoTextEntry::new("/name", "Ali");
        assert!(entry.applies_to(&ctx("com.google.gmail", "Gmail")));
        assert!(entry.applies_to(&ApplicationContext::unknown()));
    }

    #[test]
    fn scope_application_matches_id_or_name() {
        let mut entry = AutoTextEntry::new("/sig", "Best regards");
        entry.scope = AutoTextScope::Application("com.google.gmail".into());
        assert!(entry.applies_to(&ctx("com.google.gmail", "Gmail")));
        assert!(entry.applies_to(&ctx("", "Gmail")));
        assert!(!entry.applies_to(&ctx("Slack", "Slack")));
    }

    #[test]
    fn disabled_entry_never_applies() {
        let mut entry = AutoTextEntry::new("/x", "y");
        entry.enabled = false;
        assert!(!entry.applies_to(&ApplicationContext::unknown()));
    }

    // ---- Import / export ----

    fn import_from_bytes(
        store: &mut AutoTextStore,
        bytes: &[u8],
    ) -> Result<AutoTextImportCounts, String> {
        let entries = parse_export(bytes)?;
        Ok(merge_entries(store, entries))
    }

    #[test]
    fn export_import_roundtrip_is_lossless() {
        let mut store = AutoTextStore::default();
        let mut e = AutoTextEntry::new("/email", "a@b.c");
        e.description = "Personal".into();
        e.snippet = "my email".into();
        e.scope = AutoTextScope::Application("com.google.gmail".into());
        e.enabled = false;
        store.insert(e).unwrap();
        store.insert(AutoTextEntry::new("/sig", "Best")).unwrap();

        let json = export_to_json(&store).unwrap();
        assert!(json.contains("\"version\": 1"), "missing version: {json}");
        assert!(json.contains("\"entries\":"), "missing entries: {json}");

        let mut imported = AutoTextStore::default();
        let counts = import_from_bytes(&mut imported, json.as_bytes()).unwrap();
        assert_eq!((counts.imported, counts.updated, counts.skipped), (2, 0, 0));
        let e = imported.find_by_trigger("/email").expect("missing /email");
        assert_eq!(e.replacement, "a@b.c");
        assert_eq!(e.description, "Personal");
        assert_eq!(e.snippet, "my email");
        assert_eq!(
            e.scope,
            AutoTextScope::Application("com.google.gmail".into())
        );
        assert!(!e.enabled);
    }

    #[test]
    fn import_merge_updates_an_existing_trigger_and_keeps_its_identity() {
        let mut store = AutoTextStore::default();
        let mut existing = AutoTextEntry::new("/email", "old@x.y");
        existing.created_at = 111;
        store.insert(existing).unwrap();
        let id = store.find_by_trigger("/email").unwrap().id.clone();

        let json = serde_json::json!({
            "version": 1,
            "entries": [{
                "id": "other",
                "trigger": "/EMAIL",
                "replacement": "new@x.y",
                "description": "Work",
                "enabled": false,
                "scope": "everywhere",
                "createdAt": 999,
                "updatedAt": 999,
                "snippet": "work email"
            }]
        })
        .to_string();

        let counts = import_from_bytes(&mut store, json.as_bytes()).unwrap();
        assert_eq!((counts.imported, counts.updated, counts.skipped), (0, 1, 0));
        let e = store.find_by_trigger("/email").unwrap();
        assert_eq!(e.id, id, "an update keeps the existing identity");
        assert_eq!(
            e.created_at, 111,
            "an update keeps the original created time"
        );
        assert_eq!(e.replacement, "new@x.y");
        assert_eq!(e.description, "Work");
        assert!(!e.enabled);
        assert_eq!(e.snippet, "work email");
    }

    #[test]
    fn import_skips_invalid_system_and_duplicate_rows() {
        let mut store = AutoTextStore::default();
        let json = serde_json::json!({
            "version": 1,
            "entries": [
                {"trigger": "email", "replacement": "no slash"},
                {"trigger": "/ok", "replacement": "   "},
                {"trigger": "/sys", "replacement": "x", "system": true},
                {"trigger": "/dup", "replacement": "first"},
                {"trigger": "/DUP", "replacement": "second"},
                {"trigger": "/good", "replacement": "yes"}
            ]
        })
        .to_string();

        let counts = import_from_bytes(&mut store, json.as_bytes()).unwrap();
        assert_eq!(
            (counts.imported, counts.updated, counts.skipped),
            (2, 0, 4),
            "only /dup and /good are usable"
        );
        assert_eq!(store.find_by_trigger("/dup").unwrap().replacement, "first");
        assert!(store.find_by_trigger("/good").is_some());
        assert!(store.find_by_trigger("/sys").is_none());
    }

    #[test]
    fn import_mints_a_fresh_id_when_the_incoming_id_collides() {
        let mut store = AutoTextStore::default();
        store.insert(AutoTextEntry::new("/one", "1")).unwrap();
        let taken = store.entries[0].id.clone();

        let json = serde_json::json!({
            "version": 1,
            "entries": [{"id": taken, "trigger": "/two", "replacement": "2"}]
        })
        .to_string();

        import_from_bytes(&mut store, json.as_bytes()).unwrap();
        let two = store.find_by_trigger("/two").expect("missing /two");
        assert_ne!(two.id, taken, "a colliding id must not be reused");
        assert_eq!(store.entries.len(), 2);
    }

    #[test]
    fn import_rejects_a_bad_envelope_without_touching_the_store() {
        let mut store = AutoTextStore::default();
        store.insert(AutoTextEntry::new("/keep", "me")).unwrap();

        let err = import_from_bytes(&mut store, b"{not json").unwrap_err();
        assert!(err.contains("Not a valid AutoText export"), "got: {err}");
        let err = import_from_bytes(&mut store, br#"{"entries":[]}"#).unwrap_err();
        assert!(err.contains("version"), "got: {err}");
        let err = import_from_bytes(&mut store, br#"{"version":99,"entries":[]}"#).unwrap_err();
        assert!(err.contains("version 99"), "got: {err}");

        assert_eq!(store.entries.len(), 1, "a bad file must change nothing");
        assert!(store.find_by_trigger("/keep").is_some());
    }
}
