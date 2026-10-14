//! Unwired first slice of roadmap §6 A2: the dictation pipeline as stages.
//!
//! `Session::transcribed()` in `dictation.rs` still owns the whole pipeline.
//! Nothing here is called from production yet. This module defines the stage
//! boundaries, the context they share, and the trace they leave behind, so a
//! later slice can move one stage at a time without redesigning the types.
//!
//! Stage status (live = real logic, callable in isolation; anything else is
//! documented at the function):
//!
//! | Stage | Status | Notes |
//! |---|---|---|
//! | `snapshots` | LIVE | read-only clones of every `Repo` via `.get()` |
//! | `app_context` | LIVE | returns the context resolved at `new()` |
//! | `asr_input` | STUB | types only; the audio/ASR call stays in `dictation.rs` |
//! | `itn` | LIVE | pure; mirrors `Pipeline::run` step 0.6 exactly |
//! | `dictionary_packs` | PARTIAL | off/non-voice short-circuit is live; the correction tier stays inside `Pipeline::run` |
//! | `autotext` | PASS-THROUGH | identity; protect/restore brackets the transform inside `Pipeline::run` |
//! | `transform` | LIVE | calls `teletype_core::Pipeline::run`; LLM-failure fallback-to-input preserved |
//! | `format` | PASS-THROUGH | identity; reserved for post-transform shaping |
//! | `deliver` | STUB | types only; D011: the controller owns injection, the worker never injects |
//! | `record` | LIVE | `set_snapshot` + persist handle only, never direct disk |
//!
//! Two deliberate non-moves, both load-bearing:
//!
//! - `record()` does NOT append the day-wise transcript file
//!   (`append_transcript_file` does direct disk I/O on the worker today) and
//!   does NOT durably persist dictionary usage counts (`Repo::update` writes
//!   through `JsonStore`, and the persist actor has no dictionary message).
//!   Dictionary counts are applied to the in-memory snapshot only. Both stay
//!   in `transcribed()` until a later slice.
//! - `transform()` intentionally reuses `Pipeline::run` wholesale, which
//!   still runs its own internal ITN/dictionary/AutoText steps. The
//!   standalone `itn` / `dictionary_packs` / `autotext` stages must NOT be
//!   chained in front of `transform()` until `Pipeline::run` is factored to
//!   take pre-processed input otherwise every step would run twice.
//!
//! Dead code below is expected: the stages are built before they are wired.
//! The `allow` keeps the unwired slice clippy-clean until the wiring slice
//! lands, at which point it should be removed.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde::Serialize;

use teletype_core::{
    autotext::{AutoTextEntry, AutoTextStore},
    context::ApplicationContext,
    delivery::RecordTarget,
    dictionary::{Dictionary, EditDistanceChecker, FloorChecker, WordChecker, EDIT_DISTANCE_CHECKER},
    history::DictationEntry,
    personalization::{resolve_language, UserProfile},
    pipeline::{InputSource, Pipeline, UnifiedInput},
    platform::Platform,
    storage::now_ms,
    style::StyleProfileStore,
    transforms::{TransformDefinition, TransformStore},
    usage::{count_autotext, count_fillers},
};

use crate::{commands, AppState};

// ── Errors, spans, traces ─────────────────────────────────────────────

/// One stage's failure. `stage` is a static stage name (`"transform"`,
/// `"deliver"`, …); `code` is machine-readable (`"not_wired"`,
/// `"no_engine"`, …); `message` is the human sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageError {
    pub stage: &'static str,
    pub code: String,
    pub message: String,
}

impl StageError {
    pub fn new(stage: &'static str, code: &str, message: impl Into<String>) -> Self {
        Self {
            stage,
            code: code.to_string(),
            message: message.into(),
        }
    }

    /// The stage is typed but its side effect still lives in `dictation.rs`.
    pub fn not_wired(stage: &'static str) -> Self {
        Self {
            stage,
            code: "not_wired".to_string(),
            message: format!("stage '{stage}' is not wired yet; transcribed() still owns this step"),
        }
    }
}

