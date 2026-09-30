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
    sync::{atomic::AtomicBool, Arc},
    time::{Duration, Instant},
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::catalog::{CatalogEntry, CatalogShard};

/// Multiplier applied to the download size when checking free disk space.
/// EW's DeliveryManifest uses 2.2; we use the same factor to leave room
/// for the `.part` file, filesystem overhead, and any concurrent writes.
const DISK_HEADROOM_FACTOR: f64 = 2.2;

/// Checks that `needed_bytes * DISK_HEADROOM_FACTOR` fits on the volume
/// containing `dir`. Returns a user-facing error string when space is
/// insufficient.
fn check_disk_space(dir: &Path, needed_bytes: u64) -> Result<(), String> {
    let required = (needed_bytes as f64 * DISK_HEADROOM_FACTOR) as u64;
    let available = free_space_on(dir);
    // `None` means the platform probe failed; we do not block on that.
    if let Some(avail) = available {
        if avail < required {
            let avail_mb = avail / (1024 * 1024);
            let need_mb = required / (1024 * 1024);
            return Err(format!(
                "Not enough disk space: need ~{need_mb} MB free (2.2× headroom), \
                 but only {avail_mb} MB is available on this volume. \
                 Free up space or choose a different models folder."
            ));
        }
    }
    Ok(())
}

