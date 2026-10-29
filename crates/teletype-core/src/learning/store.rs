//! Durable learning store (Phase 7, P7.1).
//!
//! SQLite (rusqlite, bundled) behind a thin row-level API: dictations,
//! lexicon, style rules, examples, and daily metrics. Storage only — no
//! behavior. The caller supplies the file path; no Tauri or app-dir logic.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::dictionary::DictionaryWord;

/// Errors from the learning store.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub type StoreResult<T> = Result<T, StoreError>;

/// The durable learning store. One SQLite file, WAL mode.
pub struct LearningStore {
    conn: Connection,
}

/// Schema version. Bump and add a `PRAGMA user_version`-gated migration
/// step below when the on-disk schema changes in place.
const SCHEMA_VERSION: i64 = 2;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS dictation (
  id INTEGER PRIMARY KEY, ts INTEGER, app_id TEXT, app_category TEXT, engine TEXT, lang TEXT,
  audio_path TEXT NULL, raw_asr TEXT, words_json TEXT,
  pasted TEXT, observed TEXT NULL, edit_ops_json TEXT NULL, polish_model TEXT NULL, polish_skipped INTEGER,
  settle_edits TEXT NULL, settle_status INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS lexicon (
  term TEXT PRIMARY KEY, aliases_json TEXT, source TEXT CHECK(source IN ('manual','learned','contacts','import','pack')),
  weight REAL DEFAULT 1.0, hits INTEGER DEFAULT 0, false_alarms INTEGER DEFAULT 0,
  created INTEGER, last_used INTEGER, tombstoned INTEGER DEFAULT 0, app_scope TEXT NULL,
  learned_from TEXT NULL
);
CREATE TABLE IF NOT EXISTS style_rule (app_category TEXT, rule TEXT, value TEXT, evidence INTEGER, updated INTEGER,
  PRIMARY KEY(app_category, rule));
CREATE TABLE IF NOT EXISTS example (id INTEGER PRIMARY KEY, app_category TEXT, raw TEXT, final TEXT, accepted_unedited INTEGER, ts INTEGER);
CREATE TABLE IF NOT EXISTS metric_daily (day TEXT PRIMARY KEY, words INTEGER, edited_words INTEGER, dictations INTEGER, learned_terms INTEGER);
CREATE TABLE IF NOT EXISTS personal_accuracy (ts INTEGER PRIMARY KEY, wer_before REAL NOT NULL, wer_after REAL NOT NULL, clips INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS learn_candidate (kind TEXT NOT NULL, a TEXT NOT NULL, b TEXT NOT NULL, category TEXT NOT NULL DEFAULT '', count INTEGER NOT NULL, total INTEGER NOT NULL, last_seen INTEGER NOT NULL, accepted INTEGER NOT NULL DEFAULT 0, rejected INTEGER NOT NULL DEFAULT 0, auto_apply INTEGER NOT NULL DEFAULT 0, manual INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (kind, a, b, category));
CREATE TABLE IF NOT EXISTS learn_memory (
  misheard TEXT NOT NULL, canonical TEXT NOT NULL, no_count INTEGER NOT NULL DEFAULT 1,
  blocked INTEGER NOT NULL DEFAULT 0, last_no INTEGER NOT NULL,
  PRIMARY KEY (misheard, canonical)
);
"#;

/// True when a column already exists on a table (guards idempotent
/// `ALTER TABLE ... ADD COLUMN` for stores created before the column).
fn column_exists(conn: &Connection, table: &str, column: &str) -> StoreResult<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let found = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .any(|c| c.as_deref() == Ok(column));
    Ok(found)
}

impl LearningStore {
    /// Opens (creating if absent) the SQLite file at `path`. WAL mode,
    /// 2s busy timeout, foreign keys on. Creates the schema if absent.
    pub fn open(path: &Path) -> StoreResult<Self> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", "2000")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            // Fresh database: full schema.
            conn.execute_batch(SCHEMA)?;
        } else {
            // Existing store: `CREATE TABLE IF NOT EXISTS` is a no-op for
            // tables that already exist, so running the full schema safely
            // adds any new tables (learn_memory) without touching existing
            // ones. Then add any columns the old schema lacked.
            conn.execute_batch(SCHEMA)?;
        }
        // `learned_from` was added to `lexicon` after the first release, so
        // any pre-existing store lacks it. `ADD COLUMN` is idempotent-gated
        // by a `PRAGMA table_info` probe; a fresh create already has it.
        if !column_exists(&conn, "lexicon", "learned_from")? {
            conn.execute_batch("ALTER TABLE lexicon ADD COLUMN learned_from TEXT NULL;")?;
        }
        // U3 settle-by-survival: additive columns on dictation and
        // learn_candidate. Guarded by PRAGMA table_info probes so a
        // pre-existing store is migrated in place without a rebuild.
        if !column_exists(&conn, "dictation", "settle_edits")? {
            conn.execute_batch("ALTER TABLE dictation ADD COLUMN settle_edits TEXT NULL;")?;
        }
        if !column_exists(&conn, "dictation", "settle_status")? {
            conn.execute_batch(
                "ALTER TABLE dictation ADD COLUMN settle_status INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        for (col, default) in [
            ("accepted", "INTEGER NOT NULL DEFAULT 0"),
            ("rejected", "INTEGER NOT NULL DEFAULT 0"),
            ("auto_apply", "INTEGER NOT NULL DEFAULT 0"),
            ("manual", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            if !column_exists(&conn, "learn_candidate", col)? {
                conn.execute_batch(&format!(
                    "ALTER TABLE learn_candidate ADD COLUMN {col} {default};"
                ))?;
            }
        }
        // Partial index for the settle sweep: rows that have edits and are
        // not yet settled.
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_dictation_unsettled ON dictation (settle_status, ts) WHERE settle_edits IS NOT NULL;",
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }
}

// ── Row types ────────────────────────────────────────────────────────────────

/// One recorded dictation: raw ASR plus what the user pasted/observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationRow {
    pub id: i64,
    pub ts: u64,
    pub app_id: String,
    pub app_category: String,
    pub engine: String,
    pub lang: String,
    pub audio_path: Option<String>,
    pub raw_asr: String,
    pub words_json: String,
    pub pasted: String,
    pub observed: Option<String>,
    pub edit_ops_json: Option<String>,
    pub polish_model: Option<String>,
    pub polish_skipped: bool,
}

/// One lexicon entry (manual/learned/contacts/import/pack term).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LexiconRow {
    pub term: String,
    /// Aliases, serde_json-encoded to the TEXT column on write.
    pub aliases: Vec<String>,
    pub source: String,
    pub weight: f64,
    pub hits: i64,
    pub false_alarms: i64,
    pub created: u64,
    pub last_used: Option<u64>,
    pub tombstoned: bool,
    pub app_scope: Option<String>,
    /// The misheard original this term was promoted from (learned rows
    /// written after the schema gained this column). `None` for manual
    /// rows and older learned rows; used to record a pair refusal on undo.
    #[serde(default)]
    pub learned_from: Option<String>,
}

/// One learn-memory row: a user refusal ("no") on a (misheard, canonical)
/// pair. Keyed by lowercased, trimmed strings. `no_count` ≥ 2 means the
/// pair is permanently blocked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnMemoryRow {
    pub misheard: String,
    pub canonical: String,
    pub no_count: i64,
    pub blocked: bool,
    /// Epoch ms of the most recent refusal.
    pub last_no: u64,
}