impl std::fmt::Display for StageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}:{}] {}", self.stage, self.code, self.message)
    }
}

impl std::error::Error for StageError {}

/// One timed step in a take's trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageSpan {
    pub stage: &'static str,
    pub elapsed_ms: u64,
    pub detail: String,
}

/// The ordered spans for one session. Spans are appended in call order by
/// [`DictationTrace::record`]; there is no reordering or deduplication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationTrace {
    pub session_id: u64,
    pub spans: Vec<StageSpan>,
}

impl DictationTrace {
    pub fn new(session_id: u64) -> Self {
        Self {
            session_id,
            spans: Vec::new(),
        }
    }

    pub fn record(&mut self, stage: &'static str, elapsed_ms: u64, detail: impl Into<String>) {
        self.spans.push(StageSpan {
            stage,
            elapsed_ms,
            detail: detail.into(),
        });
    }
}

/// Bounded ring of recent traces. Cap is 20: enough to debug the last takes,
/// small enough to never matter for memory.
pub struct TraceStore {
    inner: Mutex<VecDeque<DictationTrace>>,
}

impl TraceStore {
    pub const CAP: usize = 20;

    pub fn new() -> Self {
        Self {
            inner: Mutex::new(VecDeque::new()),
        }
    }

    pub fn push(&self, trace: DictationTrace) {
        let mut guard = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while guard.len() >= Self::CAP {
            guard.pop_front();
        }
        guard.push_back(trace);
    }

    pub fn get(&self, session_id: u64) -> Option<DictationTrace> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|t| t.session_id == session_id)
            .cloned()
    }

    pub fn latest(&self) -> Option<DictationTrace> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .back()
            .cloned()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

impl Default for TraceStore {
    fn default() -> Self {
        Self::new()
    }
}

// ── Shared context ────────────────────────────────────────────────────

/// Everything the stages read, resolved once per take from `&AppState`.
///
/// All `Repo` reads are lock-free snapshots (`.get()`); the engine is the
/// lock-free `Arc` clone (`.get()`); `persist` is the non-blocking actor
/// handle. Nothing here touches disk. `app_ctx` is the frontmost app at
/// take time (mirrors `transcribed()`); `app_key` is its stable id for
/// tracing, falling back to the display name when no bundle id exists.
pub struct DictationContext<'a> {
    pub state: &'a AppState,
    pub session_id: u64,
    pub app_ctx: ApplicationContext,
    pub app_key: String,
    pub detected_language: Option<String>,
}

impl<'a> DictationContext<'a> {
    pub fn new(
        state: &'a AppState,
        session_id: u64,
        detected_language: Option<String>,
    ) -> Self {
        let app_ctx = state.platform.active_application().unwrap_or_default();
        let app_key = if app_ctx.application_id.is_empty() {
            app_ctx.application_name.clone()
        } else {
            app_ctx.application_id.clone()
        };
        Self {
            state,
            session_id,
            app_ctx,
            app_key,
            detected_language,
        }
    }

    /// The profile language for this take: snapshot, then the ASR-detected
    /// language wins, then the per-app override wins. Mirrors `transcribed()`.
    pub fn resolved_language(&self) -> String {
        let mut profile = (*self.state.profile.get()).clone();
        if let Some(detected) = &self.detected_language {
            profile.language.clone_from(detected);
        }
        resolve_language(&profile, &self.app_ctx)
    }
}

// ── snapshots: construct the owned Ctx ────────────────────────────────

/// The owned "Ctx" that `Pipeline::run` consumes: every store cloned once up
/// front so the (up to 20 s) LLM call never holds a snapshot longer than it
/// must. Mirrors the clone block in `transcribed()`.
pub struct SnapshotBundle {
    pub settings: commands::Settings,
    pub autotext: AutoTextStore,
    pub transforms: TransformStore,
    pub profile: UserProfile,
    pub styles: StyleProfileStore,
    pub dictionary: Dictionary,
    pub pack_terms: Vec<teletype_core::vocab::PackTerm>,
    pub system_entries: Vec<AutoTextEntry>,
    pub engine: Option<Arc<dyn teletype_core::llm::InferenceProvider>>,
    pub active_style: String,
    pub explicit_style: String,
    pub strictness_floor: Option<f32>,
}