/// Returns free bytes on the volume containing `path`, or `None` when the
/// platform probe is unavailable or fails.
#[cfg(target_os = "macos")]
fn free_space_on(path: &Path) -> Option<u64> {
    let out = std::process::Command::new("df")
        .args(["-k", "-P", path.to_str().unwrap_or(".")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    // df -kP output: Filesystem 1024-blocks Used Available Capacity ...
    let avail_kb: u64 = fields[3].parse().ok()?;
    Some(avail_kb * 1024)
}

#[cfg(target_os = "windows")]
fn free_space_on(path: &Path) -> Option<u64> {
    // Best-effort: use the `fsutil` command if available, otherwise skip.
    let out = std::process::Command::new("fsutil")
        .args(["volume", "diskfree", "C:\\"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("Total free bytes") {
            let val: u64 = rest.trim().trim_start_matches('=').trim().parse().ok()?;
            return Some(val);
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn free_space_on(_path: &Path) -> Option<u64> {
    None
}

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
        let downloaded = if total > 0 {
            overall.min(total)
        } else {
            overall
        };

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
/// `cancel` is an optional shared flag; when set to `true` the download
/// stops early, leaving the `.part` file on disk for a later resume.
pub fn download_entry_with_progress(
    entry: &CatalogEntry,
    models_dir: &Path,
    on_progress: Option<ProgressFn>,
) -> Result<PathBuf, String> {
    download_entry_with_cancel(entry, models_dir, on_progress, None)
}

/// Same as [`download_entry_with_progress`] but with a cancel token for
/// pause/resume. When `cancel` is set to `true` mid-download, the function
/// returns `Err("download paused")` and the partial `.part` file is kept.
pub fn download_entry_with_cancel(
    entry: &CatalogEntry,
    models_dir: &Path,
    on_progress: Option<ProgressFn>,
    cancel: Option<Arc<AtomicBool>>,
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

    // Pre-flight: verify disk space before downloading anything.
    let needed: u64 = if entry.shards.is_empty() {
        entry.size_mb as u64 * 1024 * 1024
    } else {
        entry.shards.iter().map(|s| s.size_bytes).sum()
    };
    if needed > 0 {
        check_disk_space(models_dir, needed).inspect_err(|e| {
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
                    total_bytes: needed,
                    percent: 0.0,
                    speed_bps: 0.0,
                    eta_seconds: None,
                    error: Some(e.clone()),
                });
            }
        })?;
    }
    let run = || -> Result<PathBuf, String> {
        if entry.shards.is_empty() {
            let dest = models_dir.join(format!("{}.gguf", entry.id));
            let expected_total = if entry.size_mb > 0 {
                entry.size_mb as u64 * 1024 * 1024
            } else {
                0
            };
            let mut tracker =
                ProgressTracker::new(entry.id, on_progress.clone(), 0, expected_total, 1);
            download_single(entry, &dest, &mut tracker, cancel.as_ref())?;
            tracker.emit(EmitArgs {
                status: "done",
                file_name: dest.file_name().and_then(|s| s.to_str()).unwrap_or(""),
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

            let mut tracker = ProgressTracker::new(entry.id, on_progress.clone(), prior, total, n);

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
                download_shard(shard, &dest, &mut tracker, idx, cancel.as_ref())?;
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
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    if let Some(sha) = entry.sha256 {
        match try_fetch_to_part(entry.url, dest, Some(sha), tracker, 1, cancel) {
            Ok(()) => return Ok(()),
            Err(primary_err) => {
                if primary_err == "download paused" {
                    return Err(primary_err);
                }
                if let Some(backup) = entry.backup_url {
                    tracing::warn!(error = %primary_err, "primary URL failed; trying backup");
                    return try_fetch_to_part(backup, dest, Some(sha), tracker, 1, cancel);
                }
                return Err(primary_err);
            }
        }
    }
    try_fetch_to_part(entry.url, dest, None, tracker, 1, cancel)
}

fn download_shard(
    shard: &CatalogShard,
    dest: &Path,
    tracker: &mut ProgressTracker,
    file_index: u32,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    try_fetch_to_part(shard.url, dest, Some(shard.sha256), tracker, file_index, cancel)
}

fn try_fetch_to_part(
    url: &str,
    dest: &Path,
    expected_sha: Option<&str>,
    tracker: &mut ProgressTracker,
    file_index: u32,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    let part = part_path(dest);
    let file_name = dest
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    // If a partial file already exists (from a prior paused/interrupted
    // download), resume from it instead of starting fresh.
    let existing = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if existing > 0 {
        tracing::info!(
            file = %file_name,
            existing_bytes = existing,
            "resuming download from byte {existing}"
        );
        return fetch_range(&client, url, &part, existing, expected_sha, tracker, &file_name, file_index, cancel);
    }

    // Fresh download.
    if let Err(e) = fs::remove_file(&part) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("clear {}: {e}", part.display()));
        }
    }
    match fetch_range(&client, url, &part, 0, expected_sha, tracker, &file_name, file_index, cancel) {
        Ok(()) => return Ok(()),
        Err(first_err) => {
            if first_err == "download paused" {
                return Err(first_err);
            }
            tracing::warn!(
                file = %file_name,
                error = %first_err,
                "initial download failed; attempting resume"
            );
        }
    }

    // Attempt 2: resume from the partial file if it exists and is non-empty.
    let existing = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if existing == 0 {
        return Err("download failed and no partial file to resume from".into());
    }
    tracing::info!(
        file = %file_name,
        existing_bytes = existing,
        "resuming download from byte {existing} (retry)"
    );
    match fetch_range(&client, url, &part, existing, expected_sha, tracker, &file_name, file_index, cancel) {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("resume from byte {existing} failed: {e}")),
    }
}

/// Downloads from `offset` in `url` into `part`, appending when `offset > 0`.
/// When `expected_sha` is set, the full file is re-hashed after the transfer
/// (streaming hash of the existing prefix + new bytes) and the part is
/// deleted on mismatch so the next call starts fresh.
/// Eight parameters, over clippy's threshold. The first four describe the
/// transfer and the last four are progress reporting for the multi-file case;
/// the reporting half is a cohesive unit that would be better as a struct than
/// as four more parameters on the same function.
#[allow(clippy::too_many_arguments)]
fn fetch_range(
    client: &reqwest::blocking::Client,
    url: &str,
    part: &Path,
    offset: u64,
    expected_sha: Option<&str>,
    tracker: &mut ProgressTracker,
    file_name: &str,
    file_index: u32,
    cancel: Option<&Arc<AtomicBool>>,
) -> Result<(), String> {
    let mut req = client.get(url).header("User-Agent", "teletype/0.1");
    if offset > 0 {
        req = req.header("Range", format!("bytes={offset}-"));
    }
    let mut resp = req.send().map_err(|e| format!("request failed: {e}"))?;

    let status = resp.status();
    // 416 = Range Not Satisfiable: the server says we have the whole file.
    if status.as_u16() == 416 {
        if let Some(sha) = expected_sha {
            verify_sha256(part, sha)?;
        }
        let dest = part.with_extension("");
        let dest = dest
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let dest = part.parent().unwrap().join(&dest);
        fs::rename(part, &dest).map_err(|e| format!("rename into place: {e}"))?;
        return Ok(());
    }
    // 206 = Partial Content (expected on resume); 200 = server ignored Range.
    if status.as_u16() == 200 && offset > 0 {
        tracing::warn!(
            file = file_name,
            "server ignored Range header; restarting from byte 0"
        );
        let _ = fs::File::create(part);
    } else if !status.is_success() {
        return Err(format!("HTTP {status} for {url}"));
    }

    let total_len = resp.content_length().unwrap_or(0);
    let effective_total = if offset > 0 && total_len > 0 {
        offset + total_len
    } else {
        total_len
    };

    tracker.emit(EmitArgs {
        status: "downloading",
        file_name,
        file_index,
        file_downloaded: offset,
        file_total: effective_total,
        force: true,
        error: None,
    });

    let file = if offset > 0 {
        fs::OpenOptions::new()
            .append(true)
            .open(part)
            .map_err(|e| format!("open {}: {e}", part.display()))?
    } else {
        fs::File::create(part).map_err(|e| format!("create {}: {e}", part.display()))?
    };
    let mut file = file;

    // When resuming with a checksum, hash the existing prefix first.
    let mut hasher = expected_sha.map(|_| Sha256::new());
    if offset > 0 {
        if let Some(h) = hasher.as_mut() {
            let mut prefix =
                fs::File::open(part).map_err(|e| format!("open prefix {}: {e}", part.display()))?;
            let mut pbuf = [0u8; 64 * 1024];
            loop {
                let n = prefix
                    .read(&mut pbuf)
                    .map_err(|e| format!("read prefix: {e}"))?;
                if n == 0 {
                    break;
                }
                h.update(&pbuf[..n]);
            }
        }
    }

    let mut buf = [0u8; 64 * 1024];
    let mut written: u64 = offset;
    loop {
        // Check for pause request between reads. The `.part` file is left
        // on disk so the next call to `download_entry_with_cancel` resumes.
        if let Some(flag) = cancel {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                tracker.emit(EmitArgs {
                    status: "paused",
                    file_name,
                    file_index,
                    file_downloaded: written,
                    file_total: effective_total,
                    force: true,
                    error: None,
                });
                file.flush().ok();
                return Err("download paused".into());
            }
        }
        let n = resp.read(&mut buf).map_err(|e| format!("read body: {e}"))?;
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
            file_name,
            file_index,
            file_downloaded: written,
            file_total: effective_total,
            force: false,
            error: None,
        });
    }
    file.flush().map_err(|e| format!("flush: {e}"))?;
    drop(file);

    tracker.emit(EmitArgs {
        status: "verifying",
        file_name,
        file_index,
        file_downloaded: written,
        file_total: effective_total.max(written),
        force: true,
        error: None,
    });

    if let (Some(h), Some(expected)) = (hasher, expected_sha) {
        let actual = hex::encode(h.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = fs::remove_file(part);
            return Err(format!(
                "SHA-256 mismatch for {file_name}: expected {expected}, got {actual}"
            ));
        }
    }

    let dest = dest_from_part(part);
    fs::rename(part, &dest).map_err(|e| format!("rename into place: {e}"))?;
    Ok(())
}

