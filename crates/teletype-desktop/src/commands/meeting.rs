//! Meeting recording commands: start / stop / cancel / status / open report.
//!
//! The heavy work (transcription, report) runs on a blocking thread spawned
//! by `meeting_stop`; the command returns immediately with phase
//! `finalizing` and the UI polls `meeting_status` until `done` or `idle`
//! (with an error).

use super::*;
use crate::AppState;
use tauri::{AppHandle, Manager, State};
use teletype_core::meeting::MeetingPhase;

/// The meeting state the UI renders.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingStatus {
    pub phase: MeetingPhase,
    pub elapsed_ms: Option<u64>,
    /// The real-time transcript so far (committed segments), while
    /// recording. The UI polls this every second for the live view.
    pub live_text: Option<String>,
    /// Live stats while recording (committed words / segments so far).
    pub live_word_count: Option<u32>,
    pub live_segment_count: Option<u32>,
    pub report: Option<crate::meeting::ReportRef>,
    pub error: Option<String>,
}

fn status(state: &AppState) -> MeetingStatus {
    let phase = state.meetings.phase();
    // The report is built in real time: while recording, rewrite
    // report.html whenever new segments have committed (no-op when the
    // committed text is unchanged, so steady-state polls are cheap).
    if phase == MeetingPhase::Recording {
        state.meetings.refresh_live_report();
    }
    let live = state.meetings.live_transcript();
    MeetingStatus {
        phase,
        elapsed_ms: state.meetings.elapsed_ms(),
        live_text: live.as_ref().map(|t| t.text.clone()),
        live_word_count: live.as_ref().map(|t| t.words.len() as u32),
        live_segment_count: live.as_ref().map(|t| t.segments.len() as u32),
        report: state.meetings.last_report(),
        error: state.meetings.last_error(),
    }
}

/// Starts recording a meeting. `title` may be empty (defaults to "Meeting").
/// `device_id` empty means the system default microphone. `rubric_id` picks
/// the scoring rubric (e.g. "sales", "support", "performance", "general");
/// empty or unknown means no rubric.
#[tauri::command]
pub async fn meeting_start(
    app: AppHandle,
    state: State<'_, AppState>,
    title: Option<String>,
    device_id: Option<String>,
    rubric_id: Option<String>,
) -> CommandResult<MeetingStatus> {
    let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "Meeting".into());
    let device_id = device_id.unwrap_or_default();
    state
        .meetings
        .start(&app, &title, &device_id, rubric_id.as_deref())
        .map_err(CommandError::internal)?;
    // The island appears so the meeting is visible from any screen.
    crate::island::show(&app);
    Ok(status(&state))
}

/// The built-in scoring rubrics, for the UI's rubric select.
#[tauri::command]
pub fn meeting_rubrics() -> CommandResult<Vec<teletype_core::rubric::Rubric>> {
    Ok(teletype_core::rubric::builtin_rubrics())
}

/// Stops the meeting and kicks off finalization (transcription + report) on
/// a background thread. Returns immediately.
#[tauri::command]
pub async fn meeting_stop(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<MeetingStatus> {
    let input = state
        .meetings
        .stop()
        .map_err(CommandError::internal)?;

    // The finalize thread re-resolves the managed state through the handle
    // (same pattern as the model-unload tick thread): `State<'_, _>` guards
    // cannot outlive the command, and the speech manager is not Clone.
    std::thread::Builder::new()
        .name("teletype-meeting-finalize".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let engine = state.meetings.clone();
            let engine_label = state.settings().selected_speech_model.clone();
            // The selected engine lives in a specific manager slot (sherpa →
            // neural, whisper → speech, …); active_speech_manager mirrors
            // the warm-up thread's mapping.
            let speech = state.active_speech_manager();
            let meeting =
                crate::meeting::finalize(&engine, speech, &state.inference, &engine_label, input);
            match meeting {
                Ok(m) => {
                    tracing::info!(report = %m.report.report_path, "meeting report ready");
                    engine.mark_done(m);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "meeting finalization failed");
                    engine.mark_failed(e);
                    crate::island::hide(&app);
                }
            }
        })
        .map_err(|e| CommandError::internal(format!("spawn finalize thread: {e}")))?;

    Ok(status(&state))
}