/// LIVE. Clones every snapshot `transcribed()` consumes. Read-only:
/// nothing is written back here.
pub fn stage_snapshots(ctx: &DictationContext, _input: ()) -> Result<SnapshotBundle, StageError> {
    let state = ctx.state;
    let settings = state.settings();
    let autotext = (*state.autotext.get()).clone();
    let transforms = (*state.transforms.get()).clone();
    let mut profile = (*state.profile.get()).clone();
    if let Some(detected) = &ctx.detected_language {
        profile.language.clone_from(detected);
    }
    let resolved = resolve_language(&profile, &ctx.app_ctx);
    if resolved != profile.language {
        profile.language = resolved;
    }
    let styles = (*state.styles.get()).clone();
    let dictionary = if settings.dictionary_enabled {
        (*state.dictionary.get()).clone()
    } else {
        Dictionary::default()
    };
    let mut pack_terms = teletype_core::vocab::terms_for(&settings.enabled_packs);
    pack_terms.extend(state.user_packs.get().terms_for());
    // BUG-002: the spoken-punctuation gate filters the System entries the
    // pipeline sees; custom AutoText is unaffected.
    let system_entries: Vec<AutoTextEntry> = if settings.spoken_punctuation {
        teletype_core::autotext::system::entries().to_vec()
    } else {
        teletype_core::autotext::system::entries()
            .iter()
            .filter(|e| {
                !teletype_core::autotext::system::punctuation_entries()
                    .iter()
                    .any(|p| p.snippet.trim() == e.snippet.trim())
            })
            .cloned()
            .collect()
    };
    let engine = state.inference.get();
    let active_style = settings.active_style_profile.clone();
    let strictness_floor = commands::match_strictness_floor(settings.match_strictness);
    Ok(SnapshotBundle {
        settings,
        autotext,
        transforms,
        profile,
        styles,
        dictionary,
        pack_terms,
        system_entries,
        engine,
        active_style,
        explicit_style: String::new(),
        strictness_floor,
    })
}

// ── app_context ───────────────────────────────────────────────────────

/// LIVE. Returns the frontmost-app context resolved when the
/// `DictationContext` was built. Pure read, no platform call here.
pub fn stage_app_context(ctx: &DictationContext, _input: ()) -> Result<ApplicationContext, StageError> {
    Ok(ctx.app_ctx.clone())
}

// ── asr_input: types only ─────────────────────────────────────────────

/// What the ASR step needs. The PCM never crosses this boundary: the
/// transcribe worker in `dictation.rs` keeps owning the audio buffer and the
/// speech-model lock until a later slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsrRequest {
    pub session_id: u64,
    pub language: String,
}

/// What the ASR step produces for the downstream stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsrTranscript {
    pub text: String,
    pub detected_language: Option<String>,
}

/// STUB. The `transcribe()` call (audio finish + resample + model lock +
/// peak-based no-audio distinction) stays on the transcribe worker in
/// `dictation.rs` until the wiring slice moves it.
pub fn stage_asr_input(
    _ctx: &DictationContext,
    _request: AsrRequest,
) -> Result<AsrTranscript, StageError> {
    Err(StageError::not_wired("asr_input"))
}

// ── itn ───────────────────────────────────────────────────────────────

/// Input for the deterministic ITN step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItnInput {
    pub text: String,
    pub is_voice: bool,
}

/// LIVE and pure. Mirrors `Pipeline::run` step 0.6 exactly: voice-only,
/// English-gated (`itn::should_run`), otherwise identity.
pub fn stage_itn(ctx: &DictationContext, input: ItnInput) -> Result<String, StageError> {
    if !input.is_voice {
        return Ok(input.text);
    }
    let language = ctx.resolved_language();
    if teletype_core::itn::should_run(&language) {
        Ok(teletype_core::itn::normalize(&input.text))
    } else {
        Ok(input.text)
    }
}

