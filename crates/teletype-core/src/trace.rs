//! Per-stage dictation timing (Beat Wispr Flow plan §P0.2).
//!
//! Every dictation take fills one [`DictationTrace`] with monotonic marks at
//! each stage boundary (hotkey down, first audio, release, ASR start/end,
//! text-pipeline end, polish start/first-token/end, paste sent). Finished
//! traces are appended as one JSON line each to
//! `<app_data>/traces/YYYY-MM-DD.jsonl`.
//!
//! The writer is deliberately boring: best-effort, never panics, never
//! blocks delivery. A failed write loses a measurement, never a dictation.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Process-start anchor for the single monotonic clock. Every mark is
/// microseconds since this instant, so marks are comparable across threads
/// and immune to wall-clock jumps (NTP, timezone changes).
static BOOT: OnceLock<Instant> = OnceLock::new();

/// Current time in microseconds on the single monotonic clock.
pub fn now_us() -> u64 {
    let boot = BOOT.get_or_init(Instant::now);
    boot.elapsed().as_micros() as u64
}

/// One take's per-stage timing. Field order is the pipeline order; every
/// `t_*` mark is monotonic microseconds from [`now_us`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DictationTrace {
    pub session: u64,
    /// Engine id, e.g. the selected speech-model id.
    pub engine: String,
    /// Measured speaking time (mic open to release).
    pub audio_ms: u32,
    pub t_hotkey_down: Option<u64>,
    pub t_first_audio: Option<u64>,
    pub t_hotkey_up: Option<u64>,
    pub t_asr_start: Option<u64>,
    pub t_asr_end: Option<u64>,
    pub t_text_pipeline_end: Option<u64>,
    pub t_polish_start: Option<u64>,
    pub t_polish_first_token: Option<u64>,
    pub t_polish_end: Option<u64>,
    pub t_paste_sent: Option<u64>,
    pub polish_skipped_reason: Option<String>,
    /// No segmenter exists yet (Phase 4 work); always 0 until then.
    pub committed_segments: u32,
    /// No segmenter exists yet (Phase 4 work); always 0 until then.
    pub tail_audio_ms: u32,
    pub errors: Vec<String>,
}

impl DictationTrace {
    pub fn new(session: u64, engine: impl Into<String>) -> Self {
        Self {
            session,
            engine: engine.into(),
            ..Default::default()
        }
    }

    /// Stamp the current monotonic time into whichever fields `f` sets.
    pub fn mark(&mut self, f: impl FnOnce(&mut Self, u64)) {
        f(self, now_us())
    }

    /// Merge the marks collected on the pipeline worker thread.
    pub fn apply_stage_marks(&mut self, m: TraceStageMarks) {
        self.t_polish_start = m.t_polish_start;
        self.t_polish_first_token = m.t_polish_first_token;
        self.t_polish_end = m.t_polish_end;
        self.t_text_pipeline_end = m.t_text_pipeline_end;
        self.polish_skipped_reason = m.polish_skipped_reason;
        self.committed_segments = m.committed_segments;
        self.tail_audio_ms = m.tail_audio_ms;
    }

    /// Release-to-paste latency in microseconds, the headline Gate 0 metric.
    pub fn release_to_paste_us(&self) -> Option<u64> {
        match (self.t_hotkey_up, self.t_paste_sent) {
            (Some(up), Some(paste)) => paste.checked_sub(up),
            _ => None,
        }
    }
}

/// Marks collected on the pipeline worker thread, where the controller's
/// trace is unreachable. Travels to the controller inside the pipeline
/// payload and is merged with [`DictationTrace::apply_stage_marks`].
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TraceStageMarks {
    pub t_polish_start: Option<u64>,
    pub t_polish_first_token: Option<u64>,
    pub t_polish_end: Option<u64>,
    pub t_text_pipeline_end: Option<u64>,
    pub polish_skipped_reason: Option<String>,
    pub committed_segments: u32,
    pub tail_audio_ms: u32,
}

/// Abstraction over "where finished traces go" so tests can observe the
/// write ordering without touching the filesystem.
pub trait TraceSink {
    fn write_trace(&self, trace: &DictationTrace);
}

/// Appends each finished trace as one JSON line to
/// `<app_data>/traces/YYYY-MM-DD.jsonl`. Best-effort throughout: every
/// failure is swallowed, because losing a measurement must never break
/// delivery of the user's text.
pub struct FileTraceSink {
    dir: PathBuf,
}

