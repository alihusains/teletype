//! T7: first-run calibration.
//!
//! This module owns the three onboarding prompts, the supported-profile filter,
//! the sample generation (through the *existing* Polish engine path), and the
//! local record of whether calibration was skipped or which profile was chosen.
//!
//! Privacy contract: sample text never leaves the process and is never written
//! to History. The only file this module writes is `calibration.json`, a tiny
//! local marker with the outcome (skipped / chosen profile) — no sample text.
//! There is no telemetry and no second prompt implementation: profile lines and
//! budgets come from `teletype_core::profile`, and generation calls the same
//! `run_transform_blocking` engine the playground and dictation use.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager, State};

use super::*;
use crate::AppState;
use teletype_core::profile::{self, PolishProfile};

/// One selectable profile in the calibration comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationProfileOption {
    pub key: String,
    pub label: String,
    pub budget_percent: u8,
}

/// What the UI needs to render the comparison and the "unsupported" copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationOptions {
    pub model_id: String,
    pub capability: String,
    pub supported: Vec<CalibrationProfileOption>,
    pub unsupported: Vec<CalibrationProfileOption>,
    /// False when no polish provider is loaded: the UI then shows a short
    /// explanation and lets the user skip instead of blocking onboarding.
    pub model_loaded: bool,
}

fn option_for(profile: PolishProfile) -> CalibrationProfileOption {
    CalibrationProfileOption {
        key: profile.key().to_string(),
        label: profile.label().to_string(),
        budget_percent: profile.budget_percent(),
    }
}

/// A candidate profile is "honored" when T3a's decision does not downgrade it.
/// Reusing `decide_profile` keeps the filter on the same model-capability source
/// of truth as the runtime Polish path — no second capability matrix.
fn profile_supported_by_model(profile: PolishProfile, model_id: &str) -> bool {
    !profile::decide_profile(profile, model_id, 40).fell_back()
}

/// Pure profile list/filter, shared by the command and the tests.
pub(crate) fn calibration_options(model_id: &str, model_loaded: bool) -> CalibrationOptions {
    let mut supported = Vec::new();
    let mut unsupported = Vec::new();
    // Only the three candidate profiles of §8.2 are offered. Transform is an
    // explicit user request, never a first-run comparison; Off runs no model.
    for profile in [
        PolishProfile::Clean,
        PolishProfile::Natural,
        PolishProfile::Structured,
    ] {
        let option = option_for(profile);
        if profile_supported_by_model(profile, model_id) {
            supported.push(option);
        } else {
            unsupported.push(option);
        }
    }
    let capability = profile::decide_profile(PolishProfile::Clean, model_id, 40).model_capability;
    CalibrationOptions {
        model_id: model_id.to_string(),
        capability: capability.to_string(),
        supported,
        unsupported,
        model_loaded,
    }
}

/// One typed sample sent by the UI.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSampleInput {
    pub id: String,
    pub text: String,
}

/// A structured error the UI can branch on and display verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationError {
    pub code: String,
    pub message: String,
}

/// Before/after for one sample under one profile. `after` is always the text to
/// show: on a model error it is the deterministic input, so the UI never hangs
/// on a spinner that will never resolve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSampleResult {
    pub sample_id: String,
    pub profile: String,
    pub before: String,
    pub after: String,
    pub transformed: bool,
    pub error: Option<CalibrationError>,
}

#[derive(Debug, Clone)]
struct SampleOutput {
    text: String,
    transformed: bool,
    #[allow(dead_code)]
    skip_message: Option<String>,
}

/// Pure fan-out over samples × profiles with an injected engine runner. The
/// runner is the only thing that can fail; a failure degrades that cell to the
/// deterministic text plus a structured error, never to a missing result.
fn generate_sample_results(
    samples: &[CalibrationSampleInput],
    profiles: &[PolishProfile],
    run: &impl Fn(PolishProfile, &str) -> Result<SampleOutput, CommandError>,
) -> Vec<CalibrationSampleResult> {
    let mut out = Vec::new();
    for sample in samples {
        for &profile in profiles {
            let before = sample.text.clone();
            match run(profile, &sample.text) {
                Ok(output) => out.push(CalibrationSampleResult {
                    sample_id: sample.id.clone(),
                    profile: profile.key().to_string(),
                    before: before.clone(),
                    after: output.text,
                    transformed: output.transformed,
                    error: None,
                }),
                Err(err) => out.push(CalibrationSampleResult {
                    sample_id: sample.id.clone(),
                    profile: profile.key().to_string(),
                    before: before.clone(),
                    after: before,
                    transformed: false,
                    error: Some(CalibrationError {
                        code: err.code,
                        message: err.message,
                    }),
                }),
            }
        }
    }
    out
}

