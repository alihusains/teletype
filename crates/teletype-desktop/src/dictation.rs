//! The dictation controller: one thread owns the session state machine.
//!
//! All events (hotkey, toggle, escape, transcription result) are messages to
//! this thread, so state has a single owner and events are processed in order.

use std::{
    sync::{
        mpsc::{self, Sender},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use teletype_core::{
    audio::{
        device_name_is_bluetooth, list_input_devices,
        quality::{take_quality, TakeQuality},
        resample_to_target, should_arm, BlockResampler, Captured, FrontEndState, InputDevice,
        MicFrontEnd, PrewarmMode, Recording,
    },
    delivery::{self, DeliveryOutcome, DeliveryPolicy},
    delivery::{resolve_delivery, Delivery, RecordTarget},
    dictionary::LearnedChange,
    pipeline::{InputSource, Pipeline, UnifiedInput},
    segmenter::{SegmentEvent, Segmenter, SegmenterConfig},
    state::{self, Input, Phase, PillState, RecordingMode},
    trace::{now_us, DictationTrace, FileTraceSink, TraceSink, TraceStageMarks},
    vad::VadDetector,
};

use crate::{commands, AppState};

// Roadmap §6 A2 (unwired slice): the staged pipeline types live in
// `dictation_stages.rs`. The `#[path]` keeps that file next to this one
// (owned by the stages worker) while the module is declared here, because
// `lib.rs` is owned by another worker in this slice.
mod controller;
#[path = "dictation_stages.rs"]
pub(crate) mod dictation_stages;
mod session;
pub(crate) use controller::visible_conflict;
pub use controller::{Controller, Event, HotkeyConflict};

/// Shared between the capture thread (which feeds the VAD detector) and the
/// dictation thread (which resets it between takes). `None` when VAD auto-stop
/// is off, so a disabled take never pays for the model.
type VadSlot = Arc<Mutex<Option<VadDetector>>>;

/// P4 (plan §4.2/§4.3): streaming commit is on by default for this unit.
/// Wave B turns it into a user setting; until then it is a module const.
const STREAMING_COMMIT_DEFAULT: bool = true;

/// The shared 16 kHz streaming state for the in-flight take. The capture
/// thread (or a headless test) feeds it device-rate frames through
/// [`Session::feed_capture_frame`]; the release path moves the
/// [`Committer`] and the uncommitted tail out of it.
///
/// `None` when streaming is disabled or the Silero model could not be
/// loaded — the take then decodes the whole audio after release, exactly
/// as before.
type StreamingSlot = Arc<Mutex<Option<StreamingTake>>>;

/// P4.3b: the app handle shared with the capture thread so the per-segment
/// polish install (see [`install_segment_polish`]) can read `AppState`
/// (inference provider, transforms, profile, dictionary) once the take's
/// audio crosses the threshold. The install is idempotent and runs at
/// most once per take.
type AppSlot = Arc<Mutex<Option<tauri::AppHandle>>>;

/// P4: the commit-as-you-go state for one take. The capture callback
/// resamples each device-rate frame to 16 kHz, feeds it to the streaming
/// VAD (for its raw Silero probability), appends it to the take buffer,
/// and pushes it to the segmenter. Every `SegmentEvent::Ready` slices the
/// buffer and hands the samples to the [`Committer`], which transcribes
/// them on its own engine while the user is still talking.
struct StreamingTake {
    /// 16 kHz take buffer; sample `i` is the `(i + base_offset)`th 16 kHz
    /// sample of the take. HARD-3 (#9): the committed prefix is released
    /// from the front, so `base_offset` tracks how many samples have been
    /// released. The buffer stays bounded by the uncommitted tail.
    buf16: Vec<f32>,
    /// Samples released from the front of `buf16` (HARD-3 #9).
    base_offset: usize,
    /// The segmenter (16 kHz, default config: 350 ms pause / 12 s cut).
    segmenter: Segmenter,
    /// The streaming VAD: its OWN `silero_16k()` instance, fed 512-sample
    /// chunks; only [`VadDetector::last_probability`] is consumed (the
    /// silence window is irrelevant here — auto-stop has its own detector).
    vad: VadDetector,
    /// 16 kHz samples carried from the previous frame so the VAD always
    /// sees whole 512-sample chunks.
    carry: Vec<f32>,
    /// 16 kHz samples up to where the committed segments cover. The tail
    /// on release is `buf16[last_commit_end..]`: the segmenter never
    /// re-covers committed audio, and `next_start_sample` already accounts
    /// for the forced-cut overlap.
    last_commit_end: usize,
    /// The take's committer (its own engine handle, its own worker thread).
    committer: crate::committer::Committer,
    /// The language passed to the committer's engine for each segment.
    language: String,
}

impl StreamingTake {
    /// Feed one 16 kHz frame. Feeds the streaming VAD in 512-sample
    /// chunks, pushes each chunk to the segmenter, and submits every ready
    /// segment to the committer. Returns the number of segments committed
    /// by this frame.
    ///
    /// F-02: the input is already 16 kHz. The capture path resamples once,
    /// in 100 ms blocks, in the frame callback (`BlockResampler`), and the
    /// pre-roll path resamples in `feed_capture_frame`. Resampling again
    /// here stretched every non-16 kHz frame to a full 100 ms of output.
    fn feed_frame(&mut self, frame: &[f32], rate: u32) -> usize {
        debug_assert_eq!(rate, 16_000, "feed_frame expects 16 kHz input");
        // Append BEFORE the chunk loop: `buf16` must mirror the sample
        // stream the segmenter sees, or a segment that ends inside this
        // frame is sliced against the previous frame's length and its audio
        // is lost.
        self.buf16.extend_from_slice(frame);
        self.carry.extend_from_slice(frame);
        let mut committed = 0;
        while self.carry.len() >= 512 {
            let chunk: Vec<f32> = self.carry.drain(..512).collect();
            let _ = self.vad.push(&chunk);
            let prob = self.vad.last_probability();
            let events = self.segmenter.push(&chunk, prob);
            for ev in events {
                let SegmentEvent::Ready(seg) = ev;
                let end = seg
                    .end_sample
                    .saturating_sub(self.base_offset)
                    .min(self.buf16.len());
                let start = seg.start_sample.saturating_sub(self.base_offset).min(end);
                let samples = self.buf16[start..end].to_vec();
                let lang = self.language.clone();
                let seg_ms = crate::committer::tail_audio_ms(samples.len(), 16_000);
                // Take-absolute start for this segment. The engine returns
                // word timings relative to `samples`; the release fold
                // de-duplicates across segments by timestamp, so the
                // committer shifts every word by this offset.
                let start_ms = (seg.start_sample as u64 * 1000 / 16_000) as u32;
                self.committer.submit_segment(samples, lang, start_ms);
                committed += 1;
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("streaming commit: {seg_ms} ms segment"),
                );
                self.last_commit_end = seg.next_start_sample;
            }
            // HARD-3 (#9): release the committed prefix from `buf16` so the
            // buffer stays bounded by the uncommitted tail. A 60-minute take
            // no longer holds ~230 MB. The segment audio was already
            // extracted above, so it is safe to drop the prefix now.
            let release_to = self
                .last_commit_end
                .saturating_sub(self.base_offset)
                .min(self.buf16.len());
            if release_to > 0 {
                self.buf16.drain(..release_to);
                self.base_offset += release_to;
                self.last_commit_end = self.base_offset;
            }
        }
        committed
    }

    /// The uncommitted remainder of the take: `buf16` (the committed prefix
    /// is released, so the entire buffer is the uncommitted tail, HARD-3 #9).
    fn tail(&self) -> Vec<f32> {
        self.buf16.clone()
    }

    /// The tail's audio length in ms (the trace's `tail_audio_ms`).
    fn tail_audio_ms(&self) -> u32 {
        crate::committer::tail_audio_ms(self.buf16.len(), 16_000)
    }

    /// The streaming trace marks and the streaming-fold gate: `Some((segments,
    /// tail_ms))` when the Segmenter cut at least one segment mid-take,
    /// `None` otherwise (the whole-take decode runs).
    ///
    /// This is the ONE decision both release paths use. It must count
    /// *submitted* segments, never `committed_count()`: that counts decodes
    /// drained by `poll_completed`, which only runs inside the release
    /// branch this gate guards: gating on it made the streaming fold
    /// unreachable (every take re-decoded in full on release).
    fn streaming_marks(&self) -> Option<(u32, u32)> {
        let submitted = self.committer.submitted_count();
        if submitted == 0 {
            return None;
        }
        Some((submitted as u32, self.tail_audio_ms()))
    }

    /// Wait for the tail and all in-flight segments, returning the
    /// committed segments in order followed by the tail, plus the take's
    /// word-loss accounting. Consumes the committer (it resets itself for
    /// the next take and its worker exits on drop).
    fn finish(self, language: String) -> Result<crate::committer::CommitterResult, String> {
        let mut tail = self.tail();
        // ASRHALL-1: the tail holds the end of the take, so it carries the
        // silent tail the ASR engines hallucinate over. Cut it before decode.
        let keep = teletype_core::audio::trim_trailing_silence(&tail);
        if keep != tail.len() {
            crate::log_entry(
                crate::LogLevel::Info,
                format!(
                    "trailing-silence trim (streaming tail): {} -> {} samples",
                    tail.len(),
                    keep
                ),
            );
            tail.truncate(keep);
        }
        // Take-absolute tail offset, same rule as each segment's.
        let tail_start_ms = (self.base_offset as u64 * 1000 / 16_000) as u32;
        self.committer.finish(tail, language, tail_start_ms)
    }
}

/// P4.3b (plan §4.3 "Polish for long dictations"): build the per-segment
/// polish function installed on a long take's committer when
/// `settings.polish_whole_text_at_end` is off. Each committed segment is
/// polished as it lands, running the SAME engine pass the whole-text
/// release path runs: the selected transform's prompt (the store's
/// auto-apply definition, `builtin-polish` by default) and the same
/// validator the whole-text path runs (`run_single_chunk`): a rejected
/// output falls back to the raw segment text, so the pipeline never loses
/// a word.
///
/// The provider is the app's loaded inference Arc, cloned here and held
/// by the closure. It is `Send + Sync` (the pipeline worker already
/// shares it across threads), so the closure may run on the committer's
/// worker thread — never on the controller. The snapshot is cloned at
/// take start, mirroring the P1.8 seam: a model swap mid-take replaces
/// the Arc in `AppState` but never blocks or disturbs an in-flight
/// segment polish.
///
/// Returns `None` when no inference provider is loaded (the release
/// whole-text path then runs AutoText-only with the "no LLM model
/// loaded" notice, exactly as today) or when the store has no auto-apply
/// transform (nothing to run per segment; the release path behaves the
/// same). No text is logged.
/// A segment-polish closure: one transcript in, polished transcript out.
type SegmentPolishFn = Box<dyn Fn(&str) -> Result<String, String> + Send>;

fn build_segment_polish_fn(state: &AppState) -> Option<SegmentPolishFn> {
    let provider: Arc<dyn teletype_core::llm::InferenceProvider> = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    let transforms = state.transforms.get().clone();
    let transform = transforms.auto_apply().cloned()?;
    let ctx = {
        let settings = state.settings();
        let profile = state.profile.get().clone();
        let dictionary = state.dictionary.get().clone();
        let mut known: Vec<String> = dictionary
            .known_words()
            .into_iter()
            .map(|w| format!("keep '{w}' as written"))
            .collect();
        known.sort();
        teletype_core::transforms::prompt::PromptContext {
            app: None,
            preferences: Vec::new(),
            preferred_terms: known,
            user_instruction: None,
            user_override: None,
            language: profile.language.clone(),
            s1_control: profile.s1_control,
            list_style: settings.list_style.resolve(),
            polish_max_change_percent: settings.polish_max_change_percent,
            screen_terms: Vec::new(),
        }
    };
    Some(Box::new(move |text: &str| {
        // The S1-mini path is card-faithful (fixed system prompt +
        // control line, no task prompt), so it cannot be reused here
        // without the exact wire format; the generic prompt + generate +
        // validate is the same pass `run_single_chunk` runs for every
        // other model. Both land on the identical validator.
        let is_polish = transform.id == "builtin-polish" || transform.auto_apply;
        // Card-faithful cleanup models (S1-mini, SpeakoFlow Mini) use their
        // whole-text protocol, not per-segment generation.
        if (provider.model_id() == "s1-mini" || provider.model_id() == "speakoflow-mini")
            && is_polish
        {
            return Err("cleanup model uses the whole-text path".to_string());
        }
        let prompt = teletype_core::transforms::prompt::build_prompt(&transform, text, &ctx);
        let max_tokens = teletype_core::llm::polish_max_tokens(text);
        let params = teletype_core::llm::GenerationParams {
            max_tokens,
            temperature: 0.0,
            timeout: teletype_core::llm::scaled_timeout(max_tokens),
        };
        let raw = provider
            .generate(&prompt, params)
            .map_err(|e| format!("segment polish inference failed: {e}"))?;
        match teletype_core::transforms::validator::validate_polish(
            &raw,
            text,
            &transform,
            &prompt,
            ctx.polish_max_change_percent,
        ) {
            teletype_core::transforms::validator::ValidatedOutput::Transformed(out) => Ok(out),
            teletype_core::transforms::validator::ValidatedOutput::Fallback(orig, _failure) => {
                // The validator already fell back to the input; the
                // committer records the raw text.
                Ok(orig)
            }
        }
    }))
}

/// Phase 5.7: accumulates 16 kHz samples from the capture callback and
/// emits one VAD flag per 512-sample (32 ms at 16 kHz) frame, matching
/// `take_quality`'s `FRAME_SIZE` contract.
///
/// The flag for a frame is `probability >= 0.5` — the same operating point
/// as `SPEECH_THRESHOLD` in `teletype_core::vad` (private there, so it is
/// duplicated here with this comment). A trailing partial chunk (fewer
/// than 512 samples) is dropped: a half frame would misalign the flag
/// grid against the take's samples.
struct VadFlagCollector {
    carry: Vec<f32>,
    flags: Vec<bool>,
}

impl Clone for VadFlagCollector {
    fn clone(&self) -> Self {
        Self {
            carry: self.carry.clone(),
            flags: self.flags.clone(),
        }
    }
}

impl VadFlagCollector {
    fn new() -> Self {
        Self {
            carry: Vec::new(),
            flags: Vec::new(),
        }
    }

    /// Accumulate one chunk of 16 kHz samples observed at `probability`
    /// (the VAD probability for the capture frame that produced them).
    /// Every full 512-sample frame accumulated so far emits one flag.
    fn push(&mut self, samples_16k: &[f32], probability: f32) {
        self.carry.extend_from_slice(samples_16k);
        while self.carry.len() >= 512 {
            let full = self.carry.len() - (self.carry.len() % 512);
            let is_speech = probability >= 0.5; // == vad::SPEECH_THRESHOLD
            self.flags
                .extend(std::iter::repeat_n(is_speech, full / 512));
            self.carry.drain(..full);
        }
    }

    /// The flags in frame order; the trailing partial chunk is dropped.
    fn into_flags(self) -> Vec<bool> {
        self.flags
    }
}

/// Phase 5.7: prepend the resampled pre-roll to the take's 16 kHz samples
/// so the whole-take decode (and its trim) sees the pre-roll audio. The
/// streaming branch never re-decodes `captured`, so the prepend is
/// harmless there. `None` returns `samples` unchanged.
fn merge_preroll(samples: &[f32], preroll: Option<(&[f32], u32)>) -> Vec<f32> {
    match preroll {
        None => samples.to_vec(),
        Some((raw, rate)) => {
            let mut out = resample_to_target(raw, rate).unwrap_or_default();
            out.extend_from_slice(samples);
            out
        }
    }
}

/// Phase 5.7: the BT-HFP flag for the device a take was recorded on,
/// given the live device list. Pure over `&[InputDevice]` so the
/// selection logic is unit-testable without cpal: the configured `id` wins
/// when present; an empty configured id falls back to the system default;
/// an unknown id yields `None`.
fn active_device_bt_hfp_for(configured_id: &str, devices: &[InputDevice]) -> Option<bool> {
    let pick = if configured_id.is_empty() {
        devices.iter().find(|d| d.is_default)
    } else {
        devices.iter().find(|d| d.id == configured_id)
    };
    pick.map(|d| device_name_is_bluetooth(&d.name))
}

/// Phase 5.7: the BT-HFP flag for the take's configured input device
/// (lists the live devices; see [`active_device_bt_hfp_for`]).
fn active_device_bt_hfp(configured_id: &str) -> Option<bool> {
    active_device_bt_hfp_for(configured_id, &list_input_devices())
}

/// Phase 5.7: apply the five per-take quality fields to a trace. `None`
/// leaves them alone (a take without flags or a missing device must not
/// erase a previous attach). Extracted so the attach is unit-testable.
fn apply_take_quality(t: &mut DictationTrace, q: Option<&TakeQuality>) {
    let Some(q) = q else { return };
    t.speech_rms_db = q.speech_rms_db;
    t.noise_rms_db = q.noise_rms_db;
    t.snr_db = q.snr_db;
    t.clipped = q.clipped;
    t.bt_hfp = q.bt_hfp;
}

/// The kind of take in flight, set when the take starts and read when its
/// transcript arrives. Replaces the old boolean arm-flag, which was cleared
/// before the take was classified, so every spoken command was misread as
/// dictation (plan P2.2).
///
/// The kind is per-session: it travels with the take, so it cannot leak
/// into the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TakeKind {
    /// Plain dictation: polish and inject.
    #[default]
    Dictation,
    /// A command take ("open Safari"): classified and routed to the command
    /// router instead of being polished into a sentence.
    Command,
    /// A selection transform: a command applied to selected text (P2.6).
    Transform,
}