/// One per-app style rule (e.g. "salutation" = "hey" for slack).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StyleRuleRow {
    pub app_category: String,
    pub rule: String,
    pub value: String,
    pub evidence: i64,
    pub updated: u64,
}

/// One accepted example (raw vs final) for a category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExampleRow {
    pub id: i64,
    pub app_category: String,
    pub raw: String,
    /// The final (post-edit) text. Named `final_text` because `final` is a
    /// reserved Rust keyword; the SQL column is `final`.
    pub final_text: String,
    pub accepted_unedited: bool,
    pub ts: u64,
}

/// One day of aggregate metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricRow {
    pub day: String,
    pub words: i64,
    pub edited_words: i64,
    pub dictations: i64,
    pub learned_terms: i64,
}

/// One replay run's aggregate personal-accuracy snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalAccuracyRow {
    pub ts: u64,
    pub wer_before: f64,
    pub wer_after: f64,
    pub clips: i64,
}

// ── Dictation ────────────────────────────────────────────────────────────────

impl LearningStore {
    /// Inserts a dictation and returns its new id.
    pub fn record_dictation(&self, r: &DictationRow) -> StoreResult<i64> {
        self.conn.execute(
            "INSERT INTO dictation (ts, app_id, app_category, engine, lang, audio_path, raw_asr,
              words_json, pasted, observed, edit_ops_json, polish_model, polish_skipped)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                r.ts as i64,
                r.app_id,
                r.app_category,
                r.engine,
                r.lang,
                r.audio_path,
                r.raw_asr,
                r.words_json,
                r.pasted,
                r.observed,
                r.edit_ops_json,
                r.polish_model,
                r.polish_skipped,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn dictation_by_id(&self, id: i64) -> StoreResult<Option<DictationRow>> {
        let mut stmt = self.conn.prepare("SELECT * FROM dictation WHERE id = ?1")?;
        let row = stmt.query_row(params![id], row_to_dictation).optional()?;
        Ok(row)
    }

    /// Newest first.
    pub fn recent_dictations(&self, limit: i64) -> StoreResult<Vec<DictationRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM dictation ORDER BY ts DESC, id DESC LIMIT ?1")?;
        let rows = stmt
            .query_map(params![limit], row_to_dictation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Fills in the observed text and edit ops once the observer has run.
    pub fn set_observed(
        &self,
        id: i64,
        observed: &str,
        edit_ops_json: Option<&str>,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE dictation SET observed = ?2, edit_ops_json = ?3 WHERE id = ?1",
            params![id, observed, edit_ops_json],
        )?;
        Ok(())
    }
}

fn row_to_dictation(row: &Row<'_>) -> rusqlite::Result<DictationRow> {
    Ok(DictationRow {
        id: row.get(0)?,
        ts: row.get::<_, i64>(1)? as u64,
        app_id: row.get(2)?,
        app_category: row.get(3)?,
        engine: row.get(4)?,
        lang: row.get(5)?,
        audio_path: row.get(6)?,
        raw_asr: row.get(7)?,
        words_json: row.get(8)?,
        pasted: row.get(9)?,
        observed: row.get(10)?,
        edit_ops_json: row.get(11)?,
        polish_model: row.get(12)?,
        polish_skipped: row.get::<_, i64>(13)? != 0,
    })
}

// ── Lexicon ──────────────────────────────────────────────────────────────────

impl LearningStore {
    /// Inserts or updates a lexicon entry (keyed by term).
    pub fn upsert_lexicon(&self, r: &LexiconRow) -> StoreResult<()> {
        let aliases_json = serde_json::to_string(&r.aliases)?;
        self.conn.execute(
            "INSERT INTO lexicon (term, aliases_json, source, weight, hits, false_alarms,
              created, last_used, tombstoned, app_scope, learned_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(term) DO UPDATE SET
              aliases_json = excluded.aliases_json, source = excluded.source,
              weight = excluded.weight, hits = excluded.hits,
              false_alarms = excluded.false_alarms, created = excluded.created,
              last_used = excluded.last_used, tombstoned = excluded.tombstoned,
              app_scope = excluded.app_scope, learned_from = excluded.learned_from",
            params![
                r.term,
                aliases_json,
                r.source,
                r.weight,
                r.hits,
                r.false_alarms,
                r.created as i64,
                r.last_used.map(|v| v as i64),
                r.tombstoned,
                r.app_scope,
                r.learned_from,
            ],
        )?;
        Ok(())
    }

    pub fn lexicon_by_term(&self, term: &str) -> StoreResult<Option<LexiconRow>> {
        let mut stmt = self.conn.prepare("SELECT * FROM lexicon WHERE term = ?1")?;
        let row = stmt.query_row(params![term], row_to_lexicon).optional()?;
        Ok(row)
    }

    pub fn lexicon_all(&self) -> StoreResult<Vec<LexiconRow>> {
        let mut stmt = self.conn.prepare("SELECT * FROM lexicon ORDER BY term")?;
        let rows = stmt
            .query_map([], row_to_lexicon)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Top-n boost candidates: tombstoned excluded, deterministic
    /// `weight DESC, hits DESC, term ASC`.
    pub fn lexicon_terms_for_boost(&self, top_n: i64) -> StoreResult<Vec<LexiconRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM lexicon WHERE tombstoned = 0
             ORDER BY weight DESC, hits DESC, term ASC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![top_n], row_to_lexicon)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn bump_hits(&self, term: &str, delta: i64) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE lexicon SET hits = hits + ?2 WHERE term = ?1",
            params![term, delta],
        )?;
        Ok(())
    }

    pub fn bump_false_alarms(&self, term: &str, delta: i64) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE lexicon SET false_alarms = false_alarms + ?2 WHERE term = ?1",
            params![term, delta],
        )?;
        Ok(())
    }

    pub fn set_weight(&self, term: &str, w: f64) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE lexicon SET weight = ?2 WHERE term = ?1",
            params![term, w],
        )?;
        Ok(())
    }

    pub fn tombstone(&self, term: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE lexicon SET tombstoned = 1 WHERE term = ?1",
            params![term],
        )?;
        Ok(())
    }

    pub fn untombstone(&self, term: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE lexicon SET tombstoned = 0 WHERE term = ?1",
            params![term],
        )?;
        Ok(())
    }
}

fn row_to_lexicon(row: &Row<'_>) -> rusqlite::Result<LexiconRow> {
    let aliases_json: String = row.get(1)?;
    let aliases: Vec<String> = serde_json::from_str(&aliases_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad aliases_json: {e}"),
            )),
        )
    })?;
    Ok(LexiconRow {
        term: row.get(0)?,
        aliases,
        source: row.get(2)?,
        weight: row.get(3)?,
        hits: row.get(4)?,
        false_alarms: row.get(5)?,
        created: row.get::<_, i64>(6)? as u64,
        last_used: row.get::<_, Option<i64>>(7)?.map(|v| v as u64),
        tombstoned: row.get::<_, i64>(8)? != 0,
        app_scope: row.get(9)?,
        learned_from: row.get(10)?,
    })
}

// ── Style rules ──────────────────────────────────────────────────────────────

