//! P9.6: the `--e2e` IPC hook. A one-shot harness for the delivery
//! matrix (plan §9.6) that drives real takes against real target apps.
//!
//! The app is launched with `--e2e` (windows hidden, no tray, no hotkey
//! registration); this module then listens on a unix socket and accepts
//! one JSON command per line:
//!
//! - `{"cmd":"status"}` -> app state, pill state, speech-model readiness.
//! - `{"cmd":"policy","app":"<bundle-id>","force":"clipboard|type|setvalue|hold"}`
//!   -> per-app override of the delivery cascade step (the e2e matrix's
//!   "repeat with each fallback forced" step).
//! - `{"cmd":"start","input":"<path.wav>","command":true|false}` -> queue
//!   the file as the take's audio source and trigger the take on the
//!   normal controller; a feeder thread plays the samples in real time
//!   (16 kHz, 16 ms frames) and releases the take when the file ends.
//! - `{"cmd":"stop"}` -> stop the in-flight take (same as a hotkey release).
//! - `{"cmd":"result","id":N}` -> the recorded outcome of one take.
//!
//! The file-input mode replaces ONLY the audio source and the trigger:
//! take start, release, transcription, the text pipeline, and the
//! delivery cascade are the production code paths, untouched.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::dictation::{Event, SpeechEngineKind};
use crate::AppState;

/// Where the control socket lives (`TELETYPE_E2E_SOCKET` overrides).
const DEFAULT_SOCKET: &str = "/tmp/teletype-e2e.sock";

/// The take's samples are fed to the controller at this cadence (16 kHz),
/// so a 1 s file plays for 1 s of wall-clock, like a real take.
const FEED_RATE: u32 = 16_000;
const FRAME_MS: u64 = 16;
const FRAME_SAMPLES: usize = (FEED_RATE as u64 * FRAME_MS / 1000) as usize;

/// P9.6: the command the harness asked the Command Mode take to resolve,
/// for the `result` reply. The controller owns the real pipeline's
/// decider; the e2e hook records what the harness asked for so the
/// harness can assert on it. `None` for dictation takes.
pub static E2E_COMMAND: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// The outcome of one e2e take, recorded by the controller hook and
/// returned by `result`.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct E2eResult {
    pub id: u64,
    pub text: String,
    pub command: Option<String>,
    pub delivery: String,
    pub error: Option<String>,
}

/// One forced delivery mode, keyed by exact bundle id (the same match
/// semantics as the per-app policy table).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ForcedMode {
    /// The default cascade path (clipboard paste + verify).
    Clipboard,
    /// Skip the clipboard; type the text.
    Type,
    /// Skip the clipboard; set the field value directly.
    Setvalue,
    /// Fail every route; the text is held for Paste-Last.
    Hold,
}

impl ForcedMode {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "clipboard" => Some(Self::Clipboard),
            "type" => Some(Self::Type),
            "setvalue" => Some(Self::Setvalue),
            "hold" => Some(Self::Hold),
            _ => None,
        }
    }

    /// The policy table to use for the take: the built-in table with this
    /// app's rule overridden (exact bundle-id match, like the policy file).
    /// `hold` is handled by the core cascade's forced-path hook
    /// (`TELETYPE_FORCE_DELIVERY`/`TELETYPE_FORCE_APP`), which the hook
    /// sets before the take; the table override here only pins the mode
    /// for the other three cases.
    pub fn apply(
        &self,
        base: &teletype_core::delivery::DeliveryPolicy,
        app_id: &str,
    ) -> teletype_core::delivery::DeliveryPolicy {
        use teletype_core::delivery::DeliveryMode;
        match self {
            Self::Clipboard => base.with_override(app_id, DeliveryMode::PasteVerify, None),
            Self::Type => base.with_override(app_id, DeliveryMode::Type, None),
            Self::Setvalue => base.with_override(app_id, DeliveryMode::SetValue, None),
            Self::Hold => base.clone(),
        }
    }
}