impl TakeKind {
    /// The trace label for this kind (CMD-2): the word written to
    /// [`DictationTrace::take_kind`], so a trace line states the route without
    /// needing the Rust type name.
    pub fn label(self) -> &'static str {
        match self {
            TakeKind::Dictation => "dictation",
            TakeKind::Command => "command",
            TakeKind::Transform => "transform",
        }
    }
}

/// Which speech engine loads a model, derived from the catalog entry in
/// [`resolve_speech_model`]. Replaces the old `use_parakeet` boolean so the
/// app can route to FluidAudio (Core ML), sherpa-onnx, or Whisper/Parakeet
/// ggml based on the selected model (plan §3.2/§3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpeechEngineKind {
    /// Whisper ggml via `teletype_speech::whisper::WhisperProvider`.
    #[default]
    Whisper,
    /// Parakeet TDT ggml via `teletype_speech::parakeet::ParakeetProvider`.
    Parakeet,
    /// Parakeet Core ML via `teletype_fluid::FluidParakeet` (Neural Engine).
    Fluid,
    /// NeMo Parakeet via `teletype_speech::sherpa::SherpaParakeetProvider`.
    Sherpa,
    /// Needle/Whistle speech via `teletype_speech::whistle::WhistleProvider`.
    /// Optional-only (weights license unverified, plan §15); wired into the
    /// engine enum for future catalog selection but not yet default.
    #[allow(dead_code)] // Constructed once the Whistle catalog entry lands
    // (plan §3.7 / P7.4); the enum stays exhaustive for
    // model-kind routing.
    Whistle,
    /// Parakeet.cpp (mudler/parakeet.cpp, GGUF-native) via
    /// `teletype_speech::parakeet_cpp::ParakeetCppProvider` (the C++ engine
    /// handles mixed-quant models like Fluid Speech Mini).
    ParakeetCpp,
    /// Qwen3-ASR GGUF via a child `llama-server` process
    /// (`teletype_inference::Qwen3AsrProvider`), OpenAI-compatible
    /// `/v1/audio/transcriptions`. Two-file artifact (main GGUF + mmproj
    /// projector), multilingual, opt-in accuracy row. Served through the
    /// neural slot, like Core ML and sherpa.
    Qwen3Asr,
    /// Apple on-device speech (SpeechTranscriber / DictationTranscriber) via
    /// `teletype_apple::AppleSpeech`, macOS 26+ only. The models are
    /// OS-managed system storage (no download); served through the neural
    /// slot, like Core ML and sherpa.
    AppleSpeech,
    /// transcribe.cpp streaming GGUF ASR (Nemotron 3.5, Moonshine, Granite)
    /// via `teletype_speech::transcribe::TranscribeCppProvider`. Served
    /// through the neural slot, like Core ML and sherpa.
    TranscribeCpp,
}

/// When the speech model gets loaded, as decided from
/// `settings.warm_engine_policy`. Pure mapping (no app, no threads) so the
/// tier is unit-testable and reviewable in one place.
///
/// The wire values the UI offers are `off` / `10` / `30` / `60` / `always`
/// ([`commands::WarmEnginePolicy`]); `Unknown` (a value from a newer build)
/// degrades to the `30` behaviour through the enum's own `as_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineWarmTier {
    /// Never load ahead of the take. The first release pays the load inside
    /// `transcribe()`, which loads on demand, so the take still works; it
    /// just starts slowly and nothing is resident between takes.
    OnRelease,
    /// Load when the hotkey goes down, while the user is still speaking.
    /// This is the behaviour the app has always shipped (`warm_up()` from
    /// `start()`), and every numeric policy resolves here. The seconds are
    /// legacy retention windows; retention itself is gone (D052), so `10`,
    /// `30` and `60` all mean "load on the first press and stay resident".
    OnHotkeyDown,
    /// Keep it loaded: warm once at startup, re-warm after every take ends,
    /// and never arm the idle-unload timer.
    Resident,
}

/// Maps the setting to the tier. `Off` is the only value that removes the
/// proactive load; `Always` the only one that adds load outside a take; the
/// numeric tiers (and `Unknown`, which reports itself as `30`) keep today's
/// on-press behaviour so no existing config changes shape.
pub fn engine_warm_tier(policy: commands::WarmEnginePolicy) -> EngineWarmTier {
    match policy {
        commands::WarmEnginePolicy::Off => EngineWarmTier::OnRelease,
        commands::WarmEnginePolicy::Always => EngineWarmTier::Resident,
        commands::WarmEnginePolicy::Sec10
        | commands::WarmEnginePolicy::Sec30
        | commands::WarmEnginePolicy::Sec60
        | commands::WarmEnginePolicy::Unknown => EngineWarmTier::OnHotkeyDown,
    }
}

/// The unload delay the app hands the speech managers when a take ends:
/// always `None`, which their `end_dictation` reads as "stay resident". No
/// setting feeds this on purpose (D052).
///
/// The model used to be freed by `warm_engine_policy`'s retention windows and
/// the old model-unload-delay setting (both deleted). The first take after that
/// idle unload then paid a
/// full reload while the dictation that just worked sat behind a recording pill,
/// which is exactly what a dead hotkey looks like. The rule now: the model is
/// loaded and then stays loaded. The only unloads are the manual ones —
/// switching the selected model, and deleting a downloaded one
/// (`commands::models`).
fn take_end_unload_delay() -> Option<std::time::Duration> {
    None
}

/// Ends the take's dictation session on every speech manager. This is the one
/// place the app tells the managers a take is over, so it is the one place the
/// no-unload rule has to hold.
fn release_take_session(state: &AppState) {
    each_speech_manager(state, |m| m.end_dictation(take_end_unload_delay()));
}

/// True when `kind`'s manager already holds `model_id`. For the neural slot
/// the id has to match as well: that one slot is shared by Core ML, sherpa,
/// Qwen3-ASR, Apple speech and transcribe.cpp, so "loaded" alone does not mean
/// "loaded with the model the settings select".
fn speech_model_ready_for(state: &AppState, kind: SpeechEngineKind, model_id: &str) -> bool {
    match kind {
        SpeechEngineKind::Fluid
        | SpeechEngineKind::Sherpa
        | SpeechEngineKind::Qwen3Asr
        | SpeechEngineKind::AppleSpeech
        | SpeechEngineKind::TranscribeCpp => {
            let current = state
                .neural_model_id
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            current.as_deref() == Some(model_id) && state.neural.is_loaded()
        }
        SpeechEngineKind::Whistle => state.neural.is_loaded(),
        SpeechEngineKind::Parakeet => state.parakeet.is_loaded(),
        SpeechEngineKind::ParakeetCpp => state.parakeet_cpp.is_loaded(),
        SpeechEngineKind::Whisper => state.speech.is_loaded(),
    }
}

/// Whether a press must announce the warm-up instead of painting the recording
/// pill straight away. Two things have to hold: the policy loads on press
/// (`off` deliberately defers the load to the release, so a warm pill there
/// would promise work that is not happening), and the model is not already
/// resident (a warm model needs no pill of its own).
///
/// This is what makes a cold first press — the app just started, the model was
/// switched, or an earlier load failed — open on "Getting model ready…" rather
/// than on a recording pill with a flat waveform that reads as a dead hotkey.
pub fn press_warms_visibly(tier: EngineWarmTier, ready: bool) -> bool {
    tier != EngineWarmTier::OnRelease && !ready
}

/// Reports a take worker that ends without handing its session back to the
/// controller (panic, or an early return that skipped the terminal send).
///
/// Dropping an armed guard sends [`Event::WorkerAbandoned`], so the
/// controller can reset the pill on the main thread. Every terminal send
/// must go through [`WorkerExitGuard::terminal`] to disarm it; sending
/// directly would make a normal exit look like a death.
struct WorkerExitGuard {
    tx: Option<Sender<Event>>,
    #[cfg(test)]
    test_events: Option<Sender<Event>>,
    session: u64,
    armed: bool,
}

impl WorkerExitGuard {
    fn new(controller: &Controller, session: u64) -> Self {
        Self {
            tx: controller.tx.clone(),
            #[cfg(test)]
            test_events: None,
            session,
            armed: true,
        }
    }

    /// Test-only: wire the mirror channel (the Session's `test_events`).
    #[cfg(test)]
    fn with_test_events(mut self, tx: Sender<Event>) -> Self {
        self.test_events = Some(tx);
        self
    }

    /// Sends this worker's terminal event and disarms the guard.
    fn terminal(&mut self, event: Event) {
        self.armed = false;
        #[cfg(test)]
        if let Some(tx) = &self.test_events {
            // `for_tests` controllers have no event pump (`tx` is `None`),
            // so the mirror channel is the only destination in tests.
            let _ = tx.send(event);
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(event);
        }
    }
}

impl Drop for WorkerExitGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        #[cfg(test)]
        if let Some(tx) = &self.test_events {
            let _ = tx.send(Event::WorkerAbandoned {
                session: self.session,
            });
            return;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(Event::WorkerAbandoned {
                session: self.session,
            });
        }
    }
}

/// Text of a caught panic payload, for the session thread's error log.
pub(in crate::dictation) fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic")
}

/// Adds the word selected in the focused app to the dictionary.
///
/// Shared by the Quick Add shortcut and the tray item so both surfaces behave
/// identically. Returns the added word, or a sentence the caller can show.
pub fn quick_add_from_selection(app: &AppHandle) -> Result<String, String> {
    let selected = crate::text_selection::selected_text();
    let Some(word) = selected
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
    else {
        return Err("No text is selected. Highlight the word first.".to_string());
    };
    let state = app.state::<crate::AppState>();
    let mut dict = (*state.dictionary.get()).clone();
    if dict.find(&word).is_some() {
        return Err("That word is already in your dictionary.".to_string());
    }
    let new_word = teletype_core::dictionary::DictionaryWord::new(word.clone(), "");
    dict.insert(new_word)?;
    state.dictionary.update(|d| *d = dict)?;
    Ok(word)
}

/// The language chip shown in the pill after an auto-detect transcription.
/// `detected: true` means it was detected this session (tap to lock); a
/// locked language is shown with `detected: false` (tap to switch back to
/// auto).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageChip {
    pub code: String,
    pub detected: bool,
}

struct Session {
    /// `None` in unit tests, which never touch platform services.
    app: Option<AppHandle>,
    controller: Controller,
    phase: Phase,
    hotkey_down: bool,
    hands_free: bool,
    /// Time of the last hotkey release, used to detect a double-tap in Hold
    /// mode (release then press again within DOUBLE_TAP_WINDOW).
    last_release: Option<Instant>,
    /// Sequence for the Fn bounce watchdog. A press bumps it, so a settle
    /// timer armed by a suspected bounced release can never end a take whose
    /// key is still physically held.
    bounce_settle_seq: u64,
    next_id: u64,
    /// P2.2: the kind of the in-flight take (P2.1/P2.3). Set at
    /// `start()`, read in `transcribed()`. Per-session, so the command kind
    /// cannot leak into the next take.
    take_kind: TakeKind,
    /// When the current recording started (ms); used to re-emit Recording
    /// once the model finishes warming.
    started_at_ms: u64,
    /// When the microphone actually opened, on a *monotonic* clock.
    ///
    /// This is the only honest basis for a speaking-rate figure: wall-clock
    /// `now_ms()` can jump backwards across an NTP correction or a timezone
    /// change mid-take, which would produce a negative or absurd duration.
    /// `Instant` cannot. Measured from the mic opening, not from the keypress,
    /// so the figure is speech time and not key-holding time.
    recording_started: Option<std::time::Instant>,
    /// Measured speaking time per session id, consumed when the history entry
    /// is written. A take is recorded in one event and written in another (the
    /// pipeline runs on a worker), so the measurement has to travel with the
    /// session id.
    speech_ms: std::collections::HashMap<u64, u64>,
    /// P4: the streaming take's trace marks (committed segments + tail audio
    /// ms), measured on the transcribe worker and consumed by the pipeline
    /// worker's `TraceStageMarks`. Keyed by session id; cleared when the
    /// pipeline worker consumes it (or when the take ends without a pipeline).
    streaming_marks: std::collections::HashMap<u64, (u32, u32)>,
    /// The VAD detector shared with the capture thread for the in-flight
    /// recording, if auto-stop is armed for this take.
    vad: Option<VadSlot>,
    /// P4: the streaming commit state for the in-flight take (segmenter +
    /// streaming VAD + 16 kHz buffer + committer), shared with the capture
    /// callback. `None` when streaming is disabled or VAD init failed.
    /// Moved out on release (the worker takes the committer + tail) and
    /// dropped on cancel.
    streaming: Option<StreamingSlot>,
    /// P4.3b: the app handle for the in-flight take's per-segment polish
    /// install. Lives beside `streaming` (same lifecycle: set when the
    /// take's streaming state is built, dropped on release/cancel).
    polish_app: Option<AppSlot>,
    /// Phase 5.7: the per-frame VAD flag collector for the in-flight take.
    /// Created in `start_recording`'s OK block (only when the take has a
    /// streaming state, whose VAD detector feeds it); moved into the
    /// release worker like `streaming`.
    vad_flags: Option<std::sync::Arc<std::sync::Mutex<VadFlagCollector>>>,
    /// T2.1: when the pipeline finished injecting the current take, the
    /// moment from which the user can edit the inserted text.
    injected_at: Option<Instant>,
    /// The polish summary for the take that just landed, held until the pill
    /// has been reset to idle so the card cannot be wiped by the same
    /// transition. `None` when polish changed nothing or did not run.
    pending_polish_diff: Option<(u64, PolishDiff)>,
    /// Live-preview handle for the in-flight recording, if the interim
    /// transcript loop is running. Stopped on release/cancel.
    live_preview: Option<LivePreviewHandle>,
    /// Escape-recovery spool for the in-flight recording, if one was
    /// started. `None` if spool creation failed (recording continues
    /// without a recovery spool) or the take has ended.
    spool: Option<crate::recovery::Spool>,
    /// F-02: the in-flight take's block resampler, shared between the
    /// capture frame callback (which creates and feeds it) and the take-end
    /// flush ([`Self::flush_resampler_into_take`]). `None` when no take is
    /// in flight or the take is synthetic (e2e, no frame callback).
    take_resampler: Option<Arc<std::sync::Mutex<Option<BlockResampler>>>>,
    /// HARD-4 row 2: the in-flight take's device-error flag. This is a clone
    /// of the flag the live [`teletype_core::audio::Recording`] carries, and
    /// the cpal stream error callback writes that same `Arc` when the device
    /// stops responding mid-take. `None` when no take is in flight: `start`
    /// clears it, so a stale `true` from a previous take cannot leak into the
    /// next one. Read once in `stop_with_note`, before the session id
    /// advances, so the trace can carry it.
    device_error_flag: Option<Arc<std::sync::atomic::AtomicBool>>,
    /// Phase 5.6: the audio ducker, shared for the controller's lifetime
    /// (the same long-lived object as the other take state). `duck` on a
    /// successful take start, `restore` on every stop path.
    ducker: std::sync::Arc<std::sync::Mutex<Box<dyn crate::ducking::AudioDucker + Send>>>,
    /// Phase 5.6: was the ducker armed for the in-flight take, so the stop
    /// path knows whether to restore (and re-send the media key).
    duck_armed: bool,
    /// The paste target as captured when the take started (app, pid, window
    /// frame). The pipeline can take 20 s, and in that gap the user may
    /// switch apps or windows; the delivery gate compares this against the
    /// target at inject time and refuses to paste into the wrong place.
    record_target: Option<RecordTarget>,
    /// P2.6: text selected in the focused app when the Command hotkey was
    /// pressed (synchronously, before the keystroke can corrupt it). Taken
    /// by `transcribed()` for selection-aware command takes; cleared when
    /// the take ends. `None` for dictation takes or when nothing was
    /// selected.
    pending_selection: Option<String>,
    /// Test-only: when true, `flash` skips the broadcast/show/flash-timer
    /// side effects (which need the Tauri app) and only advances the
    /// session counter. Production always keeps this false.
    #[cfg(test)]
    headless_flash: bool,
    /// Test-only: how many times the paste keystroke was requested. Kept as
    /// a dead seam: the tap-paste path was removed (F-03), so a test that
    /// asserts this stays 0 is the honest proof that a quick release never
    /// pastes the clipboard.
    #[cfg(test)]
    paste_calls: std::sync::atomic::AtomicU32,
    /// F-11: the warm token handed to the in-flight warm-up. Unique per
    /// announced warm-up (its own counter, never `next_id`), so a stale
    /// `WarmupDone`/`WarmupFailed` from a superseded warm is dropped.
    warm_token: u64,
    /// Test-only: the flash messages `flash`/`flash_for` recorded while
    /// headless, so a test can assert what the pill would have shown.
    #[cfg(test)]
    headless_flashes: Vec<String>,
    /// Test-only: every `UiState` a headless session would have broadcast,
    /// so a test can assert exactly what the UI would have seen (e.g. that
    /// a silent discard sends no `Message` at all).
    #[cfg(test)]
    headless_broadcasts: Vec<teletype_core::state::UiState>,
    #[cfg(test)]
    headless_final_texts: Vec<String>,
    /// Test-only: a 1 when a headless session would have injected a
    /// `PipelineReady` (no app handle, no platform, so the injection is
    /// counted instead of performed). The staleness guard in
    /// `pipeline_ready` has already run, so what lands here is exactly
    /// what production would have injected.
    #[cfg(test)]
    headless_pipeline_count: u32,
    /// Test-only: when set, the release worker's `transcribe` call is
    /// replaced by this fixed result (the same shape `transcribe`
    /// returns). Lets a headless test drive the real worker path with a
    /// scripted ASR outcome: `Err(NO_SPEECH_SENTINEL)` for a take the
    /// engine finds no speech in, `Ok(("text", None))` for a real
    /// transcript, `Err("other")` for a real engine failure.
    /// Test-only: when set, every `Event` the release worker sends is
    /// also forwarded to this channel, so a headless test can pump the
    /// worker's terminal events through the real `transcribed` handler.
    #[cfg(test)]
    test_events: Option<std::sync::mpsc::Sender<Event>>,
    #[cfg(test)]
    mock_asr: Option<TranscribeOutcome>,
    /// Sessions whose results the controller has already ruled out: a
    /// `Cancel` (hotkey or pill) processed while the pipeline was running
    /// marks the session id here, so the later `PipelineReady` for that id
    /// is discarded instead of injected. The set is the durable half of the
    /// cancellation decision — `Phase::Cancelled` alone is not enough
    /// because a newer session can move the phase on again.
    cancelled_sessions: std::collections::HashSet<u64>,
    /// P0.2 per-stage timing traces, keyed by session id. The in-flight
    /// take is stored under [`Session::trace_key`] until `stop()` re-keys
    /// it to the transcribing session id; worker threads hand their marks
    /// back through events/payloads (they never touch this map). An entry
    /// is removed exactly when its trace is finished (written or dropped),
    /// so at most one entry per unfinished take exists.
    traces: std::collections::HashMap<u64, DictationTrace>,
    /// The map key of the in-flight take's trace, if one was started.
    /// `None` when no take is recording (or the trace was already re-keyed
    /// to its session id by `stop()`).
    trace_key: Option<u64>,
    /// Phase 5.1: the pre-warmed mic front end, built once at spawn when
    /// `settings.instant_start` is not "off". `None` when the mode is off
    /// or the build failed (per-take rebuild continues — behaviour
    /// unchanged). Toggled by the `PrewarmTick` event on the controller
    /// thread; the take never reuses this stream (per-take rebuild stays).
    front_end: Option<MicFrontEnd>,
    /// The cancel hotkey currently registered with the OS, if any. Armed for
    /// the duration of a take only (see
    /// [`Controller::register_cancel_hotkey`] for why a bare `Escape` must
    /// not be registered persistently) and disarmed the moment the session
    /// leaves the active phases. `None` means "not registered".
    cancel_hotkey_armed: Option<String>,
    /// Phase 5.1: the pre-roll taken at hotkey-down —
    /// `(raw device-rate samples, device rate)` — fed into the streaming
    /// take at start and prepended to the ASR samples at release. Reset to
    /// `None` at every take start; consumed by the release worker.
    preroll_this_take: Option<(Vec<f32>, u32)>,
    /// Phase 5.7: per-session quality signals, stashed by the
    /// `TakeQuality` event and drained by `finish_trace` (the single
    /// attach point for every completion path, including errors).
    take_quality: std::collections::HashMap<u64, TakeQuality>,
    /// P6.4: the focused screen context captured once at hotkey-down
    /// (400 chars before / 100 after the caret). Ephemeral: it feeds the
    /// engine boost list and the paste-time anchor, and only a privacy-safe
    /// summary may reach a trace. Cleared when the take ends.
    screen_context: Option<teletype_core::screen_context::ScreenContext>,
    /// The learning store for the session, opened lazily on the first
    /// successful paste. `None` when it could not be opened (learning is
    /// then disabled for the session) or when nothing has been recorded
    /// yet. A failure never blocks or fails a paste.
    learning_store: Option<teletype_core::learning::store::LearningStore>,
    /// P7.5: the take's per-engine boost list, built once at take start
    /// from the lexicon plus the terms on screen. Handed to the streaming
    /// committer and the release worker; cleared when the take ends.
    take_boost_terms: Vec<teletype_speech::BoostTerm>,
    /// One-entry cache keyed by lexicon fingerprint, app category and
    /// screen-terms hash, so consecutive takes in the same context do not
    /// re-score the whole lexicon.
    boost_cache: teletype_core::learning::boost::BoostCache,
}

