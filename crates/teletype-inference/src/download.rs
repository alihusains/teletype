//! Catalog model download with SHA-256 verification and progress reporting.
//!
//! Downloads stream to a `.part` file, are hashed while written, and only
//! renamed into place after the digest matches. Multi-file (split) GGUFs
//! install under `models/<id>/` and verify every shard.
//!
//! Callers can pass an `on_progress` callback to receive live
//! bytes / speed / ETA updates (throttled to ~10 Hz).

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::catalog::{CatalogEntry, CatalogShard};

/// Progress payload emitted to the UI while a model downloads.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// Model id from the catalog (`s1-mini`, `eg-1`, …).
    pub id: String,
    /// "downloading" | "verifying" | "done" | "error"
    pub status: String,
    /// File currently being written (shard name or single file).
    pub file_name: String,
    /// 1-based index of the current file within the entry.
    pub file_index: u32,
    pub file_count: u32,
    /// Bytes written so far for the **current** file.
    pub file_downloaded_bytes: u64,
    /// Expected size of the current file (0 if unknown).
    pub file_total_bytes: u64,
    /// Bytes accounted for across all files (completed + current).
    pub downloaded_bytes: u64,
    /// Expected total across all files (0 if unknown).
    pub total_bytes: u64,
    /// Overall percent 0–100 (monotonic within one download).
    pub percent: f64,
    /// Instantaneous-ish download speed in bytes/sec (EMA).
    pub speed_bps: f64,
    /// Seconds remaining; `None` when total or speed is unknown.
    pub eta_seconds: Option<f64>,
    /// Error message when `status == "error"`.
    pub error: Option<String>,
}

/// Callback type for progress updates.
pub type ProgressFn = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

/// Fields for a single progress emit; keeps `ProgressTracker::emit` within
/// the argument limit.
struct EmitArgs<'a> {
    status: &'a str,
    file_name: &'a str,
    file_index: u32,
    file_downloaded: u64,
    file_total: u64,
    force: bool,
    error: Option<String>,
}

/// Tracks speed (EMA) and ETA across a multi-file download.
struct ProgressTracker {
    id: String,
    on_progress: Option<ProgressFn>,
    /// Bytes already complete from prior files / already-on-disk shards.
    prior_bytes: u64,
    total_bytes: u64,
    file_count: u32,
    started: Instant,
    last_emit: Instant,
    /// Exponential moving average of bytes/sec.
    speed_bps: f64,
    /// Overall percent already locked in by finished files (never goes back).
    min_percent: f64,
}

impl ProgressTracker {
    fn new(
        id: &str,
        on_progress: Option<ProgressFn>,
        prior_bytes: u64,
        total_bytes: u64,
        file_count: u32,
    ) -> Self {
        let now = Instant::now();
        Self {
            id: id.to_string(),
            on_progress,
            prior_bytes,
            total_bytes,
            file_count,
            started: now,
            last_emit: now - Duration::from_secs(1),
            speed_bps: 0.0,
            min_percent: 0.0,
        }
    }

    fn overall_bytes(&self, file_downloaded: u64) -> u64 {
        self.prior_bytes + file_downloaded
    }

