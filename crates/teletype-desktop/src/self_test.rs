//! §9.2 installer smoke — `teletype-desktop --self-test`.
//!
//! Runs the real dictation pipeline headlessly against the bundled
//! `hello.wav` fixture and prints a single JSON line to stdout:
//!
//! - missing speech model: `{"ok":false,"stage":"models","error":"..."}` —
//!   exit 2 (expected on machines without models; CI downloads them first);
//! - success: `{"ok":true,"transcript":"...","asr_ms":N,"pipeline_ms":N}` —
//!   exit 0;
//! - any other failure: `{"ok":false,"stage":"...","error":"..."}` — exit 1;
//! - a hang: `{"ok":false,"stage":"timeout",...}` — exit 3.
//!
//! **Headless path (no Tauri window at all).** The ASR side needs no Tauri
//! context: the speech providers are plain `SpeechProvider` implementations
//! driven by the same `SpeechModelManager` the app uses. The text pipeline
//! is assembled exactly like `commands::transcribe_file` does, with default
//! stores and a `MockPlatform` — no inference provider, so the polish/LLM
//! half is skipped and only the deterministic tiers (ITN, AutoText, filler
//! removal, …) run. `dictation::transcribe` is the one function that takes
//! an `AppHandle`, so this module drives the manager + provider directly
//! with the same load/transcribe sequence it performs. Nothing here touches
//! focus, AX, or a window, so it passes on a headless CI runner.
//!
//! The whole run is bounded by [`RUN_TIMEOUT_SECS`]: the work happens on a
//! background thread and the main thread waits, so a hung engine (C-level
//! stall, dead llama-server) fails loudly instead of hanging the CI job.
//!
//! Model locations are discovered exactly like the app: the app's data dir
//! (`<data_dir>/models`) via the `dirs` crate with the same identifier the
//! app uses, overridable with `TELETYPE_SELF_TEST_MODELS` (tests point this
//! at an empty temp dir to exercise the no-models path without downloading
//! anything).

use std::{path::PathBuf, sync::mpsc, time::Duration};

use teletype_core::platform::MockPlatform;

use crate::commands::Settings;

/// The whole self-test must finish within this wall-clock budget.
pub const RUN_TIMEOUT_SECS: u64 = 120;

/// True when the argument list carries `--self-test` or `--self-test-json`.
/// Plain `std::env::args` parsing (no clap), as the binary must stay
/// dependency-free.
pub fn has_self_test_flag(args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--self-test" || a == "--self-test-json")
}

/// The app identifier used for the `dirs` lookup — must match
/// `tauri.conf.json` (`productName` + identifier), which is what Tauri's
/// `app.path().app_data_dir()` resolves to.
const APP_IDENTIFIER: &str = "com.teletype.app";

/// The fixture the smoke test transcribes, relative to the repo root.
const HELLO_WAV_REL: &str = "crates/teletype-speech/tests/fixtures/hello.wav";

/// Resolves the models directory the app uses.
///
/// `TELETYPE_SELF_TEST_MODELS` wins (test seam / CI), otherwise the same
/// `dirs::data_dir().join(APP_IDENTIFIER).join("models")` the app derives
/// from `app.path().app_data_dir()` in `lib.rs::run`.
fn models_dir() -> PathBuf {
    if let Ok(p) = std::env::var("TELETYPE_SELF_TEST_MODELS") {
        return PathBuf::from(p);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_IDENTIFIER)
        .join("models")
}

/// Resolves `hello.wav` the way the app finds repo-relative assets: from the
/// current dir, then by walking up from the executable (a `cargo` build puts
/// it at `target/debug|release/teletype`, three levels below the repo root).
fn hello_wav() -> Option<PathBuf> {
    let candidates = [PathBuf::from(HELLO_WAV_REL)];
    let mut from_exe = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let mut p = exe.as_path().to_path_buf();
        for _ in 0..8 {
            from_exe.push(p.join(HELLO_WAV_REL));
            if !p.pop() {
                break;
            }
        }
    }
    for c in candidates.iter().chain(from_exe.iter()) {
        if c.is_file() {
            return Some(c.clone());
        }
    }
    None
}

/// The result of a successful smoke run, serialized to the success payload.
struct SmokeOutcome {
    transcript: String,
    asr_ms: u64,
    pipeline_ms: u64,
}

/// The entry point `main.rs` calls for `--self-test`. Returns the process
/// exit code and prints exactly one JSON line to stdout.
pub fn run() -> i32 {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("teletype-self-test".into())
        .spawn(move || {
            let _ = tx.send(run_inner());
        })
        .expect("spawn self-test thread");
    match rx.recv_timeout(Duration::from_secs(RUN_TIMEOUT_SECS)) {
        Ok(code) => code,
        Err(_) => {
            println!(
                "{}",
                failure_json("timeout", &format!("exceeded {RUN_TIMEOUT_SECS}s"))
            );
            3
        }
    }
}

