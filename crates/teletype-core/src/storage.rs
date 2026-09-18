//! Local JSON persistence.
//!
//! One file per concern in the app config directory, atomic writes (write to
//! `.tmp`, rename), corrupt files moved aside rather than silently overwritten.
//! Sensitive fields (AutoText replacements) live in the same files but are
//! never logged.

use serde::{Deserialize, Serialize};
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
                    eprintln!(
                        "[storage] {} unreadable ({e}); moved aside",
                        self.path.display()
                    );
                    let _ = fs::rename(&self.path, corrupt_path(&self.path));
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
        let _bytes = serde_json::to_vec_pretty(doc)
            .map_err(|e| format!("Couldn't serialize {}: {e}", self.path.display()))?;
        let tmp = self.path.with_extension("tmp");
        let bytes = serde_json::to_vec_pretty(doc)
            .map_err(|e| format!("Couldn't serialize {}: {e}", self.path.display()))?;
        std::fs::write(&tmp, &bytes)
            .map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
        fs::rename(&tmp, &self.path)
            .map_err(|e| format!("Couldn't save {}: {e}", self.path.display()))?;
        Ok(())
    }
}

fn corrupt_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".corrupt");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct Doc {
        #[serde(default)]
        value: String,
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = std::env::temp_dir().join("teletype-test-missing");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let store = JsonStore::new(&dir, "d.json");
        assert_eq!(store.load(Doc::default()), Doc::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join("teletype-test-roundtrip");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let store = JsonStore::new(&dir, "d.json");
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
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = std::env::temp_dir().join("teletype-test-corrupt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let store = JsonStore::new(&dir, "d.json");
        fs::write(store.path.clone(), b"{not json").unwrap();
        assert_eq!(store.load(Doc::default()), Doc::default());
        assert!(
            store.path.with_extension("json.corrupt").exists()
                || dir.join("d.json.corrupt").exists()
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
