//! Next-launch crash notice: after a crash, the first run shows a one-time
//! banner ("Teletype hit a problem and recovered") with a Details action
//! that opens the crash folder.

use crate::{crash, AppState};

/// The one-time crash notice, if the last run crashed and the notice has not
/// been shown yet. `None` after [`dismiss_crash_notice`] or when there is no
/// report. The report file is consumed on first read (renamed to
/// `*.json.shown`), so the notice appears exactly once per crash even if the
/// UI is reloaded or the settings refetch fires again.
pub fn crash_notice(data_dir: &std::path::Path) -> Option<crash::CrashNotice> {
    let dir = crash::crash_dir(data_dir);
    let path = crash::newest_report(&dir)?;
    // One read, parsed once.
    let (timestamp_ms, app_version) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .map(|v| {
            (
                v["timestampMs"].as_u64().unwrap_or(0),
                v["appVersion"].as_str().map(String::from),
            )
        })
        .unwrap_or((0, None));
    // Consume now: the notice must not reappear if the window reloads or a
    // settings-changed refetch runs again. A failed read above still
    // consumes, since an unreadable report is not worth showing.
    crash::mark_shown(&path);
    Some(crash::CrashNotice {
        path,
        timestamp_ms,
        app_version,
    })
}

/// Opens the crash folder in the system file manager (the Details action).
#[tauri::command]
pub async fn open_crash_dir(
    state: tauri::State<'_, AppState>,
) -> crate::commands::CommandResult<()> {
    let dir = crash::crash_dir(&state.data_dir);
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(dir.to_string_lossy().to_string())
            .spawn();
    }
    Ok(())
}

/// Clears the banner after the user has read it. The report file was already
/// consumed by [`crash_notice`], so this only exists for symmetry with the
/// other one-time notice; it is a no-op.
#[tauri::command]
pub async fn dismiss_crash_notice() -> crate::commands::CommandResult<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "teletype-crash-cmd-tests-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn crash_notice_is_one_shot() {
        let dir = temp_dir("oneshot");
        let path = crate::crash::write_report(
            &crate::crash::crash_dir(&dir),
            "0.0.0-test",
            "macos",
            "teletype-dictation",
            "assertion failed",
            None,
            &[],
        )
        .expect("report written");
        let first = crash_notice(&dir).expect("the notice is pending");
        assert_eq!(first.path, path);
        assert_eq!(first.app_version.as_deref(), Some("0.0.0-test"));
        assert!(
            crash_notice(&dir).is_none(),
            "a second read finds nothing: the notice was consumed"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn crash_notice_is_none_without_a_report() {
        let dir = temp_dir("empty");
        assert!(crash_notice(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shown_reports_are_not_picked_up() {
        let dir = temp_dir("shown");
        let path = crate::crash::write_report(
            &crate::crash::crash_dir(&dir),
            "0.0.0",
            "macos",
            "t",
            "boom",
            None,
            &[],
        )
        .unwrap();
        crate::crash::mark_shown(&path);
        assert!(crash_notice(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