// ── dictionary_packs ──────────────────────────────────────────────────

/// Input for the dictionary + vocabulary-pack correction tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryInput {
    pub text: String,
    pub is_voice: bool,
}

/// PARTIAL. The dictionary-off / non-voice short-circuit is live and pure;
/// the correction itself still runs inside `Pipeline::run` step 0 (which
/// owns the `WordChecker` wiring and the pack fuzzy tier), so any other
/// input returns `not_wired`. Do not chain this in front of `transform()`
/// yet: that would correct twice once this stage goes fully live.
pub fn stage_dictionary_packs(
    ctx: &DictationContext,
    input: DictionaryInput,
) -> Result<String, StageError> {
    if !input.is_voice {
        return Ok(input.text);
    }
    if !ctx.state.settings().dictionary_enabled {
        return Ok(input.text);
    }
    Err(StageError::not_wired("dictionary_packs"))
}

// ── autotext ──────────────────────────────────────────────────────────

/// Input for the AutoText step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutotextInput {
    pub text: String,
    pub is_voice: bool,
}

/// PASS-THROUGH (identity). AutoText protect/restore must bracket the
/// transform, and both halves live inside `Pipeline::run` today; splitting
/// them out is a later slice. Until then this stage is the identity
/// function so no trigger can expand twice.
pub fn stage_autotext(
    _ctx: &DictationContext,
    input: AutotextInput,
) -> Result<String, StageError> {
    Ok(input.text)
}

// ── transform ─────────────────────────────────────────────────────────

/// Input for the transform step: the owned snapshots plus the text.
pub struct TransformInput {
    pub snapshots: SnapshotBundle,
    pub text: String,
    pub source: InputSource,
    pub explicit_transform: Option<TransformDefinition>,
}

/// Output of the transform step: the text plus the flags the caller must
/// surface (the no-model fallback is never silent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransformOutput {
    pub final_text: String,
    pub raw_input: String,
    pub transformed: bool,
    pub transform_skipped_no_model: bool,
}

/// LIVE. Runs the existing `teletype_core::Pipeline::run` with `token_sink:
/// None` (no pill streaming in the staged path yet). The data-safety
/// contract is unchanged: an LLM failure falls back to the input, never an
/// error that loses the user's words.
///
/// One deliberate difference from `transcribed()`: when no inference engine
/// is loaded, `transcribed()` returns early, while this stage passes
/// `inference: None` through so the caller sees the `NoModelLoaded` skip
/// reason via `transform_skipped_no_model` instead of silence.
pub fn stage_transform(
    ctx: &DictationContext,
    input: TransformInput,
) -> Result<TransformOutput, StageError> {
    let platform: &dyn Platform = ctx.state.platform.as_ref();
    let snapshots = input.snapshots;
    let engine_ref: Option<&dyn teletype_core::llm::InferenceProvider> =
        snapshots.engine.as_deref();
    // Dictionary strictness is a real dial, so the floor has to reach the
    // checker. "standard" resolves to None and keeps the pipeline's own
    // default rather than a number restated here.
    let strictness_checker =
        FloorChecker::new(snapshots.strictness_floor.unwrap_or(EditDistanceChecker::FLOOR));
    let word_checker: &dyn WordChecker = match snapshots.strictness_floor {
        None => &EDIT_DISTANCE_CHECKER,
        Some(_) => &strictness_checker,
    };
    let mut pipeline = Pipeline {
        platform,
        autotext: &snapshots.autotext,
        transforms: &snapshots.transforms,
        profile: &snapshots.profile,
        inference: engine_ref,
        dictionary: &snapshots.dictionary,
        word_checker,
        styles: &snapshots.styles,
        active_style: &snapshots.active_style,
        explicit_style: snapshots.explicit_style.as_str(),
        auto_apply: snapshots.settings.auto_apply_transform,
        restore_clipboard: snapshots.settings.restore_clipboard,
        remove_filler_words: snapshots.settings.remove_filler_words,
        filler_words: snapshots.settings.filler_words.clone(),
        restore_emoji: snapshots.settings.restore_emoji,
        spoken_emoji: snapshots.settings.spoken_emoji,
        spoken_punctuation: snapshots.settings.spoken_punctuation,
        system_autotext: &snapshots.system_entries,
        token_sink: None,
        polish_gate_enabled: snapshots.settings.polish_gate_enabled,
        polish_gate_threshold_words: snapshots.settings.polish_gate_threshold_words,
        pack_terms: &snapshots.pack_terms,
        list_style: snapshots.settings.list_style.resolve(),
    };
    let unified = UnifiedInput {
        source: input.source,
        text: input.text,
    };
    let result = pipeline.run(unified, input.explicit_transform.as_ref());
    Ok(TransformOutput {
        final_text: result.final_text,
        raw_input: result.raw_input,
        transformed: result.transformed,
        transform_skipped_no_model: result.transform_skipped_no_model,
    })
}