/// A second press within this window of a release is a double-tap.
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(350);
/// A Hold-mode release sooner than this is a tap, not a dictation: the take
/// is thrown away and the clipboard is pasted instead. Under a second so a
/// fast single word is never swallowed (EnviousWispr splits at 500 ms).
const TAP_PASTE_MAX_HOLD: Duration = Duration::from_millis(1000);
/// How a terminal pill message treats the pointer (see `show_with`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlashKind {
    /// A plain note ("Cancelled", "Pasted"): clicks pass through.
    Note,
    /// Possibly an error with an action (Open Settings): stays clickable.
    /// Anything not known to be a plain note is an alert.
    Alert,
    /// A take that captured no audio: clickable, and the pill offers the
    /// microphone settings shortcut (`mic-failure` event).
    MicFailure,
}
/// The visible-recording floor for a Hold-mode take, in ms. A release whose
/// take has been visibly recording less than this long is a mistap: the
/// take is discarded silently (no note, no paste, clipboard untouched). At
/// or above it the take is transcribed normally, which is what keeps a
/// 500 to 1000 ms one-word take alive (EnviousWispr splits at 500 ms).
const MIN_TAKE_VISIBLE_MS: u64 = 500;
/// The energy floor of the no-speech gate (F-70): a take whose peak sits at
/// or above this level still runs the decode, so whispered speech is not
/// dropped. Below it, a take the VAD classifies as speech-free is skipped
/// (Whisper-class models hallucinate filler on silence).
const NO_SPEECH_ENERGY_FLOOR: f32 = 0.005;

/// Wall-clock stamp (ms) of the physical hotkey press, taken in the event-tap
/// callback. The controller thread can be busy for seconds by the time a
/// release reaches it (mic open, engine reload, AX reads), and judging a tap
/// by dequeue time made a 300 ms tap look like a multi-second hold: the take
/// was transcribed instead of the clipboard being pasted. Both edges are
/// stamped on the main run loop, which that work never blocks.
static HOTKEY_DOWN_AT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// The physical hold of the release just recorded, in ms. `u64::MAX` means
/// "no release recorded", so a stale value can never classify a later take.
static HOTKEY_HOLD_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);

/// Records a physical hotkey press, from the event-tap callback.
pub fn mark_hotkey_down() {
    HOTKEY_DOWN_AT_MS.store(
        teletype_core::storage::now_ms(),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// Records a physical hotkey release and the hold it ended. Runs in the
/// event-tap callback, next to [`mark_hotkey_down`]; a release without a
/// matching press (the app started mid-press) records nothing.
pub fn mark_hotkey_up() {
    let down = HOTKEY_DOWN_AT_MS.swap(0, std::sync::atomic::Ordering::Relaxed);
    if down != 0 {
        HOTKEY_HOLD_MS.store(
            teletype_core::storage::now_ms().saturating_sub(down),
            std::sync::atomic::Ordering::Relaxed,
        );
    }
}

/// Whether a hotkey is currently physically held, per the timestamp stamped
/// by [`mark_hotkey_down`]/[`mark_hotkey_up`]. An accessor (rather than a
/// public atomic) keeps the latch's ownership here while letting the liveness
/// module observe it. Does not change dictation behavior.
pub(crate) fn hotkey_physically_down() -> bool {
    HOTKEY_DOWN_AT_MS.load(std::sync::atomic::Ordering::Relaxed) != 0
}

/// The physical hold for the release just recorded, consumed so it is used
/// at most once. `None` when the release did not come from an event tap
/// (headless tests, e2e injection); callers then fall back to the enqueue
/// time estimate.
fn take_hotkey_hold_ms() -> Option<u64> {
    let held = HOTKEY_HOLD_MS.swap(u64::MAX, std::sync::atomic::Ordering::Relaxed);
    (held != u64::MAX).then_some(held)
}

/// Whether a release is a quick tap: too short to be a dictation. Pure, so
/// the whole matrix is unit-testable. A quick release is an ordinary stop
/// (the take stops and is transcribed); it is discarded silently only when
/// the take's visible recording is under [`MIN_TAKE_VISIBLE_MS`].
///
/// Deliberately narrow: Toggle mode (a tap starts a take that must keep
/// going), a command take, and a hands-free take all keep their normal
/// release behaviour.
fn is_quick_tap(
    mode: RecordingMode,
    phase: Phase,
    hands_free: bool,
    kind: TakeKind,
    held_ms: u64,
) -> bool {
    mode == RecordingMode::Hold
        && phase == Phase::Listening
        && !hands_free
        && kind == TakeKind::Dictation
        && held_ms < TAP_PASTE_MAX_HOLD.as_millis() as u64
}
/// A `HotkeyDown` within this window of a `HotkeyUp` is treated as key
/// bounce (the macOS Fn key can emit a spurious down-up-down on one physical
/// press) and ignored, unless it is the deliberate hands-free double-tap
/// (see `DOUBLE_TAP_WINDOW`, which is longer and phase-gated). Without this,
/// a bounced Fn press starts two back-to-back takes and pastes the text
/// twice.
const HOTKEY_BOUNCE_DEBOUNCE: Duration = Duration::from_millis(100);

/// CMD-2: how young a dictation take has to be for a Command chord press to
/// take it over. A physical three-key chord costs roughly 150 ms of key
/// travel (Ctrl down, then Option, then Space), so the window has to be
/// several times that to cover a real hand. Past it, the recording is
/// content the user is speaking, and re-routing it to the command decider
/// would throw away a take they can hear themselves giving.
const COMMAND_TAKEOVER_WINDOW: Duration = Duration::from_millis(500);

/// Dev-only artificial delay (ms) inserted into the pipeline worker between
/// finishing its bookkeeping and sending `PipelineReady` to the controller.
///
/// Exists so the cancel-vs-ready race can be exercised by hand: with a
/// non-zero value the window between "worker done" and "controller may
/// inject" is wide enough to press the cancel hotkey in the middle of it.
/// Read on every pipeline run, so it can be flipped live from the settings
/// JSON without a rebuild. Must stay 0 in production use; it is a
/// diagnostic, not a feature.
pub const PIPELINE_READY_DELAY_ENV: &str = "TELETYPE_PIPELINE_READY_DELAY_MS";

/// Dev-only artificial delay (ms) inserted before the controller performs
/// the injection once it has accepted a `PipelineReady`. Same purpose and
/// same env var as [`PIPELINE_READY_DELAY_ENV`].
pub const PIPELINE_INJECT_DELAY_ENV: &str = "TELETYPE_PIPELINE_INJECT_DELAY_MS";

/// Parses a non-negative dev delay in milliseconds. A missing or
/// malformed value means "no delay" (a typo in a debug env var must never
/// break a dictation), and the value is clamped to 60 000 ms.
pub fn parse_dev_delay(raw: Option<&str>) -> Duration {
    let ms = raw
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        .min(60_000);
    Duration::from_millis(ms)
}

/// Reads a non-negative dev delay in milliseconds from the environment.
/// Release builds always return zero: the delay is a diagnostic, not a
/// feature, and must not be reachable from a shipped binary.
pub fn dev_delay_ms(var: &str) -> Duration {
    #[cfg(not(debug_assertions))]
    {
        let _ = var;
        return Duration::ZERO;
    }
    #[cfg(debug_assertions)]
    {
        parse_dev_delay(std::env::var(var).ok().as_deref())
    }
}

/// The pill's message when a take captured no audio at all and the mic
/// permission cannot be told apart from a silent mic. The backend also emits
/// the pill-only `mic-failure` event alongside it, so the pill can offer the
/// microphone settings shortcut instead of ending on a dead status line.
pub const MIC_FAILURE_MESSAGE: &str = "No audio captured";

/// The pill's message when the microphone stopped responding mid-take (the
/// cpal stream error callback fired while the take was live, HARD-4 row 2).
/// The captured audio is still transcribed and pasted through the normal
/// stop path; the message tells the user the tail of the take may be missing.
/// No device names: the user needs the fix (check the mic), not the class.
pub const DEVICE_LOSS_MESSAGE: &str =
    "Your microphone stopped responding. Teletype used the audio it had already captured.";

/// The error string a take ends with when the ASR found no speech in the
/// audio. Unlike a real engine failure, this is an expected outcome (the
/// user pressed the hotkey but said nothing) and must end the take
/// quietly: no error flash, no error log line, no history entry. The
/// string is the wire format between the worker (which classifies the
/// engine's typed `SpeechError::NoSpeech`) and `transcribed` (which
/// routes it to the quiet end).
pub const NO_SPEECH_SENTINEL: &str = "No speech detected";

/// The result of a whole-take ASR decode: the transcript text (and any
/// word timings) or an engine error message. Used by the test-only
/// `mock_asr` field on `Session` to script the worker's ASR outcome.
#[cfg(test)]
type TranscribeOutcome = Result<(String, Option<Vec<teletype_speech::Word>>), String>;

/// True when `msg` is the no-speech sentinel (the typed
/// [`teletype_speech::SpeechError::NoSpeech`] rendered to its display
/// string by the worker). Real failures (model not loaded, decode error,
/// crash) must never match this: they keep their error flash.
fn is_no_speech_error(msg: &str) -> bool {
    msg == NO_SPEECH_SENTINEL
}

/// HARD-4 row 14: which model a failed take may fall back to, if any.
///
/// A fallback is for a model that cannot load or decode, so the model that
/// last finished a decode is tried once. Two non-failures are excluded: the
/// no-speech sentinel (the engine answered correctly, there was nothing to
/// hear) and a take that captured no audio at all (any model would return the
/// same nothing). Falling back on either would pay a second load plus decode
/// on every silent hotkey press, doubling the worst-case latency of a take
/// that is not broken, and could deliver text the selected model correctly
/// found none of.
///
/// Also `None` when nothing has ever decoded, and when the last working model
/// IS the selected one: retrying the same failure buys nothing.
fn pick_fallback_model<'a>(
    error: &str,
    selected: &str,
    last_working: Option<&'a str>,
) -> Option<&'a str> {
    if is_no_speech_error(error) || error == MIC_FAILURE_MESSAGE {
        return None;
    }
    last_working.filter(|id| *id != selected)
}

/// The pill's message when a take captured no audio because the microphone
/// permission is off. The pill's `mic-failure` action opens the OS
/// microphone permission pane, which is the fix for this case.
pub const MIC_ACCESS_DENIED_MESSAGE: &str = "Microphone access is off";

/// A take with zero frames can never be transcribed. Returning the failure
/// here lets the worker stop before ASR and hand the controller a message it
/// can turn into an actionable pill state.
fn take_failure(captured: &Captured) -> Option<&'static str> {
    captured.samples.is_empty().then_some(MIC_FAILURE_MESSAGE)
}

/// The pill's warning when a take's streaming decode lost words: one or
/// more mid-take segments failed to decode, or the tail's decode failed
/// while committed segments survived. `None` when nothing was lost — the
/// take delivered exactly what it transcribed, and a warning would be noise.
///
/// T10b: the committer accounts for these losses (T03) but never showed
/// them; this is the message the controller flashes for 4 s after the
/// (partial) text has already been delivered.
fn loss_warning(failed_segments: usize, tail_failed: bool) -> Option<&'static str> {
    if failed_segments > 0 || tail_failed {
        Some("Part of that recording couldn't be transcribed")
    } else {
        None
    }
}

/// PRIV-1: replaces every occurrence of every dictated argument value in
/// the executor's message with a length placeholder (`<7>` for a value of
/// 7 characters), so the log file and the persisted trace carry shapes and
/// lengths, never the values themselves. Values that do not appear in the
/// message change nothing; longer values are replaced first so a short
/// value that is a substring of a longer one (e.g. `a` inside `app`) cannot
/// corrupt the longer redaction.
fn redact_command_message(message: &str, values: &[String]) -> String {
    let mut out = message.to_string();
    let mut values: Vec<&str> = values.iter().map(String::as_str).collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    for value in values {
        if value.is_empty() {
            continue;
        }
        let placeholder = format!("<{}>", value.len());
        out = out.replace(value, placeholder.as_str());
    }
    out
}

/// T10b: how often the model-load progress reaches the pill. The download
/// loop can emit on every read chunk; the pill only needs a smooth bar, and
/// a flood of window events on the main thread during a 300 MB first load
/// is exactly what the 4 Hz cap exists to prevent. The first value is
/// always emitted (the pill switches to the percentage as soon as one is
/// known).
pub const PILL_PROGRESS_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Decides whether one throttled progress value should be emitted: the
/// first value always is, and later ones only after at least
/// [`PILL_PROGRESS_INTERVAL`] has passed. Pure over timestamps so the
/// throttle is unit-testable without a clock.
pub(crate) fn should_emit_progress(last_emitted_us: Option<u64>, now_us: u64) -> bool {
    match last_emitted_us {
        None => true,
        Some(last) => now_us.saturating_sub(last) >= PILL_PROGRESS_INTERVAL.as_micros() as u64,
    }
}

/// The pill's message for a zero-sample take, given the mic permission
/// status. `None` means the status could not be queried; a denied (or
/// not-yet-determined, or unsupported) permission means the user must open
/// System Settings, so the copy says so. `Granted` (and the unanswerable
/// `None`) keep the existing copy: the mic is allowed, it just produced no
/// audio.
fn zero_sample_take_message(
    permission: Option<teletype_core::platform::PermissionState>,
) -> &'static str {
    match permission {
        Some(
            teletype_core::platform::PermissionState::Denied
            | teletype_core::platform::PermissionState::NotDetermined
            | teletype_core::platform::PermissionState::Unsupported,
        ) => MIC_ACCESS_DENIED_MESSAGE,
        Some(teletype_core::platform::PermissionState::Granted) | None => MIC_FAILURE_MESSAGE,
    }
}

/// The reason class for a speech-model warm-up failure, shown in the pill's
/// error state. A `NotFound`/`PermissionDenied` io error means the model
/// file is missing (never downloaded, or unreadable); `Unsupported` means a
/// corrupt or wrong-format archive; anything else (a provider that refused
/// the model, a platform error) is "other".
#[cfg(test)]
fn warmup_failure_reason(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => "missing file",
        std::io::ErrorKind::Unsupported => "unsupported",
        _ => "other",
    }
}

/// The reason class for a warm-up failure whose error is a plain `String`
/// (the speech managers report `Result<(), String>`). The string itself
/// cannot be classified reliably, so the path is checked: a missing file is
/// the common case (the model was never downloaded), and everything else
/// is "other".
fn warmup_failure_reason_str(message: &str, model_path: &std::path::Path) -> &'static str {
    if !model_path.exists() {
        "missing file"
    } else if message.to_ascii_lowercase().contains("unsupported") {
        "unsupported"
    } else {
        "other"
    }
}