/// Keep 7 days of traces; prune older files on every write.
const RETENTION_DAYS: u64 = 7;
/// Cap the traces directory at 50 MB; delete oldest first when over.
const MAX_DIR_BYTES: u64 = 50 * 1024 * 1024;

impl FileTraceSink {
    pub fn new(app_data: &Path) -> Self {
        Self {
            dir: app_data.join("traces"),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path_for_today(&self) -> PathBuf {
        let (y, m, d) = today_ymd();
        self.dir.join(format!("{y:04}-{m:02}-{d:02}.jsonl"))
    }

    fn prune(&self) {
        use std::time::Duration;
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let mut files: Vec<(PathBuf, u64, SystemTime)> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let is_file = entry.file_type().map(|t| t.is_file()).unwrap_or(false);
            if !is_file || path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            // Age out files older than the retention window. The file name
            // is the date, but mtime is the honest signal for "old".
            if SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO)
                > Duration::from_secs(RETENTION_DAYS * 86400)
            {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            files.push((path, size, modified));
        }
        // Cap total size: delete oldest first until under budget.
        let mut total: u64 = files.iter().map(|(_, s, _)| *s).sum();
        files.sort_by_key(|(_, _, m)| *m);
        for (path, size, _) in files {
            if total <= MAX_DIR_BYTES {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(size);
            }
        }
    }
}

impl TraceSink for FileTraceSink {
    fn write_trace(&self, trace: &DictationTrace) {
        // Serialize first: a trace that cannot serialize is dropped without
        // ever touching the directory, so a bad trace can't wedge rotation.
        let Ok(mut line) = serde_json::to_string(trace) else {
            return;
        };
        line.push('\n');
        if std::fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        if std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path_for_today())
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(line.as_bytes())
            })
            .is_err()
        {
            return;
        }
        self.prune();
    }
}

/// Current UTC date as (year, month, day), via the proleptic Gregorian
/// calendar (Howard Hinnant's civil-from-days). std-only: chrono would be
/// a new dependency for one filename.
fn today_ymd() -> (i32, u32, u32) {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86400) as i64)
        .unwrap_or(0);
    civil_from_days(days)
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = (z - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 99]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y } as i32, m, d)
}

/// `traces-export-YYYY-MM-DD.jsonl` file name for the Developer export button.
pub fn export_file_name() -> String {
    let (y, m, d) = today_ymd();
    format!("traces-export-{y:04}-{m:02}-{d:02}.jsonl")
}