/// `{"ok":false,"stage":S,"error":E}` — stages and errors are quoted.
fn failure_json(stage: &str, error: &str) -> String {
    serde_json::json!({ "ok": false, "stage": stage, "error": error }).to_string()
}

/// `{"ok":true,"transcript":T,"asr_ms":N,"pipeline_ms":N}`.
fn success_json(o: &SmokeOutcome) -> String {
    serde_json::json!({
        "ok": true,
        "transcript": o.transcript,
        "asr_ms": o.asr_ms,
        "pipeline_ms": o.pipeline_ms,
    })
    .to_string()
}

/// The smoke test proper. Returns the exit code (0 ok, 1 failure, 2 no
/// models). All errors are printed as the failure JSON shape.
fn run_inner() -> i32 {
    match smoke() {
        Ok(outcome) => {
            println!("{}", success_json(&outcome));
            0
        }
        Err((stage, error, missing_models)) => {
            println!("{}", failure_json(&stage, &error));
            if missing_models {
                2
            } else {
                1
            }
        }
    }
}

type SmokeErr = (String, String, bool); // (stage, error, missing_models)

/// Runs the full smoke: discover models, load the speech model, transcribe
/// `hello.wav` through the same manager/provider the app uses, run the text
/// pipeline once, and check the transcript.
fn smoke() -> Result<SmokeOutcome, SmokeErr> {
    let settings = Settings::default();
    let models = models_dir();
    let (model_path, kind) = crate::dictation::resolve_speech_model(&settings, &models);

    // Fluid (Core ML) downloads on first load, so an empty base dir is not
    // a "missing" state for that engine; every other engine needs the file.
    let missing = match kind {
        crate::dictation::SpeechEngineKind::Fluid => false,
        _ => !model_path.exists(),
    };
    if missing {
        return Err((
            "models".into(),
            format!(
                "speech model not found at {} (download it in the app or \
                 point TELETYPE_SELF_TEST_MODELS at a models dir)",
                model_path.display()
            ),
            true,
        ));
    }

    let wav = hello_wav().ok_or_else(|| {
        (
            "fixture".into(),
            format!("hello.wav fixture not found ({HELLO_WAV_REL})"),
            false,
        )
    })?;
    let pcm = crate::commands::decode_audio_file_public(&wav)
        .map_err(|e| ("decode".into(), format!("decode {wav:?}: {e}"), false))?;

    // Load + transcribe through the same manager/provider pair the app
    // uses for this engine kind (see dictation::transcribe).
    let provider: Box<dyn teletype_speech::SpeechProvider> = match kind {
        crate::dictation::SpeechEngineKind::Parakeet => {
            Box::new(teletype_speech::parakeet::ParakeetProvider::new())
        }
        _ => Box::new(teletype_speech::whisper::WhisperProvider::new()),
    };
    let manager =
        teletype_inference::manager::SpeechModelManager::new(model_path.clone(), provider);
    let asr_started = std::time::Instant::now();
    manager
        .load()
        .map_err(|e| ("load".into(), format!("speech model load: {e}"), false))?;
    let text = {
        let mut speech = manager
            .provider()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let language = "auto";
        if speech.capabilities().word_timings {
            speech
                .transcribe_detailed(&pcm, language)
                .map(|d| d.text)
                .map_err(|e| ("asr".into(), format!("transcribe: {e}"), false))?
        } else {
            speech
                .transcribe(&pcm, language)
                .map_err(|e| ("asr".into(), format!("transcribe: {e}"), false))?
        }
    };
    let asr_ms = asr_started.elapsed().as_millis() as u64;

    let raw = crate::dictation::ensure_latin_if_english(
        &text,
        &crate::dictation::effective_language(settings.language.clone(), true),
    );
    let raw = raw.trim().to_string();
    if raw.is_empty() {
        return Err(("asr".into(), "transcript was empty".into(), false));
    }
    if !raw.to_lowercase().contains("hello") {
        return Err((
            "assert".into(),
            format!("transcript does not contain \"hello\": {raw:?}"),
            false,
        ));
    }
    let transcript = raw;

    // Run the text pipeline once (deterministic tiers; no LLM provider, so
    // the polish half is skipped — the same path a fresh, unconfigured
    // install takes).
    let pipeline_started = std::time::Instant::now();
    let polished = run_text_pipeline(&transcript)
        .map_err(|e| ("pipeline".into(), format!("text pipeline: {e}"), false))?;
    let pipeline_ms = pipeline_started.elapsed().as_millis() as u64;
    let _ = polished;

    Ok(SmokeOutcome {
        transcript,
        asr_ms,
        pipeline_ms,
    })
}

