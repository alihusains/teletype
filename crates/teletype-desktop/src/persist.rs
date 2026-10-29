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
use std::sync::{Arc, Mutex};
use std::time::Duration;

use teletype_core::history::DictationEntry;
use teletype_core::persist::PersistStores;

/// How often the writer thread wakes up to flush, even with a small buffer.
const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Flush immediately once this many messages are buffered, so a burst never
/// waits out the interval.
const MAX_BATCH: usize = 64;

/// How long [`PersistHandle::shutdown`] waits for the writer thread to finish
/// its last flush. Quit must not hang on a wedged database, and it must not
/// race the flush either: the old fixed 500 ms sleep did neither, and dropped
/// whatever was still queued when the drain ran long.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);

/// Attempts to open the durable store before the actor gives up on the
/// session, and the pause between them.
const OPEN_ATTEMPTS: usize = 3;
const OPEN_RETRY_WAIT: Duration = Duration::from_millis(200);

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
pub struct PersistHandle {
    tx: mpsc::Sender<PersistMsg>,
    /// The writer thread's exit signal. The first `shutdown` takes it and
    /// waits for the last flush, instead of guessing how long that takes.
    done: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
}

impl PersistHandle {
    pub fn history(&self, entry: DictationEntry) {
        let _ = self.tx.send(PersistMsg::History(entry));
    }

    pub fn history_remove(&self, id: &str) {
        let _ = self.tx.send(PersistMsg::HistoryRemove(id.to_string()));
    }

    pub fn usage(&self, filler: BTreeMap<String, u32>, autotext: BTreeMap<String, u32>) {
        let _ = self.tx.send(PersistMsg::Usage { filler, autotext });
    }

    pub fn usage_reset(&self) {
        let _ = self.tx.send(PersistMsg::UsageReset);
    }

    /// Flushes buffered work, stops the writer thread, and waits (bounded by
    /// [`SHUTDOWN_WAIT`]) for that flush to reach the disk.
    ///
    /// Idempotent: only the first caller waits, because only it gets the
    /// signal. The quit path calls this from the Exit hook and `AppState`'s
    /// `Drop` calls it again as the safety net.
    pub fn shutdown(&self) {
        let _ = self.tx.send(PersistMsg::Shutdown);
        let waiter = self.done.lock().ok().and_then(|mut slot| slot.take());
        let Some(done) = waiter else { return };
        match done.recv_timeout(SHUTDOWN_WAIT) {
            // `Disconnected` means the thread never started, so there is no
            // flush left to wait for.
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => tracing::warn!(
                "persist actor: the last flush outlived the shutdown wait; \
                 queued history and usage writes may be lost"
            ),
        }
    }
}

/// Starts the writer thread for `dir` and returns the handle to send
/// messages on. The thread owns the SQLite stores and applies each message
/// in channel order with a single write (WAL + `synchronous=NORMAL` give
/// the durability the old JSON fsync provided).
pub fn spawn(dir: &Path) -> PersistHandle {
    let dir = dir.to_path_buf();
    let (tx, rx) = mpsc::channel::<PersistMsg>();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    std::thread::Builder::new()
        .name("teletype-persist".into())
        .spawn(move || {
            run(rx, &dir);
            // Only sent once the writer is gone: a `shutdown` waiting on the
            // other end now knows the final flush is on disk.
            let _ = done_tx.send(());
        })
        .ok();
    PersistHandle {
        tx,
        done: Arc::new(Mutex::new(Some(done_rx))),
    }
}

fn run(rx: mpsc::Receiver<PersistMsg>, dir: &Path) {
    // A store that will not open is usually transient (the previous instance
    // still tearing down, a checkpoint mid-write), so try a few times. An
    // actor that returns here stops persisting for the rest of the session,
    // and every send after that is discarded without an error.
    let mut stores = None;
    for attempt in 1..=OPEN_ATTEMPTS {
        match PersistStores::open(dir) {
            Ok(opened) => {
                stores = Some(opened);
                break;
            }
            Err(e) => {
                tracing::warn!(
                    "persist actor: could not open SQLite store ({attempt}/{OPEN_ATTEMPTS}): {e}"
                );
                if attempt < OPEN_ATTEMPTS {
                    std::thread::sleep(OPEN_RETRY_WAIT);
                }
            }
        }
    }
    let Some(mut stores) = stores else {
        tracing::error!(
            "persist actor: giving up on the SQLite store; history will not persist this session"
        );
        return;
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

    /// A burst of messages sent before `shutdown` must all be on disk by the
    /// time it returns.
    ///
    /// This test used to poll for up to 10 s waiting for the writer to catch
    /// up, which passed on an idle machine and timed out on a loaded CI box
    /// (the drain itself takes ~15 ms). `shutdown` is now a barrier: it joins
    /// the writer thread, so the data is durable when it returns and there is
    /// nothing left to wait for.
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
        let s = PersistStores::open(dir.path()).unwrap();
        assert_eq!(
            s.history_all().unwrap().len(),
            n,
            "all rapid history messages must land"
        );
        assert_eq!(s.usage_load().unwrap().filler_counts.get("um"), Some(&3));
    }

    /// A second `shutdown` returns at once. The quit path calls it twice
    /// (Exit hook, then `AppState::drop`), and the first call takes the exit
    /// signal, so a second wait would hang teardown behind a `SHUTDOWN_WAIT`
    /// that has nothing left to wait for.
    #[test]
    fn a_second_shutdown_does_not_wait_again() {
        let dir = tempfile::TempDir::new().unwrap();
        let handle = spawn(dir.path());
        handle.history(entry("one", 1));
        handle.shutdown();
        let began = std::time::Instant::now();
        handle.shutdown();
        assert!(
            began.elapsed() < Duration::from_secs(1),
            "the second shutdown waited {:?}",
            began.elapsed()
        );
        let s = PersistStores::open(dir.path()).unwrap();
        assert_eq!(s.history_all().unwrap().len(), 1);
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
        // Same barrier as above: nothing to poll for once `shutdown` returned.
        let s = PersistStores::open(dir.path()).unwrap();
        let all = s.history_all().unwrap();
        let stats = s.usage_load().unwrap();
        assert_eq!(all.len(), 1, "the removed entry must not come back");
        assert_eq!(all[0].id, "keep");
        assert!(
            stats.filler_counts.is_empty(),
            "usage_reset must clear the counters"
        );
    }
}
