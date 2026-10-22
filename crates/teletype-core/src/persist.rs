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

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::{params, Connection};

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
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| e.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| e.to_string())?;
    create_tables(&mut conn)?;
    migrate_from_json(&mut conn, dir)?;
    Ok(conn)
}

fn create_tables(conn: &mut Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            id TEXT PRIMARY KEY,
            created_at INTEGER NOT NULL,
            text TEXT NOT NULL,
            context_json TEXT,
            duration_ms INTEGER
        );
        CREATE TABLE IF NOT EXISTS usage (
            kind TEXT NOT NULL,
            key TEXT NOT NULL,
            count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (kind, key)
        );",
    )
    .map_err(|e| e.to_string())
}

/// One-time JSON→SQLite migration. Imports when the SQLite table is empty
/// and the legacy JSON file exists, then renames the file to
/// `<name>.migrated.json`.
fn migrate_from_json(conn: &mut Connection, dir: &Path) -> Result<(), String> {
    migrate_history(conn, dir)?;
    migrate_usage(conn, dir)
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
        let context_json = entry
            .context
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT OR REPLACE INTO history (id, created_at, text, context_json, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                entry.id,
                entry.created_at as i64,
                entry.text,
                context_json,
                entry.duration_ms.map(|d| d as i64),
            ],
        )
        .map_err(|e| e.to_string())?;
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
    Ok(DictationEntry {
        id: row.get(0)?,
        created_at: row.get::<_, i64>(1)? as u64,
        text: row.get(2)?,
        context,
        duration_ms: row.get::<_, Option<i64>>(4)?.map(|d| d as u64),
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
        self.conn
            .execute(
                "INSERT OR REPLACE INTO history (id, created_at, text, context_json, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    entry.id,
                    entry.created_at as i64,
                    entry.text,
                    entry
                        .context
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()
                        .map_err(|e| e.to_string())?,
                    entry.duration_ms.map(|d| d as i64),
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
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
            .prepare(
                "SELECT id, created_at, text, context_json, duration_ms
                      FROM history ORDER BY created_at DESC, id DESC LIMIT ?1",
            )
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
            .prepare(
                "SELECT id, created_at, text, context_json, duration_ms
                      FROM history ORDER BY created_at DESC, id DESC",
            )
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
            .prepare(
                "SELECT id, created_at, text, context_json, duration_ms
                      FROM history WHERE id = ?1",
            )
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
pub fn read_history(
    dir: &Path,
    window: Option<(u64, u64)>,
) -> Result<Vec<DictationEntry>, String> {
    let path = dir.join("teletype.db");
    if !path.exists() {
        return Ok(vec![]);
    }
    let conn = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|e| e.to_string())?;

    // Return empty if the history table does not exist yet.
    let table_exists: bool = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='history'")
        .map_err(|e| e.to_string())?
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .next()
        .map(|r| r.is_ok())
        .unwrap_or(false);
    if !table_exists {
        return Ok(vec![]);
    }

    let sql = match window {
        Some(_) => {
            "SELECT id, created_at, text, context_json, duration_ms
             FROM history WHERE created_at >= ?1 AND created_at <= ?2
             ORDER BY created_at DESC, id DESC"
        }
        None => {
            "SELECT id, created_at, text, context_json, duration_ms
             FROM history
             ORDER BY created_at DESC, id DESC"
        },
    };
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
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
    fn migration_noop_without_json() {
        let dir = tempfile::TempDir::new().unwrap();
        let s = PersistStores::open(dir.path()).unwrap();
        assert!(s.history_all().unwrap().is_empty());
        assert!(s.usage_load().unwrap().filler_counts.is_empty());
    }
}
