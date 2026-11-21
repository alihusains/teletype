//! Append-only polish feedback store (T1).
//!
//! Local only: one JSON object per line, no telemetry. The store is
//! intentionally dumb — it records what the user said about a take and never
//! applies it. T5 consumes these rows.
//!
//! Dedupe rule: recording an entry identical to an earlier entry (same take,
//! kind, term, and span) is a no-op, so a double click cannot inflate counts.
//! Ordering is file order; callers that need the latest vote take the last
//! matching row.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::profile::PolishProfile;

/// Errors from the feedback store.
#[derive(Debug, Error)]
pub enum FeedbackError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type FeedbackResult<T> = Result<T, FeedbackError>;

/// What the user did with a polished take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    ThumbUp,
    ThumbDown,
    NeverChange,
    Undo,
    CopyRaw,
}

/// One changed span the user inspected.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpanFeedback {
    pub original: String,
    pub replacement: String,
}

/// A feedback action before it is timestamped and stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolishFeedback {
    /// The card/take id Rust handed out with the result card.
    pub take_id: u64,
    pub kind: FeedbackKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<SpanFeedback>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The polish profile that ran on this take, when the card carries it.
    /// Absent on older or simplified payloads; the learning loop falls back to
    /// the active change or the resolved default when it is missing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<PolishProfile>,
}

/// A stored row: the action plus its server-side timestamp (ms since epoch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackEntry {
    #[serde(flatten)]
    pub feedback: PolishFeedback,
    pub ts: u64,
}

/// The exact dedupe identity: `(take_id, kind, term, span)`. `app_id`, `model`
/// and `ts` are deliberately excluded, matching the historical `record()` check.
type DedupeKey = (u64, FeedbackKind, Option<String>, Option<SpanFeedback>);

fn dedupe_key(feedback: &PolishFeedback) -> DedupeKey {
    (
        feedback.take_id,
        feedback.kind,
        feedback.term.clone(),
        feedback.span.clone(),
    )
}

/// The append-only JSONL store at `path`.
pub struct FeedbackStore {
    path: PathBuf,
    /// Lazily seeded from the file on first `record()`. Seeding uses `load()`,
    /// so malformed rows are skipped exactly as they are on a normal read and
    /// the file stays the source of truth. `None` means "not seeded yet".
    dedupe_keys: Mutex<Option<HashSet<DedupeKey>>>,
}

