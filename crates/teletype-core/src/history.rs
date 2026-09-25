//! Dictation history: a local log of every completed dictation.
//!
//! Every transcription is saved here (with the app it was dictated into),
//! mirroring Wispr Flow's Dictation tab. When the user dictated somewhere
//! with no text field to receive it, the entry is still kept and can be
//! copied or re-pasted from the UI.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::context::ApplicationContext;
use crate::storage::JsonStore;

/// One completed dictation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationEntry {
    /// Unique id (uuid / timestamp-based).
    pub id: String,
    /// When it was dictated (ms since epoch).
    pub created_at: u64,
    /// The final (transformed, filler-stripped) text.
    pub text: String,
    /// Where it was dictated into (best-effort).
    #[serde(default)]
    pub context: Option<ApplicationContext>,
}

/// The history document: a flat list, newest first.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationHistory {
    #[serde(default)]
    pub entries: Vec<DictationEntry>,
}

/// Cap on stored entries so the file doesn't grow without bound.
const MAX_ENTRIES: usize = 1000;

impl DictationHistory {
    /// Appends an entry (newest first) and trims to [`MAX_ENTRIES`].
    pub fn push(&mut self, entry: DictationEntry) {
        self.entries.insert(0, entry);
        if self.entries.len() > MAX_ENTRIES {
            self.entries.truncate(MAX_ENTRIES);
        }
    }

    /// Removes the entry with `id`, if present.
    pub fn remove(&mut self, id: &str) {
        self.entries.retain(|e| e.id != id);
    }
}

/// Loads or creates the history store at `dir/dictation.json`.
pub fn open_history(dir: &Path) -> (JsonStore<DictationHistory>, DictationHistory) {
    let store = JsonStore::new(dir, "dictation.json");
    let history = store.load(DictationHistory::default());
    (store, history)
}

/// Appends one transcript to a day-wise text file under `dir`.
///
/// Files are named `YYYY-MM-DD.md` (local day) so a month of dictations is
/// easy to browse. Each entry is a heading with the time and source app,
/// followed by the text.
pub fn append_transcript_file(
    dir: &Path,
    created_at: u64,
    text: &str,
    app_name: &str,
) -> std::io::Result<PathBuf> {
    let day = day_stamp_ms(created_at);
    let time = time_stamp_ms(created_at);
    let path = dir.join(format!("{day}.md"));
    std::fs::create_dir_all(dir)?;
    let is_new = !path.exists();
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    if is_new {
        writeln!(f, "# Teletype transcripts — {day}\n")?;
    }
    writeln!(
        f,
        "## {time}{}",
        if app_name.is_empty() {
            String::new()
        } else {
            format!(" · {app_name}")
        }
    )?;
    writeln!(f, "{text}\n")?;
    Ok(path)
}

/// "YYYY-MM-DD" for a UTC-ms timestamp (day boundary is UTC, matching the
/// other stats in this crate).
fn day_stamp_ms(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// "HH:MM" (24h) for a UTC-ms timestamp.
fn time_stamp_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let h = (secs % 86_400) / 3600;
    let m = (secs % 3600) / 60;
    format!("{h:02}:{m:02}")
}

/// Converts days-since-1970 to a (year, month, day) civil date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, at: u64) -> DictationEntry {
        DictationEntry {
            id: id.into(),
            created_at: at,
            text: "hello".into(),
            context: None,
        }
    }

    #[test]
    fn newest_first_and_trimmed() {
        let mut h = DictationHistory::default();
        for i in 0..MAX_ENTRIES + 5 {
            h.push(entry(&i.to_string(), i as u64));
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        // First pushed (id 0) should have been trimmed; last pushed is first.
        assert_eq!(h.entries[0].id, (MAX_ENTRIES + 4).to_string());
    }

    #[test]
    fn remove_by_id() {
        let mut h = DictationHistory::default();
        h.push(entry("a", 1));
        h.push(entry("b", 2));
        h.remove("a");
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].id, "b");
    }

    #[test]
    fn civil_from_days_known_dates() {
        // 1970-01-01
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2000-01-01 = day 10957
        assert_eq!(civil_from_days(10_957), (2000, 1, 1));
        // 2024-02-29 (leap day) = day 19782
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn transcript_file_groups_by_day() {
        let dir =
            std::env::temp_dir().join(format!("teletype-transcript-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // 2024-01-15 13:10 UTC
        let at = 1_705_324_200_000u64;
        let p1 = append_transcript_file(&dir, at, "hello world", "Mail").unwrap();
        let p2 = append_transcript_file(&dir, at + 60_000, "second", "Mail").unwrap();
        assert_eq!(p1, p2, "same day → same file");
        assert!(p1
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("2024-01-15"));
        let content = std::fs::read_to_string(&p1).unwrap();
        assert!(content.contains("2024-01-15"));
        assert!(content.contains("13:10"));
        assert!(content.contains("hello world"));
        assert!(content.contains("second"));
        // A different day → different file.
        let other = append_transcript_file(&dir, at + 86_400_000, "next day", "").unwrap();
        assert_ne!(p1, other);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