impl LearningStore {
    /// Inserts or updates a style rule (keyed by app_category + rule).
    pub fn upsert_style_rule(&self, r: &StyleRuleRow) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO style_rule (app_category, rule, value, evidence, updated)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(app_category, rule) DO UPDATE SET
              value = excluded.value, evidence = excluded.evidence,
              updated = excluded.updated",
            params![
                r.app_category,
                r.rule,
                r.value,
                r.evidence,
                r.updated as i64
            ],
        )?;
        Ok(())
    }

    pub fn style_rule(&self, category: &str, rule: &str) -> StoreResult<Option<StyleRuleRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM style_rule WHERE app_category = ?1 AND rule = ?2")?;
        let row = stmt
            .query_row(params![category, rule], row_to_style_rule)
            .optional()?;
        Ok(row)
    }

    pub fn style_rules_for(&self, category: &str) -> StoreResult<Vec<StyleRuleRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM style_rule WHERE app_category = ?1 ORDER BY rule")?;
        let rows = stmt
            .query_map(params![category], row_to_style_rule)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn row_to_style_rule(row: &Row<'_>) -> rusqlite::Result<StyleRuleRow> {
    Ok(StyleRuleRow {
        app_category: row.get(0)?,
        rule: row.get(1)?,
        value: row.get(2)?,
        evidence: row.get(3)?,
        updated: row.get::<_, i64>(4)? as u64,
    })
}

// ── Examples ─────────────────────────────────────────────────────────────────

impl LearningStore {
    /// Inserts an example and returns its new id.
    pub fn save_example(&self, r: &ExampleRow) -> StoreResult<i64> {
        self.conn.execute(
            "INSERT INTO example (app_category, raw, final, accepted_unedited, ts)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                r.app_category,
                r.raw,
                r.final_text,
                r.accepted_unedited,
                r.ts as i64
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Newest first, capped.
    pub fn examples_for(&self, category: &str, limit: i64) -> StoreResult<Vec<ExampleRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT * FROM example WHERE app_category = ?1 ORDER BY ts DESC, id DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![category, limit], row_to_example)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn row_to_example(row: &Row<'_>) -> rusqlite::Result<ExampleRow> {
    Ok(ExampleRow {
        id: row.get(0)?,
        app_category: row.get(1)?,
        raw: row.get(2)?,
        final_text: row.get(3)?,
        accepted_unedited: row.get::<_, i64>(4)? != 0,
        ts: row.get::<_, i64>(5)? as u64,
    })
}

// ── Daily metrics ────────────────────────────────────────────────────────────

impl LearningStore {
    /// Upsert-add: accumulates the day's counters (inserts the row if new).
    pub fn bump_metric_daily(
        &self,
        day: &str,
        words: i64,
        edited_words: i64,
        dictations: i64,
        learned_terms: i64,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO metric_daily (day, words, edited_words, dictations, learned_terms)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(day) DO UPDATE SET
              words = words + excluded.words,
              edited_words = edited_words + excluded.edited_words,
              dictations = dictations + excluded.dictations,
              learned_terms = learned_terms + excluded.learned_terms",
            params![day, words, edited_words, dictations, learned_terms],
        )?;
        Ok(())
    }

    pub fn metric_daily_since(&self, day: &str) -> StoreResult<Vec<MetricRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT * FROM metric_daily WHERE day >= ?1 ORDER BY day")?;
        let rows = stmt
            .query_map(params![day], row_to_metric)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn row_to_metric(row: &Row<'_>) -> rusqlite::Result<MetricRow> {
    Ok(MetricRow {
        day: row.get(0)?,
        words: row.get(1)?,
        edited_words: row.get(2)?,
        dictations: row.get(3)?,
        learned_terms: row.get(4)?,
    })
}

// ── Personal accuracy (replay) ──────────────────────────────────────────────

impl LearningStore {
    /// Records one replay run's aggregate WER snapshot (keyed by `ts`). Later
    /// runs with the same `ts` replace the earlier row.
    pub fn record_personal_accuracy(
        &self,
        ts: u64,
        wer_before: f64,
        wer_after: f64,
        clips: i64,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO personal_accuracy (ts, wer_before, wer_after, clips)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(ts) DO UPDATE SET
              wer_before = excluded.wer_before,
              wer_after = excluded.wer_after,
              clips = excluded.clips",
            params![ts as i64, wer_before, wer_after, clips],
        )?;
        Ok(())
    }

    /// Personal-accuracy history, newest first, capped at `limit`.
    pub fn personal_accuracy_history(&self, limit: i64) -> StoreResult<Vec<PersonalAccuracyRow>> {
        let mut stmt = self
            .conn
            .prepare("SELECT ts, wer_before, wer_after, clips FROM personal_accuracy ORDER BY ts DESC LIMIT ?1")?;
        let rows = stmt
            .query_map(params![limit], row_to_personal_accuracy)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn row_to_personal_accuracy(row: &Row<'_>) -> rusqlite::Result<PersonalAccuracyRow> {
    Ok(PersonalAccuracyRow {
        ts: row.get::<_, i64>(0)? as u64,
        wer_before: row.get(1)?,
        wer_after: row.get(2)?,
        clips: row.get(3)?,
    })
}

// ── U3: settle-by-survival row types ─────────────────────────────────────────

/// A learned candidate with its settle tallies, for the settle sweep
/// and the inspectable list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateRow {
    pub kind: String,
    pub a: String,
    pub b: String,
    pub category: String,
    pub count: i64,
    pub total: i64,
    pub last_seen: i64,
    pub accepted: i64,
    pub rejected: i64,
    pub auto_apply: bool,
    pub manual: bool,
}

/// One survival tally to add to a candidate inside the settle transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateTally {
    pub kind: String,
    pub a: String,
    pub b: String,
    pub category: String,
    pub accepted_delta: i64,
    pub rejected_delta: i64,
}

/// One unsettled dictation row with its settle data, for the sweep.
#[derive(Debug, Clone)]
pub struct UnsettledRow {
    pub id: i64,
    pub ts: u64,
    pub observed: Option<String>,
    pub settle_edits: String,
}

// ── Retention / privacy ──────────────────────────────────────────────────────

impl LearningStore {
    /// Deletes dictations older than `older_than_days` (by `ts`, epoch ms).
    /// Returns the number of rows deleted.
    pub fn prune_dictation(&self, older_than_days: i64) -> StoreResult<i64> {
        let cutoff_ms = crate::storage::now_ms()
            .saturating_sub((older_than_days.max(0) as u64).saturating_mul(24 * 60 * 60 * 1000));
        let n = self.conn.execute(
            "DELETE FROM dictation WHERE ts < ?1",
            params![cutoff_ms as i64],
        )?;
        Ok(n as i64)
    }

    /// Bumps a learn-candidate counter (upsert on the composite key).
    /// `total` always increments; `count` increments only when `positive`
    /// (e.g. a mishearing observation, or a style observation that agrees
    /// with the proposed rule). Returns the updated `(count, total)`.
    pub fn bump_learn_candidate(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
        positive: bool,
        now_ms: u64,
    ) -> StoreResult<(i64, i64)> {
        self.conn.execute(
            "INSERT INTO learn_candidate (kind, a, b, category, count, total, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)
             ON CONFLICT(kind, a, b, category) DO UPDATE SET
              count = count + ?5, total = total + 1, last_seen = ?6",
            params![kind, a, b, category, positive as i64, now_ms as i64],
        )?;
        let Some((count, total)) = self.learn_candidate_get(kind, a, b, category)? else {
            // Unreachable: the row was just upserted on the same connection.
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        };
        Ok((count, total))
    }