impl FeedbackStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            dedupe_keys: Mutex::new(None),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads every valid row in file order. A missing file is empty; a
    /// malformed or partial line is skipped rather than failing the load, so
    /// a crash mid-write cannot brick the UI.
    pub fn load(&self) -> FeedbackResult<Vec<FeedbackEntry>> {
        let file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut entries = Vec::new();
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<FeedbackEntry>(&line) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }

    /// Appends `feedback` unless an identical row is already present.
    /// Returns true when a row was written.
    ///
    /// The dedupe set is seeded from the file on the first call and then kept
    /// in memory, so a click no longer re-reads and re-parses the whole JSONL
    /// file on every write.
    pub fn record(&self, feedback: &PolishFeedback, ts: u64) -> FeedbackResult<bool> {
        // A panicking writer can poison the mutex; the stored set is still
        // valid data, so recover it rather than failing every later write.
        let mut cache = self.dedupe_keys.lock().unwrap_or_else(|e| e.into_inner());
        let keys = if let Some(keys) = cache.as_mut() {
            keys
        } else {
            let entries = self.load()?;
            cache.insert(
                entries
                    .iter()
                    .map(|entry| dedupe_key(&entry.feedback))
                    .collect::<HashSet<_>>(),
            )
        };
        let key = dedupe_key(feedback);
        if keys.contains(&key) {
            return Ok(false);
        }
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let entry = FeedbackEntry {
            feedback: feedback.clone(),
            ts,
        };
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(&entry)?)?;
        keys.insert(key);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> (tempfile::TempDir, FeedbackStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FeedbackStore::new(dir.path().join(format!("{name}.jsonl")));
        (dir, store)
    }

    fn feedback(kind: FeedbackKind) -> PolishFeedback {
        PolishFeedback {
            take_id: 7,
            kind,
            term: None,
            span: None,
            app_id: Some("com.google.gmail".into()),
            model: Some("test-model".into()),
            profile: Some(PolishProfile::Natural),
        }
    }

    #[test]
    fn round_trip_and_append_order() {
        let (_dir, store) = temp_store("round-trip");
        assert!(store.record(&feedback(FeedbackKind::ThumbUp), 100).unwrap());
        assert!(store
            .record(&feedback(FeedbackKind::ThumbDown), 200)
            .unwrap());
        let rows = store.load().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].feedback.kind, FeedbackKind::ThumbUp);
        assert_eq!(rows[0].ts, 100);
        assert_eq!(rows[1].feedback.kind, FeedbackKind::ThumbDown);
        assert_eq!(rows[1].ts, 200);
    }

    #[test]
    fn exact_duplicate_is_not_appended() {
        let (_dir, store) = temp_store("dedupe");
        assert!(store.record(&feedback(FeedbackKind::ThumbUp), 100).unwrap());
        assert!(!store.record(&feedback(FeedbackKind::ThumbUp), 101).unwrap());
        assert_eq!(store.load().unwrap().len(), 1);
    }

    #[test]
    fn different_span_is_not_a_duplicate() {
        let (_dir, store) = temp_store("span");
        let mut first = feedback(FeedbackKind::NeverChange);
        first.term = Some("Siobhan".into());
        first.span = Some(SpanFeedback {
            original: "Shivon".into(),
            replacement: "Siobhan".into(),
        });
        assert!(store.record(&first, 100).unwrap());
        let mut second = first.clone();
        second.span = Some(SpanFeedback {
            original: "Sivon".into(),
            replacement: "Siobhan".into(),
        });
        assert!(store.record(&second, 200).unwrap());
        assert_eq!(store.load().unwrap().len(), 2);
    }

    #[test]
    fn empty_missing_file_loads_empty() {
        let (_dir, store) = temp_store("missing");
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn malformed_rows_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("malformed.jsonl");
        std::fs::write(
            &path,
            "not json\n\
             {\"takeId\":1,\"kind\":\"thumb_up\",\"ts\":10}\n\
             {\"takeId\":2,\"kind\":\"not_a_kind\",\"ts\":20}\n\
             \n\
             {\"takeId\":3,\"kind\":\"copy_raw\",\"ts\":30}\n",
        )
        .unwrap();
        let store = FeedbackStore::new(path);
        let rows = store.load().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].feedback.take_id, 1);
        assert_eq!(rows[1].feedback.take_id, 3);
    }

    #[test]
    fn legacy_rows_without_optional_fields_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.jsonl");
        std::fs::write(&path, "{\"takeId\":9,\"kind\":\"undo\",\"ts\":5}\n").unwrap();
        let store = FeedbackStore::new(path);
        let rows = store.load().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].feedback.take_id, 9);
        assert_eq!(rows[0].feedback.term, None);
        assert_eq!(rows[0].feedback.span, None);
    }

    #[test]
    fn record_after_malformed_line_keeps_working() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recover.jsonl");
        std::fs::write(&path, "garbage\n").unwrap();
        let store = FeedbackStore::new(path);
        assert!(store.record(&feedback(FeedbackKind::CopyRaw), 42).unwrap());
        let rows = store.load().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ts, 42);
    }

    #[test]
    fn cache_dedupes_identical_and_allows_different_span() {
        let (_dir, store) = temp_store("cache-dedupe");
        let mut first = feedback(FeedbackKind::NeverChange);
        first.term = Some("Siobhan".into());
        first.span = Some(SpanFeedback {
            original: "Shivon".into(),
            replacement: "Siobhan".into(),
        });

        assert!(store.record(&first, 100).unwrap(), "first write");
        assert!(
            !store.record(&first, 101).unwrap(),
            "an identical click is a no-op"
        );
        assert_eq!(store.load().unwrap().len(), 1);

        let mut different = first.clone();
        different.span = Some(SpanFeedback {
            original: "Sivon".into(),
            replacement: "Siobhan".into(),
        });
        assert!(
            store.record(&different, 200).unwrap(),
            "a different span writes again"
        );
        assert_eq!(store.load().unwrap().len(), 2);
    }

    #[test]
    fn cache_is_seeded_from_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seed.jsonl");
        let first = feedback(FeedbackKind::ThumbUp);
        let stored = FeedbackEntry {
            feedback: first.clone(),
            ts: 10,
        };
        std::fs::write(
            &path,
            format!("garbage\n{}\n", serde_json::to_string(&stored).unwrap()),
        )
        .unwrap();

        let store = FeedbackStore::new(path);
        // The one valid row seeds the cache; the malformed line is skipped.
        assert!(
            !store.record(&first, 20).unwrap(),
            "the already-stored row is deduped from the seeded cache"
        );
        assert!(store
            .record(&feedback(FeedbackKind::ThumbDown), 30)
            .unwrap());
        assert_eq!(store.load().unwrap().len(), 2);
    }
}
