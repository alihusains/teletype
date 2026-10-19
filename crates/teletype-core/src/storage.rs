//! Local JSON persistence.
//!
//! One file per concern in the app config directory, atomic writes (write to
//! `.tmp`, rename), corrupt files moved aside rather than silently overwritten.
//! Sensitive fields (AutoText replacements) live in the same files but are
//! never logged.

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Milliseconds since the epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// How many old corrupt files are kept when quarantining (`0..=N-1` suffixes,
/// plus the current `.corrupt`).
const CORRUPT_KEPT: usize = 3;

/// A small key-value-ish store: one JSON document per file.
pub struct JsonStore<T: Serialize + for<'de> Deserialize<'de>> {
    path: PathBuf,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Serialize + for<'de> Deserialize<'de>> JsonStore<T> {
    pub fn new(dir: &Path, name: &str) -> Self {
        Self {
            path: dir.join(name),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Loads the document, or `default` when missing/unreadable. A corrupt
    /// file is rotated to `<name>.corrupt` (older ones to `.corrupt.1` …) so
    /// the next save can't clobber it.
    pub fn load(&self, default: T) -> T {
        match fs::read(&self.path) {
            Ok(bytes) => match serde_json::from_slice(&bytes) {
                Ok(doc) => doc,
                Err(e) => {
                    tracing::warn!(
                        path = %self.path.display(),
                        error = %e,
                        "store file unreadable; moved aside"
                    );
                    quarantine(&self.path);
                    default
                }
            },
            Err(_) => default,
        }
    }

    /// Saves atomically (single serialization, fsync before the rename).
    pub fn save(&self, doc: &T) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Couldn't create {}: {e}", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(doc)
            .map_err(|e| format!("Couldn't serialize {}: {e}", self.path.display()))?;
        // Write to a per-process temp name so parallel saves of the same
        // file from different processes cannot clobber each other's tmp.
        let tmp = self.path.with_file_name(format!(
            "{}.tmp.{}",
            self.path.file_name().unwrap_or_default().to_string_lossy(),
            std::process::id()
        ));
        let mut file = std::fs::File::create(&tmp)
            .map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
        file.write_all(&bytes)
            .map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
        // fsync before rename so a crash cannot leave a zero-length file at
        // the final path (B4).
        file.sync_all()
            .map_err(|e| format!("Couldn't sync {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &self.path)
            .map_err(|e| format!("Couldn't save {}: {e}", self.path.display()))?;
        Ok(())
    }
}

/// Moves `path` aside to `<name>.corrupt`, rotating older quarantines to
/// `.corrupt.1` … `.corrupt.{CORRUPT_KEPT - 1}`; anything beyond the cap is
/// deleted.
fn quarantine(path: &Path) {
    // Shift existing quarantines one generation older first (2→3, 1→2, 0→1);
    // anything that lands beyond the cap is deleted afterwards.
    for i in (0..CORRUPT_KEPT).rev() {
        let older = corrupt_path(path, i);
        if older.exists() {
            let _ = fs::rename(&older, corrupt_path(path, i + 1));
        }
    }
    let oldest = corrupt_path(path, CORRUPT_KEPT);
    if oldest.exists() {
        let _ = fs::remove_file(oldest);
    }
    let _ = fs::rename(path, corrupt_path(path, 0));
}

fn corrupt_path(path: &Path, generation: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".corrupt");
    if generation > 0 {
        name.push(format!(".{generation}"));
    }
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct Doc {
        #[serde(default)]
        value: String,
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        assert_eq!(store.load(Doc::default()), Doc::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        store
            .save(&Doc {
                value: "hello".into(),
            })
            .unwrap();
        assert_eq!(
            store.load(Doc::default()),
            Doc {
                value: "hello".into()
            }
        );
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        fs::write(store.path.clone(), b"{not json").unwrap();
        assert_eq!(store.load(Doc::default()), Doc::default());
        assert!(
            store.path.with_extension("json.corrupt").exists()
                || dir.path().join("d.json.corrupt").exists()
        );
    }

    #[test]
    fn save_writes_single_serialization() {
        // save() must serialize exactly once: writing the same doc twice must
        // yield byte-identical files, and no tmp file may linger.
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        let doc = Doc { value: "x".into() };
        store.save(&doc).unwrap();
        let first = fs::read(&store.path).unwrap();
        store.save(&doc).unwrap();
        let second = fs::read(&store.path).unwrap();
        assert_eq!(first, second);
        assert_eq!(first, serde_json::to_vec_pretty(&doc).unwrap());
        let leftovers: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "tmp file must not linger: {leftovers:?}"
        );
    }

    #[test]
    fn quarantine_rotates_and_caps() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        let mark = |v: &str| format!("{{not json {v}");

        // Four corrupt loads: generations 0..3 produced, cap keeps 0, 1, 2.
        for v in ["a", "b", "c", "d"] {
            fs::write(&store.path, mark(v)).unwrap();
            assert_eq!(store.load(Doc::default()), Doc::default());
        }
        assert!(dir.path().join("d.json.corrupt").exists());
        assert!(dir.path().join("d.json.corrupt.1").exists());
        assert!(dir.path().join("d.json.corrupt.2").exists());
        assert!(!dir.path().join("d.json.corrupt.3").exists());
        // Latest corrupt content ("d") sits at generation 0, oldest kept ("b")
        // at generation 2.
        assert_eq!(
            fs::read_to_string(dir.path().join("d.json.corrupt")).unwrap(),
            mark("d")
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("d.json.corrupt.2")).unwrap(),
            mark("b")
        );
    }

    #[test]
    fn repeated_corrupt_loads_never_crash() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        for _ in 0..5 {
            fs::write(&store.path, b"{not json").unwrap();
            assert_eq!(store.load(Doc::default()), Doc::default());
        }
        // Store still usable afterwards.
        store.save(&Doc { value: "ok".into() }).unwrap();
        assert_eq!(store.load(Doc::default()), Doc { value: "ok".into() });
    }
}