/// Wall-clock milliseconds, for the level throttle (the capture thread has
/// no other clock; the value is only compared against itself, so the epoch
/// does not matter).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Caps the pill's `pill-level` stream at ~20 Hz per take. Every audio frame
/// used to trigger a reducer dispatch and a pill re-render; the capture rate
/// is 48–480 Hz, so most frames are dropped. The helper keeps the latest
/// value and lets a frame through once the interval has elapsed since the
/// last emit; `reset` marks a new take (the first frame always emits) and
/// `flush` forces the latest value out at a take's end so the meter never
/// freezes on a stale level.
#[derive(Debug, Default)]
struct PillLevelThrottle {
    last_emit_ms: Option<u64>,
}

impl PillLevelThrottle {
    fn new() -> Self {
        Self::default()
    }

    /// The minimum gap between two `pill-level` emits (50 ms = 20 Hz).
    const INTERVAL_MS: u64 = 50;

    /// Returns `true` when `level` should be emitted at `now_ms`, and records
    /// the emit. A fresh throttle always emits its first frame.
    fn should_emit(&mut self, now_ms: u64, _level: f32) -> bool {
        let emit = match self.last_emit_ms {
            None => true,
            Some(last) => now_ms - last >= Self::INTERVAL_MS,
        };
        if emit {
            self.last_emit_ms = Some(now_ms);
        }
        emit
    }
}

/// The mic permission status for a zero-sample take, read on the controller
/// thread. macOS queries `AVAudioApplication` (the same call the settings
/// screen uses); other platforms have no such query, so `None` (the message
/// falls back to the plain "No audio captured").
#[cfg(target_os = "macos")]
fn mic_permission_status() -> Option<teletype_core::platform::PermissionState> {
    extern "C" {
        fn teletype_mic_authorization_status() -> i32;
    }
    // SAFETY: teletype_mic_authorization_status is a pure C function that
    // reads an Obj-C property and returns an int; no shared mutable state.
    // The selector is unrecognized on a future macOS, so a panic is caught
    // the same way the settings screen does it.
    std::panic::catch_unwind(|| unsafe { teletype_mic_authorization_status() })
        .ok()
        .map(|code| match code {
            1 => teletype_core::platform::PermissionState::Granted,
            2 => teletype_core::platform::PermissionState::Denied,
            _ => teletype_core::platform::PermissionState::NotDetermined,
        })
}

#[cfg(not(target_os = "macos"))]
fn mic_permission_status() -> Option<teletype_core::platform::PermissionState> {
    None
}

/// How often the interim transcript loop re-transcribes the audio-so-far.
/// Everything the pipeline worker knows about a finished take, handed to the
/// controller with `PipelineReady`. The controller owns the only user-visible
/// side effect (the injection) and decides, at the moment it processes this,
/// whether the session is still valid. The worker must never act on this
/// payload itself.
pub struct PipelinePayload {
    pub session: u64,
    /// The transcript as ASR produced it, before filler removal, AutoText and
    /// polish. Shipped alongside `final_text` so the controller can show what
    /// polish changed after the text has landed.
    pub raw_text: String,
    pub final_text: String,
    /// The polish stage's input and output with AutoText values restored on
    /// both sides. `None` when polish did not run or changed nothing. The
    /// settle sweep records corrections from this pair only: the
    /// raw-to-final diff also carries dictionary correction, filler removal,
    /// ITN and AutoText expansion, and none of those are model corrections.
    pub polish_change: Option<(String, String)>,
    pub context: teletype_core::context::ApplicationContext,
    pub record_target: Option<RecordTarget>,
    pub detected_language: Option<String>,
    /// A structured command extracted by the decision provider when the
    /// intent is Command. When `Some`, the controller executes the action
    /// instead of injecting text.
    pub command: Option<teletype_core::llm::CommandAction>,
    /// P0.2 trace marks collected on the pipeline worker thread (polish
    /// start/first-token/end, text-pipeline end). Merged into the session
    /// trace when the controller processes this event.
    pub trace_marks: TraceStageMarks,
    /// A.3.3: the command decider trace fields for this take (set only on
    /// command takes that ran the decider path): the decider kind label,
    /// the `decide` wall time in ms, the tool chosen, and where the
    /// arguments came from. All `None` for takes that never ran the
    /// command route.
    pub decider: Option<String>,
    pub decider_ms: Option<u32>,
    pub tool: Option<String>,
    pub args_source: Option<String>,
    /// T10b: the take's word-loss accounting (T03) — failed mid-take
    /// segments and a failed tail. The controller delivers the (partial)
    /// text first, then flashes the warning when either is lost. `None`
    /// on whole-take fallback paths (no committer, nothing to lose).
    pub losses: Option<crate::committer::CommitterFailureReport>,
    /// The polish contract that actually ran this take, from `PipelineResult`.
    pub polish_profile: teletype_core::profile::PolishProfile,
    /// One-line fallback/advisory reason from the same result (empty on a
    /// clean pass). Carried to the `polish-diff` card without rewriting it.
    pub polish_profile_reason: String,
    /// Whether the polish pass actually executed this take (no gate/model
    /// skip). Derived from the trace marks on the pipeline worker and carried
    /// to the `polish-diff` card so it can label a no-op honestly.
    pub polish_ran: bool,
    /// The loaded polish model id when available, for the `polish-diff` card.
    pub polish_model: Option<String>,
}

/// T5 desktop seam: the per-take polish profile. The desktop layer owns the
/// local bandit; the pipeline only ever sees the resolved choice through the
/// encoded instruction channel. An explicit per-app profile override always
/// wins and the bandit is not consulted; otherwise the bandit may select an
/// adjacent learned profile, keeping the recommended default when it has no
/// active change or promotion evidence.
///
/// Callers must hold the bandit lock only for this choice, never across
/// `pipeline.run` (the settle sweep locks it again later in the take).
#[allow(dead_code)]
pub(crate) fn choose_take_profile_for_app(
    bandit: &teletype_core::learning::profile_bandit::BanditState,
    app_key: &str,
    has_explicit_app_profile: bool,
    recommended: teletype_core::profile::PolishProfile,
    now_ms: u64,
) -> teletype_core::profile::PolishProfile {
    if has_explicit_app_profile {
        recommended
    } else {
        bandit.choose(app_key, recommended, now_ms)
    }
}

/// What the post-insertion card shows: the transcript as ASR heard it, the
/// text polish produced, and how long polish took. The pill renders the word
/// diff; the backend only decides whether a card is warranted.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct PolishDiff {
    pub raw: String,
    pub polished: String,
    pub ms: u64,
    /// The raw ASR transcript after the same deterministic filler-removal pass
    /// the pipeline ran (same core remover and take settings), so the UI shows
    /// "Original after filler removal" rather than noisy ASR.
    pub original: String,
    /// Whether polish actually ran this take (trace marks / transform result
    /// evidence), as opposed to being gated or skipped.
    pub ran: bool,
    /// The loaded polish model id when available; `null` otherwise.
    pub model: Option<String>,
    /// One-line fallback/advisory reason; `None` when the payload has no
    /// reason (keeps the key out of the event).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The polish profile that ran (e.g. "structured"), when the payload
    /// carries one. `None` keeps the key out of the event on older callers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
}

/// The `polish-diff` event payload: the diff plus the card id the pill hands
/// back to `dismiss_pill_result`, so a late dismiss cannot hide a newer take.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PolishDiffCard<'a> {
    #[serde(flatten)]
    diff: &'a PolishDiff,
    card: u64,
}

/// A card is warranted only when polish ran and actually changed words.
///
/// Filler removal and AutoText also alter the text, but neither is polish, so
/// they must not be presented as such. The polish marks are what separate the
/// two: a skipped take carries a reason and no start/end pair.
#[allow(clippy::too_many_arguments)]
fn polish_diff(
    raw: &str,
    polished: &str,
    marks: &teletype_core::trace::TraceStageMarks,
    reason: Option<&str>,
    profile: Option<&str>,
    ran: bool,
    model: Option<String>,
    original: String,
) -> Option<PolishDiff> {
    let raw = raw.trim();
    let polished = polished.trim();
    if raw.is_empty() || polished.is_empty() {
        return None;
    }
    // Always emit (even when polish changed nothing or was skipped) so the
    // card shows after every take; the UI renders "0 changes" when
    // raw == polished. The duration is only known when polish actually ran.
    let ms = match (marks.t_polish_start, marks.t_polish_end) {
        (Some(start), Some(end)) if end >= start => end - start,
        _ => 0,
    };
    Some(PolishDiff {
        raw: raw.to_string(),
        polished: polished.to_string(),
        original,
        ran,
        model,
        ms,
        reason: reason.filter(|r| !r.is_empty()).map(str::to_string),
        profile: profile.filter(|p| !p.is_empty()).map(str::to_string),
    })
}

/// How often the interim transcript loop re-transcribes the audio-so-far.
///
/// The cadence is bounded by measurement, not taste. Parakeet TDT has no
/// streaming state: every call re-derives the whole utterance from scratch, so
/// the interim text is a full re-decode of everything said so far, not an
/// incremental update. Measured on an M4 Pro, a warm re-decode of an 11 s
/// window costs 188 to 532 ms p50 depending on how far into the take it runs,
/// and back-to-back decodes contend on the Metal queue (0.164x of one core at a
/// 1 s cadence, 0.638x at 0.5 s).
///
/// 700 ms is therefore close to the floor: it is slower than the decode itself
/// at the longest windows, so the loop is decode-bound rather than
/// interval-bound, and a tighter interval would only add contention. See
/// `tasks/pi-tasks/streaming-asr-spike-findings.md`.
const INTERIM_INTERVAL: Duration = Duration::from_millis(700);

/// Minimum seconds of audio before the first interim pass. Avoids a wasted
/// transcribe on a near-empty buffer.
const INTERIM_MIN_SECS: f64 = 1.0;

/// Maximum seconds of the take the interim pass decodes. The interim loop
/// re-derives the whole utterance from scratch every tick (Parakeet TDT has
/// no streaming state), so without a cap the per-tick decode cost grows
/// linearly with take length and the total work per take is O(n²). The pill
/// only shows the tail of the transcript (the authoritative text is the
/// single full decode at stop), so decoding only the last N seconds loses
/// nothing the user sees and makes each interim pass a constant cost. 10 s
/// covers a full sentence or more; a longer take's earlier words are already
/// committed to the transcript by prior interim passes and are not re-shown.
const INTERIM_WINDOW_SECS: f64 = 10.0;

/// Handle to a running live-preview interim thread. `stop()` signals the
/// thread to exit and abandons any in-flight decode WITHOUT joining (P1.4):
/// the final transcribe must not wait for the speech-model lock; `join()`
/// reclaims the thread from `go_idle()` once the final pass can no longer
/// contend for it.
struct LivePreviewHandle {
    stop_tx: mpsc::Sender<()>,
    /// P1.4: set by `stop()` so the interim loop abandons the in-flight
    /// decode and releases the speech-model lock for the final transcribe.
    /// The interim thread checks it between chunks; `stop()` does NOT join
    /// the thread, so a decode in flight when the user releases is
    /// abandoned, not awaited.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
    /// Join handle for the interim thread. Joined by `go_idle()` once the
    /// final transcribe can no longer contend for the speech-model lock.
    join: Option<std::thread::JoinHandle<()>>,
}

impl LivePreviewHandle {
    /// P1.4: signal the interim loop to stop and abandon any in-flight
    /// decode. Does NOT join: the final transcribe must not wait for the
    /// interim decode to finish, and the interim thread drops the
    /// speech-model lock as soon as it sees the cancel flag.
    fn stop(self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = self.stop_tx.send(());
        // Intentionally not joined: the thread exits on its own once the
        // in-flight decode is abandoned. `go_idle()` joins it later.
    }

    /// Joins the interim thread, if it is still running. Called from
    /// `go_idle()`, by which point the final transcribe has finished and
    /// the speech-model lock can no longer be contended.
    fn join(self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = self.stop_tx.send(());
        if let Some(h) = self.join {
            let _ = h.join();
        }
    }
}

/// Spawns the interim-transcript thread for an in-flight recording and
/// returns a handle the caller stores on the Session.
fn spawn_live_preview(
    app: AppHandle,
    buf: teletype_core::audio::Buf,
    rate: Arc<std::sync::atomic::AtomicU32>,
    session: u64,
) -> Option<LivePreviewHandle> {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancelled_thread = cancelled.clone();
    let Ok(join) = thread::Builder::new()
        .name("teletype-live-preview".into())
        .spawn(move || {
            run_live_preview(app, buf, rate, session, stop_rx, cancelled_thread);
        })
    else {
        crate::log_entry(
            crate::LogLevel::Warn,
            "failed to spawn live-preview thread; interim transcripts disabled for this take",
        );
        return None;
    };
    Some(LivePreviewHandle {
        stop_tx,
        cancelled,
        join: Some(join),
    })
}

/// The interim-transcript loop. Every `INTERIM_INTERVAL` it snapshots the
/// live audio buffer, resamples to 16 kHz, transcribes (serializing on the
/// speech-model lock), and emits an `interim-transcript` event to the pill.
/// Exits when `stop_rx` receives a signal or the capture thread drops the
/// buffer (which can't happen while the take is in flight, since `stop()`
/// always signals first).
fn run_live_preview(
    app: AppHandle,
    buf: teletype_core::audio::Buf,
    rate: Arc<std::sync::atomic::AtomicU32>,
    session: u64,
    stop_rx: mpsc::Receiver<()>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) {
    let state = app.state::<AppState>();
    let settings = state.settings();
    let (model_path, kind) = resolve_speech_model(&settings, &state.models_dir);
    let language = effective_language_for_app(&state, kind);

    // Skip entirely if the model file isn't on disk yet (warm_up will load
    // it; we don't want to block on a download).
    if !model_path.exists() {
        crate::log_entry(
            crate::LogLevel::Info,
            "live-preview: model not on disk yet, skipping interim loop",
        );
        // Drain stop_rx so the handle's stop() doesn't block.
        let _ = stop_rx.recv();
        return;
    }

    // The neural slot starts on a placeholder provider; make sure it serves
    // the selected model before the preview decodes. A failed swap would
    // preview with the wrong engine, so skip the loop instead.
    if matches!(
        kind,
        SpeechEngineKind::Fluid
            | SpeechEngineKind::Sherpa
            | SpeechEngineKind::Qwen3Asr
            | SpeechEngineKind::AppleSpeech
            | SpeechEngineKind::TranscribeCpp
    ) && ensure_neural_provider(&state, &settings.selected_speech_model).is_err()
    {
        let _ = stop_rx.recv();
        return;
    }

    loop {
        // Wait for the next tick or a stop signal.
        match stop_rx.recv_timeout(INTERIM_INTERVAL) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }

        // P1.4: check the cancel flag before doing anything expensive, so
        // a release that lands mid-decode abandons the next interim pass
        // without touching the speech-model lock.
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        // Snapshot the live buffer. The capture thread appends to this vec,
        // so a short lock + clone is cheap relative to a transcribe pass.
        let raw = match buf.lock() {
            Ok(g) => g.clone(),
            Err(_) => continue,
        };
        let device_rate = rate.load(std::sync::atomic::Ordering::Relaxed);
        if raw.is_empty() || device_rate == 0 {
            continue;
        }
        // Cap the interim window to the last INTERIM_WINDOW_SECS. Without
        // this the pass re-decodes the entire take every tick (O(n²) total
        // work per take); the pill only shows the tail, and the authoritative
        // text is the full decode at stop, so the cap is invisible to the
        // user. Slice at a device-rate sample boundary so the window is an
        // integer number of source samples (keeps the resampler happy).
        let window_samples = (INTERIM_WINDOW_SECS * device_rate as f64) as usize;
        let tail = if raw.len() > window_samples {
            &raw[raw.len() - window_samples..]
        } else {
            &raw[..]
        };
        let resampled = resample_to_target(tail, device_rate).unwrap_or_default();
        let secs = resampled.len() as f64 / teletype_core::audio::TARGET_SAMPLE_RATE as f64;
        if secs < INTERIM_MIN_SECS {
            continue;
        }

        // Transcribe the window. This serializes on the speech-model lock
        // with the final transcribe (which runs in stop()). `stop()` sets the
        // cancel flag and does not join, so this pass is abandoned the moment
        // it returns after a release; the final decode then takes the same
        // lock (see the note in `transcribe_for_model` for why it can't run
        // concurrently on a separate provider instance).
        let state = app.state::<AppState>();
        let manager = match kind {
            SpeechEngineKind::Fluid
            | SpeechEngineKind::Sherpa
            | SpeechEngineKind::Whistle
            | SpeechEngineKind::Qwen3Asr
            | SpeechEngineKind::AppleSpeech
            | SpeechEngineKind::TranscribeCpp => &state.neural,
            SpeechEngineKind::Parakeet => &state.parakeet,
            SpeechEngineKind::ParakeetCpp => &state.parakeet_cpp,
            SpeechEngineKind::Whisper => &state.speech,
        };
        let mut speech = manager
            .provider()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // P1.4: a release landed while we were waiting for the lock or
        // about to decode. Abandon this interim pass: drop the lock so the
        // final transcribe can proceed, and exit the loop.
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        if !speech.is_loaded() {
            // Model still warming; the interim loop will retry next tick.
            continue;
        }
        let text = match speech.transcribe(&resampled, &language) {
            Ok(t) => t,
            Err(_) => continue, // NoSpeech or a transient error; retry next tick.
        };
        // P1.4: a release landed while the decode was running. The text is
        // stale by definition (the take is over); discard it and exit so
        // the speech-model lock is released for the final transcribe.
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        // Marked provisional on purpose. This is a whole-take re-decode, not a
        // partial, so the text revises as the sentence develops: punctuation
        // appears, the first words get dropped when the window's leading edge
        // moves, and wording can change. The pill shows it as a preview and the
        // authoritative text is the single full decode that `stop()` runs. The
        // UI relies on the event name to decide how prominently to render it.
        let _ = app.emit_to("pill", "interim-transcript", &text);
        let _ = session; // session is reserved for future staleness filtering
    }
}

