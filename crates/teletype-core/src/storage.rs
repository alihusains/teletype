//! Local JSON persistence.
//!
//! One file per concern in the app config directory, atomic writes (write to
//! `.tmp`, rename), corrupt files moved aside rather than silently overwritten.
//! Sensitive fields (AutoText replacements) live in the same files but are
//! never logged.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// Milliseconds since the epoch.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

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
    /// file is renamed to `<name>.corrupt` so the next save can't clobber it.
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
                    let _ = fs::rename(&self.path, unique_corrupt_path(&self.path));
                    default
                }
            },
            Err(_) => default,
        }
    }

    /// Saves atomically.
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

/// A corrupt-file quarantine path that does not overwrite an earlier
/// quarantine of the same file (B5): appends a counter if needed.
fn unique_corrupt_path(path: &Path) -> PathBuf {
    let file = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let base = format!("{file}.corrupt");
    if !path.with_file_name(&base).exists() {
        return path.with_file_name(base);
    }
    for i in 1.. {
        let name = format!("{base}-{i}");
        let candidate = path.with_file_name(&name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
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
    fn second_corrupt_file_gets_a_distinct_quarantine_name() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        fs::write(store.path.clone(), b"{not json").unwrap();
        assert_eq!(store.load(Doc::default()), Doc::default());
        fs::write(store.path.clone(), b"still not json").unwrap();
        assert_eq!(store.load(Doc::default()), Doc::default());
        let quarantined = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("d.json") && n.contains("corrupt"))
            .count();
        assert_eq!(quarantined, 2, "both corrupt copies must be kept");
    }

    #[test]
    fn save_roundtrips_multibyte_text() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path(), "d.json");
        store
            .save(&Doc {
                value: "café ☕ 你好".into(),
            })
            .unwrap();
        assert_eq!(
            store.load(Doc::default()),
            Doc {
                value: "café ☕ 你好".into()
            }
        );
    }
}