// ── format ────────────────────────────────────────────────────────────

/// Input for the post-transform shaping step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatInput {
    pub text: String,
}

/// PASS-THROUGH (identity). Reserved for post-transform shaping that does
/// not exist yet. Smart insertion is NOT this stage: it reads the live
/// accessibility tree at inject time and stays in the controller's
/// injection path (`pipeline_ready`).
pub fn stage_format(_ctx: &DictationContext, input: FormatInput) -> Result<String, StageError> {
    Ok(input.text)
}

// ── deliver: types only ───────────────────────────────────────────────

/// Everything the delivery decision needs. Prepared by the worker, judged
/// by the controller.
#[derive(Debug, Clone)]
pub struct DeliverInput {
    pub final_text: String,
    pub context: ApplicationContext,
    pub record_target: Option<RecordTarget>,
}

/// The controller's verdict. Only constructed once `deliver` is wired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliverDecision {
    pub proceed: bool,
    pub reason: String,
}

/// STUB. D011: the controller owns the only user-visible side effect. The
/// worker never injects; it ships the payload and the controller verdicts
/// on its FIFO queue (`pipeline_ready`). Moving injection here would break
/// the cancellation guarantee, so this stage stays typed-but-unwired until
/// the delivery gate itself is factored out (never the injection call).
pub fn stage_deliver(
    _ctx: &DictationContext,
    _input: DeliverInput,
) -> Result<DeliverDecision, StageError> {
    Err(StageError::not_wired("deliver"))
}

// ── record ────────────────────────────────────────────────────────────

/// Input for the bookkeeping step.
#[derive(Debug, Clone)]
pub struct RecordInput {
    pub final_text: String,
    pub raw_input: String,
    pub context: ApplicationContext,
    pub speech_ms: Option<u64>,
}

/// Output of the bookkeeping step: the history entry id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOutput {
    pub entry_id: String,
}