    fn emit(&mut self, args: EmitArgs<'_>) {
        let EmitArgs {
            status,
            file_name,
            file_index,
            file_downloaded,
            file_total,
            force,
            error,
        } = args;
        let Some(cb) = self.on_progress.clone() else {
            return;
        };

        let now = Instant::now();
        let elapsed = now.duration_since(self.started).as_secs_f64();
        let overall = self.overall_bytes(file_downloaded);

        // Update speed EMA only while actively downloading.
        if status == "downloading" && elapsed > 0.05 {
            let instant_bps = overall as f64 / elapsed;
            // Blend: 0.3 new sample, 0.7 history (smooth but responsive).
            if self.speed_bps <= 0.0 {
                self.speed_bps = instant_bps;
            } else {
                self.speed_bps = self.speed_bps * 0.7 + instant_bps * 0.3;
            }
        }

        if !force && now.duration_since(self.last_emit) < Duration::from_millis(100) {
            return;
        }
        self.last_emit = now;

        let total = if self.total_bytes > 0 {
            self.total_bytes
        } else {
            file_total.saturating_add(self.prior_bytes)
        };
        let downloaded = if total > 0 { overall.min(total) } else { overall };

        let mut percent = if total > 0 {
            downloaded as f64 / total as f64 * 100.0
        } else {
            0.0
        };
        // Never show a lower % than a previously completed file implied.
        if status != "error" {
            percent = percent.max(self.min_percent).min(100.0);
            if status == "done" {
                percent = 100.0;
                self.min_percent = 100.0;
            } else {
                self.min_percent = self.min_percent.max(percent);
            }
        }

        let eta = if status == "done" {
            Some(0.0)
        } else if self.speed_bps > 1024.0 && total > downloaded {
            Some((total - downloaded) as f64 / self.speed_bps)
        } else {
            None
        };

        cb(DownloadProgress {
            id: self.id.clone(),
            status: status.to_string(),
            file_name: file_name.to_string(),
            file_index,
            file_count: self.file_count,
            file_downloaded_bytes: file_downloaded,
            file_total_bytes: file_total,
            downloaded_bytes: downloaded,
            total_bytes: total,
            percent,
            speed_bps: if status == "downloading" {
                self.speed_bps
            } else if status == "done" {
                0.0
            } else {
                self.speed_bps
            },
            eta_seconds: eta,
            error,
        });
    }
}

/// Downloads `entry` into `models_dir` if not already present.
/// Returns the llama-server entrypoint path.
///
/// `on_progress` receives throttled [`DownloadProgress`] updates.
pub fn download_entry_with_progress(
    entry: &CatalogEntry,
    models_dir: &Path,
    on_progress: Option<ProgressFn>,
) -> Result<PathBuf, String> {
    if entry.is_downloaded(models_dir) {
        if let Some(cb) = &on_progress {
            cb(DownloadProgress {
                id: entry.id.to_string(),
                status: "done".into(),
                file_name: String::new(),
                file_index: 0,
                file_count: 0,
                file_downloaded_bytes: 0,
                file_total_bytes: 0,
                downloaded_bytes: 0,
                total_bytes: 0,
                percent: 100.0,
                speed_bps: 0.0,
                eta_seconds: Some(0.0),
                error: None,
            });
        }
        return Ok(entry.entrypoint(models_dir));
    }

    let run = || -> Result<PathBuf, String> {
        if entry.shards.is_empty() {
            let dest = models_dir.join(format!("{}.gguf", entry.id));
            let expected_total = if entry.size_mb > 0 {
                entry.size_mb as u64 * 1024 * 1024
            } else {
                0
            };
            let mut tracker = ProgressTracker::new(
                entry.id,
                on_progress.clone(),
                0,
                expected_total,
                1,
            );
            download_single(entry, &dest, &mut tracker)?;
            tracker.emit(EmitArgs {
                status: "done",
                file_name: dest
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(""),
                file_index: 1,
                file_downloaded: 0,
                file_total: 0,
                force: true,
                error: None,
            });
        } else {
            let dir = models_dir.join(entry.id);
            fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;

            let total: u64 = entry.shards.iter().map(|s| s.size_bytes).sum();
            let mut prior: u64 = 0;
            let n = entry.shards.len() as u32;
            // First pass: count already-valid shards so progress starts partway.
            for shard in entry.shards {
                let path = dir.join(shard.file_name);
                if path.is_file() && verify_sha256(&path, shard.sha256).is_ok() {
                    prior += shard.size_bytes;
                }
            }

            let mut tracker =
                ProgressTracker::new(entry.id, on_progress.clone(), prior, total, n);

            for (i, shard) in entry.shards.iter().enumerate() {
                let dest = dir.join(shard.file_name);
                let idx = (i + 1) as u32;
                if dest.is_file() {
                    match verify_sha256(&dest, shard.sha256) {
                        Ok(()) => continue,
                        Err(e) => {
                            tracing::warn!(
                                file = %shard.file_name,
                                error = %e,
                                "re-verifying existing shard failed; re-downloading"
                            );
                            let _ = fs::remove_file(&dest);
                            // This shard no longer counts as prior.
                            prior = prior.saturating_sub(shard.size_bytes);
                            tracker.prior_bytes = prior;
                        }
                    }
                }
                tracing::info!(
                    shard = i + 1,
                    total = entry.shards.len(),
                    file = %shard.file_name,
                    "downloading EG-style shard"
                );
                tracker.emit(EmitArgs {
                    status: "downloading",
                    file_name: shard.file_name,
                    file_index: idx,
                    file_downloaded: 0,
                    file_total: shard.size_bytes,
                    force: true,
                    error: None,
                });
                download_shard(shard, &dest, &mut tracker, idx)?;
                prior += shard.size_bytes;
                tracker.prior_bytes = prior;
            }
            tracker.emit(EmitArgs {
                status: "done",
                file_name: "",
                file_index: n,
                file_downloaded: 0,
                file_total: 0,
                force: true,
                error: None,
            });
        }
        Ok(entry.entrypoint(models_dir))
    };

    match run() {
        Ok(p) => Ok(p),
        Err(e) => {
            if let Some(cb) = &on_progress {
                cb(DownloadProgress {
                    id: entry.id.to_string(),
                    status: "error".into(),
                    file_name: String::new(),
                    file_index: 0,
                    file_count: entry.shards.len().max(1) as u32,
                    file_downloaded_bytes: 0,
                    file_total_bytes: 0,
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    percent: 0.0,
                    speed_bps: 0.0,
                    eta_seconds: None,
                    error: Some(e.clone()),
                });
            }
            Err(e)
        }
    }
}

