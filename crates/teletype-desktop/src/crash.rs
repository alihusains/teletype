//! Local crash reports.
//!
//! The panic hook in `lib.rs` calls [`write_report`] with the panic payload,
//! the location, the thread name and the tail of the in-memory log ring.
//! Each report is one JSON file under `<app data>/crashes/`.
//!
//! ## Privacy
//!
//! A crash report must never contain dictated text, screen text, field text,
//! clipboard content or a transcript. The in-memory log ring holds messages
//! that can all of those, so a report stores only each entry's level, its
//! character count and, when the entry carries a measured duration, that
//! duration. The panic message itself is a Rust payload (usually a message
//! the developer wrote), but it is stored truncated with a marker so a
//! formatted payload that interpolated user text cannot grow the report
//! without bound.
//!
//! ## Bounding
//!
//! At most [`MAX_REPORTS`] files are kept (the oldest are deleted) and a
//! report larger than [`MAX_REPORT_BYTES`] is not written at all. Every
//! operation is fallible and ignored on error: the hook must never panic,
//! so the caller wraps this module in `catch_unwind` as well.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::json;

/// Schema version of the JSON on disk. Bump when the shape changes.
pub const SCHEMA_VERSION: u32 = 1;
/// How many crash reports to keep, newest first.
pub const MAX_REPORTS: usize = 10;
/// A report larger than this (bytes) is dropped: a corrupted or hostile
/// payload must not fill the disk.
pub const MAX_REPORT_BYTES: u64 = 256 * 1024;
/// How many log entries to attach, newest last.
pub const LOG_TAIL: usize = 20;
/// The panic message is stored truncated to this many characters.
const PANIC_MSG_MAX_CHARS: usize = 200;

/// The `crashes` folder inside the app data directory.
pub fn crash_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("crashes")
}

/// One log entry as it appears in a report: the message is replaced by its
/// character count, so no logged text survives into the file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportLogEntry {
    pub level: String,
    /// Character count of the original message. Counts only, never text.
    pub message_chars: usize,
    /// Present when the entry measured a duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// The newest crash report, for the one-shot next-launch notice.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashNotice {
    /// The report file, so "Details" can open it.
    pub path: PathBuf,
    /// Unix milliseconds the report was written.
    pub timestamp_ms: u64,
    /// The app version from the report, when it could be read.
    pub app_version: Option<String>,
}

/// Writes one crash report under `dir` and prunes the folder to
/// [`MAX_REPORTS`] files. Returns the path of the file written.
///
/// Pure on purpose: the panic hook (the only production caller) has no
/// `AppHandle`, so the version is passed in rather than looked up.
pub fn write_report(
    dir: &Path,
    version: &str,
    os: &str,
    thread: &str,
    message: &str,
    location: Option<&str>,
    logs: &[crate::LogEntry],
) -> std::io::Result<PathBuf> {
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Never echo more of the payload than the cap, and mark the cut.
    let truncated = message.chars().count() > PANIC_MSG_MAX_CHARS;
    let message: String = message.chars().take(PANIC_MSG_MAX_CHARS).collect();

    // Newest last, matching the ring buffer's own order.
    let log_tail: Vec<ReportLogEntry> = logs
        .iter()
        .rev()
        .take(LOG_TAIL)
        .rev()
        .map(|e| ReportLogEntry {
            level: format!("{:?}", e.level),
            // Char count only. The message itself never reaches the file.
            message_chars: e.message.chars().count(),
            duration_ms: e.duration_ms,
        })
        .collect();

    let report = json!({
        "schemaVersion": SCHEMA_VERSION,
        "timestampMs": now_ms,
        "appVersion": version,
        "os": os,
        "thread": thread,
        "panic": {
            "message": if truncated {
                format!("{message} [truncated]")
            } else {
                message
            },
            "location": location,
        },
        "recentLogs": {
            "count": log_tail.len(),
            "entries": log_tail,
        },
    });

    let bytes = serde_json::to_string(&report).map_err(|e| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("report encode: {e}"),
        )
    })?;
    if bytes.len() > MAX_REPORT_BYTES as usize {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "report too large ({} bytes, cap {})",
                bytes.len(),
                MAX_REPORT_BYTES
            ),
        ));
    }

    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("crash-{now_ms}.json"));
    // Atomic enough for this purpose: two panics in the same millisecond
    // would collide, and the second write simply replaces the first (both
    // describe the same crash).
    std::fs::write(&path, bytes)?;
    prune(dir);
    Ok(path)
}

/// Deletes the oldest reports beyond [`MAX_REPORTS`]. Files that cannot be
/// sorted (unreadable metadata) are left alone.
fn prune(dir: &Path) {
    let mut reports: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("crash-") || !name.ends_with(".json") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        reports.push((mtime, entry.path()));
    }
    if reports.len() <= MAX_REPORTS {
        return;
    }
    reports.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, path) in reports.iter().take(reports.len() - MAX_REPORTS) {
        let _ = std::fs::remove_file(path);
    }
}