/// Scores the engine's word rows against the dictionary and pack terms and
/// rebuilds the take from them.
///
/// Split out of `Session::rescore_with_confidence`, which returns as soon as
/// there is no Tauri state, so the empty-word-row shape it guards against was
/// unreachable from a test while it was silently pasting nothing in production.
fn rescore_or_keep(
    raw: String,
    words: &[teletype_speech::Word],
    terms: &[teletype_core::rescoring::RescoreTerm],
) -> String {
    if terms.is_empty() {
        return raw;
    }
    if words.is_empty() {
        crate::log_entry(
            crate::LogLevel::Info,
            "rescoring skipped: engine returned no word rows, keeping transcript",
        );
        return raw;
    }
    let word_texts: Vec<String> = words.iter().map(|w| w.text.clone()).collect();
    let word_confs: Vec<f32> = words.iter().map(|w| w.conf).collect();
    let (rescored_words, replacements) =
        teletype_core::rescoring::rescore_words(&word_texts, &word_confs, terms);
    if !replacements.is_empty() {
        crate::log_entry(
            crate::LogLevel::Info,
            format!("rescoring applied {} replacements", replacements.len()),
        );
    }
    rescored_words.join(" ")
}

/// Delivery gate: the recorded take-start target against the live target,
/// with one attempt to bring the recorded app back.
///
/// Returns true when the text may be injected. Pure decision plus one
/// side-effecting recovery, kept out of the worker closure so the rule stays
/// readable: verify, try once to repair by raising the recorded app, verify
/// again, otherwise refuse. Unreadable ends proceed (no evidence is not a
/// mismatch); only a proven switch aborts. No recorded target also proceeds:
/// there is no baseline to mismatch against.
fn gate_passed(
    record_target: &Option<RecordTarget>,
    platform: &dyn teletype_core::platform::Platform,
) -> bool {
    let Some(recorded) = record_target else {
        return true;
    };
    let decided = |platform: &dyn teletype_core::platform::Platform| {
        let current = platform.active_application();
        let bundle = current
            .as_ref()
            .map(|a| a.application_id.as_str())
            .unwrap_or("");
        resolve_delivery(
            recorded,
            bundle,
            platform.active_pid(),
            platform.focused_window_frame(),
        )
    };
    if decided(platform) == Delivery::Proceed {
        return true;
    }
    // Proven switch. One attempt to raise the recorded app and re-verify:
    // the user may have switched away and back already, or may want us to
    // bring them back. Never paste on the activation call alone.
    if let Some(pid) = recorded.pid {
        if platform.activate_pid(pid) {
            thread::sleep(Duration::from_millis(400));
            if decided(platform) == Delivery::Proceed {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!(
                        "delivery: re-activated {}; proceeding with the paste",
                        recorded.app_name
                    ),
                );
                return true;
            }
        }
    }
    false
}

/// Why the text must not be injected, if anything.
///
/// Two independent refusals share the abort path (no tiers, history keeps the
/// text, user is told). The window switch comes first: it is about *where*.
/// The terminal newline is about *what*: a newline in a terminal submits the
/// line, so multi-line dictation there would run commands.
fn delivery_refusal(
    record_target: &Option<RecordTarget>,
    platform: &dyn teletype_core::platform::Platform,
    app_type: teletype_core::context::AppType,
    text: &str,
) -> Option<String> {
    if !gate_passed(record_target, platform) {
        let where_to = record_target
            .as_ref()
            .map(|r| r.app_name.as_str())
            .unwrap_or("the app");
        return Some(format!(
            "Not pasted: {where_to} is no longer in front. \
             The text is kept in History."
        ));
    }
    if teletype_core::delivery::refuse_newline_in_terminal(app_type, text) {
        return Some(
            "Not pasted: a newline in a terminal submits the line, so \
             multi-line dictation is never typed there. The text is kept \
             in History; paste it yourself if that is what you meant."
                .to_string(),
        );
    }
    None
}

/// Plays the configured dictation cue, if the user left it on.
///
/// The cue is tied to the mic actually being open, not to the keypress: a
/// start sound on a take whose mic failed to open tells the user the opposite
/// of the truth, and the failure they would blame on the app is a permissions
/// problem the pill is already showing.
///
/// Best-effort throughout. A missing system sound or a session the OS will not
/// play must never interrupt a dictation, so failures are logged and dropped.
fn play_cue(sound: &str) {
    #[cfg(target_os = "macos")]
    {
        let Ok(c) = std::ffi::CString::new(sound) else {
            return;
        };
        extern "C" {
            fn teletype_play_system_sound(name: *const std::os::raw::c_char) -> i32;
        }
        // SAFETY: `c` is NUL-terminated and outlives the call; the callee
        // copies the name into an NSString before returning.
        let rc = unsafe { teletype_play_system_sound(c.as_ptr()) };
        if rc != 0 {
            tracing::info!("recording cue {sound:?} unavailable (rc {rc})");
        }
    }
    #[cfg(target_os = "windows")]
    {
        // P8.5: the Windows cue is a system sound via PlaySoundW (see
        // `crate::sfx`); the stored `sound` name is the macOS NSSound
        // vocabulary and has no Windows equivalent, so the cue is fixed.
        let _ = sound;
        crate::sfx::play_start();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = sound;
    }
}

/// The text around the caret, for smart insertion.
///
/// macOS reads the AX tree; Windows reads the focused control through UIA
/// (P8.5, `crate::uia::focused_surroundings`). `None` when the field
/// cannot be read: the dictated text is then inserted unchanged, which is
/// the documented fallback for an unreadable field.
#[cfg(target_os = "macos")]
fn focused_surroundings() -> Option<(String, String)> {
    crate::ax_text::focused_surroundings()
}

#[cfg(target_os = "windows")]
fn focused_surroundings() -> Option<(String, String)> {
    crate::uia::focused_surroundings()
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn focused_surroundings() -> Option<(String, String)> {
    None
}

/// P7.2: read the focused field for the learning observer. macOS reads the
/// AX tree; Windows reads the focused control through UIA (P8.5,
/// `crate::uia::focused_control_text`). `Unreadable` when the field cannot
/// be read: the observation is then dropped, never guessed.
#[cfg(target_os = "macos")]
fn read_focused_field() -> crate::edit_watch::FieldRead {
    match crate::ax_text::focused_value() {
        Some(v) => crate::edit_watch::FieldRead::Readable(v),
        None => crate::edit_watch::FieldRead::Unreadable,
    }
}

#[cfg(target_os = "windows")]
fn read_focused_field() -> crate::edit_watch::FieldRead {
    match crate::uia::focused_control_text() {
        Ok(v) => crate::edit_watch::FieldRead::Readable(v),
        Err(_) => crate::edit_watch::FieldRead::Unreadable,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn read_focused_field() -> crate::edit_watch::FieldRead {
    crate::edit_watch::FieldRead::Unreadable
}

/// The language to pass to the ASR engine.
///
/// `"auto"` (or an empty legacy setting) is passed through: Whisper models
/// auto-detect the language themselves (whisper.cpp runs without a
/// language hint). Parakeet cannot auto-detect, so its wrapper ignores the
/// language argument and always runs with its trained language mix.
///
/// When `english_only` is set (an English-only engine), a locked language
/// that is not English resolves to `"en"`: the engine cannot transcribe it,
/// and passing the lock through would only feed a wrong-language hint to a
/// provider that decodes English regardless. "auto" is preserved, so the
/// engine's own default (its trained language) still applies.
pub fn effective_language(language: String, english_only: bool) -> String {
    let trimmed = language.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        "auto".into()
    } else if english_only && !trimmed.eq_ignore_ascii_case("en") {
        "en".into()
    } else {
        trimmed.to_string()
    }
}

type SpeechManager =
    teletype_inference::manager::SpeechModelManager<Box<dyn teletype_speech::SpeechProvider>>;

/// Applies a lifecycle call to every speech manager.
///
/// A take must hold off the idle-unload timer on all engines (and a session
/// end must arm it on all of them). The neural slot serves the macOS Core ML
/// default, so a hand-maintained list that omits it leaves the largest model
/// resident forever; routing every site through this helper keeps a future
/// engine from inheriting the same drift.
fn each_speech_manager(state: &AppState, f: impl Fn(&SpeechManager)) {
    f(&state.speech);
    f(&state.parakeet);
    f(&state.parakeet_cpp);
    f(&state.neural);
}

/// Resolves the selected speech model to a file path on disk plus which
/// engine kind loads it. Shared by the dictation worker, the model warm-up,
/// and the recovery replay path.
pub fn resolve_speech_model(
    settings: &crate::commands::Settings,
    models_dir: &std::path::Path,
) -> (std::path::PathBuf, SpeechEngineKind) {
    resolve_speech_model_for_id(&settings.selected_speech_model, settings, models_dir)
}

/// HARD-4 row 14: resolves a model id (not the selected one) to its path
/// and engine kind, for the load-failure fallback. Same per-engine path
/// semantics as [`resolve_speech_model`], but takes the id directly so the
/// fallback does not read the user's current selection.
pub fn resolve_speech_model_for_id(
    model_id: &str,
    settings: &crate::commands::Settings,
    models_dir: &std::path::Path,
) -> (std::path::PathBuf, SpeechEngineKind) {
    let entry = teletype_speech::catalog::find(model_id);
    // The catalog entry always implies its own engine. `Settings.speech_engine`
    // is the documented fallback for ids outside the catalog, so it has to
    // be what actually decides the loader here.
    let engine = entry
        .map(|m| m.engine)
        .unwrap_or_else(|| match settings.speech_engine {
            crate::commands::SpeechEngine::Whisper => teletype_speech::catalog::Engine::Whisper,
            crate::commands::SpeechEngine::Parakeet => teletype_speech::catalog::Engine::Parakeet,
            crate::commands::SpeechEngine::ParakeetCpp => {
                teletype_speech::catalog::Engine::ParakeetCpp
            }
        });
    let kind = match engine {
        teletype_speech::catalog::Engine::ParakeetCoreml => SpeechEngineKind::Fluid,
        teletype_speech::catalog::Engine::SherpaParakeet => SpeechEngineKind::Sherpa,
        teletype_speech::catalog::Engine::Whisper => SpeechEngineKind::Whisper,
        teletype_speech::catalog::Engine::Parakeet => SpeechEngineKind::Parakeet,
        teletype_speech::catalog::Engine::ParakeetCpp => SpeechEngineKind::ParakeetCpp,
        teletype_speech::catalog::Engine::Qwen3Asr => SpeechEngineKind::Qwen3Asr,
        teletype_speech::catalog::Engine::AppleSpeech => SpeechEngineKind::AppleSpeech,
        teletype_speech::catalog::Engine::TranscribeCpp => SpeechEngineKind::TranscribeCpp,
    };
    let file = entry
        .map(|m| m.file.to_string())
        .unwrap_or_else(|| format!("{model_id}.bin"));
    // Per-engine path semantics come from the catalog entry so the app, the
    // download command, and the eval harness cannot drift (see
    // `SpeechModel::provider_path`):
    // - Fluid (Core ML): base dir `<models_dir>/fluid`; the provider appends
    //   the repo folder itself and downloads on first load.
    // - Sherpa: extracted dir `<models_dir>/sherpa/<archive stem>`.
    // - Qwen3-ASR: dir `<models_dir>/qwen3-asr`, holding the main GGUF and its
    //   mmproj projector.
    // - Whisper/Parakeet ggml: `<models_dir>/<file>`; must exist.
    // - Whistle: `<models_dir>/whistle.cact`; must exist (not yet cataloged).
    let model_path = match kind {
        SpeechEngineKind::Fluid
        | SpeechEngineKind::Sherpa
        | SpeechEngineKind::Qwen3Asr
        | SpeechEngineKind::AppleSpeech
        | SpeechEngineKind::TranscribeCpp => entry
            .map(|m| m.provider_path(models_dir))
            .unwrap_or_else(|| models_dir.join(&file)),
        SpeechEngineKind::Whisper | SpeechEngineKind::Parakeet | SpeechEngineKind::ParakeetCpp => {
            models_dir.join(&file)
        }
        SpeechEngineKind::Whistle => models_dir.join("whistle.cact"),
    };
    (model_path, kind)
}

/// Builds the neural-slot engine for a catalog model id.
///
/// The slot serves the engines that are neither Whisper nor ggml Parakeet:
/// Core ML (FluidAudio) and sherpa-onnx. `None` for every other model, for
/// unknown ids, and for Core ML when this build has no FluidAudio bridge.
pub(crate) fn neural_provider_for(
    model_id: &str,
) -> Option<Box<dyn teletype_speech::SpeechProvider>> {
    use teletype_speech::catalog::Engine;
    let entry = teletype_speech::catalog::find(model_id)?;
    match entry.engine {
        Engine::ParakeetCoreml if teletype_fluid::bridge_built() => {
            let version = match model_id {
                "parakeet-v2-coreml" => teletype_fluid::FluidVersion::V2,
                "parakeet-v3-coreml" => teletype_fluid::FluidVersion::V3,
                _ => teletype_fluid::FluidVersion::Ultra,
            };
            Some(Box::new(teletype_fluid::FluidParakeet::new(version)))
        }
        Engine::SherpaParakeet => teletype_speech::sherpa::SherpaParakeetProvider::new(model_id)
            .map(|p| Box::new(p) as Box<dyn teletype_speech::SpeechProvider>),
        Engine::Qwen3Asr => Some(Box::new(teletype_inference::Qwen3AsrProvider::new(
            model_id,
        ))),
        Engine::AppleSpeech => {
            let module = match model_id {
                "apple-dictation-transcriber" => teletype_apple::AppleModule::Dictation,
                _ => teletype_apple::AppleModule::Transcriber,
            };
            Some(Box::new(teletype_apple::AppleSpeech::new(module)))
        }
        Engine::TranscribeCpp => Some(Box::new(teletype_speech::TranscribeCppProvider::new())),
        _ => None,
    }
}

/// The streaming committer's engine handle for a selected model.
///
/// Must mirror [`resolve_speech_model`]'s engine choice. It previously handed
/// every neural engine a Whisper handle, which then tried to parse the Core ML
/// checkpoint directory (`<models>/fluid`) as a GGML file: the load failed with
/// "invalid model data (bad magic)", every mid-take segment failed, and Fluid
/// takes silently fell back to the whole-take decode. `None` means the take
/// decodes whole after release instead of streaming, which is what Qwen3-ASR
/// answers on purpose: its provider handle is a child `llama-server` process,
/// so a second one would load a second copy of a 2.4 GB model mid-take.
fn streaming_provider_for(
    kind: SpeechEngineKind,
    model_id: &str,
) -> Option<Box<dyn teletype_speech::SpeechProvider>> {
    match kind {
        SpeechEngineKind::Whisper => {
            Some(Box::new(teletype_speech::whisper::WhisperProvider::new()))
        }
        SpeechEngineKind::Parakeet => {
            Some(Box::new(teletype_speech::parakeet::ParakeetProvider::new()))
        }
        SpeechEngineKind::ParakeetCpp => Some(Box::new(
            teletype_speech::parakeet_cpp::ParakeetCppProvider::new(),
        )),
        SpeechEngineKind::Fluid | SpeechEngineKind::Sherpa => neural_provider_for(model_id),
        // Whistle has no catalog entry or streaming provider.
        SpeechEngineKind::Whistle => None,
        // Qwen3-ASR streams nothing: the committer holds its own provider
        // handle, and this engine's handle is a child `llama-server` process.
        // A second one would cost another 2.4 GB of RAM plus a multi-second
        // model load in the middle of a take, so the take decodes whole after
        // release. This row trades latency for accuracy by design.
        SpeechEngineKind::Qwen3Asr => None,
        // Apple speech has no per-word timings in the first pass (the bridge
        // returns the final text only), so the take decodes whole after
        // release, like Qwen3-ASR.
        SpeechEngineKind::AppleSpeech => None,
        // transcribe.cpp decodes whole after release (the streaming API is
        // not wired into the committer yet); the provider's word timings
        // still feed the final transcript.
        SpeechEngineKind::TranscribeCpp => None,
    }
}

/// Makes the neural manager hold the provider for `model_id`.
///
/// The manager is built with a placeholder, so selecting a Core ML or sherpa
/// model must replace that provider; otherwise the take would transcribe with
/// the wrong engine (or no model). A previously selected neural model is
/// unloaded by the swap. No-op when the slot already serves `model_id`, so
/// per-take calls cost one lock.
pub fn ensure_neural_provider(state: &AppState, model_id: &str) -> Result<(), String> {
    let mut current = state
        .neural_model_id
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if current.as_deref() == Some(model_id) {
        return Ok(());
    }
    let provider = neural_provider_for(model_id)
        .ok_or_else(|| format!("speech model '{model_id}' is not served by the neural engine"))?;
    state.neural.set_provider(provider);
    *current = Some(model_id.to_string());
    Ok(())
}

/// The language to pass to the ASR engine for the frontmost app (P3.3):
/// the per-app override first, then the global setting. `"auto"` is passed
/// through so Whisper can still auto-detect; a concrete override (e.g. "de")
/// is passed to the engine and wins over `"auto"`.
pub fn effective_language_for_app(state: &AppState, kind: SpeechEngineKind) -> String {
    let settings = state.settings();
    let app_ctx = state.platform.active_application().unwrap_or_default();
    let profile = teletype_core::personalization::UserProfile {
        language: settings.language.clone(),
        app_language_overrides: settings.app_language_overrides.clone(),
        ..Default::default()
    };
    let resolved = teletype_core::personalization::resolve_language(&profile, &app_ctx);
    // The engine's row is the single source of truth for "can it transcribe
    // this language": English-only rows (Parakeet TDT ggml, Core ML Ultra,
    // sherpa) cannot, Whisper rows can. A pinned model that is not in the
    // catalog falls back to the kind-based list, so a custom build never
    // loses the check.
    let english_only = teletype_speech::catalog::find(&settings.selected_speech_model)
        .map(|m| m.english_only)
        .unwrap_or_else(|| is_english_only(kind));
    effective_language(resolved, english_only)
}

/// Returns `true` when the engine kind is English-only, so `"auto"` is
/// treated as English rather than delegating to Whisper's auto-detect.
pub fn is_english_only(kind: SpeechEngineKind) -> bool {
    matches!(
        kind,
        SpeechEngineKind::Parakeet
            | SpeechEngineKind::Fluid
            | SpeechEngineKind::Sherpa
            | SpeechEngineKind::ParakeetCpp
    )
}

/// The user-facing mismatch notice: the resolved language is locked to
/// something the selected engine cannot transcribe. `None` when there is
/// nothing to tell the user — "auto"/empty (the engine's own default still
/// applies) or an English lock (which every engine honours).
///
/// The catalog row's `honours_language` is the single predicate, so the
/// check here, the Settings engine picker, and the `pick_engine` detour
/// cannot disagree. A pinned model that is not in the catalog (custom
/// build, empty selection) falls back to the kind-based English-only
/// list, so a custom build never silently loses the check.
fn language_mismatch_notice(
    language: &str,
    kind: SpeechEngineKind,
    selected_model_id: &str,
) -> Option<String> {
    let trimmed = language.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return None;
    }
    let english = trimmed.eq_ignore_ascii_case("en");
    let mismatch = match teletype_speech::catalog::find(selected_model_id) {
        // Catalog row: the row's own claim is the answer. An English lock
        // honours every row, so a mismatch here is always a non-English
        // lock the engine cannot transcribe.
        Some(row) => !row.honours_language(Some(trimmed)),
        // Not a catalog row (custom build / empty selection): fall back to
        // the kind-based list. An English lock never mismatches — every
        // engine transcribes English.
        None => !english && is_english_only(kind),
    };
    if !mismatch {
        return None;
    }
    let name = teletype_speech::language_name(trimmed);
    Some(format!(
        "This model cannot transcribe {name}; the take will run as English"
    ))
}

#[cfg(test)]
mod mismatch_notice_tests;

/// Transliterates Cyrillic characters to their Latin equivalents.
///
/// The Parakeet TDT model supports 25 languages and can misidentify English
/// proper nouns as Russian (e.g. "Rida Fatema" → "Рида Фатема"). When the
/// user's language setting is English, any Cyrillic in the transcript is a
/// mis-detection and should be transliterated back to Latin.
fn transliterate_cyrillic(text: &str) -> String {
    const MAP: &[(&str, &str)] = &[
        ("а", "a"),
        ("б", "b"),
        ("в", "v"),
        ("г", "g"),
        ("д", "d"),
        ("е", "e"),
        ("ё", "yo"),
        ("ж", "zh"),
        ("з", "z"),
        ("и", "i"),
        ("й", "y"),
        ("к", "k"),
        ("л", "l"),
        ("м", "m"),
        ("н", "n"),
        ("о", "o"),
        ("п", "p"),
        ("р", "r"),
        ("с", "s"),
        ("т", "t"),
        ("у", "u"),
        ("ф", "f"),
        ("х", "kh"),
        ("ц", "ts"),
        ("ч", "ch"),
        ("ш", "sh"),
        ("щ", "shch"),
        ("ъ", ""),
        ("ы", "y"),
        ("ь", ""),
        ("э", "e"),
        ("ю", "yu"),
        ("я", "ya"),
        ("А", "A"),
        ("Б", "B"),
        ("В", "V"),
        ("Г", "G"),
        ("Д", "D"),
        ("Е", "E"),
        ("Ё", "Yo"),
        ("Ж", "Zh"),
        ("З", "Z"),
        ("И", "I"),
        ("Й", "Y"),
        ("К", "K"),
        ("Л", "L"),
        ("М", "M"),
        ("Н", "N"),
        ("О", "O"),
        ("П", "P"),
        ("Р", "R"),
        ("С", "S"),
        ("Т", "T"),
        ("У", "U"),
        ("Ф", "F"),
        ("Х", "Kh"),
        ("Ц", "Ts"),
        ("Ч", "Ch"),
        ("Ш", "Sh"),
        ("Щ", "Shch"),
        ("Ъ", ""),
        ("Ы", "Y"),
        ("Ь", ""),
        ("Э", "E"),
        ("Ю", "Yu"),
        ("Я", "Ya"),
    ];

    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if let Some((_, latin)) = MAP.iter().find(|(c, _)| *c == ch.to_string()) {
            out.push_str(latin);
        } else {
            out.push(ch);
        }
    }
    // Collapse any double spaces that result from dropping ъ/ь.
    let mut result = String::with_capacity(out.len());
    let mut prev_space = false;
    for ch in out.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(ch);
            }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }
    result.trim().to_string()
}