    /// Reads a learn-candidate counter, or `None` if never bumped.
    pub fn learn_candidate_get(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
    ) -> StoreResult<Option<(i64, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT count, total FROM learn_candidate WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
        )?;
        let row = stmt
            .query_row(params![kind, a, b, category], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()?;
        Ok(row)
    }

    /// Removes a learn-candidate counter (no-op if absent).
    pub fn clear_learn_candidate(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "DELETE FROM learn_candidate WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
            params![kind, a, b, category],
        )?;
        Ok(())
    }

    /// Zeroes a candidate's promotion counters (`count`, `total`) but keeps
    /// the row and its settle evidence (`accepted`, `rejected`, `auto_apply`,
    /// `manual`).
    ///
    /// Promotion used to DELETE the row. The settle sweep writes its survival
    /// tallies to the same composite key, so the delete also erased the
    /// evidence Insights reports and any `manual` flag the user had set on
    /// the pair.
    pub fn reset_learn_candidate_counters(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE learn_candidate SET count = 0, total = 0
             WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
            params![kind, a, b, category],
        )?;
        Ok(())
    }

    /// Retires a candidate for an undone term: zeroes the promotion counters
    /// and clears `auto_apply`/`manual`, while keeping `accepted`/`rejected`
    /// as survival history. `auto_apply` must not survive an undo — the term
    /// is tombstoned, so advertising an auto-applied fix for it in Insights
    /// would be a lie.
    pub fn retire_learn_candidate(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE learn_candidate SET count = 0, total = 0, auto_apply = 0, manual = 0
             WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
            params![kind, a, b, category],
        )?;
        Ok(())
    }

    // ── U3: settle-by-survival ─────────────────────────────────────────

    /// Reads a learn-candidate row with all settle columns.
    pub fn candidate_get_full(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
    ) -> StoreResult<Option<CandidateRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, a, b, category, count, total, last_seen, accepted, rejected, auto_apply, manual
             FROM learn_candidate WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
        )?;
        let row = stmt
            .query_row(params![kind, a, b, category], |r| {
                Ok(CandidateRow {
                    kind: r.get(0)?,
                    a: r.get(1)?,
                    b: r.get(2)?,
                    category: r.get(3)?,
                    count: r.get(4)?,
                    total: r.get(5)?,
                    last_seen: r.get(6)?,
                    accepted: r.get(7)?,
                    rejected: r.get(8)?,
                    auto_apply: r.get::<_, i64>(9)? != 0,
                    manual: r.get::<_, i64>(10)? != 0,
                })
            })
            .optional()?;
        Ok(row)
    }

    /// Lists all learn-candidate rows with settle tallies (for the
    /// inspectable list).
    pub fn candidate_all(&self) -> StoreResult<Vec<CandidateRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, a, b, category, count, total, last_seen, accepted, rejected, auto_apply, manual
             FROM learn_candidate ORDER BY kind, a, b, category",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(CandidateRow {
                    kind: r.get(0)?,
                    a: r.get(1)?,
                    b: r.get(2)?,
                    category: r.get(3)?,
                    count: r.get(4)?,
                    total: r.get(5)?,
                    last_seen: r.get(6)?,
                    accepted: r.get(7)?,
                    rejected: r.get(8)?,
                    auto_apply: r.get::<_, i64>(9)? != 0,
                    manual: r.get::<_, i64>(10)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Adds to a candidate's accepted/rejected tallies. `accepted_delta`
    /// and `rejected_delta` may be positive (new count) or negative
    /// (reversal). Returns the updated `(accepted, rejected)`.
    // The four-part composite primary key is spelled out rather than boxed in
    // a struct so it matches the sibling `learn_candidate` methods.
    #[allow(clippy::too_many_arguments)]
    pub fn count_candidate(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
        accepted_delta: i64,
        rejected_delta: i64,
        now_ms: u64,
    ) -> StoreResult<(i64, i64)> {
        // Ensure the row exists (upsert with zero deltas if absent).
        self.conn.execute(
            "INSERT INTO learn_candidate (kind, a, b, category, count, total, last_seen)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5)
             ON CONFLICT(kind, a, b, category) DO UPDATE SET last_seen = ?5",
            params![kind, a, b, category, now_ms as i64],
        )?;
        self.conn.execute(
            "UPDATE learn_candidate SET accepted = accepted + ?5, rejected = rejected + ?6, last_seen = ?7
             WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
            params![kind, a, b, category, accepted_delta, rejected_delta, now_ms as i64],
        )?;
        let Some((accepted, rejected)) = self
            .candidate_get_full(kind, a, b, category)?
            .map(|c| (c.accepted, c.rejected))
        else {
            return Err(StoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows));
        };
        Ok((accepted, rejected))
    }

    /// Sets the `auto_apply` flag on a candidate. `manual` is true when the
    /// user set it themselves (the tallies stop moving it).
    pub fn set_candidate_auto(
        &self,
        kind: &str,
        a: &str,
        b: &str,
        category: &str,
        auto_apply: bool,
        manual: bool,
    ) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE learn_candidate SET auto_apply = ?5, manual = ?6
             WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
            params![kind, a, b, category, auto_apply as i64, manual as i64],
        )?;
        Ok(())
    }

    /// Records settle edits (JSON) and marks the dictation as unsettled
    /// (status 0). Called when a correction is proposed for a dictation.
    pub fn set_settle_edits(&self, id: i64, settle_edits_json: &str) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE dictation SET settle_edits = ?2, settle_status = 0 WHERE id = ?1",
            params![id, settle_edits_json],
        )?;
        Ok(())
    }

    /// Marks a dictation as settled (status 1) or reverted (status 2).
    pub fn set_settle_status(&self, id: i64, status: i64) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE dictation SET settle_status = ?2 WHERE id = ?1",
            params![id, status],
        )?;
        Ok(())
    }

    /// Settles one dictation row as a single transaction: adds every survival
    /// tally, recomputes each pair's `auto_apply`, and closes the row.
    ///
    /// The sweep used to run the insert, the update and the status write as
    /// three separate autocommit statements. A crash or a busy timeout between
    /// them left the tallies bumped and the row still unsettled, so the next
    /// sweep counted the same survival again; with `AUTO_APPLY_AFTER = 3`,
    /// repeated passes could auto-apply a fix on no real evidence.
    ///
    /// `decide_auto` is the promotion rule (`should_auto_apply`), passed in so
    /// the policy stays in `settle.rs`.
    pub fn settle_dictation_row(
        &self,
        row_id: i64,
        tallies: &[CandidateTally],
        now_ms: u64,
        decide_auto: impl Fn(i64, i64, bool, bool) -> bool,
    ) -> StoreResult<()> {
        let tx = self.conn.unchecked_transaction()?;
        for t in tallies {
            tx.execute(
                "INSERT INTO learn_candidate (kind, a, b, category, count, total, last_seen)
                 VALUES (?1, ?2, ?3, ?4, 0, 0, ?5)
                 ON CONFLICT(kind, a, b, category) DO UPDATE SET last_seen = ?5",
                params![t.kind, t.a, t.b, t.category, now_ms as i64],
            )?;
            tx.execute(
                "UPDATE learn_candidate SET accepted = accepted + ?5, rejected = rejected + ?6, last_seen = ?7
                 WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
                params![
                    t.kind,
                    t.a,
                    t.b,
                    t.category,
                    t.accepted_delta,
                    t.rejected_delta,
                    now_ms as i64
                ],
            )?;
            // Read back inside the transaction: the value `should_auto_apply`
            // decides on must be the one this transaction wrote.
            let (accepted, rejected, auto_apply, manual): (i64, i64, i64, i64) = tx.query_row(
                "SELECT accepted, rejected, auto_apply, manual FROM learn_candidate
                     WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
                params![t.kind, t.a, t.b, t.category],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
            let want = decide_auto(accepted, rejected, manual != 0, auto_apply != 0);
            if want != (auto_apply != 0) {
                tx.execute(
                    "UPDATE learn_candidate SET auto_apply = ?5, manual = ?6
                     WHERE kind = ?1 AND a = ?2 AND b = ?3 AND category = ?4",
                    params![t.kind, t.a, t.b, t.category, want as i64, manual],
                )?;
            }
        }
        tx.execute(
            "UPDATE dictation SET settle_status = ?2 WHERE id = ?1",
            params![
                row_id,
                crate::learning::settle::SettleStatus::Settled as i64
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Finds dictation rows that are past their settle deadline and not yet
    /// settled. `cutoff_ms` is `now_ms - SETTLE_AFTER_MS`. Returns at most
    /// `limit` rows, oldest first.
    pub fn unsettled_settled_edits(
        &self,
        cutoff_ms: i64,
        limit: i64,
    ) -> StoreResult<Vec<UnsettledRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, observed, settle_edits FROM dictation
             WHERE settle_status = 0 AND settle_edits IS NOT NULL AND ts <= ?1
             ORDER BY ts ASC, id ASC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![cutoff_ms, limit], |r| {
                Ok(UnsettledRow {
                    id: r.get(0)?,
                    ts: r.get::<_, i64>(1)? as u64,
                    observed: r.get(2)?,
                    settle_edits: r.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Keeps at most `per_category_cap` most-recent examples per category.
    /// Returns the number of rows deleted.
    pub fn prune_examples(&self, per_category_cap: i64) -> StoreResult<i64> {
        let n = self.conn.execute(
            "DELETE FROM example WHERE id NOT IN (
               SELECT id FROM (
                 SELECT id, ROW_NUMBER() OVER (
                   PARTITION BY app_category ORDER BY ts DESC, id DESC
                 ) AS rn FROM example
               ) WHERE rn <= ?1
             )",
            params![per_category_cap],
        )?;
        Ok(n as i64)
    }

    /// Clears every table in one transaction (Settings → Privacy →
    /// "Delete learning data").
    pub fn delete_all(&mut self) -> StoreResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM dictation;
             DELETE FROM lexicon;
             DELETE FROM style_rule;
             DELETE FROM example;
             DELETE FROM metric_daily;
             DELETE FROM learn_candidate;
             DELETE FROM learn_memory;",
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Imports existing dictionary words as `source='manual'` lexicon rows.
    /// Aliases preserved; `usage_count` seeds `hits`. Idempotent (upsert by
    /// term). Returns the number of rows written.
    pub fn import_from_dictionary(&self, words: &[DictionaryWord]) -> StoreResult<usize> {
        let mut n = 0usize;
        for w in words {
            if w.word.trim().is_empty() {
                continue;
            }
            let existing = self.lexicon_by_term(&w.word)?;
            let row = match existing {
                Some(mut e) => {
                    // Idempotent: refresh aliases/hits, keep everything else.
                    e.aliases = w.aliases.clone();
                    e.hits = e.hits.max(w.usage_count as i64);
                    e
                }
                None => LexiconRow {
                    term: w.word.clone(),
                    aliases: w.aliases.clone(),
                    source: "manual".into(),
                    weight: 1.0,
                    hits: w.usage_count as i64,
                    false_alarms: 0,
                    created: w.created_at,
                    last_used: None,
                    tombstoned: false,
                    app_scope: None,
                    learned_from: None,
                },
            };
            self.upsert_lexicon(&row)?;
            n += 1;
        }
        Ok(n)
    }
}

// ── Learn memory (refused pairs) ────────────────────────────────────────────

impl LearningStore {
    /// Reads the refusal row for `(misheard, canonical)`, or `None` if the
    /// pair was never refused.
    pub fn learn_memory_row(
        &self,
        misheard: &str,
        canonical: &str,
    ) -> StoreResult<Option<LearnMemoryRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT misheard, canonical, no_count, blocked, last_no
             FROM learn_memory WHERE misheard = ?1 AND canonical = ?2",
        )?;
        let row = stmt
            .query_row(params![misheard, canonical], row_to_learn_memory)
            .optional()?;
        Ok(row)
    }

    /// Inserts or updates a refusal row (keyed by the pair). Callers do the
    /// blocking/cap/TTL policy; this is the thin write.
    pub fn upsert_learn_memory(&self, r: &LearnMemoryRow) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO learn_memory (misheard, canonical, no_count, blocked, last_no)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(misheard, canonical) DO UPDATE SET
              no_count = excluded.no_count, blocked = excluded.blocked,
              last_no = excluded.last_no",
            params![
                r.misheard,
                r.canonical,
                r.no_count,
                r.blocked,
                r.last_no as i64
            ],
        )?;
        Ok(())
    }

    /// Deletes the refusal row for `(misheard, canonical)` (no-op if absent).
    pub fn clear_learn_memory(&self, misheard: &str, canonical: &str) -> StoreResult<()> {
        self.conn.execute(
            "DELETE FROM learn_memory WHERE misheard = ?1 AND canonical = ?2",
            params![misheard, canonical],
        )?;
        Ok(())
    }

    /// Deletes refusals older than `cutoff_ms` that are neither blocked nor
    /// at-or-past the block threshold (`no_count >= 2`). Returns rows deleted.
    pub fn prune_learn_memory(&self, cutoff_ms: u64) -> StoreResult<usize> {
        let n = self.conn.execute(
            "DELETE FROM learn_memory WHERE last_no < ?1 AND blocked = 0 AND no_count < 2",
            params![cutoff_ms as i64],
        )?;
        Ok(n)
    }

    /// Deletes the single oldest plain refusal (`no_count < 2`, unblocked)
    /// to make room at the cap. Returns rows deleted (0 or 1).
    pub fn delete_oldest_learn_memory(&self) -> StoreResult<usize> {
        let n = self.conn.execute(
            "DELETE FROM learn_memory WHERE misheard = (
               SELECT misheard FROM learn_memory WHERE blocked = 0 AND no_count < 2
               ORDER BY last_no ASC, misheard ASC, canonical ASC LIMIT 1
             )",
            [],
        )?;
        Ok(n)
    }

    /// Deletes every learn-memory row (refusals and blocks). Returns the
    /// number of rows removed.
    pub fn delete_all_learn_memory(&self) -> StoreResult<usize> {
        let n = self.conn.execute("DELETE FROM learn_memory", [])?;
        Ok(n)
    }

    /// Total learn-memory rows (refusals + blocks).
    pub fn learn_memory_count(&self) -> StoreResult<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM learn_memory", [], |r| r.get(0))?;
        Ok(n as usize)
    }

    /// Rows that are permanently blocked (the Forget row's "{N} blocked").
    pub fn blocked_learn_memory_count(&self) -> StoreResult<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM learn_memory WHERE blocked = 1",
            [],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }
}