/// The controller-facing hook: consulted by the dictation controller at
/// the two e2e seams (take start and injection). `None` outside `--e2e`.
#[derive(Default)]
pub struct E2eSlot {
    /// The file to feed the NEXT take, if one was queued by `start`.
    pub file: Mutex<Option<std::path::PathBuf>>,
    /// Forced delivery modes by exact bundle id (the `policy` command).
    pub policy: Mutex<HashMap<String, ForcedMode>>,
    /// Recorded take outcomes, keyed by session id.
    pub results: Mutex<HashMap<u64, E2eResult>>,
}

impl E2eSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// The file to feed the next take, if one was queued.
    pub fn take_file(&self) -> Option<std::path::PathBuf> {
        self.file.lock().unwrap().take()
    }

    /// The forced mode for `app_id`, if one was set for it.
    pub fn force_for(&self, app_id: Option<&str>) -> Option<ForcedMode> {
        let map = self.policy.lock().unwrap();
        app_id.and_then(|id| map.get(id).copied())
    }

    /// The recorded outcome of `id`, if the take has finished.
    pub fn result(&self, id: u64) -> Option<E2eResult> {
        self.results.lock().unwrap().get(&id).cloned()
    }

    /// Records a take's outcome (called from the controller hook).
    pub fn record(&self, result: E2eResult) {
        self.results.lock().unwrap().insert(result.id, result);
    }

    /// Sets the delivery label on the recorded outcome for `id`.
    pub fn set_delivery(&self, id: u64, delivery: &str) {
        if let Some(r) = self.results.lock().unwrap().get_mut(&id) {
            r.delivery = delivery.to_string();
        }
    }
}

/// The command the harness sends, one JSON object per line.
#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum E2eCommand {
    Status,
    Policy {
        app: String,
        force: String,
    },
    Start {
        input: String,
        #[serde(default)]
        command: bool,
    },
    Stop,
    Result {
        id: u64,
    },
}

/// Spawns the unix-socket listener. A stale socket from a crashed run is
/// removed first: the previous process is dead, so its socket is orphaned.
/// Unix-only (macOS e2e harness); on Windows the call site skips it.
#[cfg(unix)]
pub fn start(app: AppHandle, slot: std::sync::Arc<E2eSlot>) {
    let path = std::env::var("TELETYPE_E2E_SOCKET").unwrap_or_else(|_| DEFAULT_SOCKET.into());
    let _ = std::fs::remove_file(&path);
    let ok = std::thread::Builder::new()
        .name("teletype-e2e".into())
        .spawn(move || {
            let listener = match std::os::unix::net::UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!("e2e: bind {path} failed: {e}"),
                    );
                    return;
                }
            };
            crate::log_entry(crate::LogLevel::Info, format!("e2e: listening on {path}"));
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => handle_client(app.clone(), slot.clone(), s),
                    Err(e) => {
                        crate::log_entry(crate::LogLevel::Warn, format!("e2e: accept failed: {e}"))
                    }
                }
            }
        });
    if ok.is_err() {
        crate::log_entry(
            crate::LogLevel::Error,
            "e2e: listener thread failed to start",
        );
    }
}

/// Answers one client connection: one JSON command per line, one JSON
/// line per answer. The harness keeps the connection open across takes.
#[cfg(unix)]
fn handle_client(
    app: AppHandle,
    slot: std::sync::Arc<E2eSlot>,
    stream: std::os::unix::net::UnixStream,
) {
    let reader = BufReader::new(stream.try_clone().expect("dup the socket"));
    let mut writer = stream;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<E2eCommand>(&line) {
            Ok(cmd) => dispatch(&app, &slot, cmd),
            Err(e) => err_json(&format!("bad command: {e}")),
        };
        if writeln!(writer, "{reply}").is_err() {
            break;
        }
    }
}