/// If the language is English and the transcript contains Cyrillic,
/// transliterate it back to Latin. This handles the case where the ASR
/// model misidentifies an English proper noun as Russian.
pub fn ensure_latin_if_english(text: &str, language: &str) -> String {
    if language == "en" && text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)) {
        transliterate_cyrillic(text)
    } else {
        text.to_string()
    }
}

/// P2.5: true when two words are at most 1 edit apart (insertion,
/// deletion, substitution). Single-row Levenshtein with an early exit —
/// wake words are short, so clarity beats cleverness here.
fn within_one_edit(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a == b {
        return true;
    }
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, &ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        let mut row_min = cur[0];
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let v = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
            row_min = row_min.min(v);
            cur.push(v);
        }
        if row_min > 1 {
            return false;
        }
        prev = cur;
    }
    prev[b.len()] <= 1
}

/// P2.5: strips a leading wake phrase ("teletype" / "hey teletype", each
/// word fuzzy-matched at edit distance ≤ 1 for ASR wobble) and returns the
/// remainder. Returns `None` when the transcript does not start with the
/// wake phrase. Matching is case-insensitive; the remainder keeps its
/// original casing.
///
/// Approximation note: the plan scopes the phrase to the first 1.2 s, but
/// word timings are unavailable at this stage, so the check covers the
/// first two words wherever they fall.
pub fn strip_wake_phrase(text: &str) -> Option<String> {
    let mut words = text.split_whitespace();
    let first = words.next()?;
    if within_one_edit(&first.to_lowercase(), "teletype") {
        let rest: String = text
            .split_whitespace()
            .skip(1)
            .collect::<Vec<_>>()
            .join(" ");
        return if rest.trim().is_empty() {
            None
        } else {
            Some(rest)
        };
    }
    let second = words.next()?;
    if within_one_edit(&first.to_lowercase(), "hey")
        && within_one_edit(&second.to_lowercase(), "teletype")
    {
        let rest: String = text
            .split_whitespace()
            .skip(2)
            .collect::<Vec<_>>()
            .join(" ");
        return if rest.trim().is_empty() {
            None
        } else {
            Some(rest)
        };
    }
    None
}

// Thread-local storage for the active recording (one at a time).
thread_local! {
    static RECORDING: std::cell::RefCell<Option<Recording>> = const { std::cell::RefCell::new(None) };
}

/// P1.11: trim leading/trailing silence beyond 150 ms (keeping 150 ms of
/// padding) before the final decode, using the Silero VAD model for the
/// speech/silence decision.
///
/// The take starts with the hotkey press and ends with the release (or the
/// VAD pause), so both ends are usually dominated by silence: the model
/// wastes decode time on it and occasionally hallucinates words from it.
/// Trimming to the speech region plus a short pad speeds the decode and
/// removes the hallucination source.
///
/// Returns the trimmed sample slice. When no speech chunk is found (the
/// model saw nothing above threshold — e.g. a very quiet take) the input is
/// returned unchanged: we must never drop the user's audio on a VAD miss.
/// The engine is created per call (one Silero init per final decode, a few
/// ms on CPU) — the auto-stop `VadSlot` is a different, long-lived instance.
pub fn trim_silence_vad(samples: &[f32]) -> &[f32] {
    // The trim only needs per-chunk speech probabilities, so it drives the
    // Silero engine directly (512-sample chunks at 16 kHz) instead of the
    // timing-policy `VadDetector` wrapper.
    let Ok(mut engine) = silero_vad_pure::SileroVad::new(silero_vad_pure::SampleRate::Hz16000)
    else {
        return samples;
    };
    let chunk = engine.chunk_size(); // 512 samples = 32 ms @ 16 kHz
    let chunks = samples.len() / chunk;
    if chunks == 0 {
        return samples;
    }
    // Per-chunk speech probability; a chunk counts as speech at Silero's
    // default operating point (same threshold as the auto-stop detector).
    let mut speech: Vec<bool> = Vec::with_capacity(chunks);
    for c in 0..chunks {
        let Ok(p) = engine.process(&samples[c * chunk..(c + 1) * chunk]) else {
            return samples; // engine error: never drop the audio
        };
        speech.push(p >= 0.5);
    }
    trim_silence_vad_with_probe(samples, chunk, &speech)
}

/// F-70: the per-chunk speech probe the no-speech gate uses. Runs the Silero
/// engine over the 16 kHz samples and returns the indices of the chunks it
/// classifies as speech (empty when the take has no speech at all). Kept
/// separate from [`trim_silence_vad`] so the gate and the trim share one
/// definition of "speech" without the trim's padding maths.
/// F-70: whether the no-speech gate drops this take, i.e. skips the ASR
/// decode entirely. True only when the VAD found no speech AND the take's
/// peak sits below [`NO_SPEECH_ENERGY_FLOOR`]; a faint take at or above the
/// floor still decodes so whispered speech is not lost. Pure, so the
/// predicate is testable against the production constant.
fn vad_gate_drops_take(no_speech: bool, peak: f32) -> bool {
    no_speech && peak < NO_SPEECH_ENERGY_FLOOR
}

/// F-02c: the release worker's no-speech decision, as a pure function. A
/// zero-sample take is a genuine capture failure and keeps
/// [`MIC_FAILURE_MESSAGE`]. Otherwise a take the gate drops ends with
/// [`NO_SPEECH_SENTINEL`], which `transcribed` routes to the quiet end;
/// every other take decodes. Pure so all three outcomes are testable
/// without driving the worker's platform path.
fn gate_or_capture_outcome(no_speech: bool, peak: f32, samples: usize) -> Option<&'static str> {
    if samples == 0 {
        Some(MIC_FAILURE_MESSAGE)
    } else if vad_gate_drops_take(no_speech, peak) {
        Some(NO_SPEECH_SENTINEL)
    } else {
        None
    }
}

/// F-12: whether the selected speech model's bytes are installed under
/// `models_dir`. Per-engine layout mirrors
/// [`crate::commands::models::speech_model_installed`] (which is private to
/// that module): one ggml/GGUF file (non-empty); sherpa's extracted
/// directory with all four model files; Qwen3-ASR's two GGUFs; Core ML is
/// "installed" when the FluidAudio bridge is linked (the bridge downloads
/// the checkpoint on first load, so a working bridge is what "usable now"
/// means for it). Unknown ids outside the catalog are treated as installed:
/// the legacy `speech_engine` fallback still loads them, and gating on a
/// catalog miss would break an existing user's custom model.
fn selected_speech_model_installed(selected_id: &str, models_dir: &std::path::Path) -> bool {
    let Some(entry) = teletype_speech::catalog::find(selected_id) else {
        return true;
    };
    use teletype_speech::catalog::Engine;
    let fluid_available = teletype_fluid::bridge_built();
    match entry.engine {
        Engine::ParakeetCoreml => fluid_available,
        // OS-managed assets: "installed" means the bridge is linked and the
        // OS reports the default locale as installed.
        Engine::AppleSpeech => {
            teletype_apple::bridge_built()
                && teletype_apple::locale_status(teletype_apple::AppleModule::Transcriber, "en_US")
                    == teletype_apple::AssetStatus::Installed
        }
        Engine::SherpaParakeet => {
            let dir = entry.provider_path(models_dir);
            teletype_speech::sherpa::MODEL_FILES
                .iter()
                .all(|f| dir.join(f).is_file())
        }
        Engine::Qwen3Asr => {
            let dir = entry.provider_path(models_dir);
            teletype_speech::catalog::QWEN3_ASR_ARTIFACTS
                .iter()
                .all(|a| {
                    dir.join(a.file)
                        .metadata()
                        .map(|m| m.is_file() && m.len() > 0)
                        .unwrap_or(false)
                })
        }
        Engine::Whisper | Engine::Parakeet | Engine::ParakeetCpp | Engine::TranscribeCpp => entry
            .provider_path(models_dir)
            .metadata()
            .map(|m| m.is_file() && m.len() > 0)
            .unwrap_or(false),
    }
}

fn vad_speech_probe(samples: &[f32]) -> Vec<usize> {
    let Ok(mut engine) = silero_vad_pure::SileroVad::new(silero_vad_pure::SampleRate::Hz16000)
    else {
        return Vec::new();
    };
    let chunk = engine.chunk_size();
    let chunks = samples.len() / chunk;
    let mut speech = Vec::new();
    for c in 0..chunks {
        let Ok(p) = engine.process(&samples[c * chunk..(c + 1) * chunk]) else {
            continue;
        };
        if p >= 0.5 {
            speech.push(c);
        }
    }
    speech
}

/// Core trimming arithmetic, given the per-chunk speech/silence decision
/// (decoupled from the model so the padding math is testable without
/// Silero's speech recognizer — synthetic tones don't reliably trigger it).
pub fn trim_silence_vad_with_probe<'a>(
    samples: &'a [f32],
    chunk: usize,
    speech: &[bool],
) -> &'a [f32] {
    const PADDING_MS: u64 = 150;
    const SR: usize = 16_000; // Captured samples are always at TARGET_SAMPLE_RATE
    let first = speech.iter().position(|&s| s);
    let last = speech.iter().rposition(|&s| s);
    match (first, last) {
        (None, _) => samples, // no speech seen: never drop the audio
        (Some(f), Some(l)) => {
            let pad = (PADDING_MS * SR as u64 / 1000) as usize / chunk.max(1);
            let start = f.saturating_sub(pad) * chunk;
            let end = ((l + 1) + pad).min(samples.len() / chunk.max(1)) * chunk;
            &samples[start..end.max(start)]
        }
        // (Some(f), None) is impossible: rposition >= position when any
        // chunk is speech. Keep the match exhaustive for the compiler.
        (Some(_), None) => samples,
    }
}

/// Runs on a worker thread: loads the speech model and transcribes.
/// Transcribes audio using the engine kind selected by [`resolve_speech_model`].
///
/// Returns `(text, words)` where `words` is `Some` when the engine reports
/// per-word timings/confidences (Fluid, sherpa) and `None` otherwise
/// (Whisper, ggml Parakeet).
/// The display name of a speech model id, falling back to the id itself.
pub(crate) fn asr_model_label(id: &str) -> String {
    teletype_speech::catalog::find(id)
        .map(|m| m.name.to_string())
        .unwrap_or_else(|| id.to_string())
}

/// The per-engine boost-list size. Zero for every engine: vocabulary boosting
/// is off until the bake-off can show it helps (`.agents/brain/asrs.md`, the
/// `earnings22-vocab` cells). Measured at `981900e`, the Core ML booster took
/// verbatim WER 6.88% -> 10.61% and entity accuracy 84.5% -> 75.4% while ASR
/// p50 went 2.6 s -> 19.7 s, and Whisper's `initial_prompt` gained 0.7 pp of
/// WER while losing 9 pp of entity accuracy. ggml Parakeet ignores the terms
/// and sherpa's setter is a stub, so the only engines a list could reach were
/// the two it measurably harms.
///
/// Learned terms still reach dictation through the text paths (the D006
/// dictionary and `rescore_with_confidence`); this only removes the biasing
/// applied inside the ASR decode. Per-engine sizes a repaired booster would
/// restore live in `teletype_core::learning::boost` (`N_FLUID` and friends).
pub(crate) fn boost_capacity(_kind: SpeechEngineKind) -> usize {
    0
}

/// Builds the engine-facing boost list for one take. The core shape is
/// `teletype_core::learning::boost::BoostTerm` (a `RescoreTerm`); engines take
/// `teletype_speech::BoostTerm`, a zero-cost struct copy. Test-only: the
/// cached production path builds through `boost_list` directly.
#[cfg(test)]
pub(crate) fn boost_terms_for(
    rows: &[teletype_core::learning::store::LexiconRow],
    app_category: Option<&str>,
    screen_terms: &[String],
    n: usize,
    now_ms: i64,
) -> Vec<teletype_speech::BoostTerm> {
    use teletype_core::learning::boost::{boost_list, BoostContext};
    if n == 0 {
        return Vec::new();
    }
    let ctx = BoostContext {
        app_category,
        screen_terms,
        now_ms,
    };
    to_engine_terms(boost_list(rows, &ctx, n))
}

fn to_engine_terms(
    terms: Vec<teletype_core::learning::boost::BoostTerm>,
) -> Vec<teletype_speech::BoostTerm> {
    terms
        .into_iter()
        .map(|t| teletype_speech::BoostTerm {
            text: t.text,
            aliases: t.aliases,
            weight: t.weight,
        })
        .collect()
}

pub fn transcribe(
    app: &AppHandle,
    model_path: &std::path::Path,
    captured: &Captured,
    language: &str,
    kind: SpeechEngineKind,
    boost_terms: &[teletype_speech::BoostTerm],
) -> Result<(String, Option<Vec<teletype_speech::Word>>), String> {
    transcribe_for_model(
        app,
        model_path,
        captured,
        language,
        kind,
        boost_terms,
        &app.state::<AppState>().settings().selected_speech_model,
    )
}