fn row_to_learn_memory(row: &Row<'_>) -> rusqlite::Result<LearnMemoryRow> {
    Ok(LearnMemoryRow {
        misheard: row.get(0)?,
        canonical: row.get(1)?,
        no_count: row.get(2)?,
        blocked: row.get::<_, i64>(3)? != 0,
        last_no: row.get::<_, i64>(4)? as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::DictionaryWord;

    fn tmp_store() -> (tempfile::TempDir, LearningStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = LearningStore::open(&dir.path().join("learning.sqlite")).unwrap();
        (dir, store)
    }

    fn dictation(id: i64, ts: u64) -> DictationRow {
        DictationRow {
            id,
            ts,
            app_id: "slack".into(),
            app_category: "chat".into(),
            engine: "whisper".into(),
            lang: "en".into(),
            audio_path: Some("/tmp/a.m4a".into()),
            raw_asr: "hey team".into(),
            words_json: r#"["hey","team"]"#.into(),
            pasted: "hey team".into(),
            observed: None,
            edit_ops_json: None,
            polish_model: None,
            polish_skipped: true,
        }
    }

    fn lexicon(term: &str) -> LexiconRow {
        LexiconRow {
            term: term.into(),
            aliases: vec!["alias-one".into()],
            source: "learned".into(),
            weight: 1.0,
            hits: 0,
            false_alarms: 0,
            created: 1_000,
            last_used: None,
            tombstoned: false,
            app_scope: None,
            learned_from: None,
        }
    }

    #[test]
    fn open_creates_schema_and_reopen_works() {
        let (dir, store) = tmp_store();
        let path = dir.path().join("learning.sqlite");
        // Schema present: a fresh insert works.
        assert!(store.record_dictation(&dictation(0, 10)).unwrap() > 0);
        // Reopen the same file: data survives, no "table exists" error.
        let store2 = LearningStore::open(&path).unwrap();
        let rows = store2.recent_dictations(10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].raw_asr, "hey team");
    }

    #[test]
    fn old_store_gets_learned_from_column_on_reopen() {
        use rusqlite::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("learning.sqlite");
        // Build a pre-`learned_from` schema by hand: the lexicon table minus
        // the column, plus a `user_version` so open() takes the existing path.
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "PRAGMA user_version = 1;
                 CREATE TABLE lexicon (
                   term TEXT PRIMARY KEY, aliases_json TEXT,
                   source TEXT CHECK(source IN ('manual','learned','contacts','import','pack')),
                   weight REAL DEFAULT 1.0, hits INTEGER DEFAULT 0, false_alarms INTEGER DEFAULT 0,
                   created INTEGER, last_used INTEGER, tombstoned INTEGER DEFAULT 0, app_scope TEXT NULL
                 );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO lexicon (term, aliases_json, source, created, last_used)
                 VALUES ('pytorch', '[]', 'learned', 5, 9)",
                [],
            )
            .unwrap();
        }
        // Reopen: the migration must add the column and not drop the row.
        let store = LearningStore::open(&path).unwrap();
        let got = store.lexicon_by_term("pytorch").unwrap().unwrap();
        assert_eq!(got.term, "pytorch");
        assert!(got.learned_from.is_none());
        // The column is now writable end-to-end.
        let mut row = got;
        row.learned_from = Some("pie torch".into());
        store.upsert_lexicon(&row).unwrap();
        let again = store.lexicon_by_term("pytorch").unwrap().unwrap();
        assert_eq!(again.learned_from.as_deref(), Some("pie torch"));
    }

    #[test]
    fn dictation_roundtrip_and_recent_ordering() {
        let (_dir, store) = tmp_store();
        let id1 = store.record_dictation(&dictation(0, 100)).unwrap();
        let id2 = store.record_dictation(&dictation(0, 200)).unwrap();

        let got = store.dictation_by_id(id1).unwrap().unwrap();
        assert_eq!(got.ts, 100);
        assert_eq!(got.audio_path.as_deref(), Some("/tmp/a.m4a"));
        assert!(got.polish_skipped);
        assert!(store.dictation_by_id(9999).unwrap().is_none());

        // Newest first.
        let recent = store.recent_dictations(10).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].id, id2);
        assert_eq!(recent[1].id, id1);
        // limit is honored.
        assert_eq!(store.recent_dictations(1).unwrap().len(), 1);
    }

    #[test]
    fn set_observed_updates_row() {
        let (_dir, store) = tmp_store();
        let id = store.record_dictation(&dictation(0, 100)).unwrap();
        store
            .set_observed(id, "hey team!", Some(r#"["insert","!"]"#))
            .unwrap();
        let got = store.dictation_by_id(id).unwrap().unwrap();
        assert_eq!(got.observed.as_deref(), Some("hey team!"));
        assert_eq!(got.edit_ops_json.as_deref(), Some(r#"["insert","!"]"#));
    }

    #[test]
    fn lexicon_roundtrip_preserves_aliases_and_upserts() {
        let (_dir, store) = tmp_store();
        let l = lexicon("Teletype");
        store.upsert_lexicon(&l).unwrap();

        let got = store.lexicon_by_term("Teletype").unwrap().unwrap();
        assert_eq!(got.aliases, vec!["alias-one".to_string()]);
        assert_eq!(got.source, "learned");

        // Upsert updates, does not duplicate.
        let mut l2 = l.clone();
        l2.aliases = vec!["a".into(), "b".into()];
        l2.weight = 2.5;
        l2.hits = 3;
        store.upsert_lexicon(&l2).unwrap();
        assert_eq!(store.lexicon_all().unwrap().len(), 1);
        let got2 = store.lexicon_by_term("Teletype").unwrap().unwrap();
        assert_eq!(got2.aliases, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(got2.weight, 2.5);
        assert_eq!(got2.hits, 3);
    }

    #[test]
    fn boost_excludes_tombstoned_and_untombstone_reincludes() {
        let (_dir, store) = tmp_store();
        let mut a = lexicon("alpha");
        a.weight = 1.0;
        a.hits = 5;
        let mut b = lexicon("beta");
        b.weight = 3.0;
        b.hits = 1;
        let mut c = lexicon("gamma");
        c.weight = 3.0;
        c.hits = 9;
        for l in [&a, &b, &c] {
            store.upsert_lexicon(l).unwrap();
        }

        // Deterministic: weight DESC, hits DESC, term ASC.
        let top = store.lexicon_terms_for_boost(10).unwrap();
        assert_eq!(
            top.iter().map(|l| l.term.as_str()).collect::<Vec<_>>(),
            vec!["gamma", "beta", "alpha"]
        );

        // Tombstone gamma: excluded from boost.
        store.tombstone("gamma").unwrap();
        let top2 = store.lexicon_terms_for_boost(10).unwrap();
        assert_eq!(
            top2.iter().map(|l| l.term.as_str()).collect::<Vec<_>>(),
            vec!["beta", "alpha"]
        );
        // Row still exists, just tombstoned.
        assert!(store.lexicon_by_term("gamma").unwrap().unwrap().tombstoned);

        // Untombstone re-includes.
        store.untombstone("gamma").unwrap();
        let top3 = store.lexicon_terms_for_boost(10).unwrap();
        assert_eq!(top3.len(), 3);
        assert_eq!(top3[0].term, "gamma");
    }

    #[test]
    fn bump_and_weight_updates() {
        let (_dir, store) = tmp_store();
        store.upsert_lexicon(&lexicon("term")).unwrap();
        store.bump_hits("term", 4).unwrap();
        store.bump_hits("term", 2).unwrap();
        store.bump_false_alarms("term", 1).unwrap();
        store.set_weight("term", 0.75).unwrap();
        let got = store.lexicon_by_term("term").unwrap().unwrap();
        assert_eq!(got.hits, 6);
        assert_eq!(got.false_alarms, 1);
        assert_eq!(got.weight, 0.75);
    }

    #[test]
    fn style_rule_roundtrip_and_for_category() {
        let (_dir, store) = tmp_store();
        store
            .upsert_style_rule(&StyleRuleRow {
                app_category: "chat".into(),
                rule: "salutation".into(),
                value: "hey".into(),
                evidence: 3,
                updated: 100,
            })
            .unwrap();
        store
            .upsert_style_rule(&StyleRuleRow {
                app_category: "chat".into(),
                rule: "signoff".into(),
                value: "thanks".into(),
                evidence: 1,
                updated: 200,
            })
            .unwrap();

        let one = store.style_rule("chat", "salutation").unwrap().unwrap();
        assert_eq!(one.value, "hey");

        // Upsert updates, not duplicates.
        store
            .upsert_style_rule(&StyleRuleRow {
                app_category: "chat".into(),
                rule: "salutation".into(),
                value: "hi".into(),
                evidence: 7,
                updated: 300,
            })
            .unwrap();
        let updated = store.style_rule("chat", "salutation").unwrap().unwrap();
        assert_eq!(updated.value, "hi");
        assert_eq!(updated.evidence, 7);

        let rules = store.style_rules_for("chat").unwrap();
        assert_eq!(rules.len(), 2);
        // Ordered by rule.
        assert_eq!(rules[0].rule, "salutation");
        assert_eq!(rules[1].rule, "signoff");
        assert!(store.style_rules_for("email").unwrap().is_empty());
    }

    #[test]
    fn example_roundtrip_and_cap() {
        let (_dir, store) = tmp_store();
        for i in 0..5 {
            store
                .save_example(&ExampleRow {
                    id: 0,
                    app_category: "chat".into(),
                    raw: format!("raw {i}"),
                    final_text: format!("final {i}"),
                    accepted_unedited: i % 2 == 0,
                    ts: 100 + i,
                })
                .unwrap();
        }
        let all = store.examples_for("chat", 10).unwrap();
        assert_eq!(all.len(), 5);
        // Newest first.
        assert_eq!(all[0].ts, 104);
        let top2 = store.examples_for("chat", 2).unwrap();
        assert_eq!(top2.len(), 2);
        assert!(top2[0].accepted_unedited); // ts=104 (i=4, even)
    }

    #[test]
    fn metric_daily_accumulates_and_since_filters() {
        let (_dir, store) = tmp_store();
        store.bump_metric_daily("2026-01-01", 10, 2, 1, 0).unwrap();
        store.bump_metric_daily("2026-01-01", 5, 1, 1, 1).unwrap();
        store.bump_metric_daily("2026-01-02", 7, 0, 2, 0).unwrap();

        let since = store.metric_daily_since("2026-01-01").unwrap();
        assert_eq!(since.len(), 2);
        let d1 = &since[0];
        assert_eq!(d1.day, "2026-01-01");
        assert_eq!(d1.words, 15);
        assert_eq!(d1.edited_words, 3);
        assert_eq!(d1.dictations, 2);
        assert_eq!(d1.learned_terms, 1);

        // since is inclusive of the boundary day, exclusive before it.
        assert_eq!(store.metric_daily_since("2026-01-02").unwrap().len(), 1);
        assert!(store.metric_daily_since("2026-01-03").unwrap().is_empty());
    }

    #[test]
    fn prune_dictation_only_deletes_older_rows() {
        let (_dir, store) = tmp_store();
        let now = crate::storage::now_ms();
        let day = 24 * 60 * 60 * 1000u64;
        // One old (100 days), one recent (1 day).
        let old_id = store
            .record_dictation(&dictation(0, now.saturating_sub(100 * day)))
            .unwrap();
        let new_id = store.record_dictation(&dictation(0, now - day)).unwrap();

        let deleted = store.prune_dictation(90).unwrap();
        assert_eq!(deleted, 1);
        assert!(store.dictation_by_id(old_id).unwrap().is_none());
        assert!(store.dictation_by_id(new_id).unwrap().is_some());
    }

    #[test]
    fn prune_examples_enforces_per_category_cap() {
        let (_dir, store) = tmp_store();
        for i in 0..4 {
            store
                .save_example(&ExampleRow {
                    id: 0,
                    app_category: "chat".into(),
                    raw: format!("r{i}"),
                    final_text: format!("f{i}"),
                    accepted_unedited: false,
                    ts: 100 + i,
                })
                .unwrap();
        }
        // Other category untouched by its own count, but same cap applies.
        store
            .save_example(&ExampleRow {
                id: 0,
                app_category: "email".into(),
                raw: "r".into(),
                final_text: "f".into(),
                accepted_unedited: false,
                ts: 50,
            })
            .unwrap();

        let deleted = store.prune_examples(2).unwrap();
        // chat: 4 -> 2 (2 deleted), email: 1 -> 1 (0 deleted).
        assert_eq!(deleted, 2);
        let chat = store.examples_for("chat", 10).unwrap();
        assert_eq!(chat.len(), 2);
        // Kept the two most recent (ts 103, 102).
        assert_eq!(chat[0].ts, 103);
        assert_eq!(chat[1].ts, 102);
        assert_eq!(store.examples_for("email", 10).unwrap().len(), 1);
    }

    #[test]
    fn delete_all_clears_every_table() {
        let (_dir, mut store) = tmp_store();
        store.record_dictation(&dictation(0, 10)).unwrap();
        store.upsert_lexicon(&lexicon("a")).unwrap();
        store
            .upsert_style_rule(&StyleRuleRow {
                app_category: "c".into(),
                rule: "r".into(),
                value: "v".into(),
                evidence: 1,
                updated: 1,
            })
            .unwrap();
        store
            .save_example(&ExampleRow {
                id: 0,
                app_category: "c".into(),
                raw: "r".into(),
                final_text: "f".into(),
                accepted_unedited: true,
                ts: 1,
            })
            .unwrap();
        store.bump_metric_daily("2026-01-01", 1, 1, 1, 1).unwrap();

        store.delete_all().unwrap();

        assert!(store.recent_dictations(10).unwrap().is_empty());
        assert!(store.lexicon_all().unwrap().is_empty());
        assert!(store.style_rules_for("c").unwrap().is_empty());
        assert!(store.examples_for("c", 10).unwrap().is_empty());
        assert!(store.metric_daily_since("2026-01-01").unwrap().is_empty());
    }

    fn dict_word(word: &str, aliases: &[&str], usage: u32) -> DictionaryWord {
        let mut w = DictionaryWord::new(word, "");
        w.aliases = aliases.iter().map(|a| a.to_string()).collect();
        w.usage_count = usage;
        w
    }

    #[test]
    fn import_from_dictionary_counts_idempotent_and_preserves_aliases() {
        let (_dir, store) = tmp_store();
        let words = vec![
            dict_word("Teletype", &["tel-uh-type"], 7),
            dict_word("IC Markets", &[], 0),
        ];
        let n = store.import_from_dictionary(&words).unwrap();
        assert_eq!(n, 2);

        let t = store.lexicon_by_term("Teletype").unwrap().unwrap();
        assert_eq!(t.source, "manual");
        assert_eq!(t.hits, 7, "usage_count seeds hits");
        assert_eq!(t.aliases, vec!["tel-uh-type".to_string()]);

        // Idempotent: re-import the same words, no duplicates, count stable.
        let n2 = store.import_from_dictionary(&words).unwrap();
        assert_eq!(n2, 2);
        assert_eq!(store.lexicon_all().unwrap().len(), 2);
        // Aliases still preserved after re-import.
        assert_eq!(
            store.lexicon_by_term("Teletype").unwrap().unwrap().aliases,
            vec!["tel-uh-type".to_string()]
        );
    }

    #[test]
    fn personal_accuracy_roundtrip_and_history_ordering() {
        let (_dir, store) = tmp_store();
        store.record_personal_accuracy(100, 0.5, 0.4, 10).unwrap();
        store.record_personal_accuracy(200, 0.4, 0.3, 20).unwrap();
        store.record_personal_accuracy(300, 0.3, 0.2, 30).unwrap();

        // Newest first.
        let all = store.personal_accuracy_history(10).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].ts, 300);
        assert_eq!(all[0].clips, 30);
        assert!((all[0].wer_before - 0.3).abs() < 1e-9);
        assert!((all[0].wer_after - 0.2).abs() < 1e-9);
        assert_eq!(all[2].ts, 100);

        // Limit honored.
        assert_eq!(store.personal_accuracy_history(2).unwrap().len(), 2);
        assert!(store.personal_accuracy_history(0).unwrap().is_empty());

        // Same ts replaces the row (upsert), does not duplicate.
        store.record_personal_accuracy(200, 0.9, 0.8, 99).unwrap();
        assert_eq!(store.personal_accuracy_history(10).unwrap().len(), 3);
        let r200 = store
            .personal_accuracy_history(10)
            .unwrap()
            .into_iter()
            .find(|r| r.ts == 200)
            .unwrap();
        assert_eq!(r200.clips, 99);
    }

    #[test]
    fn learn_candidate_bump_math_and_negative_only() {
        let (_dir, store) = tmp_store();
        // Positive observations accumulate in both count and total.
        let (c, t) = store
            .bump_learn_candidate("mishearing", "alias", "Term", "", true, 100)
            .unwrap();
        assert_eq!((c, t), (1, 1));
        let (c, t) = store
            .bump_learn_candidate("mishearing", "alias", "Term", "", true, 200)
            .unwrap();
        assert_eq!((c, t), (2, 2));

        // Negative observations bump total but leave count untouched.
        let (c, t) = store
            .bump_learn_candidate("style", "cat", "rule", "", false, 300)
            .unwrap();
        assert_eq!((c, t), (0, 1), "negative-only: count stays 0");
        let (c, t) = store
            .bump_learn_candidate("style", "cat", "rule", "", false, 400)
            .unwrap();
        assert_eq!((c, t), (0, 2));
        let (c, t) = store
            .bump_learn_candidate("style", "cat", "rule", "", true, 500)
            .unwrap();
        assert_eq!((c, t), (1, 3));

        // Distinct (kind, a, b, category) quadruples are independent.
        assert_eq!(
            store
                .learn_candidate_get("mishearing", "alias", "Term", "")
                .unwrap(),
            Some((2, 2))
        );
        assert_eq!(
            store
                .learn_candidate_get("mishearing", "alias", "Term", "chat")
                .unwrap(),
            None
        );
        assert_eq!(
            store
                .learn_candidate_get("style", "cat", "rule", "other")
                .unwrap(),
            None
        );

        // last_seen advances with each bump.
        store
            .bump_learn_candidate("mishearing", "alias", "Term", "", true, 900)
            .unwrap();
        let (c, t) = store
            .learn_candidate_get("mishearing", "alias", "Term", "")
            .unwrap()
            .unwrap();
        assert_eq!((c, t), (3, 3));
    }

    #[test]
    fn learn_candidate_clear_removes_row() {
        let (_dir, store) = tmp_store();
        store
            .bump_learn_candidate("mishearing", "a", "b", "", true, 1)
            .unwrap();
        assert!(store
            .learn_candidate_get("mishearing", "a", "b", "")
            .unwrap()
            .is_some());
        store
            .clear_learn_candidate("mishearing", "a", "b", "")
            .unwrap();
        assert!(store
            .learn_candidate_get("mishearing", "a", "b", "")
            .unwrap()
            .is_none());
        // Clearing an absent row is a no-op, not an error.
        store
            .clear_learn_candidate("mishearing", "a", "b", "")
            .unwrap();
        // Re-bump starts from scratch.
        let (c, t) = store
            .bump_learn_candidate("mishearing", "a", "b", "", true, 2)
            .unwrap();
        assert_eq!((c, t), (1, 1));
    }

    #[test]
    fn learn_candidate_survives_reopen() {
        let (dir, store) = tmp_store();
        store
            .bump_learn_candidate("mishearing", "alias", "Term", "", true, 100)
            .unwrap();
        store
            .bump_learn_candidate("style", "cat", "rule", "", false, 200)
            .unwrap();
        let path = dir.path().join("learning.sqlite");
        let store2 = LearningStore::open(&path).unwrap();
        assert_eq!(
            store2
                .learn_candidate_get("mishearing", "alias", "Term", "")
                .unwrap(),
            Some((1, 1))
        );
        assert_eq!(
            store2
                .learn_candidate_get("style", "cat", "rule", "")
                .unwrap(),
            Some((0, 1))
        );
    }

    proptest::proptest! {
        #[test]
        fn prune_never_deletes_rows_newer_than_cutoff(
            old_offset_days in 91..=400,
            new_offset_days in 0..=89,
        ) {
            let (dir, store) = tmp_store();
            let now = crate::storage::now_ms();
            let day = 24 * 60 * 60 * 1000u64;
            // Use a 1-minute safety margin so wall-clock drift between the
            // test setup and the prune call cannot flip the boundary.
            let margin = 60_000u64;
            let old_ts = now.saturating_sub((old_offset_days as u64) * day + margin);
            let new_ts = now.saturating_sub((new_offset_days as u64) * day + margin);
            let old_id = store.record_dictation(&dictation(0, old_ts)).unwrap();
            let new_id = store.record_dictation(&dictation(0, new_ts)).unwrap();

            let deleted = store.prune_dictation(90).unwrap();

            // The newer row must survive no matter what.
            assert!(store.dictation_by_id(new_id).unwrap().is_some(),
                    "row {new_offset_days}d old was deleted but should survive");
            // The old row is deleted (it is strictly older than 90 days).
            assert!(store.dictation_by_id(old_id).unwrap().is_none(),
                    "row {old_offset_days}d old should have been pruned");
            assert_eq!(deleted, 1);
            let _ = dir;
        }
    }
}
