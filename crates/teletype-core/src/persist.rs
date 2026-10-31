//! SQLite-backed persistence for dictation history and usage stats (A4).
//!
//! The in-memory [`crate::repo::Repo`] snapshots stay the read source of
//! truth; these stores are the durable backend. The desktop app runs a
//! single background writer thread (see `teletype_desktop::persist`) that
//! owns a `PersistStores` and applies changes off the dictation hot path.
//!
//! One-time migration: when the SQLite DB is empty and a legacy JSON file
//! (`dictation.json` / `usage.json`) exists, its rows are imported and the
//! file is renamed to `<name>.migrated.json` (never deleted, so a failed
//! migration is recoverable).
//!
//! `history.json` is reconciled on every open instead: live takes were saved
//! there (and never reached SQLite) until the bookkeeper moved to the
//! persistence actor, so any of its ids the database lacks are imported
//! once, then the file is renamed the same way.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection, TransactionBehavior};

use crate::context::ApplicationContext;
use crate::history::{DictationEntry, DictationHistory};
use crate::usage::UsageStats;

/// Cap on the in-memory/UI history snapshot (`history_all`). All rows are
/// retained durably; this cap only bounds the read snapshot.
pub const MAX_HISTORY_ENTRIES: usize = 1000;

/// Opens (or creates) the SQLite database at `dir/teletype.db` and runs the
/// one-time JSON migration. WAL mode and `synchronous=NORMAL` are set here so
/// the background writer can rely on them.
pub fn open(dir: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("teletype.db");
    let mut conn = Connection::open(&path).map_err(|e| e.to_string())?;
    // Two connections open the store at once at startup (the persistence
    // actor and the setup thread), so the migrations below wait on each
    // other's write lock instead of failing on it.
    conn.busy_timeout(OPEN_BUSY_TIMEOUT)
        .map_err(|e| e.to_string())?;
    enable_wal(&conn)?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    create_tables(&mut conn)?;
    migrate_from_json(&mut conn, dir)?;
    Ok(conn)
}