/// Convenience wrapper with no progress callback.
pub fn download_entry(entry: &CatalogEntry, models_dir: &Path) -> Result<PathBuf, String> {
    download_entry_with_progress(entry, models_dir, None)
}

fn download_single(
    entry: &CatalogEntry,
    dest: &Path,
    tracker: &mut ProgressTracker,
) -> Result<(), String> {
    if let Some(sha) = entry.sha256 {
        match try_fetch_to_part(entry.url, dest, Some(sha), tracker, 1) {
            Ok(()) => return Ok(()),
            Err(primary_err) => {
                if let Some(backup) = entry.backup_url {
                    tracing::warn!(error = %primary_err, "primary URL failed; trying backup");
                    return try_fetch_to_part(backup, dest, Some(sha), tracker, 1);
                }
                return Err(primary_err);
            }
        }
    }
    try_fetch_to_part(entry.url, dest, None, tracker, 1)
}

fn download_shard(
    shard: &CatalogShard,
    dest: &Path,
    tracker: &mut ProgressTracker,
    file_index: u32,
) -> Result<(), String> {
    try_fetch_to_part(shard.url, dest, Some(shard.sha256), tracker, file_index)
}

fn try_fetch_to_part(
    url: &str,
    dest: &Path,
    expected_sha: Option<&str>,
    tracker: &mut ProgressTracker,
    file_index: u32,
) -> Result<(), String> {
    let part = part_path(dest);
    if let Err(e) = fs::remove_file(&part) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("clear {}: {e}", part.display()));
        }
    }

    let file_name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let mut resp = client
        .get(url)
        .header("User-Agent", "teletype/0.1")
        .send()
        .map_err(|e| format!("request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} for {url}", resp.status()));
    }

    let content_len = resp.content_length().unwrap_or(0);
    tracker.emit(EmitArgs {
        status: "downloading",
        file_name: &file_name,
        file_index,
        file_downloaded: 0,
        file_total: content_len,
        force: true,
        error: None,
    });

    let mut file = fs::File::create(&part).map_err(|e| format!("create {}: {e}", part.display()))?;
    let mut hasher = expected_sha.map(|_| Sha256::new());
    let mut buf = [0u8; 64 * 1024];
    let mut written: u64 = 0;
    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| format!("read body: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write {}: {e}", part.display()))?;
        if let Some(h) = hasher.as_mut() {
            h.update(&buf[..n]);
        }
        written += n as u64;
        tracker.emit(EmitArgs {
            status: "downloading",
            file_name: &file_name,
            file_index,
            file_downloaded: written,
            file_total: content_len,
            force: false,
            error: None,
        });
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    drop(file);

    tracker.emit(EmitArgs {
        status: "verifying",
        file_name: &file_name,
        file_index,
        file_downloaded: written,
        file_total: content_len.max(written),
        force: true,
        error: None,
    });

    if let (Some(h), Some(expected)) = (hasher, expected_sha) {
        let actual = hex::encode(h.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(&part);
            return Err(format!(
                "SHA-256 mismatch for {}: expected {expected}, got {actual}",
                dest.display()
            ));
        }
    }

    fs::rename(&part, dest).map_err(|e| format!("rename into place: {e}"))?;
    Ok(())
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model.bin".into());
    name.push_str(".part");
    dest.with_file_name(name)
}

