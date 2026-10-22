//! Background persistence actor (A4).
//!
//! The dictation hot path must never do synchronous disk I/O. This module
//! owns a single writer thread with an `mpsc` channel: history/usage
//! mutations are sent as messages and applied to SQLite in the background,
//! coalescing bursts (flush at most once per ~250 ms, or when the buffer
//! hits the max batch, or on shutdown). The in-memory `Repo<T>` snapshots
//! stay the read source of truth; this actor mirrors each change to the
//! durable store.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use teletype_core::history::DictationEntry;
use teletype_core::persist::PersistStores;

/// How often the writer thread wakes up to flush, even with a small buffer.
const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Flush immediately once this many messages are buffered, so a burst never
/// waits out the interval.
const MAX_BATCH: usize = 64;

/// One unit of durable work for the writer thread.
pub enum PersistMsg {
    /// Appends a completed dictation to the history store.
    History(DictationEntry),
    /// Removes one history entry by id.
    HistoryRemove(String),
    /// Adds one dictation's usage counters (filler removed, AutoText used).
    Usage {
        filler: BTreeMap<String, u32>,
        autotext: BTreeMap<String, u32>,
    },
    /// Clears all usage counters.
    UsageReset,
    /// Flushes any buffered work and stops the thread.
    Shutdown,
}

/// The sender side of the actor channel. Clone it freely; sending never
/// blocks on disk.
#[derive(Clone)]
pub struct PersistHandle(mpsc::Sender<PersistMsg>);

impl PersistHandle {
    pub fn history(&self, entry: DictationEntry) {
        let _ = self.0.send(PersistMsg::History(entry));
    }

    pub fn history_remove(&self, id: &str) {
        let _ = self.0.send(PersistMsg::HistoryRemove(id.to_string()));
    }

    pub fn usage(&self, filler: BTreeMap<String, u32>, autotext: BTreeMap<String, u32>) {
        let _ = self.0.send(PersistMsg::Usage { filler, autotext });
    }

    pub fn usage_reset(&self) {
        let _ = self.0.send(PersistMsg::UsageReset);
    }

    /// Flushes buffered work and stops the writer thread. Idempotent.
    pub fn shutdown(&self) {
        let _ = self.0.send(PersistMsg::Shutdown);
    }
}

/// Starts the writer thread for `dir` and returns the handle to send
/// messages on. The thread owns the SQLite stores and applies each message
/// in channel order with a single write (WAL + `synchronous=NORMAL` give
/// the durability the old JSON fsync provided).
pub fn spawn(dir: &Path) -> PersistHandle {
    let dir = dir.to_path_buf();
    let (tx, rx) = mpsc::channel::<PersistMsg>();
    std::thread::Builder::new()
        .name("teletype-persist".into())
        .spawn(move || run(rx, &dir))
        .ok();
    PersistHandle(tx)
}

fn run(rx: mpsc::Receiver<PersistMsg>, dir: &Path) {
    let mut stores = match PersistStores::open(dir) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("persist actor: could not open SQLite store: {e}");
            return;
        }
    };
    let mut pending: Vec<PersistMsg> = Vec::new();
    loop {
        let first = match rx.recv_timeout(FLUSH_INTERVAL) {
            Ok(msg) => msg,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if pending.is_empty() {
                    continue;
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        pending.push(first);
        while pending.len() < MAX_BATCH {
            match rx.try_recv() {
                Ok(msg) => pending.push(msg),
                Err(_) => break,
            }
        }
        if apply(&mut stores, &mut pending) {
            // Shutdown was in the batch: flush is done, exit.
            break;
        }
    }
}

/// Applies a batch of messages to the stores. Returns `true` when a
/// `Shutdown` was among them (the thread should exit after this flush).
fn apply(stores: &mut PersistStores, batch: &mut Vec<PersistMsg>) -> bool {
    let mut shutdown = false;
    for msg in batch.drain(..) {
        match msg {
            PersistMsg::History(entry) => {
                if let Err(e) = stores.push_history(&entry) {
                    tracing::warn!("persist: history push failed: {e}");
                }
            }
            PersistMsg::HistoryRemove(id) => {
                if let Err(e) = stores.remove_history(&id) {
                    tracing::warn!("persist: history remove failed: {e}");
                }
            }
            PersistMsg::Usage { filler, autotext } => {
                if let Err(e) = stores.record_usage(&filler, &autotext) {
                    tracing::warn!("persist: usage record failed: {e}");
                }
            }
            PersistMsg::UsageReset => {
                if let Err(e) = stores.usage_reset() {
                    tracing::warn!("persist: usage reset failed: {e}");
                }
            }
            PersistMsg::Shutdown => {
                shutdown = true;
            }
        }
    }
    shutdown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, at: u64) -> DictationEntry {
        DictationEntry {
            id: id.into(),
            created_at: at,
            text: format!("text {id}"),
            context: None,
            duration_ms: None,
        }
    }

    #[test]
    fn coalescing_lands_all_messages() {
        let dir = tempfile::TempDir::new().unwrap();
        let handle = spawn(dir.path());
        let n = 200usize;
        for i in 0..n {
            handle.history(entry(&i.to_string(), i as u64));
        }
        handle.usage(
            {
                let mut m = BTreeMap::new();
                m.insert("um".to_string(), 3);
                m
            },
            BTreeMap::new(),
        );
        handle.shutdown();
        // Give the writer thread bounded time to drain.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut seen = 0usize;
        while std::time::Instant::now() < deadline {
            // The writer may not have flushed yet: poll until the data lands.
            if let Ok(s) = PersistStores::open(dir.path()) {
                let all = s.history_all().unwrap();
                let stats = s.usage_load().unwrap();
                seen = all.len();
                if seen == n && stats.filler_counts.get("um") == Some(&3) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(seen, n, "all rapid history messages must land");
        let s = PersistStores::open(dir.path()).unwrap();
        assert_eq!(s.usage_load().unwrap().filler_counts.get("um"), Some(&3));
    }

    #[test]
    fn remove_and_reset_are_applied() {
        let dir = tempfile::TempDir::new().unwrap();
        let handle = spawn(dir.path());
        handle.history(entry("keep", 1));
        handle.history(entry("drop", 2));
        handle.usage(
            {
                let mut m = BTreeMap::new();
                m.insert("um".to_string(), 1);
                m
            },
            BTreeMap::new(),
        );
        handle.history_remove("drop");
        handle.usage_reset();
        handle.shutdown();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut ok = false;
        while std::time::Instant::now() < deadline {
            if let Ok(s) = PersistStores::open(dir.path()) {
                let all = s.history_all().unwrap();
                let stats = s.usage_load().unwrap();
                if all.len() == 1 && all[0].id == "keep" && stats.filler_counts.is_empty() {
                    ok = true;
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(ok, "remove + reset must be applied by the actor");
    }
}