/// The no-model path: deterministic text for every cell and one clear,
/// non-blocking message. No provider is loaded and nothing is auto-downloaded.
fn no_model_results(
    samples: &[CalibrationSampleInput],
    profiles: &[PolishProfile],
) -> Vec<CalibrationSampleResult> {
    let err = CommandError::model(
        "No AI polish model is loaded. This is your text as typed; dictation still works and you can skip.".to_string(),
    );
    generate_sample_results(samples, profiles, &|_, _| Err(err.clone()))
}

/// The model id the active Polish provider would use, from settings only.
fn active_model_id(settings: &Settings) -> String {
    match settings.selected_llm_provider.as_str() {
        "local-server" => settings.selected_llm_model.clone(),
        "openai-compat" => settings.openai_model.clone(),
        _ => String::new(),
    }
}

/// The shared engine packet for one calibration run. The learned rules,
/// settings/profile/transforms snapshots and active-app context do not vary
/// across the 3×3 samples × profiles fan-out, so they are resolved once here
/// and reused by every cell. Only the profile (encoded into `user_override`)
/// and the sample text vary per call.
struct CalibrationRun {
    provider: std::sync::Arc<dyn teletype_core::llm::InferenceProvider>,
    transform: teletype_core::transforms::TransformDefinition,
    base_ctx: teletype_core::transforms::prompt::PromptContext,
    override_text: Option<String>,
}

/// Resolve the shared packet once. Returns the same errors the per-cell path
/// used to return, so a missing preset/model still degrades every cell to the
/// deterministic text plus a structured error (see `generate_sample_results`).
fn build_calibration_run(state: &AppState) -> Result<CalibrationRun, CommandError> {
    let transforms = (*state.transforms.get()).clone();
    let transform = transforms
        .get("builtin-polish")
        .ok_or_else(|| CommandError::user("No Polish preset is available"))?
        .clone();
    let engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .ok_or_else(|| CommandError::model("No AI polish model is loaded".to_string()))?;
    let personal = (*state.profile.get()).clone();
    let settings = state.settings();
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let packet =
        resolve_packet_with_learned_rules(&personal, &app_ctx, state.data_dir.join("learning"));
    let override_text = settings
        .polish_prompt_override
        .clone()
        .filter(|s| !s.trim().is_empty());
    let base_ctx = teletype_core::transforms::prompt::PromptContext {
        app: Some(app_ctx),
        preferences: packet.style,
        preferred_terms: packet.terms,
        user_instruction: None,
        language: personal.language.clone(),
        s1_control: personal.s1_control,
        list_style: settings.list_style.resolve(),
        polish_max_change_percent: settings.polish_max_change_percent,
        screen_terms: Vec::new(),
        user_override: None,
    };
    Ok(CalibrationRun {
        provider: engine,
        transform,
        base_ctx,
        override_text,
    })
}

/// Runs one cell through the real Polish engine with a specific profile. This
/// is the same engine call `test_polish_prompt` makes; the only difference is
/// that the profile is encoded in `user_override` with T3a's marker so the
/// engine selects the matching instruction line and edit budget. No History
/// write happens on this path — `run_transform_blocking` returns text only.
fn run_calibration_transform(
    run: &CalibrationRun,
    profile: PolishProfile,
    input: &str,
) -> Result<SampleOutput, CommandError> {
    let mut prompt_ctx = run.base_ctx.clone();
    prompt_ctx.user_override = profile::encode_instruction(profile, run.override_text.as_deref());
    let result = teletype_core::transforms::engine::run_transform_blocking(
        run.provider.as_ref(),
        &run.transform,
        input,
        &prompt_ctx,
        &mut None,
    );
    let skip_message = result
        .metrics
        .skip_reason
        .as_ref()
        .and_then(teletype_core::state::skip_message);
    Ok(SampleOutput {
        text: result.text,
        transformed: result.transformed,
        skip_message,
    })
}