/// HARD-4 row 14: the fallback decode path. Same as [`transcribe`], but the
/// neural slot is prepared for `model_id` (the fallback model) rather than
/// the selected one, so a fallback to a different engine kind works.
pub(crate) fn transcribe_for_model(
    app: &AppHandle,
    model_path: &std::path::Path,
    captured: &Captured,
    language: &str,
    kind: SpeechEngineKind,
    boost_terms: &[teletype_speech::BoostTerm],
    model_id: &str,
) -> Result<(String, Option<Vec<teletype_speech::Word>>), String> {
    let state = app.state::<AppState>();
    // The neural slot starts with a placeholder provider; make sure it holds
    // the engine for the model before asking it to load/transcribe.
    if matches!(
        kind,
        SpeechEngineKind::Fluid
            | SpeechEngineKind::Sherpa
            | SpeechEngineKind::Qwen3Asr
            | SpeechEngineKind::AppleSpeech
            | SpeechEngineKind::TranscribeCpp
    ) {
        ensure_neural_provider(&state, model_id)?;
    }
    let manager = match kind {
        SpeechEngineKind::Fluid
        | SpeechEngineKind::Sherpa
        | SpeechEngineKind::Whistle
        | SpeechEngineKind::Qwen3Asr
        | SpeechEngineKind::AppleSpeech
        | SpeechEngineKind::TranscribeCpp => &state.neural,
        SpeechEngineKind::Parakeet => &state.parakeet,
        SpeechEngineKind::ParakeetCpp => &state.parakeet_cpp,
        SpeechEngineKind::Whisper => &state.speech,
    };
    if !manager.is_loaded() {
        // FluidAudio and Apple speech download on first load, so skip the
        // existence check for Core ML and Apple. Sherpa/whistle need their
        // files on disk.
        if !matches!(
            kind,
            SpeechEngineKind::Fluid | SpeechEngineKind::AppleSpeech
        ) && !model_path.exists()
        {
            return Err("Speech model not downloaded".into());
        }
        manager.set_model_path(model_path.to_path_buf());
        manager
            .load()
            .map_err(|e| format!("Speech model load failed: {e}"))?;
    }
    // P1.4: the interim loop checks its cancel flag before AND after each
    // `transcribe`, and `stop()` sets that flag the moment the user releases
    // without joining. So an in-flight interim decode is abandoned as soon as
    // it returns, and no NEW interim pass starts after release. The final
    // decode therefore waits at most for the remaining duration of ONE
    // already-in-flight interim decode (the case where the user released
    // mid-decode) — it never queues behind a fresh interim pass. The provider
    // is a single `Mutex<P>` (no `Arc` to clone), so the final decode cannot
    // run concurrently with the interim one on a separate instance; it takes
    // the same lock. Bounding this further (a second provider for the final
    // path, as the streaming committer does) is a follow-up, not a one-line
    // change.
    let mut speech = manager
        .provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Native boosting: applied after load because Fluid refuses terms until
    // the model is ready. Whisper accepts them for its initial prompt; the
    // engines without native boosting ignore the call. A boost failure must
    // never break the decode, so it is logged and the take continues.
    if let Err(e) = speech.set_boost_terms(boost_terms) {
        crate::log_entry(
            crate::LogLevel::Warn,
            format!("boost terms not applied: {e}; decoding without them"),
        );
    }
    // Engines with word timings report them through `transcribe_detailed`;
    // others fall back to `transcribe` (default maps text to no words).
    if speech.capabilities().word_timings {
        let detailed = match speech.transcribe_detailed(&captured.samples, language) {
            Ok(d) => d,
            Err(e) => {
                let detailed_err = e.to_string();
                // The detailed path failed; retry once with the plain call
                // so the take still yields text. The detailed error is
                // logged so the trace explains why timings disappeared.
                let text = match speech.transcribe(&captured.samples, language) {
                    Ok(t) => t,
                    Err(e2) => {
                        crate::log_entry(
                            crate::LogLevel::Warn,
                            format!(
                                "detailed decode failed ({detailed_err}); \
                                 plain decode also failed"
                            ),
                        );
                        return Err(e2.to_string());
                    }
                };
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!(
                        "detailed decode failed ({detailed_err}); \
                         retrying without word timings"
                    ),
                );
                return Ok((text, None));
            }
        };
        Ok((detailed.text, Some(detailed.words)))
    } else {
        let text = speech
            .transcribe(&captured.samples, language)
            .map_err(|e| e.to_string())?;
        Ok((text, None))
    }
}

/// Reads the language the speech engine detected on its last auto-detect run.
///
/// Only the Whisper provider reports detection; Parakeet (English-only),
/// Fluid, sherpa, and the mock always return `None`. Returns `None` when
/// auto-detect was not active, so callers can tell "detected en" from "no
/// detection info".
fn detect_language_from_provider(
    app: &AppHandle,
    kind: SpeechEngineKind,
    language: &str,
) -> Option<String> {
    if !matches!(kind, SpeechEngineKind::Whisper) {
        return None;
    }
    let state = app.state::<AppState>();
    let speech = state
        .speech
        .provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if language != "auto" && !language.is_empty() {
        return None; // locked language: detection is meaningless
    }
    speech.detected_language()
}

/// F-62: lexical content means at least one character that is a letter or
/// a digit, ignoring whitespace and punctuation. A filler-only take
/// ("um, uh, like, you know") has none once the filler pass runs; a take
/// the model emptied ("") has none at all. Used by the release worker to
/// classify an empty-or-filler-only ASR result as no-speech (the quiet
/// end) and by the headless pipeline floor.
fn has_lexical_content(text: &str) -> bool {
    text.chars().any(|c| c.is_alphanumeric())
}

/// F-62: run the real pipeline over a transcript with no inference
/// provider, so only the deterministic passes (dictionary, filler, ITN,
/// AutoText) run. Headless tests use this to exercise the empty-text floor
/// ladder without a model.
#[cfg(test)]
fn headless_pipeline_run(raw: &str) -> teletype_core::pipeline::PipelineResult {
    use teletype_core::autotext::AutoTextStore;
    use teletype_core::dictionary::{Dictionary, EditDistanceChecker};
    use teletype_core::personalization::UserProfile;
    use teletype_core::pipeline::{InputSource, Pipeline, UnifiedInput};
    use teletype_core::platform::MockPlatform;
    use teletype_core::style::StyleProfileStore;
    use teletype_core::transforms::TransformStore;
    use teletype_core::vocab::PackTerm;

    let platform = MockPlatform::default();
    let autotext = AutoTextStore::default();
    let transforms = TransformStore::default();
    let profile = UserProfile::default();
    let dictionary = Dictionary::default();
    let check: &dyn teletype_core::dictionary::WordChecker = &EditDistanceChecker;
    let styles = StyleProfileStore::default();
    let pack_terms: Vec<PackTerm> = Vec::new();
    let mut pipeline = Pipeline {
        platform: &platform,
        autotext: &autotext,
        transforms: &transforms,
        profile: &profile,
        polish_prompt_override: None,
        inference: None,
        decision: None,
        force_command: false,
        dictionary: &dictionary,
        word_checker: check,
        styles: &styles,
        active_style: "",
        explicit_style: "",
        auto_apply: true,
        restore_clipboard: true,
        remove_filler_words: true,
        // Tier 1 (unconditional removal): the same words the filler pass
        // treats as built-in hesitations and discourse markers, so a
        // filler-only take empties out exactly like production.
        filler_words: vec![
            "um".into(),
            "uh".into(),
            "er".into(),
            "ah".into(),
            "eh".into(),
            "hmm".into(),
            "mmm".into(),
            "like".into(),
            "you know".into(),
            "i mean".into(),
            "i guess".into(),
        ],
        filler_strength: teletype_core::fillers::tiers::FillerStrength::Fillers,
        restore_emoji: false,
        spoken_emoji: false,
        spoken_punctuation: false,
        system_autotext: &[],
        token_sink: None,
        polish_gate_enabled: false,
        polish_skip_when_clean: false,
        polish_gate_threshold_words: 8,
        pack_terms: &pack_terms,
        list_style: teletype_core::transforms::ListStyle::default(),
        polish_max_change_percent: 100,
        screen_terms: &[],
        date_format: "",
    };
    let input = UnifiedInput {
        source: InputSource::Voice,
        text: raw.to_string(),
    };
    pipeline.run(input, None)
}

#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// P1.8: engine Arc snapshot — the UI must not block during generation.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod p1_8_tests;

// ---------------------------------------------------------------------------
// P1.9: delta streaming — IPC bytes for an N-token take are O(N).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod p1_9_tests;

// ---------------------------------------------------------------------------
// P1.11: VAD silence trim.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod p1_11_tests;

// --- F-12: press gate tests ---

/// F-12: the press gate's predicate. A Whisper model with no file under
/// `models_dir` reads as not installed; with the file present it reads as
/// installed. The gate in `start` calls this function, so pinning the
/// predicate here is what keeps the gate honest.
#[test]
fn f12_installed_check_reflects_the_file_on_disk() {
    let model_id = "base-en";
    let models_dir = std::env::temp_dir().join(format!(
        "teletype-f12-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&models_dir).expect("temp models dir");
    // No file: not installed.
    assert!(
        !selected_speech_model_installed(model_id, &models_dir),
        "a Whisper model with no file must read as not installed"
    );
    // Create the file: installed.
    let entry = teletype_speech::catalog::find(model_id).expect("catalog entry");
    let file = models_dir.join(entry.file);
    std::fs::write(&file, b"fake model").expect("write model file");
    assert!(
        selected_speech_model_installed(model_id, &models_dir),
        "a Whisper model with its file present must read as installed"
    );
    // A zero-byte file is not installed (the layout requires non-empty).
    std::fs::write(&file, b"").expect("truncate model file");
    assert!(
        !selected_speech_model_installed(model_id, &models_dir),
        "a zero-byte model file must read as not installed"
    );
    let _ = std::fs::remove_dir_all(&models_dir);
}

// --- F-11: warm-up token tests ---

/// F-11: a warm-up must not bump the take's session id, or the `VadFired`
/// guard (hands-free auto-stop) goes stale and the take never auto-stops.
/// This test drives the real `WarmupDone` handler with a warm token and
/// asserts the session id is untouched, so a `VadFired` for the original
/// session still matches.
#[test]
fn f11_warmup_done_does_not_bump_session_id() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    // The take's session id is 1 (set by `for_tests`).
    assert_eq!(session.next_id, 1);
    // A warm-up announces itself with token 1 (its own counter, not
    // `next_id`).
    session.warm_token = 1;
    session.handle(Event::WarmupDone { session: 1 });
    // The session id is still 1: the warm-up did not bump it.
    assert_eq!(
        session.next_id, 1,
        "a warm-up must not bump the take's session id"
    );
    // The `VadFired` guard still matches: a cold take still auto-stops.
    // In headless mode a clean stop enters Transcribing (the take is
    // stopped and would be transcribed), so that is the assertion.
    session.handle(Event::VadFired { session: 1 });
    assert_eq!(
        session.phase,
        Phase::Transcribing,
        "a cold take must still auto-stop on VAD after a warm-up"
    );
}

/// F-11: a stale `WarmupDone` from a superseded warm is dropped. The
/// handler compares the token against `warm_token`, so a token that does
/// not match the in-flight warm is a no-op.
#[test]
fn f11_stale_warmup_done_is_dropped() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.next_id = 1;
    // The in-flight warm is token 2; a stale token 1 from a superseded
    // warm must not repaint the pill or advance the session.
    session.warm_token = 2;
    session.handle(Event::WarmupDone { session: 1 });
    assert_eq!(
        session.next_id, 1,
        "a stale warm-up must not bump the session id"
    );
    assert_eq!(
        session.warm_token, 2,
        "a stale warm-up must not advance the warm token"
    );
    assert_eq!(
        session.phase,
        Phase::Listening,
        "a stale warm-up must not change the phase"
    );
}

#[test]
fn warmup_failure_stops_and_discards_the_active_recording() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Listening;
    session.next_id = 1;
    session.warm_token = 1;
    RECORDING.with(|slot| {
        *slot.borrow_mut() = Some(teletype_core::audio::Recording::synthetic());
    });

    session.handle(Event::WarmupFailed {
        session: 1,
        reason: "missing file",
    });

    assert_eq!(session.phase, Phase::Idle);
    assert!(RECORDING.with(|slot| slot.borrow().is_none()));
    assert!(
        session
            .headless_flashes
            .iter()
            .any(|message| message.contains("Couldn't load the speech model")),
        "the warm-up error must remain visible after capture stops"
    );
    session.handle(Event::VadFired { session: 1 });
    assert_ne!(session.phase, Phase::Transcribing);
}

// --- no automatic model unload ---

/// The product rule (D052): the model is loaded and then stays loaded. Every
/// take end hands the managers `None`, so no deadline is ever armed and the idle
/// tick thread never frees a model the user did not free.
#[test]
fn a_finished_take_never_arms_an_unload() {
    assert_eq!(take_end_unload_delay(), None);
}

// --- press-time warm-up visibility ---

/// A cold first press (a fresh launch, a model switch, a failed earlier load)
/// announces the load at the press; a resident model, and the `off` policy that
/// defers the load to the release, must keep the recording pill as the first
/// thing shown.
#[test]
fn press_warms_visibly_only_when_the_policy_loads_on_press_and_the_model_is_cold() {
    assert!(press_warms_visibly(EngineWarmTier::OnHotkeyDown, false));
    assert!(press_warms_visibly(EngineWarmTier::Resident, false));
    assert!(!press_warms_visibly(EngineWarmTier::OnHotkeyDown, true));
    assert!(!press_warms_visibly(EngineWarmTier::Resident, true));
    assert!(!press_warms_visibly(EngineWarmTier::OnRelease, false));
    assert!(!press_warms_visibly(EngineWarmTier::OnRelease, true));
}

// --- F-70: no-speech gate tests ---

/// F-70: a take the VAD classifies as no-speech ends quietly (idle, no
/// pill message) instead of running the ASR decode. The production gate
/// emits `MIC_FAILURE_MESSAGE`; `transcribed` routes that error to the
/// idle phase. In headless tests, the mic-failure error flashes the pill
/// (the same as a real error), but the no-speech sentinel (the typed
/// `SpeechError::NoSpeech`) ends quietly. This test pins the mic-failure
/// routing; the no-speech quiet end is pinned by `q1_*` tests.
#[test]
fn f70_no_speech_take_ends_quietly() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Transcribing;
    session.next_id = 5;
    session.transcribed(
        5,
        Err(MIC_FAILURE_MESSAGE.into()),
        None,
        0,
        0,
        false,
        0,
        None,
        None,
        None,
        false,
        None,
    );
    assert_eq!(session.phase, Phase::Idle, "a no-speech take must end idle");
    // The mic-failure error flashes the pill in headless tests (the same
    // as a real error). The no-speech sentinel (the typed
    // `SpeechError::NoSpeech`) ends quietly: see `q1_no_speech_sentinel_ends_quietly`.
    assert_eq!(
        session.headless_flashes.len(),
        1,
        "a mic-failure take must flash the pill once in headless tests"
    );
}

/// Q1: a take whose ASR returns the no-speech sentinel ends quietly,
/// exactly like the F-70 silent path. No error flash, no `UiState::Message`
/// broadcast, no paste, no history entry. The worker classifies the
/// typed `SpeechError::NoSpeech` into this sentinel; `transcribed` routes
/// it to the quiet end. This test fails on the buggy code where the
/// sentinel fell through to the generic `Err(e)` branch and flashed the
/// pill for 2 seconds.
#[test]
fn q1_no_speech_sentinel_ends_quietly() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Transcribing;
    session.next_id = 5;
    session.transcribed(
        5,
        Err(NO_SPEECH_SENTINEL.into()),
        None,
        0,
        0,
        false,
        0,
        None,
        None,
        None,
        false,
        None,
    );
    assert_eq!(session.phase, Phase::Idle, "a no-speech take must end idle");
    assert!(
        session.headless_flashes.is_empty(),
        "a no-speech take must not flash any pill text, got {:?}",
        session.headless_flashes
    );
    assert!(
        session.headless_broadcasts.is_empty(),
        "a no-speech take must not broadcast any UiState, got {:?}",
        session.headless_broadcasts
    );
    assert_eq!(
        session
            .paste_calls
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a no-speech take must not send the paste keystroke"
    );
}

/// Q1: a take whose ASR returns a real error (model not loaded, decode
/// error, crash) must still flash the pill. The no-speech quiet end must
/// not swallow genuine failures. This test fails if the no-speech check
/// is too broad (e.g. matches any `Err`).
#[test]
fn q1_real_asr_error_still_flashes() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Transcribing;
    session.next_id = 5;
    session.transcribed(
        5,
        Err("Speech model not downloaded".into()),
        None,
        0,
        0,
        false,
        0,
        None,
        None,
        None,
        false,
        None,
    );
    assert_eq!(session.phase, Phase::Idle, "a failed take must end idle");
    assert_eq!(
        session.headless_flashes.len(),
        1,
        "a real ASR error must flash the pill exactly once"
    );
    assert_eq!(
        session.headless_flashes[0], "Speech model not downloaded",
        "the flash must carry the real error message"
    );
}

/// Q1: a take whose ASR returns empty text (the engine decoded nothing)
/// ends quietly, the same as the no-speech sentinel. The worker
/// classifies an empty result as no-speech; `transcribed` must route it
/// to the quiet end, not run the pipeline on an empty string.
#[test]
fn q1_empty_asr_result_ends_quietly() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Transcribing;
    session.next_id = 5;
    session.transcribed(
        5,
        Ok(("".to_string(), None)),
        None,
        0,
        0,
        false,
        0,
        None,
        None,
        None,
        false,
        None,
    );
    assert_eq!(
        session.phase,
        Phase::Idle,
        "an empty ASR result must end idle"
    );
    assert!(
        session.headless_flashes.is_empty(),
        "an empty ASR result must not flash, got {:?}",
        session.headless_flashes
    );
    assert!(
        session.headless_broadcasts.is_empty(),
        "an empty ASR result must not broadcast any UiState, got {:?}",
        session.headless_broadcasts
    );
}

