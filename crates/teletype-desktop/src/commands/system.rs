//! System commands: status, dictation control, dev tools, windows, recovery and misc.
//!
//! Split out of `commands.rs` (roadmap S1): pure move, no behavior change.
use super::*;
use crate::AppState;
use tauri::{AppHandle, Emitter, Manager, State};
// ---- Developer tab ----
/// Recent app log entries (oldest first), for the Developer tab.
#[tauri::command]
pub fn get_logs() -> Vec<crate::LogEntry> {
    crate::logs_snapshot()
}
/// Clears the in-memory log buffer.
#[tauri::command]
pub fn clear_logs() {
    crate::clear_logs();
}
/// Last-N finished dictation traces for the Developer panel, newest first.
/// Reads the rotated JSONL files (never the live session), so it works even
/// when no take is in flight. Missing directory or corrupt lines yield
/// fewer (or zero) traces, never an error.
#[tauri::command]
pub fn get_dictation_traces(
    app: AppHandle,
    limit: u32,
) -> Vec<teletype_core::trace::DictationTrace> {
    // Traces live under the app data dir. Any failure (no dir yet, no
    // permission) yields an empty table, never an error popup.
    let Ok(dir) = app.path().app_data_dir() else {
        return Vec::new();
    };
    teletype_core::trace::read_recent_traces(&dir.join("traces"), limit.min(500) as usize)
}