/// Switches the database to WAL (a no-op once it is). Changing the journal
/// mode needs the other connections idle, and SQLite answers "locked"
/// without running the busy handler, so a brand-new database opened by both
/// startup connections at once retries here instead of failing the open.
fn enable_wal(conn: &Connection) -> Result<(), String> {
    let deadline = std::time::Instant::now() + OPEN_BUSY_TIMEOUT;
    loop {
        match conn.pragma_update(None, "journal_mode", "WAL") {
            Ok(()) => return Ok(()),
            Err(rusqlite::Error::SqliteFailure(err, _))
                if matches!(
                    err.code,
                    rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                ) && std::time::Instant::now() < deadline =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// How long an open waits for another connection's migration to commit.
const OPEN_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Every `history` column, in the order [`entry_from_row`] reads them.
/// All four SELECTs use this list so a new column cannot be read by some
/// queries and not others.
const HISTORY_COLUMNS: &str = "id, created_at, text, context_json, duration_ms, \
     raw_text, edits_json, asr_model, polish_model, polish_ms, word_count, recording_path";

/// Columns added after the first release, with their SQL types. Existing
/// databases gain them through a guarded `ALTER TABLE ... ADD COLUMN`.
const HISTORY_ADDED_COLUMNS: [(&str, &str); 7] = [
    ("raw_text", "TEXT"),
    ("edits_json", "TEXT"),
    ("asr_model", "TEXT"),
    ("polish_model", "TEXT"),
    ("polish_ms", "INTEGER"),
    ("word_count", "INTEGER"),
    ("recording_path", "TEXT"),
];

/// True when a column already exists on a table (guards idempotent
/// `ALTER TABLE ... ADD COLUMN` for databases created before the column).
fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| e.to_string())?;
    let found = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .any(|c| c.as_deref() == Ok(column));
    Ok(found)
}

fn create_tables(conn: &mut Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            id TEXT PRIMARY KEY,
            created_at INTEGER NOT NULL,
            text TEXT NOT NULL,
            context_json TEXT,
            duration_ms INTEGER,
            raw_text TEXT,
            edits_json TEXT,
            asr_model TEXT,
            polish_model TEXT,
            polish_ms INTEGER,
            word_count INTEGER,
            recording_path TEXT
        );
        CREATE TABLE IF NOT EXISTS usage (
            kind TEXT NOT NULL,
            key TEXT NOT NULL,
            count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (kind, key)
        );",
    )
    .map_err(|e| e.to_string())?;
    // A database created before the dictation-detail columns lacks them.
    // `CREATE TABLE IF NOT EXISTS` leaves an existing table alone, so add
    // each missing column in place. Both startup connections run this at
    // the same time: the write lock is taken BEFORE the column check, so
    // the second one sees the first one's columns, and a "duplicate column"
    // that still slips through counts as done.
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    for (col, ty) in HISTORY_ADDED_COLUMNS {
        if column_exists(&tx, "history", col)? {
            continue;
        }
        match tx.execute_batch(&format!("ALTER TABLE history ADD COLUMN {col} {ty};")) {
            Ok(()) => {}
            Err(e) if e.to_string().contains("duplicate column name") => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

/// Inserts (or replaces) one history row. The live push and the legacy JSON
/// import share this, so the import can never lag behind a new column.
fn insert_history(conn: &Connection, entry: &DictationEntry) -> Result<(), String> {
    write_history(conn, entry, "INSERT OR REPLACE")
}

/// Writes one history row with `verb` (`INSERT OR REPLACE` or
/// `INSERT OR IGNORE`).
fn write_history(conn: &Connection, entry: &DictationEntry, verb: &str) -> Result<(), String> {
    let context_json = entry
        .context
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    let edits_json = entry
        .edits
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| e.to_string())?;
    conn.execute(
        &format!(
            "{verb} INTO history ({HISTORY_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
        ),
        params![
            entry.id,
            entry.created_at as i64,
            entry.text,
            context_json,
            entry.duration_ms.map(|d| d as i64),
            entry.raw_text,
            edits_json,
            entry.asr_model,
            entry.polish_model,
            entry.polish_ms.map(|d| d as i64),
            entry.word_count.map(i64::from),
            entry.recording_path,
        ],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

/// One-time JSON→SQLite migration. Imports when the SQLite table is empty
/// and the legacy JSON file exists, then renames the file to
/// `<name>.migrated.json`.
fn migrate_from_json(conn: &mut Connection, dir: &Path) -> Result<(), String> {
    migrate_history(conn, dir)?;
    reconcile_history_json(conn, dir)?;
    migrate_usage(conn, dir)
}

/// Imports every `history.json` entry whose id the database lacks, then
/// renames the file to `history.json.migrated.json`. Unlike
/// [`migrate_history`] this runs on a non-empty table: those entries are
/// live takes that only ever reached the JSON file. A row the database
/// already has is left as it is (`INSERT OR IGNORE`).
///
/// Safe beside a second connection doing the same: the import is one
/// immediate transaction, and a rename the other side already did is fine.
fn reconcile_history_json(conn: &mut Connection, dir: &Path) -> Result<(), String> {
    let json_path = dir.join("history.json");
    let bytes = match std::fs::read(&json_path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("history.json: {}", e.kind())),
    };
    let doc: DictationHistory = match serde_json::from_slice(&bytes) {
        Ok(doc) => doc,
        Err(e) => {
            // Unreadable: quarantine it so the next open does not loop on
            // it. The database is still good, so the open goes on.
            tracing::warn!("persist: history.json is unreadable, quarantined: {e}");
            let _ = std::fs::rename(&json_path, dir.join("history.json.corrupt"));
            return Ok(());
        }
    };
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    for entry in &doc.entries {
        write_history(&tx, entry, "INSERT OR IGNORE")?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    match std::fs::rename(&json_path, dir.join("history.json.migrated.json")) {
        Ok(()) => Ok(()),
        // The other startup connection renamed it first.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("migration: could not rename history.json: {e}")),
    }
}

fn migrate_history(conn: &mut Connection, dir: &Path) -> Result<(), String> {
    let json_path = dir.join("dictation.json");
    if !json_path.exists() {
        return Ok(());
    }
    let empty: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if empty != 0 {
        // Already migrated (or hand-populated): leave the file alone.
        return Ok(());
    }
    let bytes = std::fs::read(&json_path).map_err(|e| e.to_string())?;
    let doc: DictationHistory = serde_json::from_slice(&bytes).map_err(|e| {
        // Unreadable legacy file: quarantine it so the next open does not
        // loop on it, and start fresh.
        let _ = std::fs::rename(&json_path, dir.join("dictation.json.corrupt"));
        e.to_string()
    })?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for entry in &doc.entries {
        insert_history(&tx, entry)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    std::fs::rename(&json_path, dir.join("dictation.json.migrated.json"))
        .map_err(|e| format!("migration: could not rename dictation.json: {e}"))?;
    Ok(())
}

fn migrate_usage(conn: &mut Connection, dir: &Path) -> Result<(), String> {
    let json_path = dir.join("usage.json");
    if !json_path.exists() {
        return Ok(());
    }
    let empty: i64 = conn
        .query_row("SELECT COUNT(*) FROM usage", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if empty != 0 {
        return Ok(());
    }
    let bytes = std::fs::read(&json_path).map_err(|e| e.to_string())?;
    let doc: UsageStats = serde_json::from_slice(&bytes).map_err(|e| {
        let _ = std::fs::rename(&json_path, dir.join("usage.json.corrupt"));
        e.to_string()
    })?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    for (key, count) in &doc.filler_counts {
        tx.execute(
            "INSERT INTO usage (kind, key, count) VALUES ('filler', ?1, ?2)
             ON CONFLICT(kind, key) DO UPDATE SET count = count + excluded.count",
            params![key, *count as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    for (key, count) in &doc.autotext_counts {
        tx.execute(
            "INSERT INTO usage (kind, key, count) VALUES ('autotext', ?1, ?2)
             ON CONFLICT(kind, key) DO UPDATE SET count = count + excluded.count",
            params![key, *count as i64],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    std::fs::rename(&json_path, dir.join("usage.json.migrated.json"))
        .map_err(|e| format!("migration: could not rename usage.json: {e}"))?;
    Ok(())
}

fn entry_from_row(row: &rusqlite::Row) -> rusqlite::Result<DictationEntry> {
    let context_json: Option<String> = row.get(3)?;
    let context = context_json.and_then(|s| serde_json::from_str::<ApplicationContext>(&s).ok());
    // Edits that do not parse read as none: a bad diff never fails the row.
    let edits_json: Option<String> = row.get(6)?;
    let edits = edits_json.and_then(|s| serde_json::from_str(&s).ok());
    Ok(DictationEntry {
        id: row.get(0)?,
        created_at: row.get::<_, i64>(1)? as u64,
        text: row.get(2)?,
        context,
        duration_ms: row.get::<_, Option<i64>>(4)?.map(|d| d as u64),
        raw_text: row.get(5)?,
        edits,
        asr_model: row.get(7)?,
        polish_model: row.get(8)?,
        polish_ms: row.get::<_, Option<i64>>(9)?.map(|d| d.max(0) as u64),
        word_count: row
            .get::<_, Option<i64>>(10)?
            .map(|d| d.clamp(0, u32::MAX as i64) as u32),
        recording_path: row.get(11)?,
    })
}

/// The SQLite pair of stores behind the background writer. Both share one
/// connection (one writer thread, one open DB).
pub struct PersistStores {
    conn: Connection,
}

impl PersistStores {
    /// Opens the DB at `dir/teletype.db`, runs the one-time JSON migration.
    pub fn open(dir: &Path) -> Result<Self, String> {
        Ok(Self { conn: open(dir)? })
    }

    /// Pushes one history entry (newest first). All rows are retained so
    /// lifetime views stay complete.
    pub fn push_history(&mut self, entry: &DictationEntry) -> Result<(), String> {
        insert_history(&self.conn, entry)
    }

    /// Forgets every stored recording file name, leaving the rest of each
    /// row alone. Runs after the recordings directory was emptied.
    pub fn clear_recording_paths(&mut self) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE history SET recording_path = NULL WHERE recording_path IS NOT NULL",
                [],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Removes one history entry by id.
    pub fn remove_history(&mut self, id: &str) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM history WHERE id = ?1", params![id])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// All history entries, newest first, capped at [`MAX_HISTORY_ENTRIES`].
    pub fn history_all(&self) -> Result<Vec<DictationEntry>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {HISTORY_COLUMNS}
                      FROM history ORDER BY created_at DESC, id DESC LIMIT ?1"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![MAX_HISTORY_ENTRIES as i64], entry_from_row)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Every history entry, newest first, no cap. Used by lifetime views so
    /// they see the full history beyond the [`MAX_HISTORY_ENTRIES`] snapshot.
    pub fn history_full(&self) -> Result<Vec<DictationEntry>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {HISTORY_COLUMNS}
                      FROM history ORDER BY created_at DESC, id DESC"
            ))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], entry_from_row)
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// One history entry by id.
    pub fn history_get(&self, id: &str) -> Result<Option<DictationEntry>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {HISTORY_COLUMNS} FROM history WHERE id = ?1"
            ))
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query_map(params![id], entry_from_row)
            .map_err(|e| e.to_string())?;
        match rows.next() {
            Some(Ok(entry)) => Ok(Some(entry)),
            Some(Err(e)) => Err(e.to_string()),
            None => Ok(None),
        }
    }

    /// Adds `filler` / `autotext` counts to the stored counters.
    pub fn record_usage(
        &mut self,
        filler: &BTreeMap<String, u32>,
        autotext: &BTreeMap<String, u32>,
    ) -> Result<(), String> {
        let tx = self.conn.transaction().map_err(|e| e.to_string())?;
        for (key, count) in filler {
            if *count == 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO usage (kind, key, count) VALUES ('filler', ?1, ?2)
                 ON CONFLICT(kind, key) DO UPDATE SET count = count + excluded.count",
                params![key, *count as i64],
            )
            .map_err(|e| e.to_string())?;
        }
        for (key, count) in autotext {
            if *count == 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO usage (kind, key, count) VALUES ('autotext', ?1, ?2)
                 ON CONFLICT(kind, key) DO UPDATE SET count = count + excluded.count",
                params![key, *count as i64],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// Loads the full usage document.
    pub fn usage_load(&self) -> Result<UsageStats, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, key, count FROM usage")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut stats = UsageStats::default();
        for row in rows {
            let (kind, key, count) = row.map_err(|e| e.to_string())?;
            let count = count.max(0) as u32;
            match kind.as_str() {
                "filler" => {
                    stats.filler_counts.insert(key, count);
                }
                "autotext" => {
                    stats.autotext_counts.insert(key, count);
                }
                _ => {}
            }
        }
        Ok(stats)
    }

    /// Drops all usage counters.
    pub fn usage_reset(&mut self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM usage", [])
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Drops filler counters for words no longer in the active list
    /// (case-insensitive, matching `UsageStats::prune_fillers`).
    pub fn usage_prune_fillers(&mut self, active: &[String]) -> Result<(), String> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM usage WHERE kind = 'filler'")
            .map_err(|e| e.to_string())?;
        let keys: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        let mut lowered: Vec<String> = Vec::with_capacity(active.len());
        for w in active {
            lowered.push(w.to_lowercase());
        }
        for key in keys {
            if !lowered.iter().any(|w| w == &key) {
                self.conn
                    .execute(
                        "DELETE FROM usage WHERE kind = 'filler' AND key = ?1",
                        params![key],
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Drops usage counters whose keys are not in `known`
    /// (matching `UsageStats::prune_autotext`).
    pub fn usage_prune_autotext(&mut self, known: &BTreeMap<String, ()>) -> Result<(), String> {
        let mut stmt = self
            .conn
            .prepare("SELECT key FROM usage WHERE kind = 'autotext'")
            .map_err(|e| e.to_string())?;
        let keys: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        for key in keys {
            if !known.contains_key(&key) {
                self.conn
                    .execute(
                        "DELETE FROM usage WHERE kind = 'autotext' AND key = ?1",
                        params![key],
                    )
                    .map(|_| ())
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

/// Read-only history load, safe to call concurrently with the background
/// writer (WAL). Returns `Ok(vec![])` when the database or the `history`
/// table does not exist yet. `window` optionally filters by `created_at`
/// inclusive bounds. Rows are returned newest first.
pub fn read_history(dir: &Path, window: Option<(u64, u64)>) -> Result<Vec<DictationEntry>, String> {
    let Some(conn) = open_read_only(dir)? else {
        return Ok(vec![]);
    };
    let sql = match window {
        Some(_) => format!(
            "SELECT {HISTORY_COLUMNS}
             FROM history WHERE created_at >= ?1 AND created_at <= ?2
             ORDER BY created_at DESC, id DESC"
        ),
        None => format!(
            "SELECT {HISTORY_COLUMNS}
             FROM history
             ORDER BY created_at DESC, id DESC"
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = match window {
        Some((from, to)) => stmt
            .query_map(params![from as i64, to as i64], entry_from_row)
            .map_err(|e| e.to_string())?,
        None => stmt
            .query_map([], entry_from_row)
            .map_err(|e| e.to_string())?,
    };
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

/// Read-only lookup of one history entry by id, for an entry older than the
/// in-memory snapshot. `Ok(None)` when the database, the table or the row
/// does not exist.
pub fn read_history_entry(dir: &Path, id: &str) -> Result<Option<DictationEntry>, String> {
    let Some(conn) = open_read_only(dir)? else {
        return Ok(None);
    };
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {HISTORY_COLUMNS} FROM history WHERE id = ?1"
        ))
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![id], entry_from_row)
        .map_err(|e| e.to_string())?;
    match rows.next() {
        Some(Ok(entry)) => Ok(Some(entry)),
        Some(Err(e)) => Err(e.to_string()),
        None => Ok(None),
    }
}

/// Opens `dir/teletype.db` read-only (safe beside the WAL writer). `None`
/// when the database or a `history` table with the current columns does
/// not exist yet: the writer adds missing columns on its next open.
fn open_read_only(dir: &Path) -> Result<Option<Connection>, String> {
    let path = dir.join("teletype.db");
    if !path.exists() {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(
        &path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|e| e.to_string())?;
    // An unmigrated table would fail every SELECT on the new columns. Each
    // one is checked: a dev build once added `recording_path` alone, so no
    // single column can stand in for the rest.
    for (col, _) in HISTORY_ADDED_COLUMNS {
        if !column_exists(&conn, "history", col)? {
            return Ok(None);
        }
    }
    Ok(Some(conn))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, at: u64) -> DictationEntry {
        DictationEntry {
            id: id.into(),
            created_at: at,
            text: format!("text {id}"),
            context: Some(ApplicationContext {
                application_name: format!("App {id}"),
                ..Default::default()
            }),
            duration_ms: Some(123),
            ..Default::default()
        }
    }

    #[test]
    fn push_all_newest_first_and_capped() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut s = PersistStores::open(dir.path()).unwrap();
        for i in 0..(MAX_HISTORY_ENTRIES + 5) {
            s.push_history(&entry(&i.to_string(), i as u64)).unwrap();
        }
        // history_all is still capped at 1000, newest first.
        let all = s.history_all().unwrap();
        assert_eq!(all.len(), MAX_HISTORY_ENTRIES);
        assert_eq!(all[0].id, (MAX_HISTORY_ENTRIES + 4).to_string());
        // history_full returns every row, including the 5 oldest.
        let full = s.history_full().unwrap();
        assert_eq!(full.len(), MAX_HISTORY_ENTRIES + 5);
        assert!(full.iter().any(|e| e.id == "0"));
        assert!(full.iter().any(|e| e.id == "4"));
    }

    #[test]
    fn read_history_sees_writer_rows_and_filters() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut s = PersistStores::open(dir.path()).unwrap();
        s.push_history(&entry("a", 100)).unwrap();
        s.push_history(&entry("b", 200)).unwrap();
        s.push_history(&entry("c", 300)).unwrap();

        // None: all rows, newest first.
        let all = read_history(dir.path(), None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, "c");
        assert_eq!(all[2].id, "a");

        // Some: only rows within [150, 250].
        let filtered = read_history(dir.path(), Some((150, 250))).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "b");

        // Empty dir with no database.
        let empty = tempfile::TempDir::new().unwrap();
        let none = read_history(empty.path(), None).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn remove_and_get() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut s = PersistStores::open(dir.path()).unwrap();
        s.push_history(&entry("a", 1)).unwrap();
        s.push_history(&entry("b", 2)).unwrap();
        s.remove_history("a").unwrap();
        assert!(s.history_get("a").unwrap().is_none());
        let b = s.history_get("b").unwrap().unwrap();
        assert_eq!(b.text, "text b");
        assert_eq!(
            b.context.as_ref().map(|c| c.application_name.clone()),
            Some("App b".into())
        );
        assert_eq!(b.duration_ms, Some(123));
        assert_eq!(s.history_all().unwrap().len(), 1);
    }

    #[test]
    fn usage_record_load_prune_reset() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut s = PersistStores::open(dir.path()).unwrap();
        let mut f = BTreeMap::new();
        f.insert("um".to_string(), 2);
        f.insert("er".to_string(), 1);
        let mut a = BTreeMap::new();
        a.insert("/email".to_string(), 3);
        a.insert("/phone".to_string(), 1);
        s.record_usage(&f, &a).unwrap();
        let mut f2 = BTreeMap::new();
        f2.insert("um".to_string(), 2);
        s.record_usage(&f2, &BTreeMap::new()).unwrap();

        let stats = s.usage_load().unwrap();
        assert_eq!(stats.filler_counts.get("um"), Some(&4));
        assert_eq!(stats.filler_counts.get("er"), Some(&1));
        assert_eq!(stats.autotext_counts.get("/email"), Some(&3));

        // Prune fillers not in the active list.
        s.usage_prune_fillers(&["um".to_string()]).unwrap();
        let stats = s.usage_load().unwrap();
        assert_eq!(stats.filler_counts.get("um"), Some(&4));
        assert_eq!(stats.filler_counts.get("er"), None);

        // Prune autotext keys that no longer exist.
        let mut known = BTreeMap::new();
        known.insert("/email".to_string(), ());
        s.usage_prune_autotext(&known).unwrap();
        let stats = s.usage_load().unwrap();
        assert_eq!(stats.autotext_counts.get("/email"), Some(&3));
        assert_eq!(stats.autotext_counts.get("/phone"), None);

        s.usage_reset().unwrap();
        let stats = s.usage_load().unwrap();
        assert!(stats.filler_counts.is_empty());
        assert!(stats.autotext_counts.is_empty());
    }

    #[test]
    fn json_migration_imports_and_renames() {
        let dir = tempfile::TempDir::new().unwrap();
        let history = DictationHistory {
            entries: vec![entry("h1", 100), entry("h2", 200)],
        };
        std::fs::write(
            dir.path().join("dictation.json"),
            serde_json::to_vec(&history).unwrap(),
        )
        .unwrap();
        let usage = UsageStats {
            filler_counts: {
                let mut m = BTreeMap::new();
                m.insert("um".to_string(), 7);
                m
            },
            autotext_counts: {
                let mut m = BTreeMap::new();
                m.insert("/email".to_string(), 5);
                m
            },
        };
        std::fs::write(
            dir.path().join("usage.json"),
            serde_json::to_vec(&usage).unwrap(),
        )
        .unwrap();

        let s = PersistStores::open(dir.path()).unwrap();
        let all = s.history_all().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "h2", "newest first");
        assert_eq!(all[1].id, "h1");
        let stats = s.usage_load().unwrap();
        assert_eq!(stats.filler_counts.get("um"), Some(&7));
        assert_eq!(stats.autotext_counts.get("/email"), Some(&5));

        // JSON renamed, not deleted.
        assert!(dir.path().join("dictation.json.migrated.json").exists());
        assert!(dir.path().join("usage.json.migrated.json").exists());
        assert!(!dir.path().join("dictation.json").exists());
        assert!(!dir.path().join("usage.json").exists());

        // A second open does not re-migrate (no JSON left, tables non-empty).
        let s2 = PersistStores::open(dir.path()).unwrap();
        assert_eq!(s2.history_all().unwrap().len(), 2);
    }

    #[test]
    fn old_schema_rows_load_after_migration() {
        let dir = tempfile::TempDir::new().unwrap();
        {
            let conn = Connection::open(dir.path().join("teletype.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE history (
                    id TEXT PRIMARY KEY,
                    created_at INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    context_json TEXT,
                    duration_ms INTEGER
                );
                INSERT INTO history (id, created_at, text, context_json, duration_ms)
                VALUES ('old', 7, 'old text', NULL, 1500);",
            )
            .unwrap();
        }
        // Before the writer migrates, a read sees no usable table.
        assert!(read_history(dir.path(), None).unwrap().is_empty());

        let s = PersistStores::open(dir.path()).unwrap();
        for (col, _) in HISTORY_ADDED_COLUMNS {
            assert!(column_exists(&s.conn, "history", col).unwrap(), "{col}");
        }
        let old = s.history_get("old").unwrap().unwrap();
        assert_eq!(old.text, "old text");
        assert_eq!(old.duration_ms, Some(1500));
        assert!(old.raw_text.is_none());
        assert!(old.edits.is_none());
        assert!(old.asr_model.is_none());
        assert!(old.polish_model.is_none());
        assert!(old.polish_ms.is_none());
        assert!(old.word_count.is_none());
        assert!(old.recording_path.is_none());
        assert_eq!(read_history(dir.path(), None).unwrap().len(), 1);
        drop(s);

        // A second open is a no-op.
        let s2 = PersistStores::open(dir.path()).unwrap();
        assert_eq!(s2.history_all().unwrap().len(), 1);
    }

    fn rich_entry(id: &str) -> DictationEntry {
        use crate::history_edits::{SpanKind, TextSpan};
        DictationEntry {
            raw_text: Some("hey um there".into()),
            edits: Some(vec![
                TextSpan {
                    kind: SpanKind::Same,
                    from: "hey".into(),
                    to: String::new(),
                    minor: false,
                    superseded: false,
                },
                TextSpan {
                    kind: SpanKind::Filler,
                    from: "um".into(),
                    to: String::new(),
                    minor: false,
                    superseded: false,
                },
            ]),
            asr_model: Some("Parakeet v3".into()),
            polish_model: Some("Qwen3 4B".into()),
            polish_ms: Some(420),
            word_count: Some(2),
            recording_path: Some(format!("{id}.wav")),
            ..entry(id, 5)
        }
    }

    #[test]
    fn new_fields_round_trip() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut s = PersistStores::open(dir.path()).unwrap();
        let e = rich_entry("r1");
        s.push_history(&e).unwrap();
        let back = s.history_get("r1").unwrap().unwrap();
        assert_eq!(back.raw_text, e.raw_text);
        assert_eq!(back.edits, e.edits);
        assert_eq!(back.asr_model, e.asr_model);
        assert_eq!(back.polish_model, e.polish_model);
        assert_eq!(back.polish_ms, Some(420));
        assert_eq!(back.word_count, Some(2));
        assert_eq!(back.recording_path.as_deref(), Some("r1.wav"));
        let read = read_history_entry(dir.path(), "r1").unwrap().unwrap();
        assert_eq!(read.edits, e.edits);
        assert!(read_history_entry(dir.path(), "missing").unwrap().is_none());
    }

    #[test]
    fn clear_recording_paths_nulls_only_that_column() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut s = PersistStores::open(dir.path()).unwrap();
        s.push_history(&rich_entry("a")).unwrap();
        s.push_history(&entry("b", 9)).unwrap();
        s.clear_recording_paths().unwrap();
        let a = s.history_get("a").unwrap().unwrap();
        assert!(a.recording_path.is_none());
        assert_eq!(a.asr_model.as_deref(), Some("Parakeet v3"));
        assert_eq!(a.raw_text.as_deref(), Some("hey um there"));
        assert!(a.edits.is_some());
        assert_eq!(s.history_all().unwrap().len(), 2);
    }

    #[test]
    fn bad_edits_json_reads_as_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut s = PersistStores::open(dir.path()).unwrap();
        s.push_history(&rich_entry("x")).unwrap();
        s.conn
            .execute(
                "UPDATE history SET edits_json = '{not json' WHERE id = 'x'",
                [],
            )
            .unwrap();
        let x = s.history_get("x").unwrap().unwrap();
        assert!(x.edits.is_none());
        assert_eq!(x.text, "text x");
    }

    #[test]
    fn history_json_takes_missing_from_the_db_are_imported_once() {
        let dir = tempfile::TempDir::new().unwrap();
        {
            let mut s = PersistStores::open(dir.path()).unwrap();
            s.push_history(&entry("in-db", 100)).unwrap();
            s.push_history(&entry("both", 200)).unwrap();
        }
        // The retired JSON store holds one row the DB already has (with
        // other text) and two live takes that never reached the DB.
        let stranded = DictationHistory {
            entries: vec![
                DictationEntry {
                    text: "json copy".into(),
                    ..entry("both", 200)
                },
                entry("json-only-1", 300),
                entry("json-only-2", 400),
            ],
        };
        std::fs::write(
            dir.path().join("history.json"),
            serde_json::to_vec(&stranded).unwrap(),
        )
        .unwrap();

        let s = PersistStores::open(dir.path()).unwrap();
        let ids: Vec<String> = s.history_all().unwrap().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, vec!["json-only-2", "json-only-1", "both", "in-db"]);
        // A row the DB already had is not overwritten by the JSON copy.
        assert_eq!(s.history_get("both").unwrap().unwrap().text, "text both");
        assert!(!dir.path().join("history.json").exists());
        assert!(dir.path().join("history.json.migrated.json").exists());
        drop(s);

        // A second open has nothing left to import.
        let s2 = PersistStores::open(dir.path()).unwrap();
        assert_eq!(s2.history_all().unwrap().len(), 4);
    }

    #[test]
    fn unreadable_history_json_is_quarantined_and_the_open_succeeds() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("history.json"), b"{not json").unwrap();
        let s = PersistStores::open(dir.path()).unwrap();
        assert!(s.history_all().unwrap().is_empty());
        assert!(dir.path().join("history.json.corrupt").exists());
    }

    /// The desktop app opens the store from the persistence actor and the
    /// setup thread at the same moment. On a database that predates the
    /// detail columns, both used to race the check-then-ALTER and one
    /// failed with "duplicate column name".
    #[test]
    fn concurrent_opens_of_an_old_schema_all_succeed() {
        for _ in 0..5 {
            let dir = tempfile::TempDir::new().unwrap();
            {
                let conn = Connection::open(dir.path().join("teletype.db")).unwrap();
                conn.execute_batch(
                    "CREATE TABLE history (
                        id TEXT PRIMARY KEY,
                        created_at INTEGER NOT NULL,
                        text TEXT NOT NULL,
                        context_json TEXT,
                        duration_ms INTEGER
                    );",
                )
                .unwrap();
            }
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
            let handles: Vec<_> = (0..6)
                .map(|_| {
                    let dir = dir.path().to_path_buf();
                    let barrier = barrier.clone();
                    std::thread::spawn(move || {
                        barrier.wait();
                        PersistStores::open(&dir).map(|_| ())
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap().expect("every concurrent open succeeds");
            }
        }
    }

    #[test]
    fn read_only_needs_every_added_column() {
        let dir = tempfile::TempDir::new().unwrap();
        {
            // A dev build added `recording_path` alone.
            let conn = Connection::open(dir.path().join("teletype.db")).unwrap();
            conn.execute_batch(
                "CREATE TABLE history (
                    id TEXT PRIMARY KEY,
                    created_at INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    context_json TEXT,
                    duration_ms INTEGER,
                    recording_path TEXT
                );
                INSERT INTO history (id, created_at, text) VALUES ('old', 7, 'old text');",
            )
            .unwrap();
        }
        assert!(read_history(dir.path(), None).unwrap().is_empty());
        assert!(read_history_entry(dir.path(), "old").unwrap().is_none());
        // Once the writer migrates, the row reads normally.
        drop(PersistStores::open(dir.path()).unwrap());
        assert_eq!(read_history(dir.path(), None).unwrap().len(), 1);
    }

    #[test]
    fn migration_noop_without_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let s = PersistStores::open(dir.path()).unwrap();
        assert!(s.history_all().unwrap().is_empty());
        assert!(s.usage_load().unwrap().filler_counts.is_empty());
    }
}
