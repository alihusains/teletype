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
pub const MAX_SECONDS: u64 = 60;
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
    /// partial write from a crash) or the 60 s bound was hit.
    pub truncated: bool,
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
    /// Set once the bound is exceeded; further writes are dropped.
    truncated: bool,
    /// Audio-callback -> writer-thread channel. `None` once stopped.
    tx: Option<mpsc::SyncSender<Vec<f32>>>,
    /// Writer thread join handle.
    writer: Option<thread::JoinHandle<()>>,
    /// One-shot guard so "dropping frames" is logged at most once per spool.
    dropped_logged: std::sync::atomic::AtomicBool,
}

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
            "spool-{}.pcm",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
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
        if self.truncated || self.tx.is_none() {
            return; // bound already hit, or writer gone
        }
        let frame_len = frame.len() as u64 * 4;
        // If this frame would push us past the bound, stop now and mark
        // truncated. (We keep what's already written = the oldest 60 s.)
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
        self.tx.clone().map(|tx| SpoolWriter { tx })
    }

    /// Clean stop: stop the writer, wait for it to drain, delete the file.
    pub fn complete(mut self) {
        self.tx.take(); // disconnect -> writer drains remaining frames then exits
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        let _ = fs::remove_file(&self.path);
    }

    /// Crash path: leave the file in place. The writer is stopped and joined
    /// (so all buffered frames are flushed to disk) but the file is NOT
    /// deleted, so the next launch can recover it.
    #[allow(dead_code)]
    pub fn abandoned(mut self) {
        self.tx.take();
        if let Some(w) = self.writer.take() {
            let _ = w.join();
        }
        // Intentionally no remove_file: the file is the recovery artifact.
    }
}

/// A cheap, shareable handle to a spool's writer channel, for the audio
/// callback. Cloning is a cheap `mpsc::Sender` clone; writing through it
/// never blocks (see [`Spool::write`]).
#[derive(Clone)]
pub struct SpoolWriter {
    tx: mpsc::SyncSender<Vec<f32>>,
}

impl SpoolWriter {
    /// Appends a captured **16 kHz mono** frame. Unlike [`Spool::write`], this
    /// does not enforce the byte bound — the writer thread enforces it on the
    /// file side. Cheap and non-blocking: a `try_send` only; if the channel
    /// is full the frame is dropped silently (the callback must stay cheap).
    pub fn write(&self, frame: &[f32]) {
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
    let mut file = file;
    let mut total = 0u64;
    for frame in rx {
        if total >= max_bytes {
            break; // bound enforced on the write side too (belt and braces)
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
            crate::log_entry(
                crate::LogLevel::Info,
                format!("recovery: deleted expired spool {}", path.display()),
            );
            continue;
        }
        // Not old enough or too small to be worth offering.
        if age < MIN_AGE || size < MIN_BYTES {
            continue;
        }
        let truncated = size % 4 != 0;
        let seconds = (size / (4 * TARGET_SAMPLE_RATE as u64)).min(MAX_SECONDS);
        let cand = PendingRecovery {
            path,
            seconds,
            truncated,
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
    Ok((0..count)
        .map(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect())
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
    let (model_path, use_parakeet) =
        crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    let language = crate::dictation::effective_language(settings.language, use_parakeet);
    crate::dictation::transcribe(app, &model_path, &captured, &language, use_parakeet)
}

/// Deletes the pending spool file (used by both recover-on-success and
/// discard).
pub fn discard(app_data_dir: &Path) -> Result<(), String> {
    match find_pending(app_data_dir) {
        Some(p) => fs::remove_file(&p.path).map_err(|e| format!("recovery discard: {e}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "teletype-recovery-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn cleanup(d: &Path) {
        let _ = fs::remove_dir_all(d);
    }

    fn set_mtime(path: &Path, t: SystemTime) {
        #[cfg(unix)]
        {
            use std::os::macos::fs::FileTimesExt;
            let ft = std::fs::FileTimes::new()
                .set_modified(t)
                .set_accessed(t);
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
        let mut spool = Spool::begin(&d).unwrap();
        // 1 second of 16 kHz audio.
        spool.write(&vec![0.1f32; TARGET_SAMPLE_RATE as usize]);
        spool.complete();
        let remaining: Vec<_> = fs::read_dir(&d)
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
        cleanup(&d);
    }

    #[test]
    fn begin_write_abandoned_leaves_file_with_size() {
        let d = tmpdir();
        let mut spool = Spool::begin(&d).unwrap();
        let n = TARGET_SAMPLE_RATE as usize;
        let path = spool.path.clone();
        spool.write(&vec![0.25f32; n]);
        spool.abandoned();
        let meta = fs::metadata(&path).expect("spool file should exist after abandon");
        assert_eq!(meta.len(), (n * 4) as u64, "expected {} bytes", n * 4);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = meta.permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "expected 0600 perms, got {mode:o}");
        }
        cleanup(&d);
    }

    #[test]
    fn sixty_second_bound_bounds_storage() {
        let d = tmpdir();
        let mut spool = Spool::begin(&d).unwrap();
        // Write 70 seconds of 1-second frames. Only ~60 s may be stored.
        for _ in 0..70 {
            spool.write(&vec![0.1f32; TARGET_SAMPLE_RATE as usize]);
        }
        let path = spool.path.clone();
        spool.abandoned();
        // Allow the writer thread a moment to drain the channel and hit the
        // bound before we inspect the file.
        std::thread::sleep(Duration::from_millis(300));
        let meta = fs::metadata(&path).unwrap();
        let expected_max = MAX_SECONDS * TARGET_SAMPLE_RATE as u64 * 4;
        assert!(
            meta.len() <= expected_max,
            "spool {} bytes exceeds 60 s bound {}",
            meta.len(),
            expected_max
        );
        // It should have written ~60 s (not 0, not 70 s).
        assert!(
            meta.len() >= expected_max - (16000 * 4) as u64,
            "expected ~60 s, got {} bytes",
            meta.len()
        );
        cleanup(&d);
    }

    #[test]
    fn find_pending_ignores_small_and_young_finds_valid_deletes_expired() {
        let d = tmpdir();
        let dir = d.join("recovery");
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

        let pending = find_pending(&d).expect("should find the valid spool");
        assert_eq!(pending.path, valid, "expected the valid spool");
        assert_eq!(pending.seconds, 15, "expected 15 seconds");
        assert!(
            !pending.truncated,
            "valid spool is whole-f32 so not truncated"
        );

        assert!(!expired.exists(), "expired spool should be deleted");
        assert!(small.exists(), "small spool should remain");
        assert!(fresh.exists(), "fresh spool should remain");
        cleanup(&d);
    }

    #[test]
    fn find_pending_none_when_empty() {
        let d = tmpdir();
        assert!(find_pending(&d).is_none());
        cleanup(&d);
    }

    #[test]
    fn read_spool_rejects_corrupt_file() {
        let d = tmpdir();
        let dir = d.join("recovery");
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
        cleanup(&d);
    }

    #[test]
    fn read_spool_roundtrips_valid_file() {
        let d = tmpdir();
        let dir = d.join("recovery");
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
        cleanup(&d);
    }
}