/// Runs the text pipeline once over `text`, assembled the same way
/// `commands::transcribe_file` does: default stores, `MockPlatform`, no
/// inference/decision provider (so no LLM call), global language setting.
fn run_text_pipeline(text: &str) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!(
        "teletype-self-test-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let settings = Settings::default();
    let autotext = teletype_core::autotext::AutoTextStore::default();
    let transforms = teletype_core::transforms::TransformStore::with_built_ins();
    let profile = teletype_core::personalization::UserProfile {
        language: crate::dictation::effective_language(settings.language.clone(), false),
        ..Default::default()
    };
    let styles = teletype_core::style::StyleProfileStore::with_built_ins();
    let dictionary = teletype_core::dictionary::Dictionary::default();
    let platform = MockPlatform::default();
    let mut pack_terms = teletype_core::vocab::terms_for(&settings.enabled_packs);
    let user_terms = teletype_core::userpacks::UserPackStore::default().terms_for();
    pack_terms.extend(user_terms);

    let mut pipeline = teletype_core::pipeline::Pipeline {
        platform: &platform,
        autotext: &autotext,
        transforms: &transforms,
        profile: &profile,
        inference: None,
        decision: None,
        force_command: false,
        dictionary: &dictionary,
        styles: &styles,
        active_style: &settings.active_style_profile,
        explicit_style: "",
        auto_apply: settings.auto_apply_transform,
        restore_clipboard: settings.restore_clipboard,
        remove_filler_words: settings.remove_filler_words,
        filler_words: settings.filler_words.clone(),
        restore_emoji: settings.restore_emoji,
        spoken_emoji: settings.spoken_emoji,
        spoken_punctuation: settings.spoken_punctuation,
        system_autotext: teletype_core::autotext::system::entries(),
        token_sink: None,
        polish_gate_enabled: settings.polish_gate_enabled,
        polish_gate_threshold_words: settings.polish_gate_threshold_words,
        pack_terms: &pack_terms,
        word_checker: &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
        list_style: teletype_core::transforms::ListStyle::from_str_lenient(&settings.list_style),
    };
    let result = pipeline.run(
        teletype_core::pipeline::UnifiedInput {
            source: teletype_core::pipeline::InputSource::Voice,
            text: text.to_string(),
        },
        None,
    );
    let _ = std::fs::remove_dir_all(&dir);
    Ok(result.final_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// main.rs passes exactly these; everything else must be ignored so the
    /// normal app path is untouched.
    #[test]
    fn arg_parsing_detects_self_test() {
        assert!(!has_self_test_flag(&["teletype".to_string()]));
        assert!(!has_self_test_flag(&[
            "teletype".to_string(),
            "--quit".to_string()
        ]));
        assert!(has_self_test_flag(&[
            "teletype".into(),
            "--self-test".into()
        ]));
        assert!(has_self_test_flag(&[
            "teletype".into(),
            "--self-test-json".into()
        ]));
        assert!(has_self_test_flag(&[
            "teletype".into(),
            "other".into(),
            "--self-test".into()
        ]));
    }

    #[test]
    fn success_payload_shape() {
        let o = SmokeOutcome {
            transcript: "Hello.".into(),
            asr_ms: 42,
            pipeline_ms: 7,
        };
        let v: serde_json::Value = serde_json::from_str(&success_json(&o)).expect("valid json");
        assert_eq!(v["ok"], true);
        assert_eq!(v["transcript"], "Hello.");
        assert_eq!(v["asr_ms"], 42);
        assert_eq!(v["pipeline_ms"], 7);
        assert!(v.get("stage").is_none());
        assert!(v.get("error").is_none());
    }

    #[test]
    fn failure_payload_shape() {
        let v: serde_json::Value =
            serde_json::from_str(&failure_json("models", "no model")).expect("valid json");
        assert_eq!(v["ok"], false);
        assert_eq!(v["stage"], "models");
        assert_eq!(v["error"], "no model");
        assert!(v.get("transcript").is_none());
    }

    /// The no-models path: point discovery at an empty temp dir. The default
    /// macOS model (ggml Parakeet) must not exist there, so the run reports
    /// stage "models" with the missing-models exit code — without ever
    /// downloading anything.
    #[test]
    fn missing_models_reports_stage_models() {
        let dir = std::env::temp_dir().join(format!(
            "teletype-self-test-empty-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::env::set_var("TELETYPE_SELF_TEST_MODELS", &dir);
        let code = run_inner();
        std::env::remove_var("TELETYPE_SELF_TEST_MODELS");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(code, 2, "missing models must exit 2");
    }

    #[test]
    fn timeout_budget_is_sane() {
        // The CI budget: long enough for a cold Metal load + transcribe,
        // short enough that a hung job fails in minutes, not hours.
        // Compile-time guard: a runtime `assert!` on a `const` folds to
        // `assert!(true)` and is optimized away.
        const _: () = assert!(RUN_TIMEOUT_SECS >= 30 && RUN_TIMEOUT_SECS <= 300);
    }
}
