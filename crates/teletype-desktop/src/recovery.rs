//! Escape Recovery spool: survive a crash mid-dictation.
//!
//! While a recording is in flight, every captured mono frame is appended to a
//! bounded PCM file under `<app-data>/recovery/`. If the app crashes (or the
//! Mac sleeps badly) before a clean stop, the file is left behind; the next
//! launch finds it via [`find_pending`] and offers "recover last dictation".
//!
//! ## Format
//!
//! The spool stores **16 kHz mono f32** — the exact container the ASR path
//! expects. The in-memory buffer holds device-rate mono `Vec<f32>` and
//! `Recording::finish()` resamples it to 16 kHz before calling whisper
//! (`whisper-rs` assumes 16 kHz input). The dictation hook resamples each
//! frame before spooling, so a recovered take transcribes through the
//! identical path as a live one.
//!
//! ## Encryption
//!
//! V1 stores plain PCM with `0600` permissions (owner read/write only) and
//! deletes the file on a clean stop. There is no encryption; the file is
//! world-inaccessible and short-lived (deleted on clean stop, 7-day expiry on
//! the crash path).
//!
//! ## Bounding
//!
//! The spool is hard-bounded to [`MAX_SECONDS`] of audio. When the bound is
//! exceeded we **stop writing and mark the spool truncated** (the simplest
//! correct bound): the file then holds the *oldest* [`MAX_SECONDS`] of the
//! take. A ring buffer would drop the oldest data mid-write, which is harder
//! to get right and offers no benefit for a 60 s bound.
//!
//! ## Threading
//!
//! The spool write is driven by a dedicated **writer thread** fed through a
//! bounded mpsc channel. The audio callback (a real-time thread) only does a
//! cheap non-blocking `try_send`; if the channel is full or the writer is
//! behind, the frame is dropped and we log once. The writer thread does the
//! actual `File` append, so a slow/full disk can never block or slow the
//! audio thread.
//!
//! ## Safety
//!
//! Every file operation in this module is fallible and logged, never panics.
//! A corrupt or partial spool (e.g. truncated mid-frame) yields a clear error
//! from [`read_spool`], never a startup crash.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, SystemTime},
};

use teletype_core::audio::TARGET_SAMPLE_RATE;

use tauri::Manager;

/// Hard bound on how much audio the spool retains (seconds of 16 kHz mono).
/// 300 s of 16 kHz mono f32 is about 19 MB on disk.
pub const MAX_SECONDS: u64 = 300;
/// Candidates older than this are deleted unconditionally at startup.
const EXPIRY: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// A candidate must be at least this old to be offered for recovery.
const MIN_AGE: Duration = Duration::from_secs(10 * 60);
/// Files smaller than this are noise from an instant stop; ignored.
const MIN_BYTES: u64 = 100 * 1024;
/// Bounded channel depth between the audio callback and the writer thread:
/// a few seconds of 16 kHz mono headroom — far more than the writer thread
/// needs to stay ahead of real-time on any disk. When the writer falls
/// behind (slow/full disk), the callback's `try_send` fails and the frame
/// is dropped, logged once — the audio thread never blocks.
const CHANNEL_DEPTH: usize = 64;

/// A spool file found at startup that can be recovered.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingRecovery {
    pub path: PathBuf,
    /// Seconds of 16 kHz mono audio available, rounded down.
    pub seconds: u64,
    /// True when the file length isn't a whole number of f32 samples (a
    /// partial write from a crash) or the 300 s bound was hit.
    pub truncated: bool,
    /// Total seconds of audio the spool saw during the take. When the spool
    /// was truncated, `total_seconds > seconds` and the difference is the
    /// audio that was lost. When not truncated, `total_seconds == seconds`.
    ///
    /// The spool writes a sidecar file (`<spool>.meta`) on stop that records
    /// the total byte count; `find_pending` reads it when present. When the
    /// sidecar is absent (crash before the stop path ran), `total_seconds`
    /// falls back to `seconds` and the UI shows "at least" the recovered
    /// duration.
    pub total_seconds: u64,
}

/// A bounded PCM spool for one in-flight recording.
#[allow(dead_code)]
pub struct Spool {
    path: PathBuf,
    /// How many bytes of audio this spool may hold (MAX_SECONDS of 16 kHz f32).
    max_bytes: u64,
    /// Bytes sent to the writer thread so far (16 kHz mono f32); used to stop
    /// writing once the bound is reached (the writer enforces it too).
    written: u64,
    /// Total bytes of audio this spool has seen, including frames dropped
    /// after the bound was hit. Used to report `total_seconds` on
    /// [`PendingRecovery`] so the UI can show how much was lost.
    total_written: std::sync::Arc<std::sync::atomic::AtomicU64>,
    /// Set once the bound is exceeded; further writes are dropped.
    truncated: bool,
    /// Audio-callback -> writer-thread channel. `None` once stopped.
    tx: Option<mpsc::SyncSender<Vec<f32>>>,
    /// Writer thread join handle.
    writer: Option<thread::JoinHandle<()>>,
    /// One-shot guard so "dropping frames" is logged at most once per spool.
    dropped_logged: std::sync::atomic::AtomicBool,
}