/// Derives the final destination path from a `.part` path.
fn dest_from_part(part: &Path) -> PathBuf {
    let name = part
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let final_name = name.strip_suffix(".part").unwrap_or(&name);
    part.parent().unwrap().join(final_name)
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
        assert!(
            mid.percent >= 20.0 && mid.percent <= 30.0,
            "{}",
            mid.percent
        );
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
    #[test]
    fn dest_from_part_strips_part_suffix() {
        let p = Path::new("/models/eg-1/eg1-1.2-c003-00001-of-00008.gguf.part");
        let dest = dest_from_part(p);
        assert_eq!(
            dest,
            Path::new("/models/eg-1/eg1-1.2-c003-00001-of-00008.gguf")
        );
    }

    #[test]
    fn dest_from_part_single_file() {
        let p = Path::new("/models/s1-mini.gguf.part");
        let dest = dest_from_part(p);
        assert_eq!(dest, Path::new("/models/s1-mini.gguf"));
    }

    #[test]
    fn disk_space_check_passes_on_real_volume() {
        // A tiny requirement (1 MB) should always pass on any real volume.
        let dir = std::env::temp_dir();
        assert!(check_disk_space(&dir, 1024 * 1024).is_ok());
    }

    #[test]
    fn disk_space_check_fails_on_impossible_requirement() {
        // 100 PB of required space will fail on any real volume.
        let dir = std::env::temp_dir();
        let err = check_disk_space(&dir, 100 * 1024 * 1024 * 1024 * 1024 * 1024).unwrap_err();
        assert!(err.contains("Not enough disk space"), "got: {err}");
    }

    #[test]
    fn disk_space_check_reports_mb_values() {
        let dir = std::env::temp_dir();
        let err = check_disk_space(&dir, 100 * 1024 * 1024 * 1024 * 1024 * 1024).unwrap_err();
        assert!(err.contains("MB"), "error should mention MB: {err}");
    }
}