/// Read-only options for the calibration step: profiles the loaded model can
/// honor plus the ones it cannot (for the "left out" copy).
#[tauri::command]
pub fn calibration_profile_options(
    state: State<'_, AppState>,
) -> CommandResult<CalibrationOptions> {
    let settings = state.settings();
    let model_id = active_model_id(&settings);
    let loaded = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .is_some();
    Ok(calibration_options(&model_id, loaded))
}

/// Generates before/after samples through the real Polish prompt path.
///
/// A local model load + several generations can take seconds, so the whole
/// run lives on a blocking thread. If no provider is loaded, no model is
/// downloaded or started here: the UI gets deterministic text and a clear
/// message and stays skippable.
#[tauri::command]
pub async fn generate_calibration_samples(
    app: AppHandle,
    samples: Vec<CalibrationSampleInput>,
) -> CommandResult<Vec<CalibrationSampleResult>> {
    let (model_id, loaded) = {
        let state = app.state::<AppState>();
        let settings = state.settings();
        let loaded = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .is_some();
        (active_model_id(&settings), loaded)
    };
    let options = calibration_options(&model_id, loaded);
    let profiles: Vec<PolishProfile> = options
        .supported
        .iter()
        .filter_map(|o| PolishProfile::parse(&o.key))
        .collect();

    if !loaded {
        return Ok(no_model_results(&samples, &profiles));
    }

    let app_for_run = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_for_run.state::<AppState>();
        let run = build_calibration_run(&state);
        generate_sample_results(&samples, &profiles, &|profile, text| match &run {
            Ok(run) => run_calibration_transform(run, profile, text),
            Err(err) => Err(err.clone()),
        })
    })
    .await
    .map_err(|e| CommandError::internal(format!("calibration task failed: {e}")))
}

/// The local outcome marker. Contains no sample text — only the timestamp and
/// the decision, so "calibration was skipped" is durable without storing the
/// user's words.
const CALIBRATION_FILE: &str = "calibration.json";