/// The newest report in `dir`, if any. Used by the next-launch notice.
pub fn newest_report(dir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("crash-") || !name.ends_with(".json") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            newest = Some((mtime, entry.path()));
        }
    }
    newest.map(|(_, p)| p)
}

/// Renames `path` to `path.with_extension("json.shown")` so the next launch
/// does not show the same notice. Returns true when a report was consumed.
pub fn mark_shown(path: &Path) -> bool {
    let shown = path.with_extension("json.shown");
    std::fs::rename(path, shown).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LogEntry, LogLevel};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "teletype-crash-tests-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn entry(level: LogLevel, msg: &str) -> LogEntry {
        LogEntry {
            level,
            message: msg.to_string(),
            duration_ms: None,
        }
    }

    #[test]
    fn report_never_contains_logged_text() {
        let dir = temp_dir("redact");
        let secret = "SECRET-DICTATED-TEXT-123456789";
        let logs = vec![
            entry(LogLevel::Info, secret),
            entry(LogLevel::Error, "screen text: buy the house on Maple"),
        ];
        let path = write_report(
            &dir,
            "0.0.0-test",
            "macos",
            "teletype-dictation",
            "assertion failed",
            Some("crates/teletype-desktop/src/dictation.rs:1:1"),
            &logs,
        )
        .expect("report written");
        let json = std::fs::read_to_string(&path).expect("report json");
        assert!(!json.contains("SECRET-DICTATED-TEXT-12345"));
        assert!(!json.contains("Maple"));
        assert!(!json.contains("screen text"));
        // The secret entry is 30 chars; the count must be present.
        assert!(
            json.contains("\"messageChars\":30"),
            "the char count is the only trace of a logged message: {json}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid json");
        assert_eq!(parsed["schemaVersion"], SCHEMA_VERSION);
        assert_eq!(parsed["appVersion"], "0.0.0-test");
        assert_eq!(parsed["thread"], "teletype-dictation");
        assert_eq!(parsed["panic"]["message"], "assertion failed");
        assert_eq!(parsed["recentLogs"]["count"], 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_panic_message_is_truncated_not_dropped() {
        let dir = temp_dir("truncate");
        let long = "x".repeat(PANIC_MSG_MAX_CHARS + 100);
        let path =
            write_report(&dir, "0.0.0", "macos", "t", &long, None, &[]).expect("report written");
        let json = std::fs::read_to_string(&path).expect("report json");
        assert!(json.contains("[truncated]"));
        assert!(json.len() < PANIC_MSG_MAX_CHARS + 2000, "the cap held");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_panic_message_is_truncated_to_fit() {
        let dir = temp_dir("oversize");
        // A 300 KB panic message is truncated by PANIC_MSG_MAX_CHARS,
        // so the encoded report stays well under MAX_REPORT_BYTES.
        let huge = "y".repeat(300 * 1024);
        let path = write_report(&dir, "0.0.0", "macos", "t", &huge, None, &[])
            .expect("report written after truncation");
        let json = std::fs::read_to_string(&path).expect("report json");
        assert!(
            json.contains("[truncated]"),
            "the long message must be truncated, not dropped"
        );
        assert!(
            json.len() as u64 <= MAX_REPORT_BYTES,
            "report size {} exceeds cap {}",
            json.len(),
            MAX_REPORT_BYTES
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_are_capped_and_newest_is_selected() {
        let dir = temp_dir("cap");
        for i in 0..(MAX_REPORTS + 3) {
            write_report(
                &dir,
                "0.0.0",
                "macos",
                "t",
                &format!("panic {i}"),
                None,
                &[],
            )
            .expect("report written");
            // Distinct mtimes: the cap logic sorts by mtime, and modern
            // filesystems may share a timestamp within a tight loop.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let count = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with("crash-")
            })
            .count();
        assert_eq!(count, MAX_REPORTS, "the oldest were pruned");

        let newest = newest_report(&dir).expect("a report remains");
        let name = newest.file_name().unwrap().to_string_lossy().to_string();
        let parsed: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&newest).unwrap()).unwrap();
        // The last panic written is the newest report.
        assert_eq!(parsed["panic"]["message"], "panic 12");
        let _ = name;
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mark_shown_consumes_the_notice_once() {
        let dir = temp_dir("shown");
        let path = write_report(&dir, "0.0.0", "macos", "t", "boom", None, &[]).unwrap();
        assert!(
            newest_report(&dir).is_some(),
            "before: the notice is pending"
        );
        assert!(mark_shown(&path), "first mark succeeds");
        assert!(
            newest_report(&dir).is_none(),
            "after: the notice is consumed"
        );
        assert!(!mark_shown(&path), "a second mark finds nothing to consume");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