/// Merges recent traces into one export file for bug reports; returns its
/// path. Overwrites today's export if run twice.
#[tauri::command]
pub fn export_dictation_traces(app: AppHandle) -> CommandResult<String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("trace export failed: {e}"))?
        .join("traces");
    let traces = teletype_core::trace::read_recent_traces(&dir, 5000);
    let path = dir.join(teletype_core::trace::export_file_name());
    let mut text = String::new();
    for t in &traces {
        let Ok(line) = serde_json::to_string(t) else {
            continue;
        };
        text.push_str(&line);
        text.push('\n');
    }
    std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(&path, text))
        .map_err(|e| format!("trace export failed: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

/// U2: the per-stage latency percentiles (p50/p95) over the most recent
/// takes, for the Insights latency panel. Reads the rotated trace JSONL
/// (never the live session) and aggregates the per-stage durations the
/// trace derives at its single completion attach point. A stage that never
/// ran reports `sample_count: 0` and `null` percentiles, so the panel
/// renders a hyphen rather than a false zero. Any failure (no dir yet, no
/// permission) yields an empty summary, never an error popup.
#[tauri::command]
pub fn get_latency_summary(app: AppHandle) -> teletype_core::trace::LatencySummary {
    // Percentiles come from the most recent takes, so the number describes
    // how the app behaves now rather than how it behaved six months ago.
    const LATENCY_WINDOW: u32 = 200;
    let Some(dir) = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("traces"))
    else {
        return teletype_core::trace::latency_summary(&[], LATENCY_WINDOW);
    };
    let mut traces = teletype_core::trace::read_recent_traces(&dir, LATENCY_WINDOW as usize);
    // Traces written before the per-stage field existed carry only the raw
    // marks, so without this the panel would show nothing at all until enough
    // NEW takes accumulated. The derivation is pure mark arithmetic and
    // reproduces what the write path stores, so re-deriving on load is safe.
    for t in &mut traces {
        t.derive_stage_ms();
    }
    teletype_core::trace::latency_summary(&traces, LATENCY_WINDOW)
}

/// Toggles the webview devtools (debug builds only). Lets users open DevTools
/// on demand from the Developer tab instead of it being pinned open at launch.
#[tauri::command]
#[cfg(debug_assertions)]
pub fn devtools_toggle(app: AppHandle) -> CommandResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_devtools_open() {
            w.close_devtools();
        } else {
            w.open_devtools();
        }
    }
    Ok(())
}
#[cfg(not(debug_assertions))]
#[tauri::command]
pub fn devtools_toggle(_app: AppHandle) -> CommandResult<()> {
    Err(CommandError::user(
        "devtools is only available in debug builds",
    ))
}
#[tauri::command]
pub async fn get_status(app: AppHandle, state: State<'_, AppState>) -> CommandResult<Status> {
    Ok(Status {
        os: std::env::consts::OS,
        version: app.package_info().version.to_string(),
        setup_notes: state.platform.setup_notes(),
        permissions: state.platform.permissions(),
    })
}
// ---- Dictation ----
#[tauri::command]
pub async fn toggle_dictation(state: State<'_, AppState>) -> CommandResult<()> {
    state.controller.send(crate::dictation::Event::Toggle);
    Ok(())
}
/// Cancels the current dictation from the pill. While recording, the audio is
/// discarded; mid-pipeline, the in-flight transcription is dropped and the
/// result will not be inserted.
#[tauri::command]
pub async fn cancel_dictation(state: State<'_, AppState>) -> CommandResult<()> {
    state.controller.send(crate::dictation::Event::PillCancel);
    Ok(())
}
#[tauri::command]
pub async fn get_dictation_state(
    state: State<'_, AppState>,
) -> CommandResult<teletype_core::state::UiState> {
    Ok(state
        .dictation_state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone())
}
/// Per-stage timings for a recent take (roadmap A2/F3): the worker records
/// `capture → snapshots → context → pipeline → usage → history →
/// dictionary → transcript`, and the controller finishes with `deliver`.
/// Omitted session returns the latest trace; unknown ids return null.
/// In-memory only (cap 20), never persisted.
#[tauri::command]
pub async fn get_dictation_trace(
    state: State<'_, AppState>,
    session: Option<u64>,
) -> CommandResult<Option<crate::dictation::dictation_stages::DictationTrace>> {
    Ok(match session {
        Some(id) => state.traces.get(id),
        None => state.traces.latest(),
    })
}
/// Records a short audio clip and transcribes it, returning the raw text.
/// Used by the "Teach Words" flow to capture how the user pronounces a word.
/// The entire record+wait+transcribe runs on `spawn_blocking` so the tokio
/// runtime is never blocked (P1-C).
#[tauri::command]
pub async fn transcribe_word(app: AppHandle, state: State<'_, AppState>) -> CommandResult<String> {
    let settings = state.settings();
    let input_device = settings.input_device.clone();
    let language = crate::dictation::effective_language(settings.language.clone(), true);
    let (model_path, kind) = crate::dictation::resolve_speech_model(&settings, &state.models_dir);
    // P1-C: the record+silence-wait loop blocks for up to 5 s. Run it on a
    // blocking thread so the tokio worker pool stays responsive.
    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        transcribe_word_blocking(&app2, &input_device, &language, &model_path, kind)
    })
    .await
    .map_err(|e| format!("Transcription task failed: {e}"))??;
    Ok(result)
}
fn transcribe_word_blocking(
    app: &AppHandle,
    input_device: &str,
    language: &str,
    model_path: &std::path::Path,
    kind: crate::dictation::SpeechEngineKind,
) -> Result<String, String> {
    // Start the recording on the calling thread (it spawns its own capture thread).
    // P0-12: energy-based silence stop instead of a fixed sleep. Stop after
    // 800 ms of continuous silence below the threshold, capped at 5 s.
    let silence = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let silence_since = std::sync::Arc::new(std::sync::Mutex::new(None::<std::time::Instant>));
    let s_silence = silence.clone();
    let s_since = silence_since.clone();
    let recording = teletype_core::audio::Recording::start(input_device, move |level| {
        const THRESHOLD: f32 = 0.01;
        const SILENCE_MS: u64 = 800;
        let now = std::time::Instant::now();
        if level < THRESHOLD {
            let mut since = s_since
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match *since {
                None => *since = Some(now),
                Some(t) if now.duration_since(t).as_millis() >= SILENCE_MS as u128 => {
                    s_silence.store(true, std::sync::atomic::Ordering::Relaxed);
                }
                _ => {}
            }
        } else {
            *s_since
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    })
    .map_err(|e| format!("Microphone unavailable: {e}"))?;
    // Wait for silence detection or the 5 s cap.
    let cap = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !silence.load(std::sync::atomic::Ordering::Relaxed) {
        if std::time::Instant::now() >= cap {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let captured = recording
        .finish()
        .map_err(|e| format!("Recording failed: {e}"))?;
    // Already on a blocking thread (spawn_blocking), so call transcribe directly.
    let (text, _words) =
        crate::dictation::transcribe(app, model_path, &captured, language, kind, &[])?;

    Ok(text)
}
// ---- Misc ----
#[tauri::command]
pub async fn open_main_window(app: AppHandle, route: Option<String>) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
        if let Some(route) = route {
            let _ = window.emit("navigate", route);
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn quit_app(app: AppHandle) -> CommandResult<()> {
    app.exit(0);
    Ok(())
}
#[tauri::command]
pub async fn get_username() -> CommandResult<String> {
    Ok(std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "there".into()))
}
// ---- Escape recovery ----
/// The pending recovery spool, if one was left behind by a crash.
/// `seconds` is the amount of 16 kHz mono audio available.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStatus {
    pub seconds: u64,
    pub truncated: bool,
}
/// Reports the pending recovery spool, if any. `Ok(None)` when there is
/// nothing to recover.
#[tauri::command]
pub async fn recovery_status(state: State<'_, AppState>) -> CommandResult<Option<RecoveryStatus>> {
    Ok(
        crate::recovery::find_pending(&state.data_dir).map(|p| RecoveryStatus {
            seconds: p.seconds,
            truncated: p.truncated,
        }),
    )
}
/// Transcribes the pending recovery spool through the normal ASR path and
/// records the result like a normal dictation (history + day-wise transcript
/// file), then deletes the spool. The transcript is NOT injected: recovery
/// happens at startup, before the user's previous target app is known.
#[tauri::command]
pub async fn recover_last_dictation(
    app: AppHandle,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    let pending = crate::recovery::find_pending(&state.data_dir)
        .ok_or_else(|| "No pending dictation to recover".to_string())?;
    let pending_seconds = pending.seconds;
    // Transcribe on a worker: the speech-model load can take seconds.
    let handle = std::thread::Builder::new()
        .name("teletype-recover".into())
        .spawn(move || crate::recovery::recover(&app, &pending))
        .map_err(|e| format!("recover thread: {e}"))?;
    let transcript = handle
        .join()
        .map_err(|_| "recovery thread panicked".to_string())??;
    let transcript = transcript.trim().to_string();
    if transcript.is_empty() {
        return Err(CommandError::user("Recovered audio contained no speech"));
    }
    // Record the recovered dictation where normal dictations land. The
    // in-memory snapshot is updated for immediate reads; the disk write goes
    // through the persistence actor (A4).
    {
        let new_entry = teletype_core::history::DictationEntry {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: teletype_core::storage::now_ms(),
            text: transcript.clone(),
            context: None,
            duration_ms: Some(pending_seconds.saturating_mul(1000)),
        };
        let mut next = (*state.history.get()).clone();
        next.push(new_entry.clone());
        state.history.set_snapshot(next);
        state.persist.history(new_entry);
    }
    {
        let dir = state.transcripts_dir();
        if let Err(e) = teletype_core::history::append_transcript_file(
            &dir,
            teletype_core::storage::now_ms(),
            &transcript,
            "(recovered)",
        ) {
            crate::log_entry(
                crate::LogLevel::Error,
                format!("recovery transcript file: {e}"),
            );
        }
    }
    crate::log_entry(
        crate::LogLevel::Success,
        format!(
            "recovered {} s of dictation: {} chars",
            pending_seconds,
            transcript.len()
        ),
    );
    // The spool has served its purpose; delete it.
    crate::recovery::discard(&state.data_dir)?;
    Ok(transcript)
}
/// Deletes the pending recovery spool without transcribing.
#[tauri::command]
pub async fn discard_recovery(state: State<'_, AppState>) -> CommandResult<()> {
    crate::recovery::discard(&state.data_dir)?;
    Ok(())
}

// ---- P8.5: smart spacing ----
/// Punctuation that ends a word, so the dictated text starts a new
/// sentence and needs no space in front of it. Only used by the Windows
/// delivery path (`dictation.rs`), so it is dead code on other platforms.
#[cfg_attr(not(windows), allow(dead_code))]
const SENTENCE_END_PUNCT: [char; 4] = ['.', '!', '?', ':'];

/// P8.5: smart spacing for the Windows delivery path.
///
/// Decides, from the text immediately before the caret (read through UIA,
/// see `crate::uia::text_before_caret`), whether a space should be
/// prepended to the dictated text before it is injected. Pure Rust: no
/// UIA, so it is unit-testable on every platform.
///
/// Rules:
/// - empty context (start of field) -> no space;
/// - context already ends in whitespace (space, tab, newline) -> no space,
///   it is already there;
/// - context ends in sentence-ending punctuation -> no space, the dictated
///   text starts a new sentence;
/// - context ends in CJK (Han / Katakana / Hiragana) -> no space, CJK
///   scripts do not separate words with spaces;
/// - otherwise (a word character, mid-word punctuation, etc.) -> space.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn should_add_space(text_before_caret: &str) -> bool {
    let last = text_before_caret.chars().next_back();
    match last {
        None => false,
        Some(c) if c.is_whitespace() => false,
        Some(c) if SENTENCE_END_PUNCT.contains(&c) => false,
        Some(c) if is_cjk(c) => false,
        Some(_) => true,
    }
}

/// True for the CJK scripts that do not use spaces between words: CJK
/// Unified Ideographs (+ Ext A, Compatibility), Hiragana, Katakana, and
/// Hangul syllables.
#[cfg_attr(not(windows), allow(dead_code))]
fn is_cjk(c: char) -> bool {
    matches!(
        c,
        // CJK Unified Ideographs + Extension A
        '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}'
        // CJK Compatibility Ideographs
        | '\u{f900}'..='\u{faff}'
        // Hiragana / Katakana
        | '\u{3040}'..='\u{309f}' | '\u{30a0}'..='\u{30ff}'
        // Hangul Syllables
        | '\u{ac00}'..='\u{d7af}'
    )
}

// ---- Learning (self-learning loop, P7.8) ----
/// The learning store lives at `<app_data>/learning/learning.sqlite` — the
/// same convention the rest of the app uses for app-data files.
fn learning_store_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("learning").join("learning.sqlite")
}