/// Reads the most recent traces across the rotated JSONL files, newest
/// first, up to `limit`. Corrupt lines are skipped; a missing directory
/// yields an empty vec. Backend for the Developer panel.
pub fn read_recent_traces(dir: &Path, limit: usize) -> Vec<DictationTrace> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    // File names are dates, so lexical order is chronological.
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    files.reverse();
    let mut out = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Appends go last, so iterate bottom-up for newest-first.
        for line in text.lines().rev() {
            if out.len() >= limit {
                return out;
            }
            if let Ok(trace) = serde_json::from_str::<DictationTrace>(line) {
                out.push(trace);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory sink: records every trace it receives so tests can assert
    /// on write ordering without touching the filesystem.
    struct RecordingSink {
        lines: Mutex<Vec<String>>,
    }

    impl RecordingSink {
        fn new() -> Self {
            Self {
                lines: Mutex::new(Vec::new()),
            }
        }

        fn count(&self) -> usize {
            self.lines
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len()
        }

        fn last(&self) -> Option<serde_json::Value> {
            self.lines
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .last()
                .and_then(|l| serde_json::from_str(l).ok())
        }
    }

    impl TraceSink for RecordingSink {
        fn write_trace(&self, trace: &DictationTrace) {
            if let Ok(line) = serde_json::to_string(trace) {
                self.lines
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(line);
            }
        }
    }

    #[test]
    fn trace_marks_are_monotonic() {
        let mut t = DictationTrace::new(1, "test-engine");
        t.mark(|s, now| s.t_hotkey_down = Some(now));
        t.mark(|s, now| s.t_first_audio = Some(now));
        t.mark(|s, now| s.t_hotkey_up = Some(now));
        t.mark(|s, now| s.t_asr_start = Some(now));
        t.mark(|s, now| s.t_asr_end = Some(now));
        t.mark(|s, now| s.t_text_pipeline_end = Some(now));
        t.mark(|s, now| s.t_polish_start = Some(now));
        t.mark(|s, now| s.t_polish_first_token = Some(now));
        t.mark(|s, now| s.t_polish_end = Some(now));
        t.mark(|s, now| s.t_paste_sent = Some(now));
        let marks = [
            t.t_hotkey_down,
            t.t_first_audio,
            t.t_hotkey_up,
            t.t_asr_start,
            t.t_asr_end,
            t.t_text_pipeline_end,
            t.t_polish_start,
            t.t_polish_first_token,
            t.t_polish_end,
            t.t_paste_sent,
        ];
        // Every stage is stamped, and no later stage predates an earlier
        // one (equality is fine: two marks can land in the same microsecond).
        let mut prev = 0u64;
        for m in marks {
            let v = m.expect("every stage must be stamped");
            assert!(v >= prev, "marks went backwards: {v} < {prev}");
            prev = v;
        }
    }

    #[test]
    fn trace_written_after_paste_not_before() {
        let sink = RecordingSink::new();
        let mut t = DictationTrace::new(7, "test-engine");
        t.audio_ms = 1200;
        t.mark(|s, now| s.t_hotkey_down = Some(now));
        t.mark(|s, now| s.t_first_audio = Some(now));
        t.mark(|s, now| s.t_hotkey_up = Some(now));
        t.mark(|s, now| s.t_asr_start = Some(now));
        t.mark(|s, now| s.t_asr_end = Some(now));
        t.apply_stage_marks(TraceStageMarks {
            t_text_pipeline_end: Some(now_us()),
            ..Default::default()
        });
        // The whole pipeline ran, but no paste yet: nothing may be written.
        assert_eq!(
            sink.count(),
            0,
            "trace must not be written before the paste is sent"
        );
        assert!(t.release_to_paste_us().is_none());
        // Paste sent: now the trace is complete and exactly one line lands.
        t.mark(|s, now| s.t_paste_sent = Some(now));
        sink.write_trace(&t);
        assert_eq!(sink.count(), 1);
        let v = sink.last().expect("one line must have been written");
        assert_eq!(v["session"], 7);
        assert!(v["t_paste_sent"].is_number(), "{v}");
        let latency = t.release_to_paste_us().expect("both marks set");
        assert!(
            v["t_paste_sent"].as_u64().unwrap() - v["t_hotkey_up"].as_u64().unwrap() == latency,
            "{v}"
        );
    }

    #[test]
    fn trace_write_failure_never_panics() {
        // Plan §0.1 rule 4: the trace writer is instrumentation, and its
        // fallback is silence. Point it at a path that cannot be a
        // directory (an existing file) and demand no panic, no delivery
        // impact — the write is simply dropped.
        let dir = std::env::temp_dir().join(format!("teletype-trace-test-{}", std::process::id()));
        let _ = std::fs::write(&dir, b"not a directory");
        let sink = FileTraceSink::new(&dir.join("unreachable-child"));
        let mut t = DictationTrace::new(1, "test-engine");
        t.mark(|s, now| s.t_paste_sent = Some(now));
        sink.write_trace(&t);
        let _ = std::fs::remove_file(&dir);
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        // 1970-01-01 is day 0; spot-check the conversion the trace
        // filenames depend on.
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19654), (2023, 10, 24));
    }

    #[test]
    fn read_recent_traces_is_newest_first_and_skips_garbage() {
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let dir = tmp.path().join("traces");
        std::fs::create_dir_all(&dir).unwrap();
        let write_day = |name: &str, sessions: &[u64], garbage: bool| {
            let mut text = String::new();
            for s in sessions {
                let t = DictationTrace::new(*s, "test-engine");
                text.push_str(&serde_json::to_string(&t).unwrap());
                text.push('\n');
            }
            if garbage {
                text.push_str("{not json}\n");
            }
            std::fs::write(dir.join(name), text).unwrap();
        };
        write_day("2026-10-12.jsonl", &[1, 2], false);
        write_day("2026-10-13.jsonl", &[3], true);
        std::fs::write(dir.join("notes.txt"), b"ignored").unwrap();

        // Missing dir and zero limit are empty, not errors.
        assert!(read_recent_traces(&dir.join("nope"), 50).is_empty());
        assert!(read_recent_traces(&dir, 0).is_empty());

        let all = read_recent_traces(&dir, 50);
        let ids: Vec<u64> = all.iter().map(|t| t.session).collect();
        assert_eq!(
            ids,
            vec![3, 2, 1],
            "newest file first, newest line first"
        );

        let two = read_recent_traces(&dir, 2);
        assert_eq!(two.iter().map(|t| t.session).collect::<Vec<_>>(), vec![3, 2]);

        assert!(export_file_name().starts_with("traces-export-"));
        assert!(export_file_name().ends_with(".jsonl"));
    }
}
