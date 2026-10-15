//! P1.3: the bookkeeping worker.
//!
//! The dictation pipeline used to save usage stats, history and dictionary
//! usage on the pipeline worker thread BEFORE shipping `PipelineReady` to
//! the controller, so every disk write sat on the release→paste critical
//! path. The controller now ships the injection payload first and hands the
//! bookkeeping to this module.
//!
//! A single background thread owns an mpsc channel and applies writes in
//! FIFO order — so per-session ordering is preserved — while coalescing
//! bursts: when a second write arrives within `COALESCE_WINDOW` of the
//! previous flush, the burst lands as one save per store instead of one
//! save per take.

use std::{collections::BTreeMap, sync::mpsc, thread, time::Duration};

use tauri::Manager;
use teletype_core::history::{append_transcript_file, DictationEntry};

/// How long a burst of writes is held before flushing: a second dictation
/// within this window rides the same save. Long enough to absorb a burst of
/// rapid takes, short enough that a lone write is only delayed ~250 ms.
const COALESCE_WINDOW: Duration = Duration::from_millis(250);

/// How long an idle bookkeeper thread waits for the next take before
/// exiting. A new pipeline worker spawns a fresh bookkeeper per take (the
/// same pattern as the transcribe and pipeline worker threads), so the
/// thread must not hold the `AppHandle` and block on `recv()` forever.
const IDLE_EXIT: Duration = Duration::from_secs(5);

/// One unit of bookkeeping, in arrival order.
enum Job {
    /// Usage stats for one take, precomputed by the pipeline worker.
    Usage {
        filler: BTreeMap<String, u32>,
        autotext: BTreeMap<String, u32>,
    },
    /// One history entry (dictation or command audit) plus the plain-text
    /// transcript file line, precomputed by the pipeline worker.
    History {
        entry: DictationEntry,
        transcript_dir: std::path::PathBuf,
        transcript_created_at: u64,
        transcript_text: String,
        transcript_app: String,
    },
    /// Dictionary usage for one take.
    DictionaryUsage { text: String },
}

/// Handle the pipeline worker sends bookkeeping jobs to. Cheap to clone.
#[derive(Clone)]
pub struct Bookkeeper {
    tx: mpsc::Sender<Job>,
}

impl Bookkeeper {
    /// Spawns the bookkeeping thread and returns a handle for the pipeline
    /// worker. The thread exits on idle timeout or when the last handle is
    /// dropped, flushing whatever is pending on the way out.
    pub fn spawn(app: tauri::AppHandle) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("teletype-bookkeeper".into())
            .spawn(move || run(app, rx))
            .expect("spawn bookkeeper thread");
        Self { tx }
    }

    /// Queues the usage write for one take. Non-blocking; a closed channel
    /// (the bookkeeper thread is dead) drops the job rather than stalling
    /// the pipeline.
    pub fn usage(&self, filler: BTreeMap<String, u32>, autotext: BTreeMap<String, u32>) {
        let _ = self.tx.send(Job::Usage { filler, autotext });
    }

    /// Queues the history write for one take, including the plain-text
    /// transcript file line.
    pub fn history(
        &self,
        entry: DictationEntry,
        transcript_dir: std::path::PathBuf,
        transcript_created_at: u64,
        transcript_text: String,
        transcript_app: String,
    ) {
        let _ = self.tx.send(Job::History {
            entry,
            transcript_dir,
            transcript_created_at,
            transcript_text,
            transcript_app,
        });
    }

    /// Queues the dictionary usage write for one take.
    pub fn dictionary_usage(&self, text: String) {
        let _ = self.tx.send(Job::DictionaryUsage { text });
    }
}