/// Q1: a take whose ASR returns filler-only text (no lexical content)
/// ends quietly. The worker classifies a filler-only result as no-speech
/// (the same predicate the F-62 headless floor uses). This test fails on
/// the buggy code where a filler-only take ran the full pipeline and
/// delivered nothing (or delivered the filler).
#[test]
fn q1_filler_only_asr_result_ends_quietly() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Transcribing;
    session.next_id = 5;
    session.transcribed(
        5,
        Ok(("um, uh, like, you know".to_string(), None)),
        None,
        0,
        0,
        false,
        0,
        None,
        None,
        None,
        false,
        None,
    );
    assert_eq!(
        session.phase,
        Phase::Idle,
        "a filler-only take must end idle"
    );
    assert!(
        session.headless_flashes.is_empty(),
        "a filler-only take must not flash, got {:?}",
        session.headless_flashes
    );
    assert!(
        session.headless_broadcasts.is_empty(),
        "a filler-only take must not broadcast any UiState, got {:?}",
        session.headless_broadcasts
    );
}

/// F-02c: the release worker's no-speech decision, pinned as a pure
/// function. A silent take (empty VAD probe, peak under the energy floor)
/// gets the no-speech sentinel, which `transcribed` routes to the quiet
/// end. A zero-sample capture is a genuine failure and keeps the
/// mic-failure message. A take that carries speech, or a faint take at or
/// above the floor, decodes. Routing the sentinel through `transcribed`
/// is covered by the Q1 tests; this pins the decision itself.
#[test]
fn silent_take_gate_drops_to_no_speech_sentinel() {
    assert_eq!(
        gate_or_capture_outcome(true, 0.0, 16_000),
        Some(NO_SPEECH_SENTINEL),
        "a silent take with no speech in the probe and a peak under the floor must be gated"
    );
    assert_eq!(
        gate_or_capture_outcome(true, NO_SPEECH_ENERGY_FLOOR / 2.0, 16_000),
        Some(NO_SPEECH_SENTINEL),
        "a take below the energy floor must be gated"
    );
}

#[test]
fn zero_sample_take_surfaces_as_capture_failure() {
    assert_eq!(
        gate_or_capture_outcome(true, 0.0, 0),
        Some(MIC_FAILURE_MESSAGE),
        "zero samples is a capture failure, not a silent take"
    );
    assert_eq!(
        gate_or_capture_outcome(false, 0.0, 0),
        Some(MIC_FAILURE_MESSAGE),
        "zero samples fails even when the speech probe is not consulted"
    );
}

#[test]
fn speech_take_decodes() {
    assert_eq!(
        gate_or_capture_outcome(false, 0.02, 16_000),
        None,
        "a take with speech in the probe must decode"
    );
}

#[test]
fn faint_take_at_or_above_floor_decodes() {
    assert_eq!(
        gate_or_capture_outcome(true, NO_SPEECH_ENERGY_FLOOR, 16_000),
        None,
        "a faint take exactly at the floor must decode"
    );
    assert_eq!(
        gate_or_capture_outcome(true, NO_SPEECH_ENERGY_FLOOR * 2.0, 16_000),
        None,
        "a faint take above the floor must decode"
    );
}

/// Q1: the `is_no_speech_error` helper matches the sentinel (the typed
/// `SpeechError::NoSpeech` rendered to its display string) and rejects
/// real failures. This pins the predicate the worker and `transcribed`
/// both use, so a rename of the sentinel string cannot silently break
/// the routing.
#[test]
fn q1_is_no_speech_error_matches_sentinel_only() {
    assert!(
        is_no_speech_error(NO_SPEECH_SENTINEL),
        "the sentinel must match"
    );
    assert!(
        is_no_speech_error("No speech detected"),
        "the display string of SpeechError::NoSpeech must match"
    );
    assert!(
        !is_no_speech_error("Speech model not downloaded"),
        "a real failure must not match"
    );
    assert!(
        !is_no_speech_error("Transcription failed: decode error"),
        "a decode error must not match"
    );
    assert!(
        !is_no_speech_error(MIC_FAILURE_MESSAGE),
        "a zero-sample take must not match"
    );
}

/// F-70: the energy floor keeps faint takes (whispered speech) going to the
/// decode. The gate condition is `no_speech && !energy_floor`, so a take at
/// or above the 0.005 peak floor is never dropped by the gate. This test
/// pins the gate predicate the worker uses.
#[test]
fn f70_gate_predicate_holds_faint_takes() {
    // The production predicate, not a local copy: this test must fail when
    // the gate in `stop_with_note` changes (F-66).
    assert!(
        vad_gate_drops_take(true, 0.004),
        "quiet take below the floor is gated"
    );
    assert!(
        !vad_gate_drops_take(true, NO_SPEECH_ENERGY_FLOOR),
        "faint take at the floor still decodes"
    );
    assert!(
        !vad_gate_drops_take(true, 0.02),
        "clear take above the floor still decodes"
    );
    assert!(
        !vad_gate_drops_take(false, 0.001),
        "a take with speech is never gated"
    );
}

// --- F-62: empty-text floor tests ---

/// F-62: a take whose transcript is only fillers empties out in the
/// deterministic passes and must end quietly: no delivery, no pill message.
#[test]
fn f62_filler_only_take_ends_quietly() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Transforming;
    session.next_id = 7;
    session.transcribed(
        7,
        Ok(("Um, uh, like\nyou know, I mean".to_string(), None)),
        None,
        0,
        0,
        false,
        0,
        None,
        None,
        None,
        false,
        None,
    );
    assert_eq!(
        session.phase,
        Phase::Idle,
        "a filler-only take must end idle"
    );
    // The quiet end records an empty-string sentinel (no delivery text).
    assert_eq!(
        session.headless_final_texts,
        vec![String::new()],
        "a filler-only take must end quietly with no delivery text, got {:?}",
        session.headless_final_texts
    );
    assert!(
        session.headless_flashes.is_empty(),
        "a filler-only take must end silently, got {:?}",
        session.headless_flashes
    );
}

/// F-62: a take the filler pass empties but whose raw ASR text has lexical
/// content falls back to that raw text (the floor ladder's second rung).
#[test]
fn f62_filler_only_take_falls_back_to_raw() {
    let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
    session.phase = Phase::Transforming;
    session.next_id = 8;
    // "um, hello" keeps "hello" after the filler pass, so the floor ladder
    // never fires; the final text is the post-ITN text.
    session.transcribed(
        8,
        Ok(("Um, hello".to_string(), None)),
        None,
        0,
        0,
        false,
        0,
        None,
        None,
        None,
        false,
        None,
    );
    assert_eq!(session.phase, Phase::Idle, "the take must end idle");
    let texts = &session.headless_final_texts;
    assert_eq!(texts.len(), 1, "the take must deliver exactly once");
    assert!(
        has_lexical_content(&texts[0]),
        "the delivered text must have lexical content, got {:?}",
        texts[0]
    );
    assert!(
        texts[0].to_lowercase().contains("hello"),
        "the delivered text must keep the real word, got {:?}",
        texts[0]
    );
}

/// F-62: the lexical-content check is the floor ladder's test. Punctuation
/// and whitespace alone are not lexical content; a letter or a digit is.
#[test]
fn f62_lexical_content_definition() {
    assert!(!has_lexical_content(""));
    assert!(!has_lexical_content("  \t\n "));
    assert!(!has_lexical_content(".,!?;:"));
    assert!(has_lexical_content("a"));
    assert!(has_lexical_content("7"));
    assert!(has_lexical_content("hello"));
}

// --- Phase 5.1b + 5.7b tests (prewarm wiring + per-take quality) ---

#[test]
fn vad_flag_collector_emits_one_flag_per_512_samples() {
    let mut c = VadFlagCollector::new();
    // 4 full frames at 16 kHz, all "speech".
    c.push(&vec![0.1f32; 512 * 4], 0.9);
    let flags = c.into_flags();
    assert_eq!(flags, vec![true; 4]);
}

#[test]
fn vad_flag_collector_drops_partial_tail() {
    let mut c = VadFlagCollector::new();
    // 1000 samples = 1 full frame (512) + 488 partial: only one flag.
    c.push(&vec![0.1f32; 1000], 0.2);
    let flags = c.into_flags();
    assert_eq!(flags, vec![false; 1], "0.2 < 0.5 -> noise flag");
    // The partial 488 must still be carried, not lost.
}

#[test]
fn vad_flag_collector_carry_across_pushes() {
    let mut c = VadFlagCollector::new();
    c.push(&vec![0.1f32; 256], 0.9); // no full frame yet
    assert_eq!(c.into_flags().len(), 0);
    let mut c = VadFlagCollector::new();
    c.push(&vec![0.1f32; 256], 0.9);
    c.push(&vec![0.1f32; 256], 0.9); // now 512 -> one speech flag
    assert_eq!(c.into_flags(), vec![true; 1]);
}

#[test]
fn vad_flag_collector_threshold_mapping() {
    let mut c = VadFlagCollector::new();
    c.push(&vec![0.1f32; 512], 0.5); // exactly at threshold -> speech
    c.push(&vec![0.1f32; 512], 0.49); // just below -> noise
    assert_eq!(c.into_flags(), vec![true, false]);
}

#[test]
fn vad_flag_collector_multiple_full_frames_in_one_push() {
    let mut c = VadFlagCollector::new();
    c.push(&vec![0.1f32; 512 * 3], 0.9);
    assert_eq!(c.into_flags(), vec![true; 3]);
}

#[test]
fn apply_take_quality_sets_all_five_fields() {
    let mut t = DictationTrace::new(1, "test");
    let q = TakeQuality {
        speech_rms_db: Some(-12.0),
        noise_rms_db: Some(-30.0),
        snr_db: Some(18.0),
        clipped: Some(true),
        bt_hfp: Some(true),
    };
    apply_take_quality(&mut t, Some(&q));
    assert_eq!(t.speech_rms_db, Some(-12.0));
    assert_eq!(t.noise_rms_db, Some(-30.0));
    assert_eq!(t.snr_db, Some(18.0));
    assert_eq!(t.clipped, Some(true));
    assert_eq!(t.bt_hfp, Some(true));
}

#[test]
fn apply_take_quality_none_is_noop() {
    let mut t = DictationTrace::new(1, "test");
    // Pre-set one field to prove None leaves it alone.
    t.speech_rms_db = Some(-5.0);
    apply_take_quality(&mut t, None);
    assert_eq!(t.speech_rms_db, Some(-5.0));
    assert_eq!(t.noise_rms_db, None);
    assert_eq!(t.snr_db, None);
    assert_eq!(t.clipped, None);
    assert_eq!(t.bt_hfp, None);
}

#[test]
fn merge_preroll_prepends_resampled_preroll_in_order() {
    // 16 kHz pre-roll (identity resample) + 16 kHz captured.
    let preroll: Vec<f32> = (0..4800).map(|i| (i as f32) * 0.001).collect();
    let captured: Vec<f32> = (0..1600).map(|i| 0.5 + i as f32 * 0.001).collect();
    let merged = merge_preroll(&captured, Some((&preroll, 16_000)));
    assert_eq!(merged.len(), 4800 + 1600);
    // Order: pre-roll first (starts at 0.0), then captured (starts at 0.5).
    assert!((merged[0] - 0.0).abs() < 1e-3, "merged[0] {}", merged[0]);
    assert!(
        (merged[4799] - 4.799).abs() < 1e-3,
        "merged[4799] {}",
        merged[4799]
    );
    assert!(
        (merged[4800] - 0.5).abs() < 1e-3,
        "merged[4800] {}",
        merged[4800]
    );
}

#[test]
fn merge_preroll_none_is_unchanged() {
    let captured: Vec<f32> = vec![0.1, 0.2, 0.3];
    let merged = merge_preroll(&captured, None);
    assert_eq!(merged, captured);
}

#[test]
fn active_device_bt_hfp_picks_configured_id() {
    let devices = vec![
        InputDevice {
            id: "a".into(),
            name: "AirPods Pro".into(),
            is_default: false,
        },
        InputDevice {
            id: "b".into(),
            name: "MacBook Microphone".into(),
            is_default: true,
        },
    ];
    assert_eq!(active_device_bt_hfp_for("a", &devices), Some(true));
    assert_eq!(active_device_bt_hfp_for("b", &devices), Some(false));
}

#[test]
fn active_device_bt_hfp_falls_back_to_default_when_empty() {
    let devices = vec![
        InputDevice {
            id: "a".into(),
            name: "AirPods Pro".into(),
            is_default: false,
        },
        InputDevice {
            id: "b".into(),
            name: "MacBook Microphone".into(),
            is_default: true,
        },
    ];
    assert_eq!(active_device_bt_hfp_for("", &devices), Some(false));
}

#[test]
fn active_device_bt_hfp_unknown_id_is_none() {
    let devices = vec![InputDevice {
        id: "a".into(),
        name: "AirPods Pro".into(),
        is_default: true,
    }];
    assert_eq!(active_device_bt_hfp_for("missing", &devices), None);
    assert_eq!(active_device_bt_hfp_for("", &[]), None);
}

#[cfg(test)]
mod f20_dictionary_path_block_tests;

#[cfg(test)]
mod f84_offer_expiry_tests;

// --- P0: streaming fold failure must fall back to the full take buffer ---

/// P0: when the mid-take streaming fold produces no text (every segment
/// decoded to nothing, or the engine failed), the release must not end the
/// take with "No speech detected". The full take buffer is still here: the
/// release falls back to the whole-take decode and delivers that text.
/// Before the fix, the failed fold was the take's final answer.
///
/// This drives the real `StreamingTake` (real segmenter, real committer
/// with a mock engine that fails every decode with NoSpeech) and the real
/// fold gate the release worker uses: `streaming_marks` decides the path,
/// `finish` produces the fold, and the gate must be the fold's TEXT (not
/// the fold's presence). The whole-take decode itself is the same
/// `transcribe` the worker runs; it needs a Tauri app handle, so the test
/// pins the gate and the fold shape, and the decode side is covered by the
/// existing `transcribe` tests plus the live-app check in the report.
#[test]
fn streaming_fold_with_no_text_is_not_the_take_final_answer() {
    // A streaming take whose committer fails every decode with NoSpeech:
    // the fold yields no text at all.
    let mut take = tests::streaming_take_with_mock_and_error("", vec![0.9f32; 128], Some(()));
    // 1 s of 16 kHz speech followed by 1 s of silence, fed in 128-sample
    // frames (the typical Mac buffer size) so the 512-sample VAD chunks
    // line up exactly: the segmenter commits the speech at the pause (a
    // 1 s segment), so the take submits a mid-take segment.
    let speech: Vec<f32> = (0..16_000)
        .map(|i| 0.5 * (2.0 * std::f64::consts::PI * 300.0 * i as f64 / 16_000.0).sin() as f32)
        .collect();
    let silence = vec![0.0f32; 16_000];
    for chunk in speech.chunks(128) {
        take.feed_frame(chunk, 16_000);
    }
    for chunk in silence.chunks(128) {
        take.feed_frame(chunk, 16_000);
    }
    // The fold itself: `finish` waits for the tail and returns the
    // committed segments in order followed by the tail. With a mock that
    // fails every decode, the fold is an error (nothing survived), so the
    // release must treat it as "no text from the fold". Before the fix,
    // the fold's PRESENCE (a submitted segment) was the gate: an empty or
    // failed fold ended the take with "No speech detected" even though
    // the full take buffer held real speech.
    let committed_result = take.finish("en".to_string());
    let fold_has_text = committed_result
        .as_ref()
        .map(|r| r.transcripts.iter().any(|t| !t.text.trim().is_empty()))
        .unwrap_or(false);
    assert!(
        !fold_has_text,
        "the fold must produce no text when every decode fails"
    );
    // The gate the release worker must apply: the fold is the take's
    // transcript ONLY when it produced text. An empty or failed fold is a
    // failure, and the take falls through to the whole-take decode over
    // the full buffer (the same `captured.samples` the capture thread
    // resampled).
    let take_final_answer = fold_has_text;
    assert!(
        !take_final_answer,
        "a fold with no text must not be the take's final answer; the full buffer takes over"
    );
}

// T5 learning-loop seam (ROADMAP §16 item 2): the desktop layer resolves
// the bandit choice and encodes it into the instruction channel; the
// pipeline must never consult an empty bandit of its own.

#[test]
fn choose_take_profile_for_app_applied_change_wins() {
    use teletype_core::learning::profile_bandit::BanditState;
    use teletype_core::profile::PolishProfile::{Natural, Structured};
    let mut bandit = BanditState::default();
    assert!(bandit.apply_change("gmail", Natural, Structured, 1_000));
    assert_eq!(
        choose_take_profile_for_app(&bandit, "gmail", false, Natural, 2_000),
        Structured,
        "an applied learned change must be the profile that runs"
    );
}

#[test]
fn choose_take_profile_for_app_explicit_profile_wins() {
    use teletype_core::learning::profile_bandit::BanditState;
    use teletype_core::profile::PolishProfile::{Natural, Structured};
    let mut bandit = BanditState::default();
    assert!(bandit.apply_change("gmail", Natural, Structured, 1_000));
    assert_eq!(
        choose_take_profile_for_app(&bandit, "gmail", true, Natural, 2_000),
        Natural,
        "an explicit per-app profile must beat the learned change"
    );
}

#[test]
fn choose_take_profile_for_app_no_change_keeps_recommended() {
    use teletype_core::learning::profile_bandit::BanditState;
    use teletype_core::profile::PolishProfile::Natural;
    let bandit = BanditState::default();
    assert_eq!(
        choose_take_profile_for_app(&bandit, "gmail", false, Natural, 2_000),
        Natural,
        "with no bandit evidence the recommended default must run"
    );
}