/// Runs one command and builds the reply JSON.
fn dispatch(app: &AppHandle, slot: &std::sync::Arc<E2eSlot>, cmd: E2eCommand) -> String {
    let state = app.state::<AppState>();
    match cmd {
        E2eCommand::Status => {
            let pill = state
                .dictation_state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let settings = state.settings();
            let (model_path, kind) =
                crate::dictation::resolve_speech_model(&settings, &state.models_dir);
            let on_disk = model_path.exists();
            let loaded = match kind {
                SpeechEngineKind::Parakeet => state.parakeet.is_loaded(),
                _ => state.speech.is_loaded(),
            };
            ok_json(&serde_json::json!({
                "running": true,
                "pill": pill,
                "speechModel": settings.selected_speech_model,
                "speechEngine": engine_label(kind),
                "modelOnDisk": on_disk,
                "modelLoaded": loaded,
            }))
        }
        E2eCommand::Policy { app: app_id, force } => match ForcedMode::parse(&force) {
            Some(mode) => {
                slot.policy
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(app_id.clone(), mode);
                ok_json(&serde_json::json!({ "app": app_id, "force": force }))
            }
            None => err_json(&format!("unknown force mode \"{force}\"")),
        },
        E2eCommand::Start { input, command } => {
            let path = std::path::PathBuf::from(&input);
            if !path.is_file() {
                return err_json(&format!("input file not found: {input}"));
            }
            slot.file
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .replace(path);
            *E2E_COMMAND
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                command.then(|| "command-mode take".to_string());
            state.controller.send(if command {
                Event::CommandHotkeyDown
            } else {
                Event::HotkeyDown
            });
            ok_json(&serde_json::json!({ "started": true, "command": command }))
        }
        E2eCommand::Stop => {
            state.controller.send(Event::HotkeyUp);
            ok_json(&serde_json::json!({ "stopped": true }))
        }
        E2eCommand::Result { id } => match slot.result(id) {
            Some(r) => ok_json(&serde_json::json!({ "done": true, "result": r })),
            None => ok_json(&serde_json::json!({ "done": false })),
        },
    }
}

fn ok_json(v: &serde_json::Value) -> String {
    serde_json::json!({ "ok": true, "data": v }).to_string()
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "ok": false, "error": msg }).to_string()
}

fn engine_label(kind: SpeechEngineKind) -> &'static str {
    match kind {
        SpeechEngineKind::Whisper => "whisper",
        SpeechEngineKind::Parakeet => "parakeet",
        SpeechEngineKind::Fluid => "fluid",
        SpeechEngineKind::Sherpa => "sherpa",
        SpeechEngineKind::Whistle => "whistle",
    }
}

/// The controller hook: called from the dictation controller at the two
/// e2e seams. No-op unless the slot is installed (i.e. `--e2e` mode).
///
/// Seam 1 (take start): if a file was queued for the next take, spawn the
/// feeder that plays it into the in-flight recording.
pub fn on_take_start(app: &AppHandle, slot: &E2eSlot) {
    let Some(file) = slot.take_file() else {
        return;
    };
    let app = app.clone();
    std::thread::Builder::new()
        .name("teletype-e2e-feed".into())
        .spawn(move || {
            if let Err(e) = feed_file(&app, &file) {
                crate::log_entry(crate::LogLevel::Error, format!("e2e feed: {e}"));
                app.state::<AppState>().controller.send(Event::HotkeyUp);
            }
        })
        .ok();
}

/// Feeds `file` into the in-flight take, one 16 ms frame at a time at
/// 16 kHz (wall-clock paced), then releases the take (the hotkey release
/// equivalent). The take's phase must already be Listening.
fn feed_file(app: &AppHandle, path: &std::path::Path) -> Result<(), String> {
    let samples =
        crate::commands::decode_audio_file_public(path).map_err(|e| format!("decode: {e}"))?;
    if samples.is_empty() {
        return Err("audio file has no samples".into());
    }
    let state = app.state::<AppState>();
    let mut off = 0usize;
    let start = Instant::now();
    while off < samples.len() {
        let end = (off + FRAME_SAMPLES).min(samples.len());
        state.controller.send(Event::E2eFeed {
            frame: samples[off..end].to_vec(),
        });
        off = end;
        // Keep the feed at real time: a 1 s file takes 1 s to feed.
        let target = Duration::from_millis((off as f64 / FEED_RATE as f64 * 1000.0) as u64);
        if start.elapsed() < target {
            std::thread::sleep(target - start.elapsed());
        }
    }
    state.controller.send(Event::HotkeyUp);
    Ok(())
}

/// Builds the `E2eResult` for a finished take from the pieces the
/// controller knows at the recording seam.
#[allow(dead_code)]
pub fn make_result(
    id: u64,
    text: String,
    command: Option<String>,
    delivery: String,
    error: Option<String>,
) -> E2eResult {
    E2eResult {
        id,
        text,
        command,
        delivery,
        error,
    }
}