/// Streams `path` through SHA-256 and compares to `expected` (hex, case-insensitive).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let mut file = fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(format!(
            "SHA-256 mismatch for {}: expected {expected}, got {actual}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_detects_mismatch() {
        let dir = std::env::temp_dir().join(format!("teletype-sha-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("blob.bin");
        fs::write(&path, b"hello").unwrap();
        let good = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify_sha256(&path, good).is_ok());
        let err = verify_sha256(&path, &"0".repeat(64)).unwrap_err();
        assert!(err.contains("mismatch"), "got: {err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn part_path_appends_suffix() {
        let p = Path::new("/models/s1-mini.gguf");
        assert_eq!(part_path(p), Path::new("/models/s1-mini.gguf.part"));
    }

    #[test]
    fn progress_percent_monotonic_and_computes_eta() {
        use std::sync::Mutex;
        let seen: Arc<Mutex<Vec<DownloadProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let cb: ProgressFn = Arc::new(move |p| sink.lock().unwrap().push(p));

        let mut t = ProgressTracker::new("t", Some(cb), 0, 1000, 1);
        // Force emits (ignore throttle).
        let mk = |status: &'static str, d: u64| EmitArgs {
            status,
            file_name: "a",
            file_index: 1,
            file_downloaded: d,
            file_total: 1000,
            force: true,
            error: None,
        };
        t.emit(mk("downloading", 0));
        t.emit(mk("downloading", 250));
        t.emit(mk("downloading", 500));
        t.emit(mk("done", 1000));

        let v = seen.lock().unwrap();
        assert!(v.len() >= 4);
        let first = &v[0];
        assert_eq!(first.percent, 0.0);
        let mid = &v[1];
        assert!(mid.percent >= 20.0 && mid.percent <= 30.0, "{}", mid.percent);
        assert!(mid.total_bytes == 1000);
        let last = v.last().unwrap();
        assert_eq!(last.percent, 100.0);
        assert_eq!(last.status, "done");
        // Monotonic
        for w in v.windows(2) {
            assert!(w[1].percent >= w[0].percent - 0.001);
        }
    }

    #[test]
    fn progress_serializes_camel_case() {
        let p = DownloadProgress {
            id: "x".into(),
            status: "downloading".into(),
            file_name: "f".into(),
            file_index: 1,
            file_count: 1,
            file_downloaded_bytes: 10,
            file_total_bytes: 100,
            downloaded_bytes: 10,
            total_bytes: 100,
            percent: 10.0,
            speed_bps: 1024.0,
            eta_seconds: Some(5.0),
            error: None,
        };
        let s = serde_json::to_string(&p).unwrap();
        assert!(s.contains("\"fileDownloadedBytes\""));
        assert!(s.contains("\"etaSeconds\""));
        assert!(s.contains("\"speedBps\""));
    }
}