pub(crate) fn calibration_path(data_dir: &Path) -> PathBuf {
    data_dir.join(CALIBRATION_FILE)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn store_calibration_outcome(data_dir: &Path, outcome: &CalibrationOutcome) -> CommandResult<()> {
    let value = serde_json::json!({
        "completedAt": now_ms(),
        "skipped": outcome.skipped,
        "profile": outcome.profile,
    });
    let body = serde_json::to_string_pretty(&value).map_err(|e| {
        CommandError::internal(format!("Could not encode calibration outcome: {e}"))
    })?;
    std::fs::write(calibration_path(data_dir), body)
        .map_err(|e| CommandError::internal(format!("Could not save calibration outcome: {e}")))
}

/// Per-app starting defaults from §5.3 / §8.2. Only the category rows with a
/// fixed contract are written: docs/notes are length-dependent and browser /
/// unknown apps deliberately fall through to the chosen global profile.
pub(crate) fn sensible_app_defaults() -> std::collections::BTreeMap<String, PolishProfile> {
    use std::collections::BTreeMap;
    BTreeMap::from([
        ("email".to_string(), PolishProfile::Structured),
        ("chat".to_string(), PolishProfile::Natural),
        ("social".to_string(), PolishProfile::Natural),
        ("coding".to_string(), PolishProfile::Off),
        ("terminal".to_string(), PolishProfile::Off),
    ])
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationOutcome {
    pub skipped: bool,
    pub profile: Option<String>,
}

/// Records the decision, saves the chosen profile and per-app defaults, and
/// returns the fresh settings so the caller does not clobber them with a stale
/// snapshot when it later sets `hasCompletedOnboarding`.
///
/// Skipping writes only the local marker: the existing defaults are kept.
#[tauri::command]
pub fn record_calibration(
    state: State<'_, AppState>,
    outcome: CalibrationOutcome,
) -> CommandResult<Settings> {
    store_calibration_outcome(&state.data_dir, &outcome)?;
    if outcome.skipped {
        return Ok(state.settings());
    }
    let chosen = outcome
        .profile
        .as_deref()
        .and_then(PolishProfile::parse)
        .filter(|p| {
            matches!(
                p,
                PolishProfile::Clean | PolishProfile::Natural | PolishProfile::Structured
            )
        })
        .unwrap_or_default();
    let mut settings = state.settings();
    settings.polish_profile = chosen;
    settings.polish_app_profiles = sensible_app_defaults();
    state.replace_settings(settings)?;
    Ok(state.settings())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: &str, text: &str) -> CalibrationSampleInput {
        CalibrationSampleInput {
            id: id.to_string(),
            text: text.to_string(),
        }
    }

    #[test]
    fn profile_list_offers_the_three_candidates() {
        let options = calibration_options("qwen3.5-2b", true);
        assert_eq!(
            options
                .supported
                .iter()
                .map(|o| o.key.as_str())
                .collect::<Vec<_>>(),
            vec!["clean", "natural", "structured"]
        );
        assert!(options.unsupported.is_empty());
        assert_eq!(options.capability, profile::MODEL_CAPABILITY_EDITOR);
        assert!(options.model_loaded);
        // Budgets travel with the row so the UI can show the promise/budget.
        assert_eq!(options.supported[0].budget_percent, 15);
        assert_eq!(options.supported[1].budget_percent, 30);
        assert_eq!(options.supported[2].budget_percent, 45);
    }

    #[test]
    fn cleanup_only_model_filters_unsupported_profiles() {
        for model in ["s1-mini", "speakoflow-mini"] {
            let options = calibration_options(model, true);
            assert_eq!(
                options
                    .supported
                    .iter()
                    .map(|o| o.key.as_str())
                    .collect::<Vec<_>>(),
                vec!["clean"]
            );
            assert_eq!(
                options
                    .unsupported
                    .iter()
                    .map(|o| o.key.as_str())
                    .collect::<Vec<_>>(),
                vec!["natural", "structured"]
            );
            assert_eq!(options.capability, profile::MODEL_CAPABILITY_CLEANUP);
        }
    }

    #[test]
    fn model_error_returns_deterministic_text_and_a_structured_error() {
        let samples = [input("email", "original words")];
        let results = generate_sample_results(&samples, &[PolishProfile::Clean], &|_, _| {
            Err(CommandError::model("server stopped".to_string()))
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].after, "original words");
        assert_eq!(results[0].before, "original words");
        assert!(!results[0].transformed);
        let err = results[0].error.as_ref().expect("structured error");
        assert_eq!(err.code, "E_MODEL");
        assert_eq!(err.message, "server stopped");
    }

    #[test]
    fn ok_result_carries_the_engine_output() {
        let samples = [input("slack", "raw text")];
        let results = generate_sample_results(&samples, &[PolishProfile::Natural], &|_, text| {
            Ok(SampleOutput {
                text: format!("{text}."),
                transformed: true,
                skip_message: None,
            })
        });
        assert_eq!(results[0].after, "raw text.");
        assert!(results[0].transformed);
        assert!(results[0].error.is_none());
    }

    #[test]
    fn no_model_returns_deterministic_text_without_a_spinner() {
        let samples = [input("note", "my note")];
        let results = no_model_results(&samples, &[PolishProfile::Clean]);
        assert_eq!(results[0].after, "my note");
        assert!(!results[0].transformed);
        assert_eq!(results[0].error.as_ref().unwrap().code, "E_MODEL");
    }

    #[test]
    fn recording_an_outcome_never_touches_history() {
        let dir = tempfile::tempdir().unwrap();
        let sentinel = b"[{ \"id\": 1, \"raw_text\": \"user take\" }]";
        let history = dir.path().join("history.json");
        std::fs::write(&history, sentinel).unwrap();

        let outcome = CalibrationOutcome {
            skipped: true,
            profile: None,
        };
        store_calibration_outcome(dir.path(), &outcome).unwrap();

        assert_eq!(std::fs::read(&history).unwrap(), sentinel);
        let marker = std::fs::read_to_string(calibration_path(dir.path())).unwrap();
        assert!(marker.contains("\"skipped\": true"));
        assert!(!marker.contains("user take"));
    }

    #[test]
    fn sensible_app_defaults_match_the_category_contracts() {
        let defaults = sensible_app_defaults();
        assert_eq!(defaults.get("email"), Some(&PolishProfile::Structured));
        assert_eq!(defaults.get("chat"), Some(&PolishProfile::Natural));
        assert_eq!(defaults.get("coding"), Some(&PolishProfile::Off));
        assert_eq!(defaults.get("terminal"), Some(&PolishProfile::Off));
        // Length-dependent / unknown categories are left to global resolution.
        assert!(!defaults.contains_key("document"));
        assert!(!defaults.contains_key("other"));
    }
}