/// Undoes a learned term: tombstones it in the learning store so it is
/// never re-learned (candidate rows are cleared too, via `promote::undo`).
#[tauri::command]
pub async fn learning_undo_term(state: State<'_, AppState>, term: String) -> CommandResult<()> {
    let store =
        teletype_core::learning::store::LearningStore::open(&learning_store_path(&state.data_dir))
            .map_err(|e| e.to_string())?;
    teletype_core::learning::promote::undo(&store, &term).map_err(|e| e.to_string())?;
    Ok(())
}

/// Deletes all learning data: the learning store (learned terms, edit
/// history) plus the retained-audio directory when one exists. No audio
/// retention directory exists in the code yet (audio persistence is a
/// later unit), so today this removes the sqlite store and its
/// `-wal`/`-shm` siblings; the directory deletion is kept so the later
/// unit needs no change here. Idempotent: missing files are `Ok(())`.
///
/// The store is opened per call and dropped before deletion (the
/// open-close-per-call convention), so a running app never holds the
/// sqlite file open across the delete.
#[tauri::command]
pub async fn delete_learning_data(state: State<'_, AppState>) -> CommandResult<()> {
    delete_learning_data_from(&state.data_dir)
}

/// The deletion body, split out so it is testable without a `State`.
fn delete_learning_data_from(data_dir: &std::path::Path) -> CommandResult<()> {
    let store_path = learning_store_path(data_dir);
    // Drop any open handle before deleting: nothing in this command keeps
    // the store open, and `LearningStore::open` is never called here.
    let file_name = store_path
        .file_name()
        .ok_or("learning store path has no file name")?
        .to_string_lossy();
    for suffix in ["", "-wal", "-shm"] {
        let path = store_path.with_file_name(format!("{file_name}{suffix}"));
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(CommandError::user(format!(
                    "failed to delete {}: {e}",
                    path.display()
                )));
            }
        }
    }
    // Retained audio for self-tests (`keep_audio_for_self_tests`): no code
    // writes this directory yet, so today it is usually absent.
    let audio_dir = data_dir.join("learning").join("audio");
    match std::fs::remove_dir_all(&audio_dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(CommandError::user(format!(
                "failed to delete retained audio dir {}: {e}",
                audio_dir.display()
            )));
        }
    }
    Ok(())
}