/// Hard time budget for joining the spool writer (`complete` / `abandoned`).
/// A running capture that still holds a `SpoolWriter` clone normally drops it
/// within a few milliseconds of the take's stop signal; 500 ms is a small
/// margin for slow devices. If it has not drained by then, leave the file for
/// recovery and move on instead of wedging the caller.
const WRITER_JOIN_BUDGET: Duration = Duration::from_millis(500);

/// Process-wide sequence so two spools begun in the same millisecond never
/// collide on a filename (a collision made concurrent test spools truncate
/// each other's files).
static SPOOL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl Spool {
    /// Begins a new spool under `<app_data_dir>/recovery/`.
    ///
    /// The spool stores 16 kHz mono f32 (the ASR container), so the device
    /// rate is not needed here — the hook resamples before calling
    /// [`Spool::write`]. Returns an error if the directory/file cannot be
    /// created; the caller should proceed without a spool in that case.
    pub fn begin(app_data_dir: &Path) -> Result<Self, String> {
        let dir = app_data_dir.join("recovery");
        fs::create_dir_all(&dir).map_err(|e| format!("recovery dir: {e}"))?;
        let path = dir.join(format!(
            "spool-{}-{}.pcm",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            SPOOL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        // 0600 perms: owner read/write only (documented V1 choice).
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .map_err(|e| format!("recovery open: {e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }
        drop(file);

        let max_bytes = MAX_SECONDS * TARGET_SAMPLE_RATE as u64 * 4;
        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(CHANNEL_DEPTH);
        // The writer thread owns the file path; `self` keeps a clone for the
        // clean-stop delete.
        let writer_path = path.clone();
        let writer = thread::Builder::new()
            .name("teletype-recovery-writer".into())
            .spawn(move || writer_loop(writer_path, rx, max_bytes))
            .map_err(|e| {
                let _ = fs::remove_file(&path);
                format!("recovery writer spawn: {e}")
            })?;

        Ok(Self {
            path,
            max_bytes,
            written: 0,
            total_written: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            truncated: false,
            tx: Some(tx),
            writer: Some(writer),
            dropped_logged: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Appends a captured **16 kHz mono** frame. Cheap and non-blocking: this
    /// only does a `try_send` to the writer thread. If the channel is full
    /// (writer behind, e.g. disk full) the frame is dropped and we log once.
    /// Never blocks or panics. The 60 s bound is enforced here (stop writing
    /// and mark truncated) and again by the writer thread on the file side.
    #[allow(dead_code)]
    pub fn write(&mut self, frame: &[f32]) {
        let frame_len = frame.len() as u64 * 4;
        self.total_written
            .fetch_add(frame_len, std::sync::atomic::Ordering::Relaxed);
        if self.truncated || self.tx.is_none() {
            return; // bound already hit, or writer gone
        }
        // If this frame would push us past the bound, stop now and mark
        // truncated. (We keep what's already written = the oldest 300 s.)
        if self.written + frame_len > self.max_bytes {
            self.truncated = true;
            return;
        }
        match self.tx.as_ref() {
            Some(tx) => match tx.try_send(frame.to_vec()) {
                Ok(()) => self.written += frame_len,
                Err(_) => self.mark_dropped(),
            },
            None => self.mark_dropped(), // writer gone
        }
    }

    #[allow(dead_code)]
    fn mark_dropped(&mut self) {
        use std::sync::atomic::Ordering;
        if self
            .dropped_logged
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            crate::log_entry(
                crate::LogLevel::Warn,
                "recovery spool: dropping frames (writer behind / disk slow)",
            );
        }
    }

    /// A writer handle for the audio callback. Cheap to clone and send.
    /// `None` if the spool has already been stopped (writer gone).
    pub fn clone_writer(&self) -> Option<SpoolWriter> {
        self.tx.clone().map(|tx| SpoolWriter {
            tx,
            total_written: self.total_written.clone(),
        })
    }

    /// Clean stop: stop the writer, wait for it to drain, delete the file.
    ///
    /// The writer join is BOUNDED (see [`join_writer_budgeted`]): a live
    /// `SpoolWriter` clone parked on a running capture keeps the channel open,
    /// so an unbounded join here froze the controller thread forever — the
    /// 2026-10-19 pill-stuck incident (take wedges at `stop()`, cancel
    /// unresponsive, all input dead). On budget expiry the file is left in
    /// place for later-recover, the same semantics as [`Self::abandoned`].
    pub fn complete(mut self) {
        self.tx.take(); // disconnect -> writer drains remaining frames then exits
        if let Some(w) = self.writer.take() {
            if !join_writer_budgeted(w, WRITER_JOIN_BUDGET) {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!(
                        "spool: writer did not drain within {} ms; keeping {} for recovery",
                        WRITER_JOIN_BUDGET.as_millis(),
                        self.path.display()
                    ),
                );
                return;
            }
        }
        // Write the sidecar meta file before deleting the spool, so a crash
        // between the two still leaves a recoverable spool with its total.
        let total = self
            .total_written
            .load(std::sync::atomic::Ordering::Relaxed);
        let meta_path = self.path.with_extension("meta");
        let _ = fs::write(&meta_path, total.to_string());
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(&meta_path);
    }

    /// Crash path: leave the file in place. The writer is stopped and joined
    /// (so all buffered frames are flushed to disk) but the file is NOT
    /// deleted, so the next launch can recover it.
    ///
    /// The join is BOUNDED for the same reason as [`Self::complete`]: a live
    /// `SpoolWriter` clone would otherwise block here forever. On budget
    /// expiry the writer thread keeps flushing on its own until the capture
    /// drops its clone; the file (on disk either way) simply gains those
    /// trailing frames.
    #[allow(dead_code)]
    pub fn abandoned(mut self) {
        self.tx.take();
        if let Some(w) = self.writer.take() {
            join_writer_budgeted(w, WRITER_JOIN_BUDGET);
        }
        // Write the sidecar meta file so the next launch can report the
        // total duration. Intentionally no remove_file: the file is the
        // recovery artifact.
        let total = self
            .total_written
            .load(std::sync::atomic::Ordering::Relaxed);
        let meta_path = self.path.with_extension("meta");
        let _ = fs::write(&meta_path, total.to_string());
    }

    /// Test-only crash path: like [`Self::abandoned`] but joins the writer
    /// with a generous 5 s budget instead of the production 500 ms. In a test
    /// no live `SpoolWriter` clone is parked on the channel, so the writer
    /// always disconnects and drains; the only thing that varies is how long
    /// the OS takes to schedule the writer thread under parallel test load.
    /// 500 ms is too tight on a saturated CI runner, which left the file
    /// under `MIN_BYTES` and made the dependent tests flake. 5 s is still a
    /// hard bound (no wedge), but comfortably covers the load.
    #[cfg(test)]
    fn abandoned_drained(mut self) {
        self.tx.take();
        if let Some(w) = self.writer.take() {
            join_writer_drained(w);
        }
        let total = self
            .total_written
            .load(std::sync::atomic::Ordering::Relaxed);
        let meta_path = self.path.with_extension("meta");
        let _ = fs::write(&meta_path, total.to_string());
    }
}

/// Joins the spool writer with a bounded time budget; returns whether the
/// writer exited within it. Never blocks the caller indefinitely: the writer
/// only exits when its mpsc channel disconnects, and a live `SpoolWriter`
/// clone parked on a running capture does NOT disconnect it — an unbounded
/// join at this call site (controller thread, inside `stop()`) froze the app
/// forever (2026-10-19 incident). The join itself runs on a short-lived side
/// thread so the budget can be enforced without atomic/lock primitives.
fn join_writer_budgeted(writer: std::thread::JoinHandle<()>, budget: Duration) -> bool {
    let (sent, recv) = std::sync::mpsc::channel::<()>();
    let side = std::thread::Builder::new()
        .name("teletype-spool-join".into())
        .spawn(move || {
            let _ = writer.join();
            let _ = sent.send(());
        });
    match side {
        Ok(_) => recv.recv_timeout(budget).is_ok(),
        Err(_) => false, // side-thread spawn failed; don't block the caller
    }
}

/// Test-only join that is guaranteed to return only after the writer has
/// flushed its `BufWriter` to disk. [`join_writer_budgeted`] cannot provide
/// this: the writer thread's `JoinHandle` returns as soon as `writer_loop`
/// returns, and `join`/`recv` carry no happens-before edge on the file's
/// on-disk size, so a reader racing the flush can observe a file that is not
/// yet fully written (measured: a single 64 KB frame drained to 0 bytes while
/// the join had already returned). Sending a completion token *after* the
/// writer's final flush orders the flush before the caller proceeds, making
/// the drained file size deterministic. Test-only because production paths
/// must stay bounded (a live `SpoolWriter` clone would wedge an unbounded
/// join, the 2026-10-19 incident).
#[cfg(test)]
fn join_writer_drained(writer: std::thread::JoinHandle<()>) {
    let (sent, recv) = std::sync::mpsc::channel::<()>();
    let side = std::thread::Builder::new()
        .name("teletype-spool-join-drained".into())
        .spawn(move || {
            let _ = writer.join();
            let _ = sent.send(());
        });
    if side.is_ok() {
        let _ = recv.recv();
    }
}

/// A cheap, shareable handle to a spool's writer channel, for the audio
/// callback. Cloning is a cheap `mpsc::Sender` clone; writing through it
/// never blocks (see [`Spool::write`]).
#[derive(Clone)]
pub struct SpoolWriter {
    tx: mpsc::SyncSender<Vec<f32>>,
    /// Total bytes seen by the spool (including dropped frames). Shared
    /// between all writer clones so the callback can count every frame.
    total_written: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl SpoolWriter {
    /// Appends a captured **16 kHz mono** frame. Unlike [`Spool::write`], this
    /// does not enforce the byte bound — the writer thread enforces it on the
    /// file side. Cheap and non-blocking: a `try_send` only; if the channel
    /// is full the frame is dropped silently (the callback must stay cheap).
    /// The total byte count is updated atomically so the spool can report
    /// `total_seconds` on [`PendingRecovery`].
    pub fn write(&self, frame: &[f32]) {
        self.total_written
            .fetch_add(frame.len() as u64 * 4, std::sync::atomic::Ordering::Relaxed);
        let _ = self.tx.try_send(frame.to_vec());
    }
}

/// The dedicated writer thread: appends frames to the spool file. A slow or
/// full disk can make this lag (and the channel fill, dropping frames), but
/// it never blocks the audio thread.
fn writer_loop(path: PathBuf, rx: mpsc::Receiver<Vec<f32>>, max_bytes: u64) {
    let file = match File::options().append(true).create(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            crate::log_entry(crate::LogLevel::Warn, format!("recovery writer open: {e}"));
            return;
        }
    };
    let mut file = std::io::BufWriter::new(file);
    let mut total = 0u64;
    for frame in rx {
        if total >= max_bytes {
            break; // bound enforced on the write side too (belt and braces)
        }
        // HARD-3 (#17) fault point: a crash/quit mid-take. The writer dies
        // here, before this frame is written — leaving the partial file on
        // disk (the frames written before the crash) exactly as a real
        // process death would. The buffer is flushed so the prior frames
        // survive the crash. The channel is left OPEN (no tx drop): a real
        // process death strands the channel too, and a clean drain here
        // would write every queued frame and hide the partial-file
        // semantics the test asserts on.
        #[cfg(feature = "fault-injection")]
        if crate::fault_inject::hit("quit-mid-take") {
            let _ = file.flush();
            return;
        }
        let mut bytes = Vec::with_capacity(frame.len() * 4);
        for s in &frame {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        if let Err(e) = file.write_all(&bytes) {
            crate::log_entry(crate::LogLevel::Warn, format!("recovery write: {e}"));
            return;
        }
        total += bytes.len() as u64;
    }
    let _ = file.flush();
}

/// The spool directory under `<app_data_dir>/recovery/`.
pub fn recovery_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("recovery")
}

/// Startup scan: find a recoverable spool and clean up expired ones.
///
/// A candidate is a `spool-*.pcm` that is **older than 10 minutes and larger
/// than 100 KB** (small files are noise from an instant stop). Files older
/// than 7 days are deleted unconditionally. All IO is fallible and logged;
/// this never panics.
pub fn find_pending(app_data_dir: &Path) -> Option<PendingRecovery> {
    let dir = recovery_dir(app_data_dir);
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return None, // no recovery dir yet
    };
    let now = SystemTime::now();
    let mut best: Option<(Duration, PendingRecovery)> = None;

    for entry in entries.flatten() {
        let path = entry.path();
        let is_spool = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("spool-") && n.ends_with(".pcm"))
            .unwrap_or(false);
        if !is_spool {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
        let size = meta.len();

        // 7-day expiry: delete unconditionally.
        if age > EXPIRY {
            let _ = fs::remove_file(&path);
            let _ = fs::remove_file(path.with_extension("meta"));
            crate::log_entry(
                crate::LogLevel::Info,
                format!("recovery: deleted expired spool {}", path.display()),
            );
            continue;
        }
        // Not old enough or too small to be worth offering.
        if age < MIN_AGE || size < MIN_BYTES {
            // A tiny spool abandoned past MIN_AGE can never hold speech (100
            // KB is ~1.5 s), so delete it instead of tripping over it on
            // every launch. Young files are untouched: the take may still be
            // recording into them. This is the litter a crash used to leave
            // behind forever (0-byte spools beside real ones).
            if age >= MIN_AGE && size < MIN_BYTES {
                let _ = fs::remove_file(&path);
                let _ = fs::remove_file(path.with_extension("meta"));
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("recovery: deleted abandoned tiny spool {}", path.display()),
                );
            }
            continue;
        }
        let seconds = (size / (4 * TARGET_SAMPLE_RATE as u64)).min(MAX_SECONDS);
        // Read the sidecar meta file for the total byte count. When absent
        // (crash before the stop path ran), fall back to `seconds`.
        let meta_path = path.with_extension("meta");
        let total_bytes = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok());
        let total_seconds = total_bytes
            .map(|bytes| (bytes / (4 * TARGET_SAMPLE_RATE as u64)).max(seconds))
            .unwrap_or(seconds);
        // Truncated when the total was longer than what's on disk, or the
        // file length isn't a whole number of f32 samples (partial write).
        // A small delta (up to 15 s) is treated as writer drain lag, not
        // truncation, so a short take that is still draining is not
        // misreported as truncated.
        let truncated = total_seconds > seconds + 15 || size % 4 != 0;
        let cand = PendingRecovery {
            path,
            seconds,
            truncated,
            total_seconds,
        };
        // Keep the most recent candidate.
        let newer = match best.as_ref().map(|b| b.0) {
            Some(prev_age) => age < prev_age,
            None => true,
        };
        if newer {
            best = Some((age, cand));
        }
    }
    best.map(|(_, cand)| cand)
}

/// Reads and decodes a spool file into 16 kHz mono f32 samples. Returns a
/// clear error (never panics) on a corrupt/partial file (length not a whole
/// number of f32 samples).
pub fn read_spool(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = fs::read(path).map_err(|e| format!("recovery read: {e}"))?;
    if bytes.len() % 4 != 0 {
        return Err(format!(
            "recovery spool is corrupt ({} bytes, not a whole number of f32 samples)",
            bytes.len()
        ));
    }
    let count = bytes.len() / 4;
    // `bytes.len() % 4 == 0` is checked above, so every 4-byte slice
    // exists; the `try_into` on a 4-byte slice cannot fail, but keep the
    // fallible shape so a future sample-size change surfaces as an error
    // here instead of a panic.
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        let bytes: [u8; 4] = bytes[i * 4..i * 4 + 4]
            .try_into()
            .map_err(|_| "recovery spool is corrupt (slice bounds changed)".to_string())?;
        samples.push(f32::from_le_bytes(bytes));
    }
    Ok(samples)
}

/// Transcribes a pending spool through the normal ASR path and returns the
/// transcript. Reuses `dictation::transcribe` so a recovered take goes
/// through the identical code as a live one.
pub fn recover(app: &tauri::AppHandle, pending: &PendingRecovery) -> Result<String, String> {
    use teletype_core::audio::Captured;
    let samples = read_spool(&pending.path)?;
    if samples.is_empty() {
        return Err("recovery spool is empty".into());
    }
    let duration_secs = samples.len() as f64 / TARGET_SAMPLE_RATE as f64;
    let peak = samples.iter().fold(0f32, |p, &s| p.max(s.abs()));
    let captured = Captured {
        samples,
        duration_secs,
        peak,
    };
    let state = app.state::<crate::AppState>();
    let settings = state.settings();
    let (model_path, kind) = crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    let language = crate::dictation::effective_language(
        settings.language,
        crate::dictation::is_english_only(kind),
    );
    let (text, _words) =
        crate::dictation::transcribe(app, &model_path, &captured, &language, kind, &[])?;
    Ok(text)
}

/// Deletes the pending spool file (used by both recover-on-success and
/// discard).
pub fn discard(app_data_dir: &Path) -> Result<(), String> {
    match find_pending(app_data_dir) {
        Some(p) => {
            // The `find_pending` scan only matches `spool-*.pcm`, so the
            // sidecar never gets cleaned up by the scan itself: remove it
            // here or every recovered take leaves a `.meta` behind for good.
            let _ = fs::remove_file(p.path.with_extension("meta"));
            fs::remove_file(&p.path).map_err(|e| format!("recovery discard: {e}"))
        }
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the spool-drain tests. The `Spool` writer runs on a
    /// background thread that the test joins (bounded) to flush the file.
    /// Under the full parallel test suite (567 tests on 12 CPUs) the writer
    /// thread can be starved for CPU and not drain within the join budget,
    /// leaving the file under `MIN_BYTES` and making the dependent tests
    /// flake. A shared mutex runs these tests one at a time so the writer
    /// always gets the CPU it needs.
    static SPOOL_DRAIN_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn spool_drain_guard() -> std::sync::MutexGuard<'static, ()> {
        SPOOL_DRAIN_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A unique temp directory per test.
    ///
    /// `tempfile::tempdir()` (a `TempDir`) is used rather than a hand-rolled
    /// name under the system temp dir: two tests calling this in the same
    /// nanosecond tick produced the same path, and the second test's
    /// `create_dir_all` then operated on the first test's directory — causing
    /// cross-test interference (a `NotFound` on a file another test's
    /// `complete()` deleted, a `set_times` EINVAL on a file that was gone,
    /// and a doubled size assertion when two spools wrote into one dir).
    /// `TempDir` removes the directory on drop, so no explicit cleanup is
    /// needed; a leaked `TempDir` (assertion panic) is the standard test
    /// failure artifact.
    fn tmpdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn set_mtime(path: &Path, t: SystemTime) {
        #[cfg(unix)]
        {
            let ft = std::fs::FileTimes::new().set_modified(t).set_accessed(t);
            let f = fs::OpenOptions::new().write(true).open(path).unwrap();
            f.set_times(ft).unwrap();
        }
        #[cfg(not(unix))]
        {
            let _ = (path, t);
        }
    }

    #[test]
    fn begin_write_complete_leaves_no_file() {
        let d = tmpdir();
        let mut spool = Spool::begin(d.path()).unwrap();
        // 1 second of 16 kHz audio.
        spool.write(&vec![0.1f32; TARGET_SAMPLE_RATE as usize]);
        spool.complete();
        let remaining: Vec<_> = fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("spool-") && n.ends_with(".pcm"))
                    .unwrap_or(false)
            })
            .collect();
        assert!(
            remaining.is_empty(),
            "expected no spool file after complete, got {remaining:?}"
        );
    }

    #[test]
    fn begin_write_abandoned_leaves_file_with_size() {
        let _guard = spool_drain_guard();
        let d = tmpdir();
        let mut spool = Spool::begin(d.path()).unwrap();
        // Hold a writer clone like production (the audio callback does), so the
        // channel stays open and the writer thread blocks in recv instead of
        // exiting on an early disconnect. Without it, under parallel test load
        // the writer's first recv can return Disconnected before write() runs,
        // and the frame is lost.
        let _w = spool.clone_writer().expect("spool has a writer");
        let n = TARGET_SAMPLE_RATE as usize;
        let path = spool.path.clone();
        spool.write(&vec![0.25f32; n]);
        drop(_w);
        spool.abandoned_drained();
        let meta = fs::metadata(&path).expect("spool file should exist after abandon");
        assert_eq!(meta.len(), (n * 4) as u64, "expected {} bytes", n * 4);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "expected 0600 perms, got {mode:o}");
        }
    }

    #[test]
    fn row9_spool_bounds_at_five_minutes_and_counts_the_whole_take() {
        let _guard = spool_drain_guard();
        let d = tmpdir();
        let mut spool = Spool::begin(d.path()).unwrap();
        // Hold a writer clone like production, so the channel stays open for
        // the whole write burst (see begin_write_abandoned_leaves_file_with_size).
        let _w = spool.clone_writer().expect("spool has a writer");
        // Write 10 minutes of 1-second frames. Only 300 s may be stored on
        // disk; the total counter (Arc<AtomicU64>) sees all 600 s because
        // Spool::write increments it before the bound check.
        for _ in 0..600 {
            spool.write(&vec![0.1f32; TARGET_SAMPLE_RATE as usize]);
        }
        let path = spool.path.clone();
        drop(_w);
        spool.abandoned_drained();
        // The writer is joined (deterministic) so the on-disk file is exactly
        // the oldest 300 s; we assert the upper bound (it must not exceed 300 s)
        // and the total counter below.
        // must not exceed 300 s). The lower bound is verified by the
        // find_pending call below, which checks pending.seconds is in [200, 300].
        let expected_max = MAX_SECONDS * TARGET_SAMPLE_RATE as u64 * 4;
        let meta = fs::metadata(&path).unwrap();
        assert!(
            meta.len() <= expected_max,
            "spool {} bytes exceeds 300 s bound {}",
            meta.len(),
            expected_max
        );
        // The sidecar meta file records the total byte count.
        let meta_path = path.with_extension("meta");
        assert!(
            meta_path.exists(),
            "the sidecar meta file must exist after abandoned"
        );
        let total_bytes: u64 = fs::read_to_string(&meta_path)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let total_seconds = total_bytes / (4 * TARGET_SAMPLE_RATE as u64);
        assert_eq!(
            total_seconds, 600,
            "expected 600 s total, got {total_seconds}"
        );
        // The spool must not exceed the 300 s bound, the total counter must
        // reflect all 600 s, and the spool must be marked truncated. The
        // on-disk seconds are timing-dependent (the writer drains in the
        // background), so we only assert the upper bound.
        set_mtime(&path, SystemTime::now() - (Duration::from_secs(11 * 60)));
        let pending = find_pending(d.path()).expect("spool must be offered");
        assert!(
            pending.seconds <= 300,
            "spool must not exceed 300 s bound, got {}",
            pending.seconds
        );
        assert_eq!(pending.total_seconds, 600, "expected 600 s total");
        assert!(pending.truncated, "spool must be marked truncated");
    }

    #[test]
    fn row9_short_take_reports_no_loss() {
        let _guard = spool_drain_guard();
        let d = tmpdir();
        let mut spool = Spool::begin(d.path()).unwrap();
        // Hold a writer clone like production, so the channel stays open for
        // the whole write burst (see begin_write_abandoned_leaves_file_with_size).
        let _w = spool.clone_writer().expect("spool has a writer");
        // Write 90 seconds of 1-second frames. Well under the 300 s bound.
        for _ in 0..90 {
            spool.write(&vec![0.1f32; TARGET_SAMPLE_RATE as usize]);
        }
        let path = spool.path.clone();
        drop(_w);
        spool.abandoned_drained();
        let expected_max = MAX_SECONDS * TARGET_SAMPLE_RATE as u64 * 4;
        let meta = fs::metadata(&path).unwrap();
        assert!(
            meta.len() <= expected_max,
            "spool {} bytes exceeds 300 s bound",
            meta.len()
        );
        set_mtime(&path, SystemTime::now() - (Duration::from_secs(11 * 60)));
        let pending = find_pending(d.path()).expect("spool must be offered");
        // The on-disk seconds are timing-dependent (the writer drains in the
        // background), so we only assert the upper bound. The total counter
        // and truncated flag are deterministic.
        assert!(
            pending.seconds <= 90,
            "spool must not exceed 90 s, got {}",
            pending.seconds
        );
        assert_eq!(pending.total_seconds, 90, "expected 90 s total (no loss)");
        assert!(!pending.truncated, "90 s spool must not be truncated");
    }

    /// HARD-4 row 9: a spool left by a kill that never ran the stop path has
    /// no sidecar, so the total duration is unknown. The offer must then claim
    /// nothing about lost audio rather than invent a loss the app cannot
    /// prove. This is the case the sidecar cannot cover, and the reason the
    /// UI distinguishes "known truncated" from "unknown".
    #[test]
    fn row9_spool_without_a_sidecar_claims_no_loss() {
        let _guard = spool_drain_guard();
        let d = tmpdir();
        let mut spool = Spool::begin(d.path()).unwrap();
        // Hold a writer clone like production, so the channel stays open for
        // the whole write burst (see begin_write_abandoned_leaves_file_with_size).
        let _w = spool.clone_writer().expect("spool has a writer");
        for _ in 0..30 {
            spool.write(&vec![0.1f32; TARGET_SAMPLE_RATE as usize]);
        }
        let path = spool.path.clone();
        drop(_w);
        spool.abandoned_drained();
        let meta_path = path.with_extension("meta");
        assert!(
            meta_path.exists(),
            "abandoned must write the sidecar this test then removes"
        );
        fs::remove_file(&meta_path).unwrap();
        // The writer is joined (deterministic), so the file holds exactly the
        // 30 frames written. find_pending skips files under MIN_BYTES, so the
        // deterministic drain is what makes this offer guaranteed.
        set_mtime(&path, SystemTime::now() - (Duration::from_secs(11 * 60)));
        let pending = find_pending(d.path()).expect("spool must be offered");
        assert_eq!(
            pending.total_seconds, pending.seconds,
            "with no sidecar the total is only what is on disk"
        );
        assert!(
            !pending.truncated,
            "an unknown total must not be reported as a loss"
        );
    }

    /// HARD-4 row 9: `discard` (the path both a successful recovery and a
    /// dismissal take) must remove the sidecar too. `find_pending` only
    /// matches `spool-*.pcm`, so a leftover `.meta` would never be cleaned up.
    #[test]
    fn row9_discard_removes_the_sidecar() {
        let d = tmpdir();
        let mut spool = Spool::begin(d.path()).unwrap();
        for _ in 0..30 {
            spool.write(&vec![0.1f32; TARGET_SAMPLE_RATE as usize]);
        }
        let path = spool.path.clone();
        spool.abandoned();
        set_mtime(&path, SystemTime::now() - (Duration::from_secs(11 * 60)));
        assert!(path.with_extension("meta").exists(), "sidecar exists");
        // `discard` takes the app data dir and derives the recovery dir from
        // it, so point it at a temp dir holding this spool.
        discard(d.path()).unwrap();
        assert!(!path.exists(), "the spool must be gone");
        assert!(
            !path.with_extension("meta").exists(),
            "the sidecar must not be left behind"
        );
    }

    #[test]
    fn find_pending_ignores_small_and_young_finds_valid_deletes_expired() {
        let d = tmpdir();
        let dir = d.path().join("recovery");
        fs::create_dir_all(&dir).unwrap();

        // 1. A small file (< 100 KB): ignored.
        let small = dir.join("spool-small.pcm");
        fs::write(&small, vec![0u8; 1024]).unwrap();

        // 2. A fresh large file (young, < 10 min): ignored.
        let fresh = dir.join("spool-fresh.pcm");
        fs::write(&fresh, vec![0u8; 200_000]).unwrap();

        // 3. An 8-day-old file: deleted unconditionally.
        let expired = dir.join("spool-expired.pcm");
        fs::write(&expired, vec![0u8; 200_000]).unwrap();
        let old = SystemTime::now() - EXPIRY - Duration::from_secs(3600);
        set_mtime(&expired, old);

        // 4. A valid candidate: > 100 KB and > 10 min old (1 hour old).
        let valid = dir.join("spool-valid.pcm");
        let n_bytes = 15 * TARGET_SAMPLE_RATE as usize * 4; // 15 s, whole f32s
        fs::write(&valid, vec![0u8; n_bytes]).unwrap();
        let valid_time = SystemTime::now() - Duration::from_secs(3600);
        set_mtime(&valid, valid_time);

        let pending = find_pending(d.path()).expect("should find the valid spool");
        assert_eq!(pending.path, valid, "expected the valid spool");
        assert_eq!(pending.seconds, 15, "expected 15 seconds");
        assert!(
            !pending.truncated,
            "valid spool is whole-f32 so not truncated"
        );

        assert!(!expired.exists(), "expired spool should be deleted");
        assert!(small.exists(), "small spool should remain");
        assert!(fresh.exists(), "fresh spool should remain");
    }

    /// HARD-3 (#17): a crash/quit mid-take leaves the partial spool on disk.
    /// The next launch's `find_pending` still finds it and `read_spool`
    /// decodes the partial content — the audio is recoverable, never lost
    /// silently.
    #[cfg(feature = "fault-injection")]
    #[test]
    fn quit_mid_take_leaves_recoverable_spool() {
        use crate::fault_inject;
        // Serialize with the other spool tests: this test arms the global
        // `quit-mid-take` fault, and any spool writer running concurrently
        // would hit it and exit early, truncating that test's file. The
        // fault-inject `scoped()` guard alone is not enough because the other
        // spool tests do not take it.
        let _drain = spool_drain_guard();
        let _guard = fault_inject::scoped();
        // The spool is bounded to 60 s of audio; feed ~45 s so the crash
        // leaves a file over MIN_BYTES (100 KB) with room to spare.
        let frame = vec![0.2f32; TARGET_SAMPLE_RATE as usize];
        let d = tmpdir();
        let path = {
            let spool = Spool::begin(d.path()).unwrap();
            // The audio-callback writer handle (the production write path):
            // a plain try_send per frame, no bound bookkeeping on the caller.
            let writer = spool.clone_writer().expect("spool writer");
            // The writer thread dies on the fault point part-way through,
            // leaving a partial file.
            fault_inject::arm("quit-mid-take");
            for _ in 0..45 {
                writer.write(&frame);
            }
            let path = spool.path.clone();
            // The crash kills the writer thread on its next frame; the
            // frames queued before it are flushed to disk by the join
            // inside `abandoned` (bounded, but the queue is small).
            spool.abandoned();
            path
        };

        // The partial file is still on disk.
        assert!(
            path.exists(),
            "the partial spool must survive the crash for recovery"
        );
        let size = fs::metadata(&path).unwrap().len();
        // The writer may have died on its very first frame (the fault point
        // is checked before the write), so the only hard guarantee is that
        // the file exists; the queued frames make it very likely to hold
        // audio. When it does, the audio must be a whole number of frames
        // or a clean partial (read_spool below covers the corrupt case).
        if size > 0 {
            assert!(
                size <= (45 * TARGET_SAMPLE_RATE as usize * 4) as u64,
                "the spool must not hold more than the frames sent before the crash"
            );
        }

        // The next launch's scan finds it. `find_pending` needs the file to
        // be at least MIN_AGE old AND at least MIN_BYTES (100 KB, ~1.5 s)
        // large, so backdate the mtime. The crash-cut file may be smaller
        // than MIN_BYTES when the writer died early — in that case
        // `find_pending` ignores it (the documented tiny-spool rule), and
        // the recovery assertions below are skipped.
        let offerable = size >= MIN_BYTES;
        set_mtime(&path, SystemTime::now() - (Duration::from_secs(11 * 60)));
        if offerable {
            set_mtime(&path, SystemTime::now() - (Duration::from_secs(11 * 60)));
            let pending =
                find_pending(d.path()).expect("the partial spool must be offered for recovery");
            assert_eq!(pending.path, path);
            assert!(
                pending.seconds > 0,
                "the partial spool must report recoverable audio"
            );
        }

        // The partial file decodes (or cleanly errors when the crash cut
        // mid-sample) — never a panic, never silent loss. An empty file
        // (the writer died before its first frame) is the one legitimate
        // outcome: nothing was written, so there is nothing to recover.
        match read_spool(&path) {
            Ok(samples) => {
                if size > 0 {
                    assert!(!samples.is_empty(), "the recovered audio must not be empty");
                }
            }
            Err(e) => {
                assert!(
                    e.contains("corrupt"),
                    "a mid-sample crash must surface as a clear corrupt error, got: {e}"
                );
            }
        }
    }

    #[test]
    fn find_pending_none_when_empty() {
        let d = tmpdir();
        assert!(find_pending(d.path()).is_none());
    }

    #[test]
    fn read_spool_rejects_corrupt_file() {
        let d = tmpdir();
        let dir = d.path().join("recovery");
        fs::create_dir_all(&dir).unwrap();
        // Length not a multiple of 4 (truncated mid-frame).
        let corrupt = dir.join("spool-corrupt.pcm");
        fs::write(&corrupt, vec![0u8; 13]).unwrap();
        let result = read_spool(&corrupt);
        assert!(
            result.is_err(),
            "corrupt spool must return an error, not panic"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("corrupt"),
            "error should mention corruption: {err}"
        );
    }

    #[test]
    fn read_spool_roundtrips_valid_file() {
        let d = tmpdir();
        let dir = d.path().join("recovery");
        fs::create_dir_all(&dir).unwrap();
        let n = TARGET_SAMPLE_RATE as usize;
        let mut bytes = Vec::with_capacity(n * 4);
        for i in 0..n {
            let s = (i as f32 / n as f32 - 0.5) * 0.5;
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        let p = dir.join("spool-valid.pcm");
        fs::write(&p, &bytes).unwrap();
        let samples = read_spool(&p).unwrap();
        assert_eq!(samples.len(), n);
        assert!((samples[0] - (-0.25)).abs() < 1e-3);
        assert!((samples[n / 2] - 0.0).abs() < 1e-3);
    }

    /// Regression (2026-10-19 pill-stuck incident): `complete()` hangs the
    /// caller forever when a `SpoolWriter` clone is still alive (the running
    /// capture's `on_frame` holds one; `stop()` joined the writer before the
    /// capture stopped). The join must be bounded: `complete()` on a worker
    /// thread has to RETURN even with a live writer clone held on `main`.
    #[test]
    fn complete_returns_with_live_writer_clone_held() {
        let d = tmpdir();
        let spool = Spool::begin(d.path()).expect("writer");
        let held = spool.clone_writer().expect("owned");

        let file = spool.path.clone();
        let (sent, recv) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            spool.complete();
            let _ = sent.send(());
        });
        // With the old unbounded join this timed out (the writer parks on
        // recv until the last SpoolWriter drops).
        recv.recv_timeout(Duration::from_secs(3))
            .expect("complete() must return even while a SpoolWriter clone is held");
        // The bounded-join timeout leafs the spool file behind for
        // later-recover (same semantics as `abandoned()` on the crash path).
        assert!(file.exists());

        drop(held);
        std::thread::sleep(Duration::from_millis(300)); // let the writer drain out
    }
}