/// LIVE. Mirrors the worker bookkeeping in `transcribed()`: usage counts
/// and the history entry go to the in-memory snapshots via `set_snapshot`
/// and to disk via the background persist actor. Never direct disk I/O.
///
/// Explicitly NOT moved here (see module docs): the day-wise transcript
/// file append and the durable dictionary-usage write stay in
/// `transcribed()` until a later slice.
pub fn stage_record(ctx: &DictationContext, input: RecordInput) -> Result<RecordOutput, StageError> {
    let state = ctx.state;
    let settings = state.settings();

    // Usage counts, from the raw transcript (before cleanup) and the
    // autotext store — same sources as `transcribed()`.
    {
        let autotext = state.autotext.get();
        let filler_counts = if settings.remove_filler_words {
            count_fillers(&input.raw_input, &settings.filler_words)
        } else {
            std::collections::BTreeMap::new()
        };
        let autotext_counts = count_autotext(&input.raw_input, &autotext, &input.context);
        let mut next = (*state.usage.get()).clone();
        next.record(&filler_counts, &autotext_counts);
        state.usage.set_snapshot(next);
        state.persist.usage(filler_counts, autotext_counts);
    }

    // History entry (always, so the take is recoverable even when there
    // was no text field to paste into).
    let entry = DictationEntry {
        id: uuid::Uuid::new_v4().to_string(),
        created_at: now_ms(),
        text: input.final_text,
        context: Some(input.context),
        duration_ms: input.speech_ms,
    };
    {
        let mut next = (*state.history.get()).clone();
        next.push(entry.clone());
        state.history.set_snapshot(next);
        state.persist.history(entry.clone());
    }

    // Dictionary usage counts: in-memory snapshot only. The durable write
    // stays in `transcribed()` (see module docs).
    if settings.dictionary_enabled {
        let mut dict = (*state.dictionary.get()).clone();
        if dict.record_usage(&entry.text) > 0 {
            state.dictionary.set_snapshot(dict);
        }
    }

    Ok(RecordOutput { entry_id: entry.id })
}

// ── tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_error_codes_are_preserved() {
        let e = StageError::new("transform", "no_engine", "no model loaded");
        assert_eq!(e.stage, "transform");
        assert_eq!(e.code, "no_engine");
        assert_eq!(e.message, "no model loaded");
        assert!(e.to_string().contains("transform"));
        assert!(e.to_string().contains("no_engine"));
    }

    #[test]
    fn not_wired_error_contract() {
        for stage in ["asr_input", "dictionary_packs", "deliver"] {
            let e = StageError::not_wired(stage);
            assert_eq!(e.stage, stage);
            assert_eq!(e.code, "not_wired");
            assert!(!e.message.is_empty(), "message must say something");
        }
    }

    #[test]
    fn trace_records_spans_in_call_order() {
        let mut trace = DictationTrace::new(7);
        trace.record("snapshots", 1, "ok");
        trace.record("itn", 0, "skipped: typed");
        trace.record("transform", 42, "fell_back=false");
        let stages: Vec<&str> = trace.spans.iter().map(|s| s.stage).collect();
        assert_eq!(stages, vec!["snapshots", "itn", "transform"]);
        assert_eq!(trace.spans[2].elapsed_ms, 42);
        assert_eq!(trace.spans[2].detail, "fell_back=false");
    }

    #[test]
    fn trace_store_caps_at_20_and_serves_get_and_latest() {
        let store = TraceStore::new();
        for id in 0..25u64 {
            let mut t = DictationTrace::new(id);
            t.record("itn", 0, "ok");
            store.push(t);
        }
        assert_eq!(store.len(), TraceStore::CAP);
        // The first five were evicted.
        assert!(store.get(0).is_none());
        assert!(store.get(4).is_none());
        // The survivors are intact and ordered by push.
        let t5 = store.get(5).expect("session 5 must survive");
        assert_eq!(t5.session_id, 5);
        assert_eq!(t5.spans.len(), 1);
        let latest = store.latest().expect("latest must exist");
        assert_eq!(latest.session_id, 24);
    }

    #[test]
    fn trace_store_latest_is_none_when_empty() {
        let store = TraceStore::default();
        assert!(store.latest().is_none());
        assert!(store.get(1).is_none());
    }

    #[test]
    fn itn_stage_is_idempotent_spot_check() {
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 1, None);
        let input = ItnInput {
            text: "meet me at ninety four".to_string(),
            is_voice: true,
        };
        let once = stage_itn(&ctx, input).expect("itn stage is live");
        assert_eq!(once, teletype_core::itn::normalize("meet me at ninety four"));
        assert!(once.contains("94"), "number words must normalize: {once}");
        // Idempotence: normalizing the output changes nothing.
        let twice = stage_itn(
            &ctx,
            ItnInput {
                text: once.clone(),
                is_voice: true,
            },
        )
        .expect("itn stage is live");
        assert_eq!(twice, once, "ITN must be a fixed point");
    }

    #[test]
    fn itn_stage_passes_typed_input_through() {
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 1, None);
        let text = "meet me at ninety four".to_string();
        let out = stage_itn(&ctx, ItnInput { text: text.clone(), is_voice: false })
            .expect("typed input is identity");
        assert_eq!(out, text);
    }

    #[test]
    fn dictionary_stage_short_circuits_when_disabled_or_typed() {
        let state = AppState::for_tests();
        state
            .settings
            .update(|s| s.dictionary_enabled = false)
            .expect("test settings write");
        let ctx = DictationContext::new(&state, 1, None);
        // Disabled: voice input passes through untouched.
        let text = "some misheard proper noun here".to_string();
        let out = stage_dictionary_packs(
            &ctx,
            DictionaryInput { text: text.clone(), is_voice: true },
        )
        .expect("dictionary-off must short-circuit");
        assert_eq!(out, text);
        // Non-voice passes through even when enabled.
        state
            .settings
            .update(|s| s.dictionary_enabled = true)
            .expect("test settings write");
        let out = stage_dictionary_packs(&ctx, DictionaryInput { text: text.clone(), is_voice: false })
            .expect("typed input must short-circuit");
        assert_eq!(out, text);
        // Enabled + voice: the correction still lives in Pipeline::run.
        let err = stage_dictionary_packs(&ctx, DictionaryInput { text, is_voice: true })
            .expect_err("enabled voice correction is not wired");
        assert_eq!(err.code, "not_wired");
        assert_eq!(err.stage, "dictionary_packs");
    }

    #[test]
    fn passthrough_stages_are_identity() {
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 1, None);
        let text = "keep /email exactly as written".to_string();
        let out = stage_autotext(
            &ctx,
            AutotextInput { text: text.clone(), is_voice: true },
        )
        .expect("autotext is identity until factored");
        assert_eq!(out, text);
        let out = stage_format(&ctx, FormatInput { text: text.clone() })
            .expect("format is identity until shaped");
        assert_eq!(out, text);
    }

    #[test]
    fn stub_stages_return_not_wired() {
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 1, None);
        let err = stage_asr_input(&ctx, AsrRequest { session_id: 1, language: "en".into() })
            .expect_err("asr_input must stay stubbed");
        assert_eq!(err, StageError::not_wired("asr_input"));
        let err = stage_deliver(
            &ctx,
            DeliverInput {
                final_text: "hello".into(),
                context: ApplicationContext::default(),
                record_target: None,
            },
        )
        .expect_err("deliver must stay stubbed");
        assert_eq!(err.code, "not_wired");
        assert_eq!(err.stage, "deliver");
    }

    #[test]
    fn snapshots_stage_clones_everything_transcribed_consumes() {
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 1, Some("de".into()));
        let bundle = stage_snapshots(&ctx, ()).expect("snapshots stage is live");
        // Detected language flows into the profile, like transcribed().
        assert_eq!(bundle.profile.language, "de");
        // Fresh test stores: empty autotext/dictionary, built-in transforms.
        assert!(bundle.autotext.entries.is_empty());
        assert!(bundle.dictionary.words.is_empty());
        assert!(!bundle.transforms.transforms.is_empty());
        // No engine in tests: None flows through (the NoModelLoaded path).
        assert!(bundle.engine.is_none());
        assert!(bundle.explicit_style.is_empty());
    }

    #[test]
    fn app_context_stage_returns_the_take_time_context() {
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 3, None);
        let app_ctx = stage_app_context(&ctx, ()).expect("app_context is live");
        assert_eq!(app_ctx, ctx.app_ctx);
    }

    #[test]
    fn transform_stage_flags_no_model_without_losing_words() {
        // No engine installed in tests: the pipeline must fall back to the
        // input (AutoText-only) and say so via the flag, never an error.
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 1, None);
        let bundle = stage_snapshots(&ctx, ()).expect("snapshots");
        let out = stage_transform(
            &ctx,
            TransformInput {
                snapshots: bundle,
                text: "my important words".to_string(),
                source: InputSource::Voice,
                explicit_transform: None,
            },
        )
        .expect("transform stage is live even with no model");
        assert_eq!(out.raw_input, "my important words");
        assert_eq!(out.final_text, "my important words");
        assert!(!out.transformed);
    }

    #[test]
    fn transform_stage_with_mock_engine_polishes() {
        struct MockLlm;
        impl teletype_core::llm::InferenceProvider for MockLlm {
            fn model_id(&self) -> &str {
                "mock"
            }
            fn model_name(&self) -> &str {
                "mock"
            }
            fn is_local(&self) -> bool {
                true
            }
            fn generate(
                &self,
                _prompt: &str,
                _params: teletype_core::llm::GenerationParams,
            ) -> Result<String, String> {
                // Overlaps the input words on purpose: the transform
                // validator rejects off-topic output, and this test is about
                // the stage plumbing, not the validator.
                Ok("Polish this sentence please, it is quite rough.".to_string())
            }
        }
        let state = AppState::for_tests();
        state
            .inference
            .set(Arc::new(MockLlm) as Arc<dyn teletype_core::llm::InferenceProvider>);
        // The deterministic polish gate would skip this short clean input;
        // switch it off so the test exercises the LLM path.
        state
            .settings
            .update(|s| s.polish_gate_enabled = false)
            .expect("test settings write");
        let ctx = DictationContext::new(&state, 1, None);
        let bundle = stage_snapshots(&ctx, ()).expect("snapshots");
        assert!(bundle.engine.is_some());
        let out = stage_transform(
            &ctx,
            TransformInput {
                snapshots: bundle,
                text: "polish this sentence please it is quite rough".to_string(),
                source: InputSource::Voice,
                explicit_transform: None,
            },
        )
        .expect("transform with engine");
        assert!(out.transformed, "mock engine output must count as transformed");
        assert_eq!(out.final_text, "Polish this sentence please, it is quite rough.");
        assert!(!out.transform_skipped_no_model);
    }

    #[test]
    fn transform_stage_preserves_input_on_llm_failure() {
        struct FailLlm;
        impl teletype_core::llm::InferenceProvider for FailLlm {
            fn model_id(&self) -> &str {
                "fail"
            }
            fn model_name(&self) -> &str {
                "fail"
            }
            fn generate(
                &self,
                _prompt: &str,
                _params: teletype_core::llm::GenerationParams,
            ) -> Result<String, String> {
                Err("model crashed".into())
            }
        }
        let state = AppState::for_tests();
        state
            .inference
            .set(Arc::new(FailLlm) as Arc<dyn teletype_core::llm::InferenceProvider>);
        let ctx = DictationContext::new(&state, 1, None);
        let bundle = stage_snapshots(&ctx, ()).expect("snapshots");
        let out = stage_transform(
            &ctx,
            TransformInput {
                snapshots: bundle,
                text: "my important words".to_string(),
                source: InputSource::Voice,
                explicit_transform: None,
            },
        )
        .expect("LLM failure must fall back, not error");
        assert_eq!(out.final_text, "my important words");
        assert!(!out.transformed);
    }

    #[test]
    fn record_stage_writes_snapshots_and_queues_persist() {
        let state = AppState::for_tests();
        let ctx = DictationContext::new(&state, 9, None);
        let before_history = state.history.get().entries.len();
        let out = stage_record(
            &ctx,
            RecordInput {
                final_text: "hello world".to_string(),
                raw_input: "hello world".to_string(),
                context: ApplicationContext::default(),
                speech_ms: Some(1200),
            },
        )
        .expect("record stage is live");
        assert!(!out.entry_id.is_empty());
        // In-memory snapshots are current for immediate reads.
        let history = state.history.get();
        assert_eq!(history.entries.len(), before_history + 1);
        let last = history.entries.last().expect("entry recorded");
        assert_eq!(last.id, out.entry_id);
        assert_eq!(last.text, "hello world");
        assert_eq!(last.duration_ms, Some(1200));
    }
}