/// The bookkeeping loop: apply jobs in FIFO order, deferring each flush up
/// to `COALESCE_WINDOW` so a burst coalesces into one save per store.
fn run(app: tauri::AppHandle, rx: mpsc::Receiver<Job>) {
    let state = app.state::<crate::AppState>();
    let mut burst = Burst::default();
    loop {
        // Wait for the first job of this burst. An idle bookkeeper (no
        // takes for `IDLE_EXIT`) exits: each pipeline worker spawns its own
        // bookkeeper, so a stuck thread would hold the `AppHandle` forever.
        let first = match rx.recv_timeout(IDLE_EXIT) {
            Ok(job) => job,
            Err(mpsc::RecvTimeoutError::Timeout) => return,
            // Channel closed: flush whatever is pending, then exit.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                flush(&state, &mut burst);
                return;
            }
        };
        burst.absorb(first);
        // Coalesce: while writes are queued, hold the flush. The window
        // restarts on every arrival, so a steady burst defers the saves and
        // a lone write flushes ~250 ms after it arrives.
        loop {
            match rx.recv_timeout(COALESCE_WINDOW) {
                Ok(job) => burst.absorb(job),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    flush(&state, &mut burst);
                    return;
                }
            }
        }
        flush(&state, &mut burst);
    }
}

/// The burst accumulator: one slot per store, merged in FIFO order.
#[derive(Default)]
struct Burst {
    fillers: BTreeMap<String, u32>,
    autotexts: BTreeMap<String, u32>,
    entries: Vec<DictationEntry>,
    /// The newest burst's plain-text transcript line (commands have an
    /// empty text, so no line is written for them).
    transcript: Option<(std::path::PathBuf, u64, String, String)>,
    dictionary: Vec<String>,
}

impl Burst {
    fn absorb(&mut self, job: Job) {
        match job {
            Job::Usage { filler, autotext } => {
                for (k, v) in filler {
                    *self.fillers.entry(k).or_insert(0) += v;
                }
                for (k, v) in autotext {
                    *self.autotexts.entry(k).or_insert(0) += v;
                }
            }
            Job::History {
                entry,
                transcript_dir,
                transcript_created_at,
                transcript_text,
                transcript_app,
            } => {
                self.entries.push(entry);
                if !transcript_text.is_empty() {
                    self.transcript = Some((
                        transcript_dir,
                        transcript_created_at,
                        transcript_text,
                        transcript_app,
                    ));
                }
            }
            Job::DictionaryUsage { text } => {
                self.dictionary.push(text);
            }
        }
    }

    fn take(&mut self) -> BurstParts {
        BurstParts {
            fillers: std::mem::take(&mut self.fillers),
            autotexts: std::mem::take(&mut self.autotexts),
            entries: std::mem::take(&mut self.entries),
            transcript: self.transcript.take(),
            dictionary: std::mem::take(&mut self.dictionary),
        }
    }
}

/// The burst's writes, factored out of the return tuple so the `take`
/// signature stays under clippy's `type_complexity` limit.
struct BurstParts {
    fillers: BTreeMap<String, u32>,
    autotexts: BTreeMap<String, u32>,
    entries: Vec<DictationEntry>,
    transcript: Option<(std::path::PathBuf, u64, String, String)>,
    dictionary: Vec<String>,
}

/// Applies the burst's writes, one save per store. Each read-modify-write
/// is the same one the pipeline worker used to do inline; only the thread
/// it runs on changed.
fn flush(state: &crate::AppState, burst: &mut Burst) {
    let BurstParts {
        fillers,
        autotexts,
        entries,
        transcript,
        dictionary,
    } = burst.take();

    if !fillers.is_empty() || !autotexts.is_empty() {
        let mut usage = state
            .usage
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        usage.record(&fillers, &autotexts);
        if let Err(e) = state.usage_store.save(&usage) {
            tracing::warn!("Failed to save usage stats: {e}");
        }
    }

    if !entries.is_empty() {
        let mut history = state
            .history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for entry in &entries {
            history.push(entry.clone());
        }
        if let Err(e) = state.history_store.save(&history) {
            tracing::warn!("Failed to save history after dictation: {e}");
        }
    }

    if let Some((dir, created_at, text, app_name)) = transcript {
        if let Err(e) = append_transcript_file(&dir, created_at, &text, &app_name) {
            crate::log_entry(crate::LogLevel::Error, format!("transcript file: {e}"));
        }
    }

    if !dictionary.is_empty() && state.settings().dictionary_enabled {
        let mut dict = state
            .dictionary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut changed = false;
        for text in &dictionary {
            if dict.record_usage(text) > 0 {
                changed = true;
            }
        }
        if changed {
            let _ = state.dictionary_store.save(&*dict);
        }
    }
}
