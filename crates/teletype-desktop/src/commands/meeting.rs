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
                crate::meeting::finalize(&engine, speech, &state.inference, &engine_label, &state.models_dir, input);
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

/// The meeting's report path as a `file://` URL.
// ---------------------------------------------------------------------------
// Speaker diarization models (optional companion artifacts, ~34 MB total).
// They are pinned by SHA-256 and installed under <models>/sherpa/diarization/;
// the download reuses the app's reqwest stack and emits the same
// `model-download-progress` event the model downloads use (kind "diarization").
// ---------------------------------------------------------------------------

/// Pinned segmentation archive (k2-fsa/sherpa-onnx, tag speaker-segmentation-models).
const SEGMENTATION_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2";
const SEGMENTATION_SHA256: &str = "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488";
const SEGMENTATION_BYTES: u64 = 6_958_444;
/// Pinned English campplus embedding (k2-fsa/sherpa-onnx, tag speaker-recongition-models).
const EMBEDDING_URL: &str = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx";
const EMBEDDING_SHA256: &str = "98daf9bcf1c8abf3db374fec4c27a5eda7c58705bb3f180d057b085a72a88bb5";
const EMBEDDING_BYTES: u64 = 28_000_000;

/// Whether the diarization models are installed.
#[tauri::command]
pub async fn diarization_status(
    state: State<'_, AppState>,
) -> CommandResult<serde_json::Value> {
    Ok(serde_json::json!({
        "installed": teletype_speech::diarization::diarization_available(&state.models_dir),
    }))
}

/// Downloads the two pinned diarization artifacts (segmentation archive +
/// embedding model), verifies their SHA-256, and installs them. Idempotent:
/// already-installed artifacts are skipped. Runs on a blocking thread.
#[tauri::command]
pub async fn download_diarization_models(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    let models_dir = state.models_dir.clone();
    let worker = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || install_diarization_models(&models_dir, &worker))
        .await
        .map_err(|e| CommandError::internal(format!("diarization download task: {e}")))?;
    result.map_err(CommandError::internal)
}

fn install_diarization_models(models_dir: &std::path::Path, app: &AppHandle) -> Result<String, String> {
    use teletype_speech::diarization as dz;
    let dir = dz::diarization_dir(models_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create diarization dir: {e}"))?;

    // 1. Segmentation archive → extract → drop the archive.
    if !dz::segmentation_model(models_dir).is_file() {
        let archive = dir.join("sherpa-onnx-pyannote-segmentation-3-0.tar.bz2");
        download_verified(
            SEGMENTATION_URL,
            SEGMENTATION_SHA256,
            SEGMENTATION_BYTES,
            &archive,
            app,
            0,
            2,
            "pyannote-segmentation-3.0 (6 MB)",
        )?;
        let extract_parent = dir.clone();
        let extract_to = dir.join(dz::SEGMENTATION_DIR);
        if !extract_to.join("model.onnx").is_file() {
            let file = std::fs::File::open(&archive).map_err(|e| format!("open archive: {e}"))?;
            let decoder = bzip2::read::BzDecoder::new(file);
            let mut tar = tar::Archive::new(decoder);
            tar.unpack(&extract_parent)
                .map_err(|e| format!("extract segmentation archive: {e}"))?;
        }
        if !dz::segmentation_model(models_dir).is_file() {
            let _ = std::fs::remove_dir_all(&extract_to);
            let _ = std::fs::remove_file(&archive);
            return Err("segmentation archive did not contain model.onnx".into());
        }
        let _ = std::fs::remove_file(&archive);
    }

    // 2. Embedding model (single file).
    let emb = dz::embedding_model(models_dir);
    if !emb.is_file() {
        download_verified(
            EMBEDDING_URL,
            EMBEDDING_SHA256,
            EMBEDDING_BYTES,
            &emb,
            app,
            1,
            2,
            "campplus-embedding (28 MB)",
        )?;
    }

    emit_diarization_progress(app, 2, 2, "", 1, 1, "done", None);
    Ok(dir.to_string_lossy().to_string())
}

/// Streams `url` to `dest.part`, verifies the SHA-256, renames into place.
/// Emits `model-download-progress` (kind `diarization`) while it runs.
fn download_verified(
    url: &str,
    sha256: &str,
    expected_bytes: u64,
    dest: &std::path::Path,
    app: &AppHandle,
    file_index: u32,
    file_count: u32,
    label: &str,
) -> Result<(), String> {
    use std::io::Read;
    use std::io::Write;
    let tmp = dest.with_extension("part");
    emit_diarization_progress(app, file_index, file_count, label, 0, expected_bytes, "downloading", None);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("download {label}: {e}"))?;
    if !response.status().is_success() {
        let code = response.status();
        return Err(format!("download {label}: HTTP {code}"));
    }
    let total = response
        .content_length()
        .unwrap_or(expected_bytes)
        .max(expected_bytes.saturating_mul(99) / 100);
    let mut file = std::fs::File::create(&tmp).map_err(|e| format!("create temp file: {e}"))?;
    let mut stream = response;
    let mut written: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let mut buf = [0u8; 65_536];
    loop {
        let n = stream
            .read(&mut buf)
            .map_err(|e| format!("download {label}: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write temp file: {e}"))?;
        written += n as u64;
        if last_emit.elapsed() > std::time::Duration::from_millis(250) {
            last_emit = std::time::Instant::now();
            emit_diarization_progress(app, file_index, file_count, label, written, total, "downloading", None);
        }
    }
    file.sync_all().map_err(|e| format!("sync temp file: {e}"))?;
    drop(file);

    emit_diarization_progress(app, file_index, file_count, label, written, total, "verifying", None);
    teletype_inference::download::verify_sha256(&tmp, sha256)
        .and_then(|_| {
            std::fs::rename(&tmp, dest).map_err(|e| format!("install {label}: {e}"))
        })
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            emit_diarization_progress(app, file_index, file_count, label, written, total, "error", Some(e.clone()));
            format!("{label} failed verification or install: {e}")
        })?;
    Ok(())
}

fn emit_diarization_progress(
    app: &AppHandle,
    file_index: u32,
    file_count: u32,
    file_name: &str,
    file_downloaded: u64,
    file_total: u64,
    status: &str,
    error: Option<String>,
) {
    let percent = if file_total > 0 {
        (file_downloaded as f64 / file_total as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let payload = serde_json::json!({
        "id": "diarization",
        "status": status,
        "fileName": file_name,
        "fileIndex": file_index,
        "fileCount": file_count,
        "fileDownloadedBytes": file_downloaded,
        "fileTotalBytes": file_total,
        "downloadedBytes": file_downloaded,
        "totalBytes": file_total,
        "percent": percent,
        "speedBps": 0.0,
        "etaSeconds": null,
        "error": error,
        "kind": "diarization",
    });
    use tauri::Emitter;
    let _ = app.emit("model-download-progress", payload);
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