/// Cancels the meeting: stops capture and discards the audio.
#[tauri::command]
pub async fn meeting_cancel(app: AppHandle, state: State<'_, AppState>) -> CommandResult<MeetingStatus> {
    state.meetings.cancel();
    crate::island::hide(&app);
    Ok(status(&state))
}

/// Current meeting state. The UI polls this while recording (live
/// transcript + stats) and while `finalizing` (phase changes).
#[tauri::command]
pub async fn meeting_status(state: State<'_, AppState>) -> CommandResult<MeetingStatus> {
    Ok(status(&state))
}

/// The full report content of the last finished meeting, for the
/// same-page render: overview (what it was about), action items, details,
/// and the complete transcript with speaker labels.
#[tauri::command]
pub async fn meeting_report(
    state: State<'_, AppState>,
) -> CommandResult<crate::meeting::ReportData> {
    state
        .meetings
        .report_data()
        .ok_or_else(|| CommandError::not_found("No finished meeting report available"))
}

/// Clears the last report/error and returns to idle.
#[tauri::command]
pub async fn meeting_reset(state: State<'_, AppState>) -> CommandResult<MeetingStatus> {
    state.meetings.reset();
    Ok(status(&state))
}

/// Opens the meeting's report in a NEW BROWSER TAB: the report path is
/// passed as a `file://` URL (URLs open as tabs), with a fallback to
/// launching the file in its default app if the URL open fails.
///
/// While a meeting is still recording, this opens the LIVE report — the
/// self-refreshing file the engine rewrites on every committed segment —
/// so the report can be watched without stopping the meeting.
#[tauri::command]
pub async fn meeting_open_report(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path: String = if state.meetings.is_recording() {
        // Make sure the live file exists before opening it (it is seeded at
        // start; this is belt-and-braces for a start that raced the open).
        state.meetings.ensure_live_report();
        let live = state
            .meetings
            .live_report_path()
            .ok_or_else(|| CommandError::not_found("No live report available"))?;
        live.to_string_lossy().to_string()
    } else {
        let report = state
            .meetings
            .last_report()
            .ok_or_else(|| CommandError::not_found("No meeting report available"))?;
        report.report_path
    };
    let url = report_file_url(&path);
    if let Err(e) = app.opener().open_url(&url, None::<&str>) {
        // Fallback: launch the file in its default app (new window).
        app.opener()
            .open_path(&path, None::<&str>)
            .map_err(|e2| CommandError::internal(format!("open report: {e}; {e2}")))?;
    }
    Ok(())
}

/// `C:\Users\x\report.html` → `file:///C:/Users/x/report.html` (Windows);
/// `/home/x/report.html` → `file:///home/x/report.html` (Unix). Opening a
/// URL — as opposed to a raw file path — makes the default browser open it
/// as a new tab instead of a new window.
fn report_file_url(path: &str) -> String {
    let sanitized = path.replace('\\', "/");
    let with_leading = if sanitized.starts_with('/') {
        sanitized
    } else {
        format!("/{sanitized}")
    };
    format!("file://{with_leading}")
}

#[cfg(test)]
mod tests {
    use super::report_file_url;

    #[test]
    fn file_url_windows_path() {
        assert_eq!(
            report_file_url(r"C:\Users\e\meetings\a\report.html"),
            "file:///C:/Users/e/meetings/a/report.html"
        );
    }

    #[test]
    fn file_url_unix_path() {
        assert_eq!(
            report_file_url("/home/e/meetings/a/report.html"),
            "file:///home/e/meetings/a/report.html"
        );
    }
}
