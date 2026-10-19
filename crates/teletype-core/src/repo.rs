//! A snapshot repository: one JSON file, one lock-free snapshot.
//!
//! Replaces the old `Mutex<T>` + `JsonStore<T>` pairs. Reads are
//! `ArcSwap` loads (no lock, no clone); writes clone-on-write,
//! persist through [`JsonStore`] (atomic, corrupt-quarantining), and
//! publish the new snapshot to broadcast subscribers.

use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::storage::JsonStore;

/// Snapshot channel capacity: one slot per in-flight subscriber event.
/// Slow consumers simply drop the oldest event (they re-read on the
/// next update anyway).
const CHANNEL_CAPACITY: usize = 16;

/// A single document owned by a [`JsonStore`] with a lock-free snapshot.
pub struct Repo<T: Serialize + for<'de> Deserialize<'de>> {
    cur: ArcSwap<T>,
    store: JsonStore<T>,
    tx: broadcast::Sender<Arc<T>>,
}

impl<T: Clone + Serialize + for<'de> Deserialize<'de> + Send + Sync + 'static> Repo<T> {
    /// Wraps an existing store and its current value.
    pub fn new(store: JsonStore<T>, initial: T) -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_CAPACITY);
        Self {
            cur: ArcSwap::new(Arc::new(initial)),
            store,
            tx,
        }
    }
    /// The current snapshot. Lock-free; the `Arc` is cheap to hold.
    pub fn get(&self) -> Arc<T> {
        self.cur.load_full()
    }

    /// Swaps in a new snapshot without touching the backing store.
    ///
    /// Used by stores whose disk writes moved off the hot path (A4): the
    /// snapshot is hydrated at startup and mirrored by the background
    /// persistence actor, so the JSON store is retired for these repos.
    pub fn set_snapshot(&self, value: T) {
        let arc = Arc::new(value);
        self.cur.store(Arc::clone(&arc));
        let _ = self.tx.send(arc);
    }

    /// Clone-on-write update: applies `f` to a clone of the current
    /// snapshot, persists it, and swaps the snapshot in.
    ///
    /// Returns the new snapshot on success. On a persistence error the
    /// snapshot is NOT swapped, so memory and disk stay consistent.
    pub fn update(&self, f: impl FnOnce(&mut T)) -> Result<Arc<T>, String> {
        let mut cur = (*self.cur.load()).as_ref().clone();
        f(&mut cur);
        self.store.save(&cur)?;
        self.cur.store(Arc::new(cur));
        let arc = self.cur.load_full();
        let _ = self.tx.send(arc.clone());
        Ok(arc)
    }

    /// Replaces the whole document, persists it, and swaps the snapshot.
    pub fn replace(&self, value: T) -> Result<Arc<T>, String> {
        self.update(|cur| *cur = value)
    }

    /// Subscribes to snapshot changes. Receives the new `Arc<T>` after
    /// each successful [`update`](Self::update).
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<T>> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::path::Path;

    #[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
    struct Doc {
        #[serde(default)]
        value: String,
        #[serde(default)]
        count: u32,
    }

    fn repo(dir: &Path) -> Repo<Doc> {
        Repo::new(JsonStore::new(dir, "repo.json"), Doc::default())
    }

    #[test]
    fn get_returns_initial_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let r = repo(dir.path());
        assert_eq!(r.get().value, "");
    }

    #[test]
    fn update_persists_and_swaps() {
        let dir = tempfile::TempDir::new().unwrap();
        let r = repo(dir.path());
        r.update(|d| {
            d.value = "hello".into();
            d.count = 1;
        })
        .unwrap();
        assert_eq!(r.get().value, "hello");
        assert_eq!(r.get().count, 1);
        // Persisted: a fresh store sees the same document.
        let fresh = JsonStore::<Doc>::new(dir.path(), "repo.json");
        assert_eq!(
            fresh.load(Doc::default()),
            Doc {
                value: "hello".into(),
                count: 1
            }
        );
    }

    #[test]
    fn update_covers_previous_value() {
        let dir = tempfile::TempDir::new().unwrap();
        let r = repo(dir.path());
        r.update(|d| d.value = "old".into()).unwrap();
        r.update(|d| d.value = "new".into()).unwrap();
        assert_eq!(r.get().value, "new");
        assert_eq!(r.get().count, 0);
    }

    #[test]
    fn replace_swaps_whole_document() {
        let dir = tempfile::TempDir::new().unwrap();
        let r = repo(dir.path());
        r.update(|d| d.count = 3).unwrap();
        r.replace(Doc {
            value: "fresh".into(),
            count: 0,
        })
        .unwrap();
        assert_eq!(r.get().value, "fresh");
        assert_eq!(r.get().count, 0);
        let fresh = JsonStore::<Doc>::new(dir.path(), "repo.json");
        assert_eq!(
            fresh.load(Doc::default()),
            Doc {
                value: "fresh".into(),
                count: 0
            }
        );
    }

    #[test]
    fn update_notifies_subscribers() {
        let dir = tempfile::TempDir::new().unwrap();
        let r = repo(dir.path());
        let mut rx = r.subscribe();
        r.update(|d| d.value = "a".into()).unwrap();
        r.update(|d| d.value = "b".into()).unwrap();
        let mut got: Vec<String> = Vec::new();
        while let Ok(arc) = rx.try_recv() {
            got.push(arc.value.clone());
        }
        assert_eq!(got, vec!["a", "b"]);
    }

    #[test]
    fn save_failure_keeps_snapshot_unchanged_and_unpersisted() {
        let dir = tempfile::TempDir::new().unwrap();
        let r = repo(dir.path());
        // Make the store path a directory so `save` cannot write.
        std::fs::create_dir_all(dir.path().join("repo.json")).unwrap();
        let before = r.get();
        let err = r.update(|d| d.value = "boom".into()).unwrap_err();
        assert!(
            err.contains("repo.json"),
            "error should name the file: {err}"
        );
        // Snapshot not swapped.
        assert_eq!(r.get().value, "");
        // Same pointer (no new allocation was installed).
        assert!(Arc::ptr_eq(&before, &r.get()));
    }

    #[test]
    fn get_is_lock_free_for_concurrent_readers() {
        let dir = tempfile::TempDir::new().unwrap();
        let r = std::sync::Arc::new(repo(dir.path()));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let r = std::sync::Arc::clone(&r);
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    r.get();
                }
            }));
        }
        for _ in 0..50 {
            r.update(|d| d.count += 1).unwrap();
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(r.get().count, 50);
    }
}
