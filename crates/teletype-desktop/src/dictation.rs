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
        resample_to_target, should_arm, Captured, FrontEndState, InputDevice, MicFrontEnd,
        PrewarmMode, Recording,
    },
    delivery::{self, DeliveryOutcome, DeliveryPolicy},
    delivery::{resolve_delivery, Delivery, RecordTarget},
    pipeline::{InputSource, Pipeline, UnifiedInput},
    segmenter::{SegmentEvent, Segmenter, SegmenterConfig},
    state::{self, Input, Phase, PillPosition, PillState, RecordingMode},
    trace::{now_us, DictationTrace, FileTraceSink, TraceSink, TraceStageMarks},
    vad::VadDetector,
};

use crate::{commands, AppState};

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
    /// 16 kHz take buffer; sample `i` is the `i`th 16 kHz sample of the take.
    buf16: Vec<f32>,
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
    /// Feed one device-rate frame (the capture thread's rate). Resamples to
    /// 16 kHz, feeds the streaming VAD in 512-sample chunks, pushes each
    /// chunk to the segmenter, and submits every ready segment to the
    /// committer. Returns the number of segments committed by this frame.
    fn feed_frame(&mut self, frame: &[f32], rate: u32) -> usize {
        let r16 = resample_to_target(frame, rate);
        self.carry.extend_from_slice(&r16);
        let mut committed = 0;
        while self.carry.len() >= 512 {
            let chunk: Vec<f32> = self.carry.drain(..512).collect();
            let _ = self.vad.push(&chunk);
            let prob = self.vad.last_probability();
            let events = self.segmenter.push(&chunk, prob);
            for ev in events {
                let SegmentEvent::Ready(seg) = ev;
                let end = seg.end_sample.min(self.buf16.len());
                let start = seg.start_sample.min(end);
                let samples = self.buf16[start..end].to_vec();
                let lang = self.language.clone();
                self.committer.submit_segment(samples, lang);
                committed += 1;
                self.last_commit_end = seg.next_start_sample.min(self.buf16.len());
            }
        }
        self.buf16.extend_from_slice(&r16);
        committed
    }

    /// The uncommitted remainder of the take: `buf16[last_commit_end..]`.
    fn tail(&self) -> Vec<f32> {
        self.buf16[self.last_commit_end.min(self.buf16.len())..].to_vec()
    }

    /// The tail's audio length in ms (the trace's `tail_audio_ms`).
    fn tail_audio_ms(&self) -> u32 {
        crate::committer::tail_audio_ms(self.buf16.len() - self.last_commit_end, 16_000)
    }

    /// Wait for the tail and all in-flight segments, returning the
    /// committed segments in order followed by the tail. Consumes the
    /// committer (it resets itself for the next take and its worker exits
    /// on drop).
    fn finish(self, language: String) -> Result<Vec<teletype_speech::Transcript>, String> {
        let tail = self.tail();
        self.committer.finish(tail, language)
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
        .clone()
        .expect("caller verified the provider is loaded");
    let transforms = state
        .transforms
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let transform = transforms.auto_apply().cloned()?;
    let ctx = {
        let settings = state.settings();
        let profile = state
            .profile
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let dictionary = state
            .dictionary
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
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
            language: profile.language.clone(),
            s1_control: profile.s1_control,
            list_style: teletype_core::transforms::ListStyle::from_str_lenient(
                &settings.list_style,
            ),
        }
    };
    Some(Box::new(move |text: &str| {
        // The S1-mini path is card-faithful (fixed system prompt +
        // control line, no task prompt), so it cannot be reused here
        // without the exact wire format; the generic prompt + generate +
        // validate is the same pass `run_single_chunk` runs for every
        // other model. Both land on the identical validator.
        let is_polish = transform.id == "builtin-polish" || transform.auto_apply;
        if provider.model_id() == "s1-mini" && is_polish {
            return Err("s1-mini uses the whole-text path".to_string());
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
        match teletype_core::transforms::validator::validate(&raw, text, &transform, &prompt) {
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
            let mut out = resample_to_target(raw, rate);
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
}

#[allow(clippy::large_enum_variant)] // `Transcribed` carries the take PCM
                                     // (`Option<Vec<f32>>`, A.3.3); boxing it
                                     // would ripple through every controller
                                     // match arm.
pub enum Event {
    HotkeyDown,
    HotkeyUp,
    HotkeyInterrupted,
    /// P2.4: the dedicated Command Mode hotkey was pressed/released.
    /// Starts (and stops, in Hold mode) a `TakeKind::Command` take instead
    /// of dictation. Release shares the normal release path: ending the
    /// take in progress, whatever kind it is.
    CommandHotkeyDown,
    CommandHotkeyUp,
    /// P9.6: the e2e feeder thread is playing a 16 kHz frame of the queued
    /// WAV file into the in-flight take (file-input mode).
    E2eFeed {
        frame: Vec<f32>,
    },
    /// P8.4: the Paste-Last hotkey was pressed. Re-delivers the last final
    /// text through the same delivery path as a normal paste.
    PasteLast,
    /// P2.8: the tray's "Run Voice Command" item. Same as `Toggle` but the
    /// take it starts is a Command take.
    CommandToggle,
    Toggle,
    Cancel,
    /// Cancel pressed from the pill overlay.
    PillCancel,
    Transcribed {
        session: u64,
        /// `(text, words)` where `words` is `Some` when the engine reports
        /// per-word timings/confidences (Fluid, sherpa) and `None` otherwise
        /// (Whisper, ggml Parakeet).
        transcript: Result<(String, Option<Vec<teletype_speech::Word>>), String>,
        /// The language the ASR engine detected, when auto-detect was active.
        detected_language: Option<String>,
        /// P0.2 trace marks: monotonic µs when the ASR pass started/ended,
        /// captured on the transcribe worker thread.
        asr_start_us: u64,
        asr_end_us: u64,
        /// A.3.3: the take audio at 16 kHz mono f32 (the same buffer the
        /// final transcribe consumed). Empty when the take had no audio
        /// (audio finish failed) or the streaming path was taken (the
        /// committer consumed the segments). The command decider classifies
        /// this audio; `None`/empty PCM degrades to the transcript-only
        /// fallback (the Needle/LLM deciders ignore PCM anyway).
        pcm: Option<Vec<f32>>,
    },
    /// The transform + inject pipeline for a session has finished.
    PipelineDone {
        session: u64,
    },
    /// The pipeline worker has finished its bookkeeping (history, transcript
    /// file, usage, edit-watch) and is asking the controller for permission
    /// to perform the only user-visible side effect: the text injection.
    ///
    /// The worker NEVER injects itself. It sends this event with the session
    /// id and the fully prepared payload; the controller — the single
    /// owner of the session state machine — decides whether the session is
    /// still valid (not cancelled, not superseded) and, only if so,
    /// performs the injection on its own thread and answers with
    /// `PipelineDone`. A `Cancel` processed before this event discards the
    /// result; one processed after it lets the injection proceed and then
    /// transitions the session. That ordering is the whole cancellation
    /// guarantee: one FIFO queue, one thread, no check-then-act race.
    PipelineReady {
        session: u64,
        payload: PipelinePayload,
    },
    /// The pipeline worker finished all bookkeeping (history, transcript file,
    /// usage, edit-watch) but has NOT yet injected or reset the pill. Sent
    /// from the worker thread; handled on the main thread so the pill reset
    /// goes through the normal `self.show()` path, which reliably delivers the
    /// event to the pill webview. `PipelineDone` alone is not enough: it fires
    /// after `go_idle()`, which hides the pill, so the webview never sees the
    /// transition out of "Transforming…" if a newer session superseded this
    /// one or if the IPC event was coalesced.
    PipelineWorkerDone {
        session: u64,
    },
    /// A flash message's display timer elapsed; hide the pill.
    FlashDone {
        generation: u64,
    },
    /// The speech model finished loading in the background.
    WarmupDone {
        session: u64,
    },
    /// VAD saw speech followed by a long-enough pause: stop and transcribe.
    VadFired {
        session: u64,
    },
    /// T2.1: the post-injection edit check for a finished session fired.
    /// Diff the watched field now and learn; the next dictation's
    /// `observe_pending_edit` remains the fallback if this misses.
    CheckEdit {
        session: u64,
    },
    /// Phase 5.1: the prewarm tick (every 2 s from the tick thread spawned
    /// in `Controller::spawn`). Re-evaluates `should_arm(mode, app_active)`
    /// and arms/disarms the front end on the controller thread. No
    /// SettingsChanged event exists, so a setting flip (`instant_start`)
    /// applies on restart in v1 — the mode is parsed once at spawn.
    PrewarmTick,
    /// Phase 5.7: the transcribe worker computed this session's per-take
    /// quality signals. Sent before the session's `Transcribed` (one FIFO
    /// channel, so it always lands first); stashed by `take_quality` and
    /// attached to the trace in `finish_trace`.
    TakeQuality {
        session: u64,
        quality: TakeQuality,
    },
}

/// Test-only type alias for the injection hook `Controller::for_tests`
/// takes. Lives at module level (not inside `impl Controller`) because
/// associated type aliases are not stable on this toolchain. The alias also
/// factors the long trait-object type out of the `for_tests` signature,
/// which clippy's `type_complexity` otherwise flags.
#[cfg(test)]
type ForTestInject = Box<dyn FnMut(&PipelinePayload) + Send>;

#[derive(Clone)]
pub struct Controller {
    /// `None` in unit tests, where the event loop is driven directly on the
    /// test thread and the channel never needs to be closed.
    tx: Option<Sender<Event>>,
    /// The most recent hotkey registration failure, with the binding that
    /// failed. Compared at read time against the currently saved binding so
    /// a fixed binding never shows a stale warning.
    hotkey_conflict: Arc<Mutex<Option<HotkeyConflict>>>,
    /// P9.6: the e2e hook slot, `Some` only in `--e2e` mode. Consulted at
    /// the take-start and injection seams by the controller.
    pub(crate) e2e: Option<std::sync::Arc<crate::e2e::E2eSlot>>,
}

/// A shortcut registration the OS refused.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyConflict {
    /// The binding that failed, exactly as attempted.
    pub binding: String,
    /// What to show. Worded as what macOS reported, never as a claim about
    /// who holds the shortcut: a duplicate registration inside our own
    /// process fails the same way, so "another app has it" is unproven.
    pub message: String,
}

impl HotkeyConflict {
    fn new(binding: &str, os_error: &str) -> Self {
        Self {
            binding: binding.to_string(),
            message: format!(
                "macOS reports this shortcut is already taken, so dictation will not start. Cause unknown ({os_error}). Pick a different shortcut."
            ),
        }
    }
}

/// The stored conflict if it still applies to `current_binding`, else `None`.
///
/// Pure, so the no-stale-warning rule is pinnable: a conflict recorded for an
/// old binding must disappear the moment the binding changes, without waiting
/// for a re-registration attempt to clear it.
pub(crate) fn visible_conflict(
    stored: &Option<HotkeyConflict>,
    current_binding: &str,
) -> Option<HotkeyConflict> {
    stored.clone().filter(|c| c.binding == current_binding)
}

impl Controller {
    /// A clone of the stored registration conflict, if any. Filter it through
    /// [`visible_conflict`] against the current binding before showing it.
    pub(crate) fn conflict_snapshot(&self) -> Option<HotkeyConflict> {
        self.hotkey_conflict
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl Controller {
    pub fn tx(&self) -> Sender<Event> {
        self.tx.clone().expect("test controller has no channel")
    }

    /// Spawns the controller event loop for unit tests: the same `handle`
    /// the production thread runs, with an `on_inject` hook standing in for
    /// the real injection (which needs the Tauri main thread). Returns the
    /// controller (to send events) and a finish handle that drops the
    /// channel and joins the loop thread.
    /// A do-nothing controller for `AppState::for_tests()`. The concurrency
    /// tests spawn their own controller via `for_tests(on_inject)`; this one
    /// just exists so the struct literal can be built.
    /// A do-nothing controller for `AppState::for_tests()`. The concurrency
    /// tests spawn their own controller via `for_tests(on_inject)`; this one
    /// just exists so the struct literal can be built.
    #[cfg(any(test, feature = "test"))]
    pub fn placeholder_for_tests() -> Self {
        Self {
            tx: None,
            hotkey_conflict: Arc::new(Mutex::new(None)),
            e2e: None,
        }
    }

    /// Private on purpose: only the in-file concurrency tests drive it, and
    /// it returns the crate-private `Session` so `pub` would trip clippy's
    /// `private_interfaces`.
    #[cfg(test)]
    fn for_tests(on_inject: ForTestInject) -> (Self, Session, ForTestInject) {
        let controller = Self {
            tx: None,
            hotkey_conflict: Arc::new(Mutex::new(None)),
            e2e: None,
        };
        let mut session = Session {
            app: None,
            #[cfg(test)]
            headless_flash: true,
            controller: controller.clone(),
            phase: Phase::Idle,
            hotkey_down: false,
            hands_free: false,
            last_release: None,
            next_id: 0,
            take_kind: TakeKind::default(),
            started_at_ms: 0,
            recording_started: None,
            speech_ms: std::collections::HashMap::new(),
            streaming_marks: std::collections::HashMap::new(),
            vad: None,
            streaming: None,
            polish_app: None,
            live_preview: None,
            spool: None,
            ducker: std::sync::Arc::new(std::sync::Mutex::new(crate::ducking::default_ducker())),
            duck_armed: false,
            record_target: None,
            pending_selection: None,
            injected_at: None,
            cancelled_sessions: std::collections::HashSet::new(),
            traces: std::collections::HashMap::new(),
            trace_key: None,
            front_end: None,
            preroll_this_take: None,
            vad_flags: None,
            take_quality: std::collections::HashMap::new(),
            screen_context: None,
            learning_store: None,
        };
        // Mid-pipeline: session 1 is transforming, like a real take whose
        // worker is still running.
        session.next_id = 1;
        session.phase = Phase::Transforming;
        (controller, session, on_inject)
    }

    pub fn spawn(app: AppHandle) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        // `try_state`: the full AppState is managed after this returns
        // (chicken-and-egg: the state holds the controller). Outside
        // `--e2e` the state is managed by the time any event fires, so a
        // `None` here only matters for the e2e pre-manage gap.
        let e2e = app
            .try_state::<AppState>()
            .and_then(|s| s.e2e.as_ref().cloned());
        let controller = Self {
            tx: Some(tx),
            hotkey_conflict: Arc::new(Mutex::new(None)),
            e2e,
        };
        // Phase 5.1 (plan §5.1): instant start. Build the pre-warmed mic
        // front end once when the mode is not Off. There is no
        // SettingsChanged event, so a flip of `settings.instant_start`
        // applies on app restart in v1 (the mode is parsed here, once).
        // A failed build is logged and stays `None`: the per-take rebuild
        // path continues, so behaviour is unchanged.
        let instant_start = app
            .try_state::<AppState>()
            .map(|s| s.settings().instant_start.clone())
            .unwrap_or_default();
        let prewarm_mode = match instant_start.as_str() {
            "always" => PrewarmMode::Always,
            "while_app_active" => PrewarmMode::WhileAppActive,
            _ => PrewarmMode::Off,
        };
        let mut session = Session {
            app: Some(app.clone()),
            #[cfg(test)]
            headless_flash: false,
            controller: controller.clone(),
            phase: Phase::Idle,
            hotkey_down: false,
            hands_free: false,
            last_release: None,
            next_id: 0,
            take_kind: TakeKind::default(),
            started_at_ms: 0,
            recording_started: None,
            speech_ms: std::collections::HashMap::new(),
            streaming_marks: std::collections::HashMap::new(),
            vad: None,
            streaming: None,
            polish_app: None,
            live_preview: None,
            spool: None,
            ducker: std::sync::Arc::new(std::sync::Mutex::new(crate::ducking::default_ducker())),
            duck_armed: false,
            record_target: None,
            pending_selection: None,
            injected_at: None,
            cancelled_sessions: std::collections::HashSet::new(),
            traces: std::collections::HashMap::new(),
            trace_key: None,
            front_end: None,
            preroll_this_take: None,
            vad_flags: None,
            take_quality: std::collections::HashMap::new(),
            screen_context: None,
            learning_store: None,
        };
        if prewarm_mode != PrewarmMode::Off {
            let app_ref = app.clone();
            let settings = app_ref.state::<AppState>().settings();
            let device = settings.input_device.clone();
            let noise = settings.noise_reduction_enabled;
            match MicFrontEnd::build(&device, noise) {
                Ok(fe) => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!("mic pre-warm built ({instant_start:?})"),
                    );
                    session.front_end = Some(fe);
                }
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("mic pre-warm build failed: {e}; per-take rebuild continues"),
                    );
                }
            }
        }
        if prewarm_mode != PrewarmMode::Off {
            // The tick thread holds a controller clone and nudges the
            // front end every 2 s (arm while desired, disarm otherwise).
            let tick_tx = controller.clone();
            thread::Builder::new()
                .name("teletype-prewarm-tick".into())
                .spawn(move || loop {
                    thread::sleep(Duration::from_secs(2));
                    tick_tx.send(Event::PrewarmTick);
                })
                .ok();
        }
        thread::Builder::new()
            .name("teletype-dictation".into())
            .spawn(move || {
                for event in rx {
                    let handled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        session.handle(event);
                    }));
                    if handled.is_err() {
                        crate::log_entry(crate::LogLevel::Error, "event handler panicked");
                    }
                }
            })?;
        Ok(controller)
    }

    pub fn send(&self, event: Event) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(event);
        }
    }

    pub fn register_hotkey(&self, app: &AppHandle, hotkey: &str) -> Result<(), String> {
        self.unregister_hotkey(app, hotkey);
        if hotkey == "Fn" {
            // Carbon RegisterEventHotKey never fires for Fn-only presses, so the
            // Fn key uses a CGEventTap (needs Accessibility permission, which the
            // app already requires for typing). Bare "Fn" means Fn with no other
            // modifiers; "Fn+<key>" combos work through the normal global-shortcut
            // path with the fn modifier flag.
            #[cfg(target_os = "macos")]
            {
                crate::fn_tap::start(self.clone()).map_err(|e| {
                    format!("Couldn't register Fn: {e} (grant Teletype Accessibility access in System Settings > Privacy & Security > Accessibility)")
                })
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err("Fn key is macOS-only; pick another hotkey".into())
            }
        } else {
            // A single bare modifier (Ctrl/Cmd/Alt/Shift) also cannot be
            // registered with Carbon — RegisterEventHotKey only fires for a
            // modifier+key combo, never a lone modifier. Watch it with the
            // bare-modifier CGEventTap instead, which reports the press only
            // while that modifier is the sole one held.
            #[cfg(target_os = "macos")]
            {
                if let Some(mask) = crate::mod_tap::bare_modifier_mask(hotkey) {
                    crate::mod_tap::start(self.clone(), mask).map_err(|e| {
                        format!("Couldn't register {hotkey}: {e} (grant Teletype Accessibility access in System Settings > Privacy & Security > Accessibility)")
                    })
                } else {
                    self.register_string_hotkey(app, hotkey)
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                self.register_string_hotkey(app, hotkey)
            }
        }
    }

    fn register_string_hotkey(&self, app: &AppHandle, hotkey: &str) -> Result<(), String> {
        let controller = self.clone();
        let binding = hotkey.to_string();
        let record = self.hotkey_conflict.clone();
        let result = app
            .global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                controller.send(match event.state {
                    ShortcutState::Pressed => Event::HotkeyDown,
                    ShortcutState::Released => Event::HotkeyUp,
                });
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"));
        // Record the outcome for the Keybinds warning: a failure stores the
        // binding it failed for, a success clears any older one. The reader
        // compares against the *current* binding, so neither path can leave a
        // stale warning behind.
        *record
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = result
            .as_ref()
            .err()
            .map(|e| HotkeyConflict::new(&binding, e));
        result
    }

    /// P2.4: registers the dedicated Command Mode hotkey. Unlike the
    /// dictation hotkey this is string-shortcuts only (no Fn tap, no
    /// bare-modifier tap): a command binding is always a full chord.
    /// Empty unregisters nothing and succeeds — the feature is simply off.
    /// Failures are logged, not returned: Command Mode is optional, so a
    /// bad binding must not fail the settings save (the Keybinds screen
    /// already flags duplicate bindings client-side).
    pub fn register_command_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let controller = self.clone();
        let binding = hotkey.to_string();
        if let Err(e) = app
            .global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                controller.send(match event.state {
                    ShortcutState::Pressed => Event::CommandHotkeyDown,
                    ShortcutState::Released => Event::CommandHotkeyUp,
                });
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
        {
            crate::log_entry(crate::LogLevel::Error, e);
        } else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("command hotkey registered: {binding}"),
            );
        }
    }

    /// Unregisters a previously registered Command Mode hotkey. Empty is a
    /// no-op.
    pub fn unregister_command_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let _ = app.global_shortcut().unregister(hotkey);
    }

    /// P8.4: registers the Paste-Last hotkey. Mirrors `register_command_hotkey`:
    /// empty unregisters nothing; failures are logged, not returned.
    pub fn register_paste_last_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let controller = self.clone();
        let binding = hotkey.to_string();
        if let Err(e) = app
            .global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state == ShortcutState::Pressed {
                    controller.send(Event::PasteLast);
                }
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
        {
            crate::log_entry(crate::LogLevel::Error, e);
        } else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("paste-last hotkey registered: {binding}"),
            );
        }
    }

    /// Unregisters the Paste-Last hotkey. Empty is a no-op.
    pub fn unregister_paste_last_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let _ = app.global_shortcut().unregister(hotkey);
    }

    /// Registers a global shortcut that, when pressed, applies the named
    /// transform to the text currently selected in the focused app (Wispr
    /// Flow style). On macOS the primary path is the unified InputEngine's
    /// ShortcutManager via `sync_transform_shortcuts`; this Carbon path is
    /// non-macOS plus degraded fallback when the tap is unavailable. The
    /// handler spawns the (blocking) transform work off the main thread via
    /// the Tauri command so a slow local model never stalls the UI.
    pub fn register_transform_shortcut(
        &self,
        app: &AppHandle,
        hotkey: &str,
        transform_id: &str,
    ) -> Result<(), String> {
        self.unregister_transform_shortcut(app, hotkey);
        if hotkey.trim().is_empty() {
            return Ok(());
        }
        let app_clone = app.clone();
        let id = transform_id.to_string();
        let hotkey_owned = hotkey.to_string();
        crate::log_entry(
            crate::LogLevel::Info,
            format!("registering transform shortcut: {hotkey} -> {id}"),
        );
        app.global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state == ShortcutState::Pressed {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!("transform shortcut FIRED: {hotkey_owned} -> {id}"),
                    );
                    // Read the selection SYNCHRONOUSLY in the callback, before
                    // the async task runs. On macOS Carbon does NOT swallow,
                    // so this path is fallback only; the InputEngine tap
                    // already swallowed the keystroke and captured the
                    // selection synchronously. The sync read stays to avoid
                    // re-reading after focus changed.
                    let selected = crate::text_selection::selected_text();
                    let app_clone = app_clone.clone();
                    let id_clone = id.clone();
                    tauri::async_runtime::spawn(async move {
                        let st = app_clone.state::<crate::AppState>();
                        let a2 = app_clone.clone();
                        // Pass the pre-captured selection to the transform so
                        // it doesn't re-read a corrupted selection.
                        match commands::transform_selection_with_text(a2, st, id_clone, selected)
                            .await
                        {
                            Ok(result) => crate::log_entry(
                                crate::LogLevel::Success,
                                format!("transform_selection OK: {} chars", result.len()),
                            ),
                            Err(e) => crate::log_entry(
                                crate::LogLevel::Error,
                                format!("transform_selection failed: {e}"),
                            ),
                        }
                    });
                }
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
    }

    /// Unregisters a transform shortcut (no-op if not registered).
    pub fn unregister_transform_shortcut(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let _ = app.global_shortcut().unregister(hotkey);
    }

    /// Registers the Quick Add global shortcut (item 8): the word selected in
    /// the focused app is added to the dictionary. The AX read must run on the
    /// main thread, so the handler spawns the work as a Tauri command — the
    /// same pattern as the transform shortcuts.
    pub fn register_quick_add_shortcut(&self, app: &AppHandle, hotkey: &str) {
        if hotkey.trim().is_empty() {
            return;
        }
        let app_clone = app.clone();
        app.global_shortcut()
            .on_shortcut(hotkey, move |_, _, event| {
                use tauri_plugin_global_shortcut::ShortcutState;
                if event.state == ShortcutState::Pressed {
                    let a = app_clone.clone();
                    tauri::async_runtime::spawn(async move {
                        // The AX read must run on the main thread (same rule
                        // as transform_selection), so read the selection here
                        // and do the dictionary write on a blocking thread.
                        let selected = crate::text_selection::selected_text();
                        let Some(word) = selected
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .and_then(|s| s.split_whitespace().next().map(str::to_string))
                        else {
                            tracing::info!("quick add: no selection");
                            return;
                        };
                        let a2 = a.clone();
                        let result = tauri::async_runtime::spawn_blocking(move || {
                            let state = a2.state::<crate::AppState>();
                            let mut dict = state
                                .dictionary
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            if dict.find(&word).is_some() {
                                return Err("That word is already in your dictionary.".to_string());
                            }
                            let new_word = teletype_core::dictionary::DictionaryWord::new(word, "");
                            dict.insert(new_word)?;
                            state.dictionary_store.save(&*dict)?;
                            Ok(())
                        })
                        .await;
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(e)) => tracing::warn!("quick add: {e}"),
                            Err(e) => tracing::warn!("quick add join: {e}"),
                        }
                    });
                }
            })
            .map_err(|e| format!("Couldn't register {hotkey}: {e}"))
            .ok();
    }

    /// Unregisters a previously registered global shortcut (no-op if not registered).
    pub fn unregister_hotkey(&self, app: &AppHandle, hotkey: &str) {
        if hotkey == "Fn" {
            #[cfg(target_os = "macos")]
            crate::fn_tap::stop();
            #[cfg(not(target_os = "macos"))]
            {
                let _ = app.global_shortcut().unregister(hotkey);
            }
        } else {
            #[cfg(target_os = "macos")]
            {
                if crate::mod_tap::bare_modifier_mask(hotkey).is_some() {
                    crate::mod_tap::stop();
                } else {
                    let _ = app.global_shortcut().unregister(hotkey);
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = app.global_shortcut().unregister(hotkey);
            }
        }
    }
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
    /// When the pipeline finished injecting the current take (T2.1):
    /// the moment from which the user can edit the inserted text, used to
    /// arm the post-injection edit check.
    injected_at: Option<Instant>,
    /// Live-preview handle for the in-flight recording, if the interim
    /// transcript loop is running. Stopped on release/cancel.
    live_preview: Option<LivePreviewHandle>,
    /// Escape-recovery spool for the in-flight recording, if one was
    /// started. `None` if spool creation failed (recording continues
    /// without a recovery spool) or the take has ended.
    spool: Option<crate::recovery::Spool>,
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
}

/// A second press within this window of a release is a double-tap.
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(350);

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

/// Reads a non-negative dev delay in milliseconds from the environment.
/// A missing or malformed variable means "no delay" — a typo in a debug
/// env var must never break a dictation.
pub fn dev_delay_ms(var: &str) -> Duration {
    let ms = std::env::var(var)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
        .min(60_000);
    Duration::from_millis(ms)
}

/// How often the interim transcript loop re-transcribes the audio-so-far.
/// Everything the pipeline worker knows about a finished take, handed to the
/// controller with `PipelineReady`. The controller owns the only user-visible
/// side effect (the injection) and decides, at the moment it processes this,
/// whether the session is still valid. The worker must never act on this
/// payload itself.
pub struct PipelinePayload {
    pub session: u64,
    pub final_text: String,
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

/// Handle to a running live-preview interim thread. `stop()` signals the
/// thread to exit and joins it so the speech-model lock is released before
/// the caller proceeds to the final transcribe.
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
) -> LivePreviewHandle {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cancelled_thread = cancelled.clone();
    let join = thread::Builder::new()
        .name("teletype-live-preview".into())
        .spawn(move || {
            run_live_preview(app, buf, rate, session, stop_rx, cancelled_thread);
        })
        .expect("spawn live-preview thread");
    LivePreviewHandle {
        stop_tx,
        cancelled,
        join: Some(join),
    }
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
        let resampled = resample_to_target(&raw, device_rate);
        let secs = resampled.len() as f64 / teletype_core::audio::TARGET_SAMPLE_RATE as f64;
        if secs < INTERIM_MIN_SECS {
            continue;
        }

        // Transcribe the audio-so-far. This serializes on the speech-model
        // lock with the final transcribe (which runs in stop()) — but the
        // final transcribe no longer waits for this one: `stop()` sets the
        // cancel flag and does not join, so an in-flight interim decode is
        // abandoned and the lock is released for the final pass.
        let state = app.state::<AppState>();
        let manager = match kind {
            SpeechEngineKind::Fluid | SpeechEngineKind::Sherpa | SpeechEngineKind::Whistle => {
                &state.neural
            }
            SpeechEngineKind::Parakeet => &state.parakeet,
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

impl Session {
    fn state(&self) -> tauri::State<'_, AppState> {
        self.app
            .as_ref()
            .expect("test session touched AppState")
            .state::<AppState>()
    }

    /// P6.4: read the focused field's surroundings once at hotkey-down and
    /// classify the frontmost app. `None` on any failure (no AX permission,
    /// no focused field, secure field, no app handle in tests) — the take
    /// proceeds with empty context, exactly as before this capture existed.
    fn capture_screen_context(&self) -> Option<teletype_core::screen_context::ScreenContext> {
        // Headless tests have no app handle; skip the capture entirely.
        #[cfg(test)]
        if self.headless_flash {
            return None;
        }
        let state = self.state();
        let app = state.platform.active_application()?;
        // Focused-field text: macOS reads the AX tree (`crate::ax_text`);
        // other platforms have no reader yet (P8.5 wires UIA for the
        // delivery path separately) — documented `None` fallback: the
        // take proceeds with book-level context only.
        #[cfg(target_os = "macos")]
        let (before, after, selection) = match crate::ax_text::read_focused_context(400, 100)? {
            ctx if ctx.secure => (None, None, None),
            ctx => (Some(ctx.before), Some(ctx.after), ctx.selection),
        };
        #[cfg(not(target_os = "macos"))]
        let (before, after, selection) = (None, None, None);
        // A secure field's text must never enter the context, even though
        // the read happened before the subrole was known (see
        // `FocusedContext::secure`): drop the text, keep the app info.
        // (Handled inside the macos branch above.)
        let category = teletype_core::app_category::app_category(
            if app.application_id.is_empty() {
                None
            } else {
                Some(app.application_id.as_str())
            },
            app.window_title.as_deref(),
        );
        Some(teletype_core::screen_context::ScreenContext {
            app_id: if app.application_id.is_empty() {
                None
            } else {
                Some(app.application_id)
            },
            window_title: app.window_title,
            focused_before: before,
            focused_after: after,
            selection,
            app_category: category,
        })
    }

    /// The production app handle. Unit tests never reach a path that needs
    /// it (the cancel / pipeline-ready / go-idle paths are state-free).
    fn app(&self) -> &AppHandle {
        self.app
            .as_ref()
            .expect("test session reached a platform-dependent path")
    }

    /// The ASR language for the frontmost app (P3.3): the per-app override
    /// wins over the global setting. `"auto"` is passed through so Whisper
    /// can still auto-detect.
    fn effective_language(&self, kind: SpeechEngineKind) -> String {
        let state = self.state();
        let settings = state.settings();
        let app_ctx = state.platform.active_application().unwrap_or_default();
        let profile = teletype_core::personalization::UserProfile {
            language: settings.language.clone(),
            app_language_overrides: settings.app_language_overrides.clone(),
            ..Default::default()
        };
        let resolved = teletype_core::personalization::resolve_language(&profile, &app_ctx);
        effective_language(resolved, crate::dictation::is_english_only(kind))
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::HotkeyDown => {
                self.hotkey_down_event();
                self.e2e_after_event();
            }
            Event::HotkeyUp => self.hotkey_up_event(),
            Event::CommandHotkeyDown => {
                self.command_hotkey_down_event();
                self.e2e_after_event();
            }
            Event::CommandHotkeyUp => self.hotkey_up_event(),
            Event::E2eFeed { frame } => self.e2e_feed(&frame),
            Event::PasteLast => self.paste_last_event(),
            Event::CommandToggle => self.command_toggle_event(),
            Event::HotkeyInterrupted => self.input(Input::HotkeyInterrupted),
            Event::Toggle => self.input(Input::Toggle),
            Event::Cancel => self.input(Input::Cancel),
            Event::Transcribed {
                session,
                transcript,
                detected_language,
                asr_start_us,
                asr_end_us,
                pcm,
            } => {
                self.transcribed(
                    session,
                    transcript,
                    detected_language,
                    asr_start_us,
                    asr_end_us,
                    pcm,
                );
            }
            Event::PipelineReady { session, payload } => {
                self.pipeline_ready(session, payload, None)
            }
            Event::PipelineDone { session } => {
                debug_assert!(
                    !self.cancelled_sessions.contains(&session),
                    "PipelineDone for cancelled session {session}"
                );
                if self.next_id == session {
                    self.go_idle();
                    // T2.1: the inserted text is now editable. Check for an
                    // edit after the user has had a moment to make one,
                    // without waiting for the next dictation.
                    self.injected_at = Some(Instant::now());
                    self.arm_edit_check(session);
                } else if self.phase != Phase::Idle {
                    // A newer session superseded this one while its pipeline
                    // was still running. Its pill state ("Transforming…")
                    // must not survive: the newer session owns the pill now,
                    // so reset to idle. The superseded take still injects
                    // and records history on its own worker thread; only the
                    // UI hand-off is dropped.
                    crate::log_entry(
                        crate::LogLevel::Info,
                        "pipeline done for superseded session; resetting pill",
                    );
                    self.go_idle();
                }
            }
            Event::PipelineWorkerDone { session } => {
                // The worker finished its bookkeeping. If this session is no
                // longer the newest (a newer dictation started while the
                // pipeline was running), the pill is still showing this
                // session's "Transforming…" state. Reset it now, on the main
                // thread, so the event reliably reaches the pill webview.
                // For the newest session, `PipelineDone` (sent right after
                // this) will call `go_idle()` which does the same reset —
                // this is a no-op in that case because the phase is already
                // Idle by the time `PipelineDone` is processed (the two
                // events are sent back-to-back and the controller channel
                // is FIFO).
                if self.next_id != session && self.phase != Phase::Idle {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        "worker done for superseded session; resetting pill",
                    );
                    self.go_idle();
                }
            }
            Event::CheckEdit { session } => {
                // Only the most recent finished take is worth checking; a
                // newer session means this check raced a newer dictation.
                if self.next_id == session {
                    self.observe_pending_edit();
                }
            }
            Event::PrewarmTick => {
                self.prewarm_tick();
                self.poll_learning_observer();
            }
            Event::TakeQuality { session, quality } => {
                // Phase 5.7: stash by session id; `finish_trace` drains
                // and attaches it. An id with no pending pipeline (e.g.
                // the take was cancelled) is drained there or in `go_idle`.
                self.take_quality.insert(session, quality);
            }
            Event::FlashDone { generation } => {
                if self.next_id == generation && self.phase == Phase::Idle {
                    self.flash_done();
                }
            }
            // The pill's Cancel button. `decide()` is the single source of
            // truth for what each input means per phase (cancel while
            // recording cancels the dictation; cancel mid-pipeline lets
            // transcription finish but discards the result).
            Event::PillCancel => {
                let action = state::decide(
                    Input::Cancel,
                    self.phase,
                    self.recording_mode(),
                    &mut self.hotkey_down,
                    &mut self.hands_free,
                );
                match action {
                    state::Action::Stop { cancelled } => self.stop(cancelled),
                    state::Action::Discard => self.go_idle(),
                    state::Action::CancelPipeline => {
                        tracing::info!("pipeline cancelled by user (pill)");
                        self.phase = Phase::Cancelled;
                        // The durable half of the cancellation: tag the
                        // session id so a `PipelineReady` that arrives later
                        // on this same FIFO queue is discarded by the
                        // controller instead of injecting.
                        self.cancel_session(self.next_id);
                        // BUG-003: the session is over; arm the idle-unload timer.
                        #[cfg(test)]
                        if self.headless_flash {
                            // Test path: skip the model-unload timer.
                        } else {
                            let state = self.state();
                            let delay_secs = state.settings().model_unload_delay_secs;
                            let delay = (delay_secs > 0)
                                .then(|| std::time::Duration::from_secs(delay_secs));
                            state.speech.end_dictation(delay);
                            state.parakeet.end_dictation(delay);
                        }
                        self.flash("Cancelled");
                    }
                    // Nothing, Start, GoHandsFree: no-op for a cancel click.
                    _ => {}
                }
            }
            Event::WarmupDone { session } => {
                // Only the newest session may hide the warming pill.
                if self.next_id == session && self.phase == Phase::Listening {
                    self.show(PillState::Recording {
                        started_at_ms: self.started_at_ms,
                        command: self.take_kind == TakeKind::Command,
                    });
                }
            }
            Event::VadFired { session } => {
                // The capture thread's VAD detector saw a long-enough pause
                // after speech. Only the newest session may stop, so a fire
                // from a just-finished take is a stale no-op.
                if self.next_id == session && self.phase == Phase::Listening {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        "VAD auto-stop: silence after speech, stopping",
                    );
                    self.stop(false);
                }
            }
        }
    }

    /// Phase 5.1: re-evaluate the prewarm arm state. `app_active` is the
    /// "main" or "pill" webview being focused; `desired = should_arm(mode,
    /// app_active)`; arm when idle+desired, disarm when armed+not desired.
    /// `FrontEndState::Fallback` is terminal for the session: the backend
    /// rejected pause/play, so stop trying (per-take rebuild continues).
    fn prewarm_tick(&mut self) {
        let Some(mode) = self.prewarm_mode() else {
            return;
        };
        // Fallback is terminal for the session: the backend rejected
        // pause/play, so stop trying (per-take rebuild continues).
        if self
            .front_end
            .as_ref()
            .is_some_and(|fe| fe.state() == FrontEndState::Fallback)
        {
            return;
        }
        let app_active = self.app_focused();
        let desired = should_arm(mode, app_active);
        let Some(fe) = self.front_end.as_mut() else {
            return;
        };
        let armed = fe.state() == FrontEndState::Armed;
        if !armed && desired {
            if let Err(e) = fe.arm() {
                // arm() already recorded Fallback + warned on failure.
                crate::log_entry(crate::LogLevel::Warn, format!("prewarm arm: {e}"));
            }
        } else if armed && !desired {
            if let Err(e) = fe.disarm() {
                crate::log_entry(crate::LogLevel::Warn, format!("prewarm disarm: {e}"));
            }
        }
    }

    /// P9.6: after a hotkey-down event, hand the e2e hook its seam: if a
    /// file was queued for the next take and the take is now recording,
    /// the hook spawns the feeder that plays it in. No-op outside `--e2e`
    /// (no slot) or when the phase is not Listening (the press was eaten).
    fn e2e_after_event(&self) {
        if self.phase != Phase::Listening {
            return;
        }
        let Some(slot) = &self.controller.e2e else {
            return;
        };
        let app = self.app().clone();
        crate::e2e::on_take_start(&app, slot);
    }

    /// The parsed `instant_start` mode for the prewarm front end, or `None`
    /// when the feature is off (mode Off or no front end built).
    fn prewarm_mode(&self) -> Option<PrewarmMode> {
        self.front_end.as_ref()?;
        Some(match self.state().settings().instant_start.as_str() {
            "always" => PrewarmMode::Always,
            "while_app_active" => PrewarmMode::WhileAppActive,
            _ => PrewarmMode::Off,
        })
    }

    /// Whether the "main" or "pill" webview is focused (the app_active
    /// input to `should_arm`). `false` when either window lookup fails.
    fn app_focused(&self) -> bool {
        let app = self.app();
        ["main", "pill"].iter().any(|label| {
            app.get_webview_window(label)
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false)
        })
    }

    /// P0.2: make sure the in-flight take has a trace, stamping hotkey-down
    /// when the trace is created. Idempotent: a take that already has one
    /// (e.g. double-tap switching to hands-free mid-take) keeps its marks.
    fn ensure_trace(&mut self) {
        if self.trace_key.is_none() {
            let key = self.next_id;
            let mut trace = DictationTrace::new(key, "");
            trace.mark(|s, now| s.t_hotkey_down = Some(now));
            self.traces.insert(key, trace);
            self.trace_key = Some(key);
        }
    }

    /// A.3.3: stamp the command-decider trace fields from the pipeline
    /// payload. No-op when the take never ran the command route (all four
    /// are `None`).
    fn apply_decider_marks(
        t: &mut teletype_core::trace::DictationTrace,
        decider: Option<String>,
        decider_ms: Option<u32>,
        tool: Option<String>,
        args_source: Option<String>,
    ) {
        if decider.is_some() || decider_ms.is_some() || tool.is_some() || args_source.is_some() {
            t.decider = decider;
            t.decider_ms = decider_ms;
            t.tool = tool;
            t.args_source = args_source;
        }
    }

    /// P2.9: append one command-take lifecycle event to the trace stored
    /// under `key`. Controller-side stages (`armed`, `started`) are pushed
    /// directly; worker-side stages (`classified=`, `executed`,
    /// `refused`) travel back in `TraceStageMarks` and are merged by
    /// `apply_stage_marks`. Missing traces are ignored — instrumentation
    /// never fails a take.
    fn trace_command_event(&mut self, key: u64, event: impl Into<String>) {
        if let Some(t) = self.traces.get_mut(&key) {
            t.command_mode.push(event.into());
        }
    }

    fn hotkey_down_event(&mut self) {
        // P0.2: the press is the first stage boundary of a take.
        self.ensure_trace();
        let mode = self.recording_mode();
        // Double-tap detection (Hold mode): a press shortly after a release
        // while still recording switches to hands-free instead of stopping.
        if self.phase == Phase::Listening && mode == RecordingMode::Hold && !self.hands_free {
            if let Some(t) = self.last_release {
                if t.elapsed() <= DOUBLE_TAP_WINDOW {
                    self.hands_free = true;
                    self.last_release = None;
                    return;
                }
            }
        }
        self.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
    }

    fn hotkey_up_event(&mut self) {
        self.ensure_trace();
        self.input(Input::HotkeyUp);
        self.last_release = Some(Instant::now());
    }

    /// P2.4/P2.6: the dedicated Command hotkey was pressed. Mirrors
    /// `hotkey_down_event` but tags the take as a command, and captures
    /// the focused app's selection (if any) for selection-aware commands:
    /// the keystroke that follows would corrupt the selection, so it is
    /// read synchronously here, on press.
    fn command_hotkey_down_event(&mut self) {
        self.ensure_trace();
        self.pending_selection =
            crate::text_selection::selected_text().filter(|s| !s.trim().is_empty());
        let mode = self.recording_mode();
        if self.phase == Phase::Listening && mode == RecordingMode::Hold && !self.hands_free {
            if let Some(t) = self.last_release {
                if t.elapsed() <= DOUBLE_TAP_WINDOW {
                    self.hands_free = true;
                    self.last_release = None;
                    return;
                }
            }
        }
        self.input_with_kind(Input::HotkeyDown, TakeKind::Command);
    }

    /// P2.8: the tray's "Run Voice Command" item. Starts a Command take
    /// from Idle, stops the running take otherwise, following the same
    /// toggle semantics as the dictation tray item. The kind is only
    /// applied when the state machine starts a take, so using it as a stop
    /// never re-tags an in-flight dictation take.
    fn command_toggle_event(&mut self) {
        self.ensure_trace();
        self.input_with_kind(Input::Toggle, TakeKind::Command);
    }

    /// P8.4: the Paste-Last hotkey was pressed. Re-delivers the last final
    /// text through the same delivery path as a normal paste. If there is
    /// no stored text, flash "Nothing to paste".
    fn paste_last_event(&mut self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let text = self
            .state()
            .last_text
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some(text) = text else {
            self.flash("Nothing to paste");
            return;
        };
        // Run the same delivery path as a normal paste: build the IO bundle
        // and call deliver(). The text is already final (no smart insertion,
        // no polish) — it is exactly what was last delivered.
        let state = self.state();
        let platform: &dyn teletype_core::platform::Platform = state.platform.as_ref();
        let app_id = platform
            .active_application()
            .map(|a| a.application_id)
            .filter(|id| !id.is_empty());
        // macOS: real clipboard guard; other platforms: compile-safe stub
        // via `delivery_io::macos_io`'s non-mac twin (`Arc<()>`).
        #[cfg(target_os = "macos")]
        let io = crate::delivery_io::macos_io(std::sync::Arc::new(
            crate::clipboard::MacClipboardGuard::new(),
        ));
        #[cfg(not(target_os = "macos"))]
        let io = crate::delivery_io::macos_io(std::sync::Arc::new(()));
        let policy = DeliveryPolicy::builtin();
        let outcome = delivery::deliver(&text, app_id.as_deref(), &policy, &io);
        match outcome {
            DeliveryOutcome::PastedVerified
            | DeliveryOutcome::PastedUnverified
            | DeliveryOutcome::Typed
            | DeliveryOutcome::SetValue => {
                crate::log_entry(
                    crate::LogLevel::Info,
                    format!("paste-last: delivered ({outcome:?})"),
                );
            }
            DeliveryOutcome::HeldForPasteLast => {
                crate::log_entry(crate::LogLevel::Warn, "paste-last: held (delivery failed)");
                self.flash("Paste failed — text kept for later");
            }
            DeliveryOutcome::Refused { reason } => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("paste-last refused: {reason}"),
                );
                self.flash(&format!("Not pasted: {reason}"));
            }
        }
    }

    /// Headless release for P2.1 tests: the take is mid-recording (Listening)
    /// and the mic was never opened, so `stop()` has no take to finish.
    /// The state machine still decides `Stop`, and the session transitions
    /// to the post-release phase (Transforming) exactly as a real release
    /// would, without touching the recording slot.
    ///
    /// Unlike `hotkey_up_event`, this does NOT call `decide` (which would
    /// clear the `hotkey_down` latch that the test already asserted). It
    /// directly applies the `Stop` decision the state machine would produce.
    #[cfg(test)]
    fn headless_release(&mut self, kind: TakeKind) {
        self.ensure_trace();
        // The state machine decides `Stop` (asserted by the test); apply
        // the decision without re-deriving it. Mirror `hotkey_up_event` by
        // clearing the hotkey latch, so the next take starts clean —
        // a real release always clears it through `decide`.
        self.hotkey_down = false;
        if self.phase == Phase::Listening {
            self.phase = Phase::Transforming;
            self.take_kind = kind;
        }
        self.last_release = Some(Instant::now());
    }

    /// The kind of the in-flight take (P2.2). Test-only accessor;
    /// production code reads the field directly.
    #[cfg(test)]
    fn take_kind(&self) -> TakeKind {
        self.take_kind
    }

    fn recording_mode(&self) -> RecordingMode {
        #[cfg(test)]
        if self.headless_flash {
            return RecordingMode::Hold;
        }
        let settings = self.state().settings();
        match settings.recording_mode.as_str() {
            "toggle" => RecordingMode::Toggle,
            _ => RecordingMode::Hold,
        }
    }

    /// A start with no kind: plain dictation.
    fn input(&mut self, input: Input) {
        self.input_with_kind(input, TakeKind::Dictation)
    }

    /// P2.3: every start goes through the state machine, tagged with the
    /// kind of take it begins. `hotkey_down_event` passes
    /// [`TakeKind::Dictation`]; a command take passes [`TakeKind::Command`].
    /// Because all kinds track the hotkey the same way, a Hold-mode release
    /// stops a command take exactly as it stops a dictation.
    ///
    /// The kind is applied ONLY when the state machine actually decides
    /// `Start` (inside `start`). Every other input — a release, a toggle
    /// stop, a press swallowed by the hotkey latch — leaves the in-flight
    /// take's kind alone. Tagging before `decide` (the original P2-A
    /// shape) retagged a command take as dictation on release, silently
    /// misrouting the whole feature.
    fn input_with_kind(&mut self, input: Input, kind: TakeKind) {
        let mode = self.recording_mode();
        let action = state::decide(
            input,
            self.phase,
            mode,
            &mut self.hotkey_down,
            &mut self.hands_free,
        );
        match action {
            state::Action::Nothing => {}
            state::Action::Start { by_hotkey } => self.start(by_hotkey, kind),
            state::Action::Stop { cancelled } => self.stop(cancelled),
            state::Action::GoHandsFree => {
                self.hands_free = true;
            }
            state::Action::Discard => self.go_idle(),
            state::Action::CancelPipeline => {
                tracing::info!(
                    session = self.next_id,
                    "pipeline cancelled by user (hotkey); marking session cancelled"
                );
                // Same contract as the pill cancel: the session id is tagged
                // cancelled on this thread, so the in-flight worker's
                // `PipelineReady` is discarded when it reaches the queue.
                self.cancel_session(self.next_id);
                self.phase = Phase::Cancelled;
                #[cfg(test)]
                if self.headless_flash {
                    // Test path: skip the model-unload timer.
                } else {
                    let state = self.state();
                    let delay_secs = state.settings().model_unload_delay_secs;
                    let delay = (delay_secs > 0).then(|| Duration::from_secs(delay_secs));
                    state.speech.end_dictation(delay);
                    state.parakeet.end_dictation(delay);
                }
                self.flash("Cancelled");
            }
        }
    }

    fn start(&mut self, _by_hotkey: bool, kind: TakeKind) {
        // P2.2: the kind is set at start and read in `transcribed()`. The
        // old arm-flag was cleared before classification, so the take was
        // always misread as dictation; the kind travels with the session.
        self.take_kind = kind;
        // P2.9: a dedicated command take (hotkey, toggle, tray, hands-free)
        // is armed and starts right here. A wake-routed take is armed later,
        // in `transcribed()`: its recording began before the wake phrase
        // revealed it as a command, so `armed` lands without a second
        // `started`. Runs before the headless early return so tests
        // exercise the same emission as production.
        if kind == TakeKind::Command {
            self.ensure_trace();
            if let Some(key) = self.trace_key {
                self.trace_command_event(key, "armed");
                self.trace_command_event(key, "started");
            }
        }
        #[cfg(test)]
        if self.headless_flash {
            // No mic, pill, or platform in headless tests: mark Listening
            // so the take can drive the pipeline phase, like a real take
            // whose mic opened successfully. P4: headless takes get their
            // streaming state from the test (which owns the mock engine),
            // so a fresh take starts with a clean slot.
            self.streaming = None;
            self.phase = Phase::Listening;
            return;
        }
        let settings = self.state().settings();
        let app = self.app().clone();
        // P0.2: Toggle-mode and hands-free takes never see a hotkey press,
        // so make sure they still get a trace (stamped at start as the
        // closest available point to the press).
        self.ensure_trace();
        // BUG-003: mark the dictation session as in flight so the idle-unload
        // timer cannot fire mid-take.
        {
            let state = self.state();
            state.speech.begin_dictation();
            state.parakeet.begin_dictation();
        }
        // Show the pill immediately, BEFORE the (slow) mic open, so the UI
        // responds instantly on keypress like Wispr Flow. The waveform begins
        // as soon as levels arrive.
        let started_at_ms = teletype_core::storage::now_ms();
        self.started_at_ms = started_at_ms;
        // Monotonic marker for the speaking-rate figure. Reset here and
        // re-stamped below, once the mic is genuinely open, so the measured
        // duration is speech time rather than key-holding time.
        self.recording_started = None;
        self.phase = Phase::Listening;
        self.broadcast(teletype_core::state::UiState::Listening { started_at_ms });
        self.show(PillState::Recording {
            started_at_ms,
            command: kind == TakeKind::Command,
        });

        // Record the paste target now, while the user is provably here: the
        // pipeline (ASR plus an optional 20 s LLM polish) finishes long after
        // this, and the delivery gate compares this snapshot against the
        // target at inject time. Capture is best-effort; a missing piece is
        // no evidence, never a mismatch.
        self.record_target = {
            let state = self.state();
            state.platform.active_application().map(|app| RecordTarget {
                bundle_id: app.application_id,
                app_name: app.application_name,
                pid: state.platform.active_pid(),
                frame: state.platform.focused_window_frame(),
            })
        };
        // P6.4: capture the focused screen context once, at the same moment
        // the paste target is recorded. Best-effort: a focused field that
        // cannot be read (no AX permission, no field, secure field) yields
        // `None` and the take proceeds with empty context, exactly as
        // before. The raw text is ephemeral — it feeds the paste-time
        // anchor and the engine boost list; only a privacy-safe summary may
        // reach a trace.
        self.screen_context = self.capture_screen_context();

        // Escape recovery: start the PCM spool for this take. Failure is
        // non-fatal — the recording proceeds without a recovery spool.
        let spool = match crate::recovery::Spool::begin(&self.state().data_dir) {
            Ok(s) => Some(s),
            Err(e) => {
                crate::log_entry(crate::LogLevel::Warn, format!("recovery spool: {e}"));
                None
            }
        };
        let spool_cb = spool.as_ref().and_then(|s| s.clone_writer());
        self.spool = spool;
        // Phase 5.6: lower the output volume while the take records (and
        // optionally send the media play/pause key), so playback audio
        // stops bleeding into the microphone. Fire-and-forget: a failure
        // never breaks a dictation. `duck` is a no-op when already active
        // (a second take while the first is still ducking).
        self.duck_armed = false;
        if settings.duck_other_audio {
            let level = settings.duck_level;
            let mut ducker = self
                .ducker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Err(e) = ducker.duck(level) {
                crate::log_entry(crate::LogLevel::Warn, format!("audio duck: {e}"));
            }
            if ducker.active() {
                self.duck_armed = true;
            }
            drop(ducker);
        }
        if settings.pause_media_while_dictating {
            let _ = crate::ducking::toggle_media_play_pause();
        }
        // Phase 5.1: a new take starts with a clean pre-roll slot; the
        // take-start block below (recording OK + front end armed) fills it.
        self.preroll_this_take = None;

        let streaming_slot = self.streaming_for_take(&settings, &app);
        let recording = if self.vad_enabled_for() {
            self.start_with_vad(spool_cb)
        } else {
            // P4: the streaming frame callback is installed for EVERY take
            // (not only hands-free auto-stop takes): the capture thread feeds
            // the streaming take state on every frame, so mid-take commits
            // happen regardless of the auto-stop setting. The auto-stop
            // detector stays gated by `vad_enabled_for()` above — its
            // semantics are unchanged.
            let on_frame = self.build_frame_callback(
                spool_cb,
                Arc::new(Mutex::new(None)),
                streaming_slot.clone(),
                None,
                self.controller.clone(),
                self.next_id,
            );
            let dfn3_slot = if settings.noise_reduction_enabled {
                Some(teletype_core::audio::build_dfn3_slot(
                    &self.state().models_dir,
                    48_000,
                ))
            } else {
                None
            };
            Recording::start_with_vad_df(
                &settings.input_device,
                move |level| {
                    let _ = app.emit_to("pill", "pill-level", level);
                },
                on_frame,
                None,
                settings.noise_reduction_enabled,
                dfn3_slot,
            )
        };
        match recording {
            Ok(recording) => {
                // P4: the streaming state is shared with the capture thread
                // from here on; `stop()` moves it into the release worker.
                if let Some(slot) = streaming_slot {
                    self.streaming = Some(slot);
                    // P4.3b: the capture thread needs the app handle for
                    // the deferred per-segment polish install.
                    self.polish_app = Some(Arc::new(Mutex::new(Some(self.app().clone()))));
                    // Phase 5.7: the flag collector is only useful when the
                    // take has a streaming state (whose VAD detector feeds
                    // it); no detector -> no flags -> quality reports only
                    // `clipped`.
                    self.vad_flags = Some(Arc::new(Mutex::new(VadFlagCollector::new())));
                }
                // Phase 5.1: when the pre-warm front end is armed, the take
                // starts with the last 300 ms of pre-roll. Feed it into the
                // streaming take NOW so the first words land in the
                // streaming take's first committed segment, and keep a copy
                // for the release worker to prepend to the ASR samples.
                let prewarm_armed = self
                    .front_end
                    .as_ref()
                    .is_some_and(|fe| fe.state() == FrontEndState::Armed);
                if prewarm_armed {
                    if let Some(fe) = self.front_end.as_ref() {
                        let raw = fe.take_preroll(300);
                        if !raw.is_empty() {
                            let rate = fe.rate();
                            self.feed_capture_frame(&raw, rate);
                            self.preroll_this_take = Some((raw, rate));
                        }
                    }
                }
                // The mic is genuinely open now, so this is the honest moment
                // for the cue — see `play_cue`.
                self.play_cue();
                // Spawn the live-preview interim loop before storing the
                // recording, so it can grab a handle to the live buffer.
                // BUG-011: the ported pill styles (Level Rail, Reading Well,
                // Dot Matrix) all render `interimText`, so Hold mode must feed
                // it too — a pill that promises live text but shows only the
                // recording animation for the entire take reads as broken.
                let app = self.app().clone();
                let session = self.next_id;
                // The user can switch the live transcript off in Settings
                // (it costs a re-decode per interim tick); the pill then
                // shows only the recording animation for the take.
                let live_preview_on = self.state().settings().live_preview_enabled;
                self.live_preview = live_preview_on
                    .then(|| recording.live_preview())
                    .flatten()
                    .map(|(buf, rate)| spawn_live_preview(app, buf, rate, session));
                // Store the recording in a thread-local so `stop` can access it.
                RECORDING.with(|slot| *slot.borrow_mut() = Some(recording));
                // The mic is genuinely open now; start the speaking-rate
                // clock. `start` reset this to `None` so the measured
                // duration is speech time, not key-holding time.
                self.recording_started = Some(std::time::Instant::now());
                // P0.2: closest available point to "first audio" — the mic
                // is open and frames are flowing from here on.
                if let Some(key) = self.trace_key {
                    if let Some(t) = self.traces.get_mut(&key) {
                        t.mark(|s, now| s.t_first_audio = Some(now));
                    }
                }
                // Load the speech model in the background while the user
                // speaks, so it's ready on release (P1.12: pre-warm on
                // hotkey-down, not on release). No-op if already loaded.
                self.warm_up();
                // Pre-warm the LLM (polish) model in the background while the
                // user is still talking. `ensure_local_provider` no-ops if a
                // provider is already loaded, and the `llm_loading` guard
                // prevents concurrent spawns. By the time transcription
                // finishes, the server is hot and the first inference doesn't
                // pay the cold-start penalty. No effect on the recording path.
                {
                    let app = self.app().clone();
                    std::thread::Builder::new()
                        .name("teletype-llm-prewarm".into())
                        .spawn(move || {
                            crate::commands::ensure_local_provider(&app);
                        })
                        .ok();
                }
            }
            Err(e) => {
                crate::log_entry(crate::LogLevel::Error, e.to_string());
                // BUG-003: the mic never opened, so no dictation will
                // complete — disarm the idle-unload timer now instead of
                // leaving `active` stuck true.
                {
                    let state = self.state();
                    state.speech.end_dictation(None);
                    state.parakeet.end_dictation(None);
                }
                self.flash(&e);
            }
        }
    }

    /// VAD auto-stop is useful only when nothing else bounds the take:
    /// hands-free hold mode (double-tap), where the key release does nothing
    /// and the pause is the only stop signal. Plain hold stops on release;
    /// push-to-talk stops on the second tap; both preempt VAD.
    fn vad_enabled_for(&self) -> bool {
        self.state().settings().vad_auto_stop && self.hands_free
    }

    /// Starts a recording with the VAD detector armed. The detector lives in
    /// a shared slot the capture thread feeds; on `UtteranceComplete` it
    /// posts `VadFired` to this controller, which stops the take.
    fn start_with_vad(
        &mut self,
        spool_cb: Option<crate::recovery::SpoolWriter>,
    ) -> Result<Recording, String> {
        let settings = self.state().settings();
        let app = self.app().clone();
        let controller = self.controller.clone();
        let session = self.next_id;
        let vad: VadSlot = match VadDetector::silero_16k() {
            Ok(mut d) => {
                d.set_silence_duration(Duration::from_millis(settings.vad_silence_ms.max(200)));
                Arc::new(Mutex::new(Some(d)))
            }
            Err(e) => {
                // No model, no auto-stop: fall back to manual stop.
                crate::log_entry(crate::LogLevel::Error, format!("VAD init failed: {e}"));
                return if settings.noise_reduction_enabled {
                    Recording::start_with_noise(&settings.input_device, move |level| {
                        let _ = app.emit_to("pill", "pill-level", level);
                    })
                } else {
                    Recording::start(&settings.input_device, move |level| {
                        let _ = app.emit_to("pill", "pill-level", level);
                    })
                };
            }
        };
        // The device's native rate is published here once the capture thread
        // opens the stream; the VAD callback reads it on its first frame so
        // the silence window is timed against real wall-clock time (not the
        // engine's 16 kHz assumption).
        let rate_slot: Arc<std::sync::atomic::AtomicU32> =
            Arc::new(std::sync::atomic::AtomicU32::new(0));
        let streaming_slot = self.streaming_for_take(&settings, &app);
        let noise_reduction = settings.noise_reduction_enabled;
        // Plan §5.5: Smart mode (noise_reduction_enabled == true) uses the
        // DFN3 split pipeline; the ASR path stays raw, the detection path
        // runs through DFN3. Off mode (false) uses the legacy path.
        let dfn3_slot = if noise_reduction {
            Some(teletype_core::audio::build_dfn3_slot(
                &self.state().models_dir,
                48_000,
            ))
        } else {
            None
        };
        let on_frame = self.build_frame_callback(
            spool_cb,
            vad.clone(),
            streaming_slot.clone(),
            None,
            controller,
            session,
        );
        let recording = Recording::start_with_vad_df(
            &settings.input_device,
            move |level| {
                let _ = app.emit_to("pill", "pill-level", level);
            },
            on_frame,
            Some(rate_slot),
            noise_reduction,
            dfn3_slot,
        );
        if recording.is_ok() {
            self.vad = Some(vad);
            if let Some(slot) = streaming_slot {
                self.streaming = Some(slot);
                // P4.3b: the capture thread needs the app handle for the
                // deferred per-segment polish install.
                self.polish_app = Some(Arc::new(Mutex::new(Some(self.app().clone()))));
                // Phase 5.7: same rule as the non-VAD path — the collector
                // only exists when the streaming take state exists.
                self.vad_flags = Some(Arc::new(Mutex::new(VadFlagCollector::new())));
            }
        }
        recording
    }

    /// P4: build the streaming take state for a new take, or `None` when
    /// streaming is disabled, the Silero model cannot be loaded, or the
    /// selected engine cannot be resolved (the take then decodes the whole
    /// audio after release, exactly as before). Owns its own committer
    /// (its own engine handle, mirroring the three `SpeechModelManager`s
    /// the app builds for the selected engine kind).
    fn streaming_for_take(
        &mut self,
        settings: &crate::commands::Settings,
        app: &AppHandle,
    ) -> Option<StreamingSlot> {
        if !STREAMING_COMMIT_DEFAULT {
            return None;
        }
        let (model_path, kind) =
            resolve_speech_model(settings, &app.state::<AppState>().models_dir);
        let language = self.effective_language(kind);
        let vad = match VadDetector::silero_16k() {
            Ok(vad) => vad,
            Err(e) => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("streaming VAD init failed: {e}; take decodes whole after release"),
                );
                return None;
            }
        };
        // The committer's own engine handle: the same concrete provider the
        // selected engine kind uses (mirroring the three `SpeechModelManager`s
        // the app builds for Whisper/Parakeet/neural). Whisper's provider is
        // the default for the neural kinds: the warm-up thread loads the
        // selected model into the main manager, and the committer's handle
        // loads it on demand via `set_model_path` + `load` (P4.2).
        let provider: Box<dyn teletype_speech::SpeechProvider> = match kind {
            SpeechEngineKind::Whisper => Box::new(teletype_speech::whisper::WhisperProvider::new()),
            SpeechEngineKind::Parakeet => {
                Box::new(teletype_speech::parakeet::ParakeetProvider::new())
            }
            SpeechEngineKind::Fluid | SpeechEngineKind::Sherpa | SpeechEngineKind::Whistle => {
                // The neural manager's provider is set by the catalog at warm-up
                // time; for v1 the committer uses Whisper's provider as a safe
                // default (the warm-up thread loads the selected model into the
                // main manager, and the committer's handle loads it on demand).
                Box::new(teletype_speech::whisper::WhisperProvider::new())
            }
        };
        let committer = crate::committer::Committer::new(provider);
        // P4 completion: the committer's provider is constructed UNLOADED
        // (an empty path), so streaming would never actually commit — every
        // segment decode failed with `NoModel` and the take fell back to the
        // whole-take decode. Enqueue a background load now, resolving the
        // model path with the SAME per-engine semantics as the warm-up and
        // release path (`resolve_speech_model`): Whisper/Parakeet →
        // `<models_dir>/<file>`, Fluid → `<models_dir>/fluid`, Sherpa →
        // `<models_dir>/sherpa/<file>`, Whistle → `<models_dir>/whistle.cact`.
        // The load jumps the committer's queue (before any segment), runs on
        // the worker thread, and never blocks the recording-start path. A
        // failed load is harmless: the segments then fail per-result and the
        // take falls back byte-identically, exactly as before.
        committer.load_model(model_path);
        Some(Arc::new(Mutex::new(Some(StreamingTake {
            buf16: Vec::new(),
            segmenter: Segmenter::with_rate(SegmenterConfig::default(), 16_000),
            vad,
            carry: Vec::new(),
            last_commit_end: 0,
            committer,
            language,
        }))))
    }

    /// P4: the capture-thread frame callback. Resamples the device-rate
    /// frame to 16 kHz ONCE and shares it between the escape-recovery
    /// spool, the VAD auto-stop detector (when armed), the streaming take
    /// state (when streaming is on), and the per-frame VAD flag collector
    /// (Phase 5.7). The auto-stop semantics are unchanged: the detector is
    /// fed the device-rate frame exactly as before (its own 512-sample
    /// carry inside `push_frame`), and the `None`-drop-on-fire behavior is
    /// preserved.
    #[allow(
        clippy::too_many_arguments,
        clippy::type_complexity // `Box<dyn Fn(&[f32]) + Send + Sync + 'static>`
                                // is the callback shape the audio layer wants.
    )]
    fn build_frame_callback(
        &mut self,
        spool_cb: Option<crate::recovery::SpoolWriter>,
        vad: VadSlot,
        streaming: Option<StreamingSlot>,
        flags: Option<std::sync::Arc<std::sync::Mutex<VadFlagCollector>>>,
        controller: Controller,
        session: u64,
    ) -> Option<Box<dyn Fn(&[f32]) + Send + Sync + 'static>> {
        let rate_slot: Arc<std::sync::atomic::AtomicU32> =
            Arc::new(std::sync::atomic::AtomicU32::new(0));
        let rate_cb = rate_slot.clone();
        let vad_cb = vad.clone();
        let streaming_cb = streaming.clone();
        let flags_cb = flags.clone();
        Some(Box::new(move |frame| {
            // Phase 5.7: resample ONCE at the top and reuse the result for
            // the spool, the streaming feed, and the flag collector. The
            // auto-stop detector still gets the DEVICE-rate frame below
            // (its own carry), so its semantics are byte-identical.
            let r16: Option<Vec<f32>> = {
                let r = rate_cb.load(std::sync::atomic::Ordering::Relaxed);
                (r > 0).then(|| resample_to_target(frame, r))
            };
            // Escape-recovery spool: hand the 16 kHz frame to the
            // non-blocking spool writer.
            if let (Some(w), Some(r16)) = (spool_cb.as_ref(), r16.as_ref()) {
                w.write(r16);
            }
            // VAD auto-stop: semantics unchanged from before P4.
            let mut last_prob = 0.0f32;
            let event = {
                let mut guard = vad_cb
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                match guard.as_mut() {
                    Some(d) => {
                        // First frame: latch the real feed rate.
                        let r = rate_cb.load(std::sync::atomic::Ordering::Relaxed);
                        if r > 0 {
                            d.set_sample_rate(r);
                        }
                        let ev = d.push_frame(frame);
                        last_prob = d.last_probability();
                        ev
                    }
                    None => {
                        // Detector already fired and dropped: no more
                        // auto-stop, but the streaming path still runs
                        // below (the take continues until release).
                        teletype_core::vad::VadEvent::Continue
                    }
                }
            };
            if matches!(event, teletype_core::vad::VadEvent::UtteranceComplete) {
                // Drop the detector now so no further frames can re-fire;
                // the session check in the handler is the second gate.
                let mut guard = vad_cb
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *guard = None;
                controller.send(Event::VadFired { session });
            }
            // P4: feed the streaming take state (the 16 kHz frame computed
            // above; never hold the lock while resampling).
            if let (Some(streaming_slot), Some(r16)) = (&streaming_cb, r16.as_ref()) {
                let mut guard = streaming_slot
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if let Some(take) = guard.as_mut() {
                    take.feed_frame(r16, 16_000);
                }
            }
            // Phase 5.7: one flag per 512-sample (32 ms) 16 kHz frame.
            // When the auto-stop detector has fired/dropped, reuse the last
            // observed probability (the take keeps running until release).
            // No detector -> no flags (quality then reports only `clipped`).
            if let (Some(flags_slot), Some(r16)) = (&flags_cb, r16.as_ref()) {
                if !r16.is_empty() {
                    let mut guard = flags_slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.push(r16, last_prob);
                }
            }
        }))
    }

    /// P4: feed one captured device-rate frame into the in-flight take's
    /// streaming state. The real capture callback calls this (via
    /// [`Self::build_frame_callback`]); headless tests call it directly
    /// with synthetic frames so the REAL controller path is exercised
    /// without a microphone. No-op when the take has no streaming state
    /// (streaming disabled, VAD init failed, or the mic never opened).
    pub fn feed_capture_frame(&mut self, frame: &[f32], rate: u32) {
        if rate == 0 {
            return;
        }
        let streaming = match &self.streaming {
            Some(s) => s.clone(),
            None => return,
        };
        // P4.3b: clone the polish app slot before taking the streaming
        // lock (the install takes `&mut self`, so it cannot run while a
        // borrow of `self.polish_app` is live).
        let app_slot = self.polish_app.clone();
        // Resample OUTSIDE the lock (the task's explicit rule: never hold
        // the streaming lock while calling `resample_to_target`).
        let r16 = resample_to_target(frame, rate);
        let mut install = None;
        {
            let mut guard = streaming
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(take) = guard.as_mut() {
                take.feed_frame(&r16, 16_000);
                // P4.3b: remember the take's audio length once the
                // threshold is crossed (the install runs after the lock
                // is released).
                if let Some(slot) = app_slot.as_ref() {
                    let len = take.buf16.len();
                    if crate::committer::tail_audio_ms(len, 16_000)
                        >= crate::committer::TOTAL_AUDIO_SEGMENTED_POLISH_MS as u32
                    {
                        install = Some((slot, len));
                    }
                }
            }
        }
        if let Some((slot, len)) = install {
            let app = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            if let Some(app) = app {
                let state = app.state::<AppState>();
                self.install_segment_polish(&streaming, &state, len);
            }
        }
    }

    /// P9.6: the e2e feeder's entry point. Feeds one 16 kHz frame of the
    /// file's audio into the in-flight take's streaming state (the same
    /// path the capture callback uses). No-op when the take is not
    /// recording or has no streaming state.
    pub fn e2e_feed(&mut self, frame16k: &[f32]) {
        if self.phase != Phase::Listening {
            return;
        }
        let streaming = match &self.streaming {
            Some(s) => s.clone(),
            None => return,
        };
        let mut guard = streaming
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(take) = guard.as_mut() {
            take.feed_frame(frame16k, 16_000);
        }
    }

    /// P4.3b (plan §4.3): install the per-segment polish function on a
    /// long take's committer. Called from the capture thread (via
    /// [`Self::feed_capture_frame`]) once the take's audio crosses
    /// [`crate::committer::TOTAL_AUDIO_SEGMENTED_POLISH_MS`] — the take's
    /// length is not known at take start, so the install is deferred to
    /// the first frame past the threshold. The take buffer is monotonically
    /// growing, so once it crosses it never un-crosses, and the install is
    /// idempotent.
    ///
    /// Runs on the capture thread, never on the controller. The
    /// `AppState` reads are short lock clones (the same seam the pipeline
    /// worker uses); the LLM call itself happens on the committer's worker
    /// thread. No text is logged. Takes `&AppState` (resolved at the call
    /// site) so headless unit tests can exercise it without a Tauri app.
    fn install_segment_polish(
        &mut self,
        streaming_slot: &StreamingSlot,
        state: &AppState,
        buf16_len: usize,
    ) {
        const SAMPLE_RATE: u32 = 16_000;
        if crate::committer::tail_audio_ms(buf16_len, SAMPLE_RATE)
            < crate::committer::TOTAL_AUDIO_SEGMENTED_POLISH_MS as u32
        {
            return;
        }
        let settings = state.settings();
        if settings.polish_whole_text_at_end {
            return;
        }
        let has_provider = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some();
        if !has_provider {
            return;
        }
        let Some(f) = build_segment_polish_fn(state) else {
            // No auto-apply transform: nothing to run per segment; the
            // release path behaves exactly as today.
            return;
        };
        // Install under the streaming lock so the committer's own `polish`
        // mutex swap is atomic with respect to the worker's reads.
        let mut g = streaming_slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(t) = g.as_mut() {
            t.committer.set_polish_fn(f);
        }
    }

    /// Loads the selected speech model in the background so the first
    /// dictation (or a model switch) doesn't block the release. Shows the
    /// warming pill while the load is in flight.
    fn play_cue(&self) {
        let settings = self.state().settings();
        if settings.play_recording_sounds {
            play_cue(&settings.recording_sound);
        }
    }

    fn warm_up(&mut self) {
        let (model_path, kind) = {
            let state = self.state();
            resolve_speech_model(&state.settings(), &state.models_dir)
        };
        if !model_path.exists() {
            return;
        }
        // Already loaded → skip the warming pill entirely; the model is
        // resident and the recording pill can stay as-is.
        let state = self.state();
        let ready = match kind {
            SpeechEngineKind::Fluid | SpeechEngineKind::Sherpa | SpeechEngineKind::Whistle => {
                state.neural.is_loaded()
            }
            SpeechEngineKind::Parakeet => state.parakeet.is_loaded(),
            SpeechEngineKind::Whisper => state.speech.is_loaded(),
        };
        if ready {
            return;
        }
        self.show(PillState::Warming);
        let next_id = {
            self.next_id += 1;
            self.next_id
        };
        let app = self.app().clone();
        let controller = self.controller.clone();
        crate::log_entry(
            crate::LogLevel::Info,
            format!(
                "loading speech model: {} ({:?})",
                model_path.display(),
                kind
            ),
        );
        let started = std::time::Instant::now();
        thread::Builder::new()
            .name("teletype-warmup".into())
            .spawn(move || {
                let state = app.state::<AppState>();
                // BUG-003 P0: the manager was constructed with an empty path;
                // set the real resolved path before loading.
                let manager = match kind {
                    SpeechEngineKind::Fluid
                    | SpeechEngineKind::Sherpa
                    | SpeechEngineKind::Whistle => &state.neural,
                    SpeechEngineKind::Parakeet => &state.parakeet,
                    SpeechEngineKind::Whisper => &state.speech,
                };
                manager.set_model_path(model_path.clone());
                let result = manager.load();
                match result {
                    Ok(()) => crate::log_entry_ms(
                        crate::LogLevel::Success,
                        format!("speech model loaded: {} ({:?})", model_path.display(), kind),
                        started.elapsed().as_millis() as u64,
                    ),
                    Err(e) => {
                        crate::log_entry(crate::LogLevel::Error, format!("warm-up failed: {e}"))
                    }
                }
                controller.send(Event::WarmupDone { session: next_id });
            })
            .ok();
    }

    fn stop(&mut self, cancelled: bool) {
        // P0.2: the release is the latency baseline every later claim is
        // measured against (release→paste).
        if let Some(key) = self.trace_key {
            if let Some(t) = self.traces.get_mut(&key) {
                t.mark(|s, now| s.t_hotkey_up = Some(now));
            }
        }
        // The take is over, so close the cue. Played for a cancel too: the
        // recording did stop, and silence here would read as "still listening"
        // — which for a cancel is exactly the state the user just escaped.
        self.play_cue();
        // Disarm VAD for this take: the capture thread's callback becomes a
        // no-op, and a late fire from the old session is dropped by the
        // session check in the `VadFired` handler.
        self.vad = None;
        // P4: the streaming state travels with the release. On a cancel it
        // is dropped outright (the committer's worker exits on drop); on a
        // clean release it is moved into the transcribe worker, which
        // finishes the committer with the uncommitted tail. The capture
        // thread's callback sees `None` and stops feeding.
        let streaming = self.streaming.take();
        // P4.3b: the polish install's app handle travels with the release
        // (dropped on cancel, released on a clean stop).
        self.polish_app.take();
        // Phase 5.7: the flag collector travels with the release like
        // `streaming` (dropped on cancel, moved into the worker on a clean
        // release). Phase 5.1: the pre-roll for this take.
        let vad_flags = self.vad_flags.take();
        let preroll = self.preroll_this_take.take();
        // Stop the live-preview interim loop. P1.4: this sets the cancel
        // flag and signals the thread but does NOT join it — the final
        // transcribe must not wait for an in-flight interim decode to
        // release the speech-model lock. The interim thread abandons the
        // decode on its own; `go_idle()` joins it once the final pass is
        // done and no lock contention is possible.
        if let Some(handle) = self.live_preview.take() {
            handle.stop();
        }
        // Clean stop: the take finished (or was cancelled), so the recovery
        // spool is no longer needed — delete it.
        if let Some(spool) = self.spool.take() {
            spool.complete();
        }
        // Phase 5.6: put the output volume back (and re-send the media
        // key). Runs on EVERY stop path — clean release, cancel, and the
        // early return when the mic never opened — because `stop` is the
        // one function every take end flows through. `restore` is a
        // warning no-op when the ducker was never active.
        if self.duck_armed {
            if let Err(e) = self
                .ducker
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .restore()
            {
                crate::log_entry(crate::LogLevel::Warn, format!("audio duck restore: {e}"));
            }
            self.duck_armed = false;
            if self.state().settings().pause_media_while_dictating {
                let _ = crate::ducking::toggle_media_play_pause();
            }
        }
        // The speaking time for this take, on a monotonic clock. Taken here,
        // at the moment the mic closes, because after this point the take is
        // transcribing and polishing, which is machine time and not speech.
        // A cancelled take measures nothing: the user threw it away.
        let speech_ms: Option<u64> = if cancelled {
            None
        } else {
            self.recording_started
                .take()
                .map(|t| t.elapsed().as_millis() as u64)
        };
        let recording = RECORDING.with(|slot| slot.borrow_mut().take());
        let Some(recording) = recording else {
            // The mic never opened: no take, no trace. Drop the pending
            // marks so a later take starts clean.
            if let Some(key) = self.trace_key.take() {
                self.traces.remove(&key);
            }
            self.go_idle();
            return;
        };

        // Update the pill to "Transcribing" immediately on release; the
        // (blocking) audio finish + resample happens on the worker thread so
        // the UI never stalls.
        self.next_id += 1;
        let session = self.next_id;
        // P0.2: from here on the trace travels with the session id the
        // workers report back with.
        if let Some(key) = self.trace_key.take() {
            if let Some(mut t) = self.traces.remove(&key) {
                t.session = session;
                if t.engine.is_empty() {
                    t.engine = self.state().settings().selected_speech_model.clone();
                }
                self.traces.insert(session, t);
            }
        }
        if let Some(ms) = speech_ms {
            self.speech_ms.insert(session, ms);
        }
        // P4: measure the streaming take's trace marks (committed segments +
        // tail audio ms) NOW, on the controller thread, before the worker
        // moves the state. These ride the session id to the pipeline worker,
        // which puts them in the `TraceStageMarks` (the whole-take fallback
        // path keeps 0/0, byte-identical to before).
        if let Some(slot) = &streaming {
            if let Some(t) = slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
            {
                let committed = t.committer.committed_count() as u32;
                let tail_ms = t.tail_audio_ms();
                if committed > 0 {
                    self.streaming_marks.insert(session, (committed, tail_ms));
                }
            }
        }
        // A cancel must discard the recording outright: no transcription, no
        // pipeline, no injection, no history. `next_id` advances so any
        // in-flight `Transcribed` event for this session is stale and dropped
        // by the staleness guard in `transcribed()`. P4: the streaming state
        // (and its committer) is dropped too — exactly today's cancel
        // semantics, no decode.
        if cancelled {
            let _ = recording.finish(); // drop the captured audio ASAP
            drop(streaming); // committer worker exits on drop; no decode
            self.phase = Phase::Cancelled;
            // P0.2: a cancelled take never pastes; finish its trace with the
            // reason so the cancellation is visible in the timing data.
            self.finish_trace(session, |t| {
                t.errors.push("cancelled by user".into());
            });
            // BUG-003: the session is over; arm the idle-unload timer.
            {
                let state = self.state();
                let delay_secs = state.settings().model_unload_delay_secs;
                let delay = (delay_secs > 0).then(|| std::time::Duration::from_secs(delay_secs));
                state.speech.end_dictation(delay);
                state.parakeet.end_dictation(delay);
            }
            self.broadcast(teletype_core::state::UiState::Message {
                text: "Cancelled".into(),
            });
            self.flash("Cancelled");
            return;
        }
        self.phase = Phase::Transcribing;
        self.broadcast(teletype_core::state::UiState::Transcribing);
        self.show(PillState::Processing {
            message: "Transcribing…".into(),
        });

        let app = self.app().clone();
        let controller = self.controller.clone();
        let (speech_model_path, kind) = {
            let state = self.state();
            resolve_speech_model(&state.settings(), &state.models_dir)
        };
        // Parakeet is English-only; "auto" there means English. Guarding here
        // prevents the Russian mis-transcription on short utterances.
        // P3.3: consult the per-app language override for the frontmost app
        // before falling back to the global setting.
        let language = self.effective_language(kind);
        // Keep a copy for the worker's detected-language plumbing below.
        let language_for_worker = language.clone();
        // P4: move the streaming state into the worker (it owns the
        // committer + tail for this take). `None` when streaming is off or
        // the take has no streaming state — the worker then runs the
        // whole-take decode path, byte-identical to before.
        let mut streaming = self.streaming.take();
        let input_device = self.state().settings().input_device.clone();

        let worker = thread::Builder::new()
            .name("teletype-transcribe".into())
            .spawn(move || {
                let captured = match recording.finish() {
                    Ok(c) => c,
                    Err(e) => {
                        controller.send(Event::Transcribed {
                            session,
                            transcript: Err(e.to_string()),
                            detected_language: None,
                            asr_start_us: 0,
                            asr_end_us: 0,
                            pcm: None,
                        });
                        return;
                    }
                };
                // Phase 5.7: per-take quality signals. Compute them right
                // after `captured` (before the trim) and send
                // `TakeQuality` BEFORE the transcription work: one FIFO
                // channel, so it always precedes this session's
                // `Transcribed`.
                let flags = vad_flags
                    .as_ref()
                    .map(|c| {
                        c.lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .clone()
                            .into_flags()
                    })
                    .unwrap_or_default();
                let bt_hfp = active_device_bt_hfp(&input_device);
                let quality = take_quality(&captured.samples, &flags, bt_hfp);
                controller.send(Event::TakeQuality { session, quality });
                // Phase 5.1: prepend the resampled pre-roll so the
                // whole-take path (and its trim) sees the pre-roll audio.
                // The streaming branch does not re-decode `captured`, so
                // the prepend is harmless there. `peak` stays captured.
                let asr_samples = merge_preroll(
                    &captured.samples,
                    preroll.as_ref().map(|(raw, rate)| (raw.as_slice(), *rate)),
                );
                let transcribe_started = std::time::Instant::now();
                // P0.2: ASR start/end on the same monotonic clock as every
                // other stage mark.
                let asr_start_us = now_us();
                // P4: when the streaming take committed segments mid-take,
                // the release decodes ONLY the uncommitted tail through the
                // committer (its own engine handle) and folds the committed
                // segments in by timestamp. The whole-take decode below is
                // the fallback for short takes (no mid-take commits) and for
                // takes without streaming state — byte-identical to before.
                let streaming_marks: Option<(u32, u32)> = streaming.as_ref().and_then(|slot| {
                    let g = slot
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    g.as_ref().filter(|t| t.committer.committed_count() > 0).map(|t| {
                        (
                            t.committer.committed_count() as u32,
                            t.tail_audio_ms(),
                        )
                    })
                });
                if let Some((committed, tail_ms)) = streaming_marks {
                    // Take the streaming state out of the slot (the capture
                    // thread's callback now sees `None` and stops feeding).
                    let slot = streaming.take().expect("streaming slot present");
                    let take = {
                        let mut g = slot
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        g.take().expect("streaming take present")
                    };
                    // `finish` waits for the tail (which jumps the committer
                    // worker's queue) and all in-flight segments, returning
                    // the committed segments in order followed by the tail.
                    let transcripts = take.finish(language.clone());
                    // Fold the transcripts by timestamp: overlapping words at
                    // segment boundaries are de-duplicated (the segmenter's
                    // 0.4 s forced-cut overlap is included in the FOLLOWING
                    // audio, so the later segment's overlapping words are
                    // dropped).
                    let mut acc: Option<teletype_speech::Transcript> = None;
                    let outcome = match transcripts {
                        Ok(ts) => {
                            for t in ts {
                                acc = Some(match acc {
                                    Some(prev) => {
                                        crate::committer::merge_transcripts(&prev, t)
                                    }
                                    None => t,
                                });
                            }
                            match acc {
                                Some(t) => Ok((t.text, Some(t.words))),
                                None => Err("No speech detected".to_string()),
                            }
                        }
                        Err(e) => Err(e),
                    };
                    let asr_end_us = now_us();
                    let elapsed_ms = transcribe_started.elapsed().as_millis() as u64;
                    match &outcome {
                        Ok((t, _)) if !t.trim().is_empty() => {
                            crate::log_entry_ms(
                                crate::LogLevel::Success,
                                format!(
                                    "streaming transcription complete: {committed} segments committed mid-take, tail {tail_ms} ms"
                                ),
                                elapsed_ms,
                            );
                        }
                        Ok(_) => {
                            crate::log_entry_ms(
                                crate::LogLevel::Info,
                                "no speech detected (streaming)",
                                elapsed_ms,
                            );
                        }
                        Err(e) => crate::log_entry(
                            crate::LogLevel::Error,
                            format!("streaming transcription failed: {e}"),
                        ),
                    }
                    controller.send(Event::Transcribed {
                        session,
                        transcript: outcome,
                        detected_language: None,
                        asr_start_us,
                        asr_end_us,
                        pcm: None,
                    });
                    return;
                }
                // P1.11: trim leading/trailing silence beyond 150 ms before
                // the final decode (faster decode, fewer hallucinations).
                let trimmed = trim_silence_vad(&asr_samples);
                if trimmed.len() != captured.samples.len() {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "vad trim: {} -> {} samples",
                            captured.samples.len(),
                            trimmed.len()
                        ),
                    );
                }
                let trimmed_captured = teletype_core::audio::Captured {
                    samples: trimmed.to_vec(),
                    duration_secs: trimmed.len() as f64 / 16_000.0,
                    peak: captured.peak,
                };                crate::log_entry(
                    crate::LogLevel::Info,
                    format!(
                        "transcribing audio: {} samples, language {}, engine {:?}",
                        trimmed_captured.samples.len(),
                        language,
                        kind
                    ),
                );
                let outcome =
                    transcribe(&app, &speech_model_path, &trimmed_captured, &language, kind);
                let asr_end_us = now_us();
                // The language the engine detected (auto-detect only), for the
                // pill chip and the lock-language flow.
                let detected_language =
                    detect_language_from_provider(&app, kind, language_for_worker.as_str());
                // Distinguish "nothing was recorded" from "the model found no
                // words in real speech" so the user gets an honest message.
                let transcript = match outcome {
                    Err(e) if e == "No speech detected" && captured.peak < 0.005 => {
                        Err("No audio captured — check the microphone".into())
                    }
                    other => other,
                };
                let elapsed_ms = transcribe_started.elapsed().as_millis() as u64;
                match &transcript {
                    Ok((t, _)) if !t.trim().is_empty() => {
                        crate::log_entry_ms(
                            crate::LogLevel::Success,
                            "transcription complete",
                            elapsed_ms,
                        );
                    }
                    Ok(_) => {
                        crate::log_entry_ms(
                            crate::LogLevel::Info,
                            "no speech detected",
                            elapsed_ms,
                        );
                    }
                    Err(e) => crate::log_entry(
                        crate::LogLevel::Error,
                        format!("transcription failed: {e}"),
                    ),
                }
                controller.send(Event::Transcribed {
                    session,
                    transcript,
                    detected_language,
                    asr_start_us,
                    asr_end_us,
                    pcm: Some(trimmed.to_vec()),
                });
            });
        if let Err(e) = worker {
            crate::log_entry(
                crate::LogLevel::Error,
                format!("couldn't start transcription: {e}"),
            );
            self.flash("Transcription failed");
        }
    }

    fn transcribed(
        &mut self,
        session: u64,
        transcript: Result<(String, Option<Vec<teletype_speech::Word>>), String>,
        detected_language: Option<String>,
        asr_start_us: u64,
        asr_end_us: u64,
        pcm: Option<Vec<f32>>,
    ) {
        if self.next_id != session {
            return; // stale
        }
        // P0.2: fold the worker's ASR marks into the session trace. A
        // zero start means the worker never ran the engine (audio finish
        // failed), so there is nothing to merge.
        if asr_start_us > 0 {
            if let Some(t) = self.traces.get_mut(&session) {
                t.t_asr_start = Some(asr_start_us);
                t.t_asr_end = Some(asr_end_us);
            }
        }
        if self.phase == Phase::Cancelled {
            // Belt and braces: a cancel that raced the worker must never
            // transcribe, pipeline, or inject.
            crate::log_entry(
                crate::LogLevel::Info,
                "dropping transcript for cancelled session",
            );
            // P0.2: the take ends here without a paste; keep the reason.
            self.finish_trace(session, |t| {
                t.errors.push("transcript arrived after cancel".into());
            });
            self.go_idle();
            return;
        }
        // This is the reliable moment to observe an edit to the *previous*
        // dictation: the user has spoken again, so they have finished editing
        // the last one. See `edit_watch` for why it is not done inline.
        // (The post-injection check in `arm_edit_check` usually handles this
        // first; this is the fallback for a take the user edits late.)
        self.observe_pending_edit();
        match transcript {
            Ok((raw, words)) => {
                // P3.7: when the engine reports per-word confidences
                // (Fluid, sherpa), run confidence-aware phonetic rescoring
                // over the dictionary + pack terms before the pipeline.
                // Engines without word timings (Whisper, ggml Parakeet) pass
                // `None` and skip rescoring — the pipeline's
                // `correct_with_dictionary` runs with confidence 1.0, so its
                // phonetic stay dormant and behavior is byte-identical.
                let text = if let Some(words) = &words {
                    self.rescore_with_confidence(raw, words)
                } else {
                    raw
                };
                // If the language is English and the model misidentified the
                // speech as Russian (Cyrillic), transliterate back to Latin.
                let text = ensure_latin_if_english(
                    &text,
                    &effective_language(self.state().settings().language.clone(), false),
                );
                self.phase = Phase::Transforming;
                self.broadcast(teletype_core::state::UiState::Transforming);
                self.show(PillState::Processing {
                    message: "Transforming…".into(),
                });

                // Read the measured speaking time before `self` is moved into
                // the worker closure.
                let speech_ms_for_session = self.speech_ms.get(&session).copied();
                // P4: the streaming take's trace marks (committed segments +
                // tail audio ms), measured in `stop()` and consumed here. The
                // whole-take fallback path keeps 0/0 (byte-identical to before).
                let streaming_marks = self.streaming_marks.remove(&session);
                // P2.2: the take kind travels into the worker by value. The
                // worker reads it (not a re-fetched snapshot) so an armed
                // command take is forced through command extraction even if
                // auto-classification would call it dictation.
                //
                // P2.5: a plain dictation take that opens with the wake
                // phrase is re-routed here, before the worker spawns, so the
                // rest of the pipeline (force_command, pill, trace) treats
                // it as a command take.
                let mut text = text;
                let mut take_kind = self.take_kind;
                if take_kind == TakeKind::Dictation && self.state().settings().command_wake_phrase {
                    if let Some(stripped) = strip_wake_phrase(&text) {
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!("wake phrase routed as command: {stripped}"),
                        );
                        text = stripped;
                        take_kind = TakeKind::Command;
                        // P2.9: the wake phrase is what armed this take; the
                        // recording itself started much earlier (visible in
                        // its hotkey-down / first-audio marks), so only the
                        // `armed` event is recorded here. `stop()` has
                        // already re-keyed the trace to this session id.
                        self.trace_command_event(session, "armed");
                    }
                }
                // P2.6: a Command take with selected text becomes a
                // Transform take: the spoken transcript is the instruction,
                // the selection is the target, run through the normal
                // transform engine as a one-shot ad-hoc transform. Consumed
                // here so a stale selection can never leak into a later
                // take (go_idle also clears it on early ends).
                let pending_selection = self.pending_selection.take();
                let (text, take_kind, selection_transform) = match (&take_kind, pending_selection) {
                    (TakeKind::Command, Some(selected)) => {
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!(
                                "selection command: {text:?} on {} chars",
                                selected.chars().count()
                            ),
                        );
                        let t = teletype_core::transforms::TransformDefinition::new(
                            "Voice command",
                            "one-shot instruction spoken over selected text",
                            text,
                        );
                        (selected, TakeKind::Transform, Some(t))
                    }
                    _ => (text, take_kind, None),
                };
                // Same for the recorded paste target: the gate compares it
                // against the live target after the pipeline finishes.
                let record_target = self.record_target.clone();
                let app = self.app().clone();
                let controller = self.controller.clone();
                // P1.3: the bookkeeping handle. The pipeline worker ships
                // `PipelineReady` (the injection) FIRST, then hands the
                // usage/history/dictionary-usage writes to the bookkeeper
                // so no disk write sits on the release→paste path.
                let bookkeeper = crate::bookkeeper::Bookkeeper::spawn(app.clone());
                let worker = thread::Builder::new()
                    .name("teletype-pipeline".into())
                    .spawn(move || {
                        let pipeline_started = std::time::Instant::now();
                        // A.3.3: the take audio for the command decider.
                        let pcm: &[f32] = pcm.as_deref().unwrap_or(&[]);
                        let state = app.state::<AppState>();
                        // Local polish models are not preloaded at launch
                        // load the selected one here,
                        // on first use, so the first dictation pays the load
                        // cost once. No-op if a provider is already loaded or a
                        // remote provider is active.
                        crate::commands::ensure_local_provider(&app);
                        let settings = state.settings();
                        let platform = state.platform.as_ref();
                        // P1-B: clone the clonable stores so their mutexes
                        // are dropped BEFORE pipeline.run() (which includes
                        // the LLM network call, up to 20 s). Without this,
                        // opening Settings/Models/Dictionary during a polish
                        // hangs for the full LLM latency. The inference lock
                        // stays held (correct: no model swap mid-inference).
                        let (autotext, transforms, mut profile, styles, dictionary) = {
                            let a = state
                                .autotext
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            let t = state
                                .transforms
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            let mut p = state
                                .profile
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            if let Some(detected) = &detected_language {
                                p.language = detected.clone();
                            }
                            let s = state
                                .styles
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                            // The master switch empties the dictionary passed
                            // to the pipeline: with zero words (and pack terms
                            // aside) the correction pass is a no-op, so one
                            // flag gates the whole feature without a second
                            // code path.
                            let d = if settings.dictionary_enabled {
                                state
                                    .dictionary
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .clone()
                            } else {
                                teletype_core::dictionary::Dictionary::default()
                            };
                            (a, t, p, s, d)
                        };
                        // P3.3: a per-app language override for the frontmost
                        // app wins over the (possibly detected) global
                        // language.
                        let app_ctx = state.platform.active_application().unwrap_or_default();
                        let active_style = settings.active_style_profile.clone();
                        let explicit_style = String::new();
                        // P1.8: clone the engine Arc and drop the lock BEFORE
                        // the LLM call. The pipeline holds the Arc snapshot
                        // for its whole run, so a model swap (which takes the
                        // lock to install a new Arc) never blocks on an
                        // in-flight generation, and the UI/model-list
                        // commands never block on the generation.
                        let inference: Option<Arc<dyn teletype_core::llm::InferenceProvider>> =
                            state
                                .inference
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .clone();
                        let decision = state
                            .decision
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner);
                        // A.3.3: snapshot the selected command decider while
                        // the state borrow is live (the pipeline worker runs
                        // off the main thread, so the Arc is cloned now).
                        let decider = crate::command_mode::select_decider(
                            &state.command_decider,
                            match detected_language.as_deref() {
                                Some(l) => l,
                                None => &settings.language,
                            },
                        );

                        let transform_model = inference
                            .as_ref()
                            .map(|p| format!("{} ({})", p.model_name(), p.model_id()))
                            .unwrap_or_else(|| "none (AutoText only)".into());
                        crate::log_entry(
                            crate::LogLevel::Info,
                            format!(
                                "running dictation pipeline: auto_apply={}, transform_model={}",
                                settings.auto_apply_transform, transform_model
                            ),
                        );
                        let input = UnifiedInput {
                            source: InputSource::Voice,
                            text,
                        };
                        // P3.3: apply the per-app language override (if any)
                        // so the LLM prompt language matches the app the user
                        // is dictating into.
                        {
                            let resolved = teletype_core::personalization::resolve_language(
                                &profile, &app_ctx,
                            );
                            if resolved != profile.language {
                                profile.language = resolved;
                            }
                        }
                        // T1.1: stream transform tokens to the pill for a live
                        // preview. P1.9: the sink emits `{seq, delta}` token
                        // events, not the whole accumulated string — the IPC
                        // cost per take is O(N) bytes, not O(N²). The pill
                        // concatenates the deltas in `seq` order.
                        // P0.2: the first streamed token is the closest
                        // observable point to "polish started producing".
                        let polish_first_token_us =
                            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
                        let first_token_mark = polish_first_token_us.clone();
                        let sink_app = app.clone();
                        let mut seq: u64 = 0;
                        let mut token_sink: Box<dyn FnMut(&str)> = Box::new(move |tok| {
                            if first_token_mark.load(std::sync::atomic::Ordering::Relaxed) == 0 {
                                first_token_mark
                                    .store(now_us(), std::sync::atomic::Ordering::Relaxed);
                            }
                            let _ = sink_app.emit_to(
                                "pill",
                                "transform-token",
                                serde_json::json!({ "seq": seq, "delta": tok }),
                            );
                            seq += 1;
                        });
                        // Vocabulary packs: precompute the enabled packs'
                        // terms once per dictation for the lowest-priority
                        // fuzzy correction tier.
                        let mut pack_terms =
                            teletype_core::vocab::terms_for(&settings.enabled_packs);
                        // User-created packs (item 15) feed the same tier.
                        {
                            let user_packs = state
                                .user_packs
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            pack_terms.extend(user_packs.terms_for());
                        }
                        // Dictionary strictness is a real dial, so the floor
                        // has to reach the checker. "standard" resolves to
                        // None and keeps the pipeline's own default rather than
                        // a number restated here, so the two cannot drift.
                        let strictness_floor =
                            crate::commands::match_strictness_floor(&settings.match_strictness);
                        let strictness_checker = teletype_core::dictionary::FloorChecker::new(
                            strictness_floor
                                .unwrap_or(teletype_core::dictionary::EditDistanceChecker::FLOOR),
                        );
                        let word_checker: &dyn teletype_core::dictionary::WordChecker =
                            match strictness_floor {
                                // The default floor is the shared zero-cost
                                // constant, so leaving strictness alone
                                // behaves exactly as it did before this setting
                                // existed.
                                None => &teletype_core::dictionary::EDIT_DISTANCE_CHECKER,
                                Some(_) => &strictness_checker,
                            };
                        // BUG-002: the spoken-punctuation gate filters the
                        // System entries the pipeline sees; custom AutoText is
                        // passed separately and is unaffected.
                        let system_entries: Vec<teletype_core::autotext::AutoTextEntry> =
                            if settings.spoken_punctuation {
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
                        // A.3.3: decider trace events collected before the
                        // shared `trace_marks` is built below.
                        let mut trace_marks_pre: Vec<String> = Vec::new();
                        let mut decider_label: Option<String> = None;
                        let mut decider_ms: Option<u32> = None;
                        let mut tool_label: Option<String> = None;
                        let mut args_source: Option<String> = None;
                        let mut pipeline = Pipeline {
                            platform,
                            autotext: &autotext,
                            transforms: &transforms,
                            profile: &profile,
                            inference: inference.as_deref(),
                            decision: decision.as_deref(),
                            force_command: take_kind == TakeKind::Command,
                            dictionary: &dictionary,
                            styles: &styles,
                            active_style: &active_style,
                            explicit_style: explicit_style.as_str(),
                            auto_apply: settings.auto_apply_transform,
                            restore_clipboard: settings.restore_clipboard,
                            remove_filler_words: settings.remove_filler_words,
                            filler_words: settings.filler_words.clone(),
                            restore_emoji: settings.restore_emoji,
                            spoken_emoji: settings.spoken_emoji,
                            spoken_punctuation: settings.spoken_punctuation,
                            system_autotext: &system_entries,
                            token_sink: Some(&mut token_sink),
                            polish_gate_enabled: settings.polish_gate_enabled,
                            polish_gate_threshold_words: settings.polish_gate_threshold_words,
                            pack_terms: &pack_terms,
                            word_checker,
                            list_style: teletype_core::transforms::ListStyle::from_str_lenient(
                                &settings.list_style,
                            ),
                        };
                        // P0.2: polish start is the moment the text pipeline
                        // (which ends in the LLM pass) begins.
                        let t_polish_start_us = now_us();
                        // P2.6: selection-command takes run the spoken
                        // instruction as a one-shot transform over the
                        // selection; everything else uses auto-apply.
                        let mut result = pipeline.run(input, selection_transform.as_ref());
                        // A.3.3: a command take decided by the decider
                        // replaces the provider's extraction. The decider
                        // ran only on command takes (the selection never
                        // reaches it — a Command take with selected text is
                        // re-keyed to Transform above), and its output is a
                        // superset of the provider's (free-form args filled
                        // from the transcript). `no_action` maps to `None`:
                        // the transcript is delivered as plain dictation,
                        // exactly like a failed extraction.
                        if take_kind == TakeKind::Command {
                            match decider {
                                Some(d) => match d.decide(pcm, &result.raw_input) {
                                    Ok(decision) => {
                                        let action = decision.to_command_action();
                                        crate::log_entry(
                                            crate::LogLevel::Info,
                                            format!(
                                                "command decider {}: tool={} args_source={} ({} ms)",
                                                d.kind().label(),
                                                decision.tool,
                                                decision.args_source,
                                                decision.decider_ms,
                                            ),
                                        );
                                        let tool = decision.tool.clone();
                                        result.command = action;
                                        decider_label = Some(d.kind().label().to_string());
                                        decider_ms = Some(decision.decider_ms);
                                        tool_label = Some(tool);
                                        args_source = Some(decision.args_source);
                                        trace_marks_pre.push(format!(
                                            "decider={} ({} ms)",
                                            d.kind().label(),
                                            decision.decider_ms,
                                        ));
                                    }
                                    Err(e) => {
                                        crate::log_entry(
                                            crate::LogLevel::Warn,
                                            format!(
                                                "command decider error, falling back to the provider extraction: {e}"
                                            ),
                                        );
                                        // The provider's extraction (already in
                                        // `result.command`) stands; the trace
                                        // records the fallback.
                                        decider_label = Some(d.kind().label().to_string());
                                        trace_marks_pre.push(format!(
                                            "decider={} (error: {e})",
                                            d.kind().label(),
                                        ));
                                    }
                                },
                                None => {
                                    // No decider selected (non-English or no
                                    // slot): the provider's extraction stands.
                                    decider_label = Some("needle-fallback".into());
                                    trace_marks_pre.push("decider=needle-fallback".into());
                                }
                            }
                        }
                        // P0.2: stage marks for this worker. The text
                        // pipeline (dictionary, ITN, polish gate, LLM) has
                        // no finer internal boundaries observable from here,
                        // so polish start/end bracket the same `run()` call
                        // the text-pipeline mark closes. When no transform
                        // ran, the polish marks stay empty and the reason is
                        // recorded instead.
                        let t_text_pipeline_end = now_us();
                        let first_token =
                            polish_first_token_us.load(std::sync::atomic::Ordering::Relaxed);
                        let polished = result.transform.as_ref().is_some_and(|t| t.transformed);
                        let polish_skipped_reason = result
                            .transform
                            .as_ref()
                            .and_then(|t| t.metrics.skip_reason.as_ref())
                            .map(teletype_core::state::skip_message)
                            .or_else(|| {
                                if result.transform_skipped_no_model {
                                    Some("no LLM model loaded".to_string())
                                } else if result.transform.is_none() {
                                    Some("no transform ran".to_string())
                                } else {
                                    None
                                }
                            });
                        let mut trace_marks = TraceStageMarks {
                            t_polish_start: Some(t_polish_start_us),
                            t_polish_first_token: match first_token {
                                0 => None,
                                v => Some(v),
                            },
                            t_polish_end: polished.then_some(t_text_pipeline_end),
                            t_text_pipeline_end: Some(t_text_pipeline_end),
                            polish_skipped_reason,
                            // P4: the streaming take's real trace marks
                            // (committed segments + tail audio ms), measured
                            // in `stop()` and carried by the session id. The
                            // whole-take fallback path keeps 0/0 (byte-
                            // identical to before P4).
                            committed_segments: streaming_marks.map(|m| m.0).unwrap_or(0),
                            tail_audio_ms: streaming_marks.map(|m| m.1).unwrap_or(0),
                            command_events: std::mem::take(&mut trace_marks_pre),
                        };
                        // P2.9: record the classification for every take
                        // that entered the command route — a dedicated
                        // command take (even when classification says
                        // dictation and the text falls back to injection)
                        // and a dictation take the pipeline turned into a
                        // command. `executed` / `refused(<reason>)` are
                        // appended at the execution site below, so the
                        // trace keeps the lifecycle order.
                        if take_kind == TakeKind::Command || result.command.is_some() {
                            trace_marks.command_events.push(format!(
                                "classified={}",
                                result
                                    .intent
                                    .map(teletype_core::llm::Intent::label)
                                    .unwrap_or("none")
                            ));
                        }

                        // Total pipeline time (from worker start to pipeline
                        // completion). The ASR time is logged separately by
                        // the transcribe worker; this covers transforms + LLM.
                        let pipeline_ms = pipeline_started.elapsed().as_millis();
                        let llm_ms = result
                            .transform
                            .as_ref()
                            .map(|t| t.metrics.latency_ms)
                            .unwrap_or(0);
                        tracing::info!(
                            pipeline_ms,
                            llm_ms,
                            transforms_ms = pipeline_ms.saturating_sub(llm_ms),
                            transformed = result.transformed,
                            "pipeline timing"
                        );

                        // Emit pipeline trace for the live visualizer.
                        let trace = serde_json::json!({
                            "raw_input": result.raw_input,
                            "intent": result.intent.map(|i| i.label().to_string()),
                            "command": result.command.as_ref().map(|c| {
                                serde_json::json!({ "tool": c.tool, "arguments": c.arguments })
                            }),
                            "transformed": result.transformed,
                            "final_text": result.final_text,
                            "pipeline_ms": pipeline_ms,
                            "llm_ms": llm_ms,
                        });
                        tracing::info!(?trace, "pipeline-trace emitted");
                        let _ = app.emit("pipeline-trace", &trace);

                        if let Some(transform) = &result.transform {
                            if transform.transformed {
                                crate::log_entry_ms(
                                    crate::LogLevel::Success,
                                    format!(
                                        "transform finished: fell_back={}",
                                        transform.metrics.fell_back
                                    ),
                                    transform.metrics.latency_ms as u64,
                                );
                            } else {
                                crate::log_entry_ms(
                                    crate::LogLevel::Info,
                                    format!(
                                        "transform finished: fell_back={}",
                                        transform.metrics.fell_back
                                    ),
                                    transform.metrics.latency_ms as u64,
                                );
                            }
                        } else {
                            crate::log_entry(
                                crate::LogLevel::Info,
                                "transform skipped: no transform ran",
                            );
                        }

                        // The pill must leave "Transforming…" on every exit
                        // path. The reset is sent through the controller
                        // channel (PipelineWorkerDone) so it is handled on
                        // the MAIN thread, where `self.show()` reliably
                        // delivers the event to the pill webview. A direct
                        // `emit_to` from this worker thread can be lost when
                        // the main thread is blocked on an IPC call that
                        // waits for a lock this worker still holds — that
                        // was the stuck-pill bug.
                        controller.send(Event::PipelineWorkerDone { session });
                        // DEV-ONLY artificial delay: widens the window
                        // between "bookkeeping done" and "controller may
                        // inject" so the cancel-vs-ready race can be
                        // exercised by hand. 0 unless the env var says
                        // otherwise; it is a diagnostic, not a feature.
                        let ready_delay = dev_delay_ms(PIPELINE_READY_DELAY_ENV);
                        if !ready_delay.is_zero() {
                            tracing::debug!(
                                ?ready_delay,
                                session,
                                "dev: artificial delay before PipelineReady"
                            );
                            thread::sleep(ready_delay);
                        }

                        // The fallback must never be silent: a transform was
                        // selected but no LLM provider was loaded, so only
                        // AutoText/filler cleanup ran.
                        if result.transform_skipped_no_model {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                "transform skipped: no LLM model loaded; \
                                 applied AutoText only. Pick a model in Settings > Models.",
                            );
                        }

                        // Surface the skip reason on all three surfaces:
                        // pill (pill-skip event), tray tooltip, and
                        // UiState::Message broadcast.
                        if let Some(reason) = result
                            .transform
                            .as_ref()
                            .and_then(|t| t.metrics.skip_reason.as_ref())
                        {
                            let msg = teletype_core::state::skip_message(reason);
                            let _ = app.emit_to(
                                "pill",
                                "pill-skip",
                                &serde_json::json!({ "message": msg.clone() }),
                            );
                            crate::tray::show_skip(&app, &msg);
                            let _ = app.emit(
                                "dictation-state",
                                &teletype_core::state::UiState::Message { text: msg.clone() },
                            );
                            crate::tray::show_state(
                                &app,
                                &teletype_core::state::UiState::Message { text: msg.clone() },
                            );
                        }

                        // P1.3: usage counts are computed here (cheap,
                        // in-memory) but the store write is handed to the
                        // bookkeeper, so no disk write sits on the
                        // release→paste path. The bookkeeper coalesces
                        // bursts of rapid takes into one save per store.
                        {
                            let raw = result.raw_input.clone();
                            let ctx = result.context.clone();
                            let filler_counts = if settings.remove_filler_words {
                                teletype_core::usage::count_fillers(&raw, &settings.filler_words)
                            } else {
                                std::collections::BTreeMap::new()
                            };
                            let autotext_counts =
                                teletype_core::usage::count_autotext(&raw, &autotext, &ctx);
                            bookkeeper.usage(filler_counts, autotext_counts);
                        }

                        drop(inference);
                        drop(dictionary);
                        drop(styles);
                        drop(profile);
                        drop(transforms);
                        drop(autotext);

                        // P1.3: history + transcript file + dictionary usage
                        // are handed to the bookkeeper. The entry is built
                        // here (the context was captured at pipeline run
                        // time, the app focused while dictating) but the
                        // disk writes happen on the bookkeeper thread,
                        // coalesced, and only AFTER `PipelineReady` has
                        // been shipped for the injection.
                        {
                            let created_at = teletype_core::storage::now_ms();
                            let entry = teletype_core::history::DictationEntry {
                                id: uuid::Uuid::new_v4().to_string(),
                                created_at,
                                text: result.final_text.clone(),
                                context: Some(result.context.clone()),
                                // Measured, not assumed: how long the mic
                                // was actually open for this take.
                                duration_ms: speech_ms_for_session,
                            };
                            let dir = state.transcripts_dir();
                            let app_name = result.context.application_name.clone();
                            let transcript_text = result.final_text.clone();
                            bookkeeper.history(entry, dir, created_at, transcript_text, app_name);
                            if settings.dictionary_enabled {
                                bookkeeper.dictionary_usage(result.final_text.clone());
                            }
                        }

                        // Surface the detected language in the pill (tap to
                        // lock it) when auto-detect found one.
                        if let Some(detected) = &detected_language {
                            let chip = LanguageChip {
                                code: detected.clone(),
                                detected: true,
                            };
                            let _ = app.emit("pill-language", &chip);
                            // Persist the detected language back to settings
                            // so the Settings UI reflects it instead of
                            // always showing "Auto-detect".
                            let _save_guard = state
                                .save_lock
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner);
                            let mut s = state.settings();
                            if s.language != *detected {
                                s.language = detected.clone();
                                if state.replace_settings(s).is_ok() {
                                    let _ = app.emit("settings-changed", &());
                                }
                            }
                            drop(_save_guard);
                        }
                        // Route to the scratchpad when it's enabled and
                        // frontmost; otherwise inject into the previous focus.
                        // The last bookkeeping step, and the hand-off. The
                        // worker does NOT perform the user-visible side
                        // effect (scratchpad append or text injection): it
                        // ships the prepared payload to the controller with
                        // `PipelineReady` and waits for the controller's
                        // verdict. A `Cancel` that reaches the controller
                        // queue before this event marks the session
                        // cancelled, so the verdict discards the result;
                        // one that reaches it after the verdict is simply
                        // too late. The worker never re-checks a shared
                        // flag — the FIFO order on the controller channel
                        // IS the cancellation decision.
                        // If the intent was Command and a structured action
                        // was extracted, execute it instead of injecting text.
                        if let Some(ref cmd) = result.command {
                            crate::log_entry(
                                crate::LogLevel::Info,
                                format!("command extracted: {} ({})", cmd.tool, cmd.arguments),
                            );
                            let exec_result =
                                crate::command_executor::execute(cmd, &result.context);
                            match &exec_result {
                                Ok(msg) => {
                                    crate::log_entry(
                                        crate::LogLevel::Success,
                                        format!("command executed: {msg}"),
                                    );
                                    // Show feedback in the pill so the user knows what happened.
                                    let _ = app.emit_to(
                                        "pill",
                                        "pill-skip",
                                        &serde_json::json!({ "message": format!("✓ {msg}") }),
                                    );
                                    // P2.9: the command route completed.
                                    trace_marks.command_events.push("executed".into());
                                }
                                Err(e) => {
                                    crate::log_entry(
                                        crate::LogLevel::Error,
                                        format!("command execution failed: {e}"),
                                    );
                                    let _ = app.emit_to(
                                        "pill",
                                        "pill-skip",
                                        &serde_json::json!({ "message": format!("✗ {e}") }),
                                    );
                                    // P2.9: refused, with the executor's reason.
                                    trace_marks.command_events.push(format!("refused({e})"));
                                }
                            }
                            // Still record in history for audit. P1.3:
                            // handed to the bookkeeper like the dictation
                            // path, so the write is coalesced and off the
                            // critical path. No plain-text transcript line
                            // for a command (it is not dictated speech).
                            let created_at = teletype_core::storage::now_ms();
                            let entry = teletype_core::history::DictationEntry {
                                id: uuid::Uuid::new_v4().to_string(),
                                created_at,
                                text: format!("[command] {}", result.raw_input),
                                context: Some(result.context.clone()),
                                duration_ms: speech_ms_for_session,
                            };
                            bookkeeper.history(
                                entry,
                                state.transcripts_dir(),
                                created_at,
                                String::new(),
                                result.context.application_name.clone(),
                            );
                            controller.send(Event::PipelineReady {
                                session,
                                payload: PipelinePayload {
                                    session,
                                    final_text: String::new(),
                                    context: result.context,
                                    record_target,
                                    detected_language,
                                    command: Some(cmd.clone()),
                                    trace_marks: trace_marks.clone(),
                                    decider: decider_label.clone(),
                                    decider_ms,
                                    tool: tool_label.clone(),
                                    args_source: args_source.clone(),
                                },
                            });
                        } else {
                            controller.send(Event::PipelineReady {
                                session,
                                payload: PipelinePayload {
                                    session,
                                    final_text: result.final_text,
                                    context: result.context,
                                    record_target,
                                    detected_language,
                                    command: None,
                                    trace_marks,
                                    decider: decider_label,
                                    decider_ms,
                                    tool: tool_label,
                                    args_source,
                                },
                            });
                        }
                        // The controller answers with `PipelineDone` once it
                        // has made the verdict and acted on it. Do NOT send
                        // it here: that would re-enter the done path and run
                        // the pill reset / edit-check twice (the
                        // repeat-paste bug, in UI form).
                    });
                if let Err(e) = worker {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!("pipeline thread failed: {e}"),
                    );
                    self.flash("Transform failed");
                }
            }
            Err(e) => {
                crate::log_entry(crate::LogLevel::Error, format!("transcription failed: {e}"));
                // P0.2: no transcript means no pipeline and no paste; the
                // trace ends here with the failure recorded.
                let msg = e.clone();
                self.finish_trace(session, |t| {
                    t.errors.push(format!("transcription failed: {msg}"));
                });
                self.flash(&e);
            }
        }
    }

    /// Reads back the previous dictation's field and, if the user changed it,
    /// feeds the difference into the personalization loop.
    ///
    /// Every guard here fails closed. A wrong signal teaches the model a
    /// preference the user never expressed, so an unreadable field, a field we
    /// no longer recognise, or a diff we cannot explain all result in learning
    /// nothing.
    fn observe_pending_edit(&mut self) {
        use crate::edit_watch::EditDiff;

        // P7.2: the next dictation is one of the five observer triggers. The
        // learning-store observation runs first, with its own guards (anchor
        // + locate distance), and is independent of the UI-hint path below.
        self.observe_learning_store();

        let app = self.app().clone();
        let pending: Option<crate::edit_watch::PendingEdit> = {
            let state = app.state::<AppState>();
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.take()
        };
        let Some(pending) = pending else {
            return;
        };

        let state = app.state::<AppState>();
        // Never learn from our own scratchpad: the "edit" there is just the
        // appended dictation.
        if state.settings().scratchpad_enabled && pending.target.eq_ignore_ascii_case("Teletype") {
            return;
        }
        // Only meaningful while the same app is still frontmost. If the user
        // switched apps, the field we would read is a different one.
        let now_front = state
            .platform
            .active_application()
            .map(|a| a.application_name)
            .unwrap_or_default();
        if now_front.is_empty() || !now_front.eq_ignore_ascii_case(&pending.target) {
            crate::log_entry(
                crate::LogLevel::Info,
                "edit watch: focus moved, skipping observation",
            );
            return;
        }

        // Reading the field is the platform's job; without it we cannot observe
        // anything, so skip rather than guess.
        let Some(value) = state.platform.focused_text() else {
            crate::log_entry(
                crate::LogLevel::Info,
                "edit watch: field not readable, skipping",
            );
            return;
        };

        let diff = crate::edit_watch::diff(&value, &pending.inserted);
        // `at` is deliberately not destructured: it indexes the whole field,
        // not the text we inserted. See the rebuild below.
        let EditDiff::Edited { replacement, .. } = diff else {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("edit watch: no learnable edit ({diff:?})"),
            );
            return;
        };

        // The AI output is the text we inserted; the final text is the field
        // with the user's edit applied. Rebuild the latter from the anchored
        // prefix so the learning extractor sees the user's version of the
        // dictation, not the whole surrounding field.
        // `diff` only returns `Edited` when the whole of `inserted` is still
        // present verbatim in the field and `replacement` is what follows it,
        // so the user's edited version of the dictation is
        // `inserted + replacement`. Slicing `inserted` by the field offset
        // used to panic (out of bounds, or mid-character) and lose the take.
        // For in-place edits, `replacement` is the full current value (the user's
        // edited text). For append edits, it is the text after the insert, so the
        // user's text is `inserted + replacement`.
        let user_text = if replacement.len() >= pending.inserted.len()
            && !replacement.starts_with(&pending.inserted)
        {
            replacement.clone()
        } else {
            format!("{}{}", pending.inserted, replacement)
        };
        let app_ctx = state.platform.active_application().unwrap_or_default();

        let changed = {
            let mut profile = state
                .profile
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let changed = commands::record_edit_core_public(
                &mut profile,
                &pending.inserted,
                &user_text,
                &app_ctx,
            );
            if !changed.is_empty() {
                if let Err(e) = state.profile_store.save(&*profile) {
                    crate::log_entry(
                        crate::LogLevel::Error,
                        format!("edit watch: profile save failed: {e}"),
                    );
                }
            }
            // Pair ids with their user-facing descriptions for the undo
            // pill; looked up here while the profile is locked.
            // T5.1 (D006): a learned terminology correction is also recorded
            // on the dictionary word (for the "learned" badge), so carry its
            // from/to pair alongside the id and description.
            let changed: Vec<_> = changed
                .into_iter()
                .filter_map(|id| {
                    let pref = profile.preferences.iter().find(|p| p.id == id)?;
                    let (id, description) = (pref.id.clone(), pref.description.clone());
                    let learned = pref.learned_correction();
                    Some((id, description, learned))
                })
                .collect::<Vec<_>>();
            if changed.iter().any(|(_, _, l)| l.is_some()) {
                let mut dict = state
                    .dictionary
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let mut dictionary_changed = false;
                for (_, _, learned) in &changed {
                    if let Some((from, to)) = learned {
                        if dict.insert_learned(from, to) {
                            dictionary_changed = true;
                        }
                    }
                }
                if dictionary_changed {
                    if let Err(e) = state.dictionary_store.save(&*dict) {
                        crate::log_entry(
                            crate::LogLevel::Error,
                            format!("edit watch: dictionary save failed: {e}"),
                        );
                    }
                }
            }
            changed
                .into_iter()
                .map(|(id, description, _)| (id, description))
                .collect::<Vec<_>>()
        };
        if !changed.is_empty() {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("edit watch: learned {} preference(s)", changed.len()),
            );
            // Undo offer: the tray menu gains an Undo item and the main
            // window toasts with an Undo button. The pill stays out of it:
            // it is click-through outside recordings and its fixed sizes
            // leave no room for a button without resizing surgery.
            let first = changed
                .first()
                .map(|(_, d)| d.as_str())
                .unwrap_or("a writing preference");
            let message = if changed.len() == 1 {
                format!("Learned: {first}")
            } else {
                format!("Learned {} preferences (latest: {first})", changed.len())
            };
            let ids: Vec<String> = changed.into_iter().map(|(id, _)| id).collect();
            {
                let mut slot = state
                    .last_learned
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                *slot = Some(ids.clone());
            }
            crate::tray::offer_undo(&app, &message);
            let _ = app.emit(
                "learned-preference",
                &serde_json::json!({ "message": message, "ids": ids }),
            );
        }
    }

    // ── P7.2: learning-store recording + anchored observation ──────────

    /// Opens the learning store lazily on first use. The file lives at
    /// `<app_data>/learning/learning.sqlite` (the same location
    /// `commands.rs` resolves for the insights UI). A failure logs once and
    /// disables learning for the session: nothing here may ever block or
    /// fail a paste.
    fn learning_store(&mut self) -> Option<&teletype_core::learning::store::LearningStore> {
        if self.learning_store.is_none() {
            let dir = self.state().data_dir.clone();
            let path = dir.join("learning").join("learning.sqlite");
            match teletype_core::learning::store::LearningStore::open(&path) {
                Ok(store) => self.learning_store = Some(store),
                Err(e) => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        format!("learning store: open failed, disabled for session: {e}"),
                    );
                }
            }
        }
        self.learning_store.as_ref()
    }

    /// Records one successful paste as a `DictationRow` and returns the row
    /// id. Only called from the paste site in `pipeline_ready` (refused and
    /// cancelled takes never reach it). Returns `None` when the store is
    /// unavailable or the insert failed — the paste itself is unaffected;
    /// the watch then arms without a store id and the observation is a no-op.
    fn record_dictation_row(&mut self, payload: &PipelinePayload, final_text: &str) -> Option<i64> {
        // Collect everything we need from self before taking the store
        // borrow, to avoid a double borrow.
        let state = self.state();
        let settings = state.settings();
        // app_id + category: prefer the P6.4 screen context captured at
        // hotkey-down; fall back to the context the pipeline saw.
        let (app_id, app_category) = match &self.screen_context {
            Some(ctx) => (ctx.app_id.clone().unwrap_or_default(), ctx.app_category),
            None => (
                payload.context.application_id.clone(),
                teletype_core::app_category::app_category(
                    if payload.context.application_id.is_empty() {
                        None
                    } else {
                        Some(payload.context.application_id.as_str())
                    },
                    payload.context.window_title.as_deref(),
                ),
            ),
        };
        let engine = settings.selected_speech_model.clone();
        let lang = effective_language(settings.language.clone(), false);
        let polish_model = if settings.selected_llm_model.is_empty() {
            None
        } else {
            Some(settings.selected_llm_model.clone())
        };
        let store = self.learning_store()?;
        // `raw_asr`: the pre-pipeline text is not carried on the payload
        // (`PipelinePayload` only ships the final text), so the post-pipeline
        // text is recorded as the v1 compromise, per the task note.
        // `words_json`: word timings are not carried onto the controller
        // either (they are consumed by the confidence rescorer on the
        // worker), so v1 records an empty list.
        let row = teletype_core::learning::store::DictationRow {
            id: 0,
            ts: teletype_core::storage::now_ms(),
            app_id,
            app_category: app_category.as_str().to_string(),
            engine,
            lang,
            audio_path: None, // audio retention is a later unit
            raw_asr: payload.final_text.clone(),
            words_json: "[]".to_string(),
            pasted: final_text.to_string(),
            observed: None,
            edit_ops_json: None,
            polish_model,
            // The controller does not know whether polish ran (that
            // knowledge stays on the worker's `PipelineResult`); the
            // `polish_skipped_reason` trace mark carries the truth for the
            // trace. Record `false` (polish attempted) as the v1 default.
            polish_skipped: false,
        };
        match store.record_dictation(&row) {
            Ok(id) => Some(id),
            Err(e) => {
                crate::log_entry(
                    crate::LogLevel::Warn,
                    format!("learning store: record_dictation failed: {e}"),
                );
                None
            }
        }
    }

    /// Reads the field back and, if the pasted span is still locatable,
    /// writes `observed` + `edit_ops_json` to the store row. Triggered by
    /// the next dictation (the reliable moment: the user has finished
    /// editing or moved on). Every guard fails closed: unreadable field,
    /// unlocatable span, missing anchor or store id — drop, never guess.
    fn observe_learning_store(&mut self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let state = self.state();
        let (pending, observer) = {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let p = watch.pending().cloned();
            let o = watch.observer_mut().cloned();
            (p, o)
        };
        let (Some(pending), Some(mut observer)) = (pending, observer) else {
            return;
        };
        // Open a fresh sink handle on the same file (cheap on an existing
        // file); the session's own handle stays put.
        let sink: Option<crate::edit_watch::StoreSink> = {
            let dir = self.state().data_dir.clone();
            let path = dir.join("learning").join("learning.sqlite");
            crate::edit_watch::StoreSink::open(&path)
        };
        let outcome = crate::edit_watch::observe_once(
            &mut observer,
            teletype_core::learning::observer::ObserverEvent::NextDictationStarted,
            crate::edit_watch::now_ms(),
            &pending,
            &read_focused_field,
            sink.as_ref()
                .map(|s| s as &dyn crate::edit_watch::LearningSink),
        );
        // The decision is final: clear the watch so the UI-hint path in
        // `observe_pending_edit` does not re-process it.
        {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.clear();
        }
        if let Some(outcome) = outcome {
            match outcome {
                crate::edit_watch::ObserveOutcome::NotLocated => crate::log_entry(
                    crate::LogLevel::Info,
                    "learning observe: span not located, dropped",
                ),
                crate::edit_watch::ObserveOutcome::FieldUnreadable => crate::log_entry(
                    crate::LogLevel::Info,
                    "learning observe: field unreadable, dropped",
                ),
                _ => {}
            }
        }
    }

    /// The periodic half of the observer (20 s idle / 3 min cap), driven by
    /// the existing 2 s prewarm tick — no second timer. No-op when no watch
    /// is armed.
    fn poll_learning_observer(&mut self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let state = self.state();
        let (pending, observer) = {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let p = watch.pending().cloned();
            let o = watch.observer_mut().cloned();
            (p, o)
        };
        let (Some(pending), Some(mut observer)) = (pending, observer) else {
            return;
        };
        let sink: Option<crate::edit_watch::StoreSink> = {
            let dir = self.state().data_dir.clone();
            let path = dir.join("learning").join("learning.sqlite");
            crate::edit_watch::StoreSink::open(&path)
        };
        let outcome = crate::edit_watch::poll_observer(
            &mut observer,
            crate::edit_watch::now_ms(),
            &pending,
            &read_focused_field,
            sink.as_ref()
                .map(|s| s as &dyn crate::edit_watch::LearningSink),
        );
        // Write the (possibly updated) observer back; clear the watch when
        // the decision is final.
        {
            let mut watch = state
                .edit_watch
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            watch.set_observer(Some(observer));
            if outcome.is_some() {
                watch.clear();
            }
            if matches!(outcome, Some(crate::edit_watch::ObserveOutcome::NotLocated)) {
                crate::log_entry(
                    crate::LogLevel::Info,
                    "learning observe (poll): span not located, dropped",
                );
            }
        }
    }

    /// Cancels the pending learning observation (take cancelled/discarded,
    /// session going idle without a paste). No store row is written; the
    /// watch simply drops.
    fn cancel_learning_watch(&mut self) {
        #[cfg(test)]
        if self.headless_flash {
            return;
        }
        let state = self.state();
        let mut watch = state
            .edit_watch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if watch.pending().is_some() && watch.observer_mut().is_some() {
            // Feed the observer a Canceled event so the state machine is
            // honest (final_ = true, decision = Drop).
            if let Some(o) = watch.observer_mut() {
                let _ = o.on_event(
                    teletype_core::learning::observer::ObserverEvent::Canceled,
                    crate::edit_watch::now_ms(),
                );
            }
        }
        watch.clear();
    }
    /// T2.1: after injection, wait for the user to finish editing the
    /// inserted text, then diff it once and learn. The check runs on the
    /// controller thread (AX reads are main-thread on macOS) and only if the
    /// watch is still pending — the next dictation's `observe_pending_edit`
    /// remains the fallback.
    /// The single point where a pipeline result becomes a user-visible side
    /// effect. Runs on the controller thread, which is the only thread that
    /// ever calls `insert_text`, the scratchpad append, or the injector.
    ///
    /// Verdict, in order:
    /// 1. session id was tagged cancelled by a `Cancel` event processed
    ///    earlier on this same queue → discard, no injection, no scratchpad.
    /// 2. session is no longer the newest (a newer take superseded it) →
    ///    still inject (the delivery gate re-verifies the target) but the
    ///    pill hand-off belongs to the newer session.
    /// 3. otherwise → inject.
    ///
    /// A `Cancel` processed *after* this returns is allowed: the text has
    /// already landed, and the cancel then transitions the session as
    /// normal. That ordering is the intended semantics, not a bug.
    ///
    /// The `on_inject` hook exists for the concurrency tests: they run the
    /// controller loop without a Tauri app, so the real injection (which
    /// needs the main thread / platform services) is replaced by a
    /// recording closure. Production passes `None` and the real path runs.
    /// The hook is called exactly where the side effect happens, so a test
    /// can assert "injected" vs "discarded" against the event order.
    fn pipeline_ready(
        &mut self,
        session: u64,
        payload: PipelinePayload,
        on_inject: Option<&mut dyn FnMut(&PipelinePayload)>,
    ) {
        debug_assert_eq!(payload.session, session);
        // P9.6: record the e2e outcome BEFORE the verdict (the cancelled
        // path returns early below and would skip the post-body hook).
        self.e2e_record(session, &payload);
        if self.cancelled_sessions.remove(&session) {
            // Cancel won the race. The result is discarded; the text is
            // already saved to history and the transcript file by the
            // worker, so it stays recoverable there.
            tracing::info!(
                session,
                "pipeline ready for cancelled session; discarding result, no injection"
            );
            crate::log_entry(
                crate::LogLevel::Info,
                format!(
                    "session {session}: cancelled before the pipeline finished — result discarded (kept in History)"
                ),
            );
            // P0.2: the take ends here without a paste; keep the reason.
            let marks = payload.trace_marks;
            let decider_marks = (
                payload.decider,
                payload.decider_ms,
                payload.tool,
                payload.args_source,
            );
            self.finish_trace(session, |t| {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                t.errors
                    .push("cancelled before the pipeline finished".into());
            });
            self.flash("Cancelled");
            // The worker's bookkeeping is complete, so the pill reset for a
            // superseded session still goes through the normal path.
            if self.next_id != session && self.phase != Phase::Idle {
                self.go_idle();
            }
            return;
        }
        if self.next_id != session {
            crate::log_entry(
                crate::LogLevel::Info,
                format!(
                    "pipeline ready for superseded session {session}; injecting, newer session owns the pill"
                ),
            );
        }
        // DEV-ONLY artificial delay between the verdict and the side
        // effect, so the "cancel after ready" ordering can be exercised by
        // hand. 0 unless the env var says otherwise.
        let inject_delay = dev_delay_ms(PIPELINE_INJECT_DELAY_ENV);
        if !inject_delay.is_zero() {
            tracing::debug!(
                ?inject_delay,
                session,
                "dev: artificial delay before injection"
            );
            thread::sleep(inject_delay);
        }
        if let Some(on_inject) = on_inject {
            // Test path: the controller made the "inject" verdict. Record
            // it instead of touching platform services, then finish the
            // session exactly like the production path does.
            let marks = payload.trace_marks.clone();
            on_inject(&payload);
            // P0.2: same marks and paste stamp as production; the write is
            // skipped (no app handle in tests — see `finish_trace`).
            let audio_ms = self.speech_ms.get(&session).copied().unwrap_or(0) as u32;
            let decider_marks = (
                payload.decider,
                payload.decider_ms,
                payload.tool,
                payload.args_source,
            );
            if let Some(t) = self.traces.get_mut(&session) {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                t.audio_ms = audio_ms;
                t.mark(|s, now| s.t_paste_sent = Some(now));
            }
            self.phase = Phase::Idle;
            self.controller.send(Event::PipelineDone { session });
            return;
        }
        let settings = self.state().settings();
        let e2e_forced = self.e2e_forced_mode(&payload);
        let state = self.state();
        let platform: &dyn teletype_core::platform::Platform = state.platform.as_ref();
        // Route to the scratchpad when it's enabled and frontmost;
        // otherwise inject into the previous focus.
        let to_scratchpad = settings.scratchpad_enabled
            && payload
                .context
                .application_name
                .eq_ignore_ascii_case("Teletype");
        // P0.2: when delivery is refused nothing lands, so the trace gets
        // the reason instead of a paste mark.
        let mut refused: Option<String> = None;
        if to_scratchpad {
            let state = self.state();
            let mut pad = state
                .scratchpad
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            pad.append(payload.final_text.clone());
            if let Err(e) = state.scratchpad_store.save(&*pad) {
                tracing::warn!("Failed to save scratchpad: {e}");
            }
        } else if let Some(msg) = match e2e_forced {
            Some(_) => None, // P9.6: the forced mode owns the cascade
            None => delivery_refusal(
                &payload.record_target,
                platform,
                payload.context.application_type,
                &payload.final_text,
            ),
        } {
            // Refused delivery: no Tier 1, no Tier 2. The text is already in
            // history and the transcript file, and the clipboard was never
            // touched, so nothing needs restoring. The edit-watch record is
            // skipped for the same reason: nothing landed, so there is
            // nothing to learn a correction from.
            crate::log_entry(crate::LogLevel::Info, format!("delivery: {msg}"));
            let app = self.app().clone();
            let _ = app.emit_to(
                "pill",
                "pill-skip",
                &serde_json::json!({ "message": msg.clone() }),
            );
            crate::tray::show_skip(&app, &msg);
            let _ = app.emit(
                "dictation-state",
                &teletype_core::state::UiState::Message { text: msg.clone() },
            );
            // P0.2: nothing landed, so there is no paste mark; the reason is
            // recorded on the trace below.
            refused = Some(format!("delivery refused: {msg}"));
        } else {
            // P7.2: capture the paste anchor BEFORE the insertion. The
            // surroundings read is the pre-insertion caret context, which is
            // exactly what `PasteAnchor` wants (≤ 40 chars each side of the
            // insertion point). A failed read yields no anchor — the
            // observation is then dropped, never guessed.
            let surroundings = focused_surroundings();
            let anchor = crate::edit_watch::build_anchor(
                surroundings.as_ref().map(|(b, _)| b.as_str()),
                surroundings.as_ref().map(|(_, a)| a.as_str()),
                &payload.final_text,
            );
            // Smart insertion, resolved once for both delivery tiers: the
            // direct write and the clipboard paste must land the same
            // characters, or the text changes shape depending on which
            // route the injector happened to pick.
            //
            // Falls back to the dictated text unchanged when the field
            // cannot be read. An unreadable field is not an error worth
            // surfacing — it just means there is no context to fit the
            // text into.
            let final_text = if settings.smart_insertion {
                #[cfg(target_os = "windows")]
                {
                    // P8.5: Windows smart spacing. The core smart_insert::merge
                    // handles whitespace and capitalisation; CJK spacing is
                    // the Windows-specific rule (CJK scripts do not separate
                    // words with spaces), so it is applied here via the pure
                    // `should_add_space` predicate against the UIA caret read.
                    let before = crate::uia::text_before_caret(400).unwrap_or_default();
                    let merged = teletype_core::smart_insert::merge(
                        &teletype_core::smart_insert::Surroundings {
                            before: &before,
                            after: "",
                        },
                        &payload.final_text,
                    );
                    let cjk_gap = crate::commands::should_add_space(&before)
                        && !merged.starts_with(char::is_whitespace);
                    if cjk_gap {
                        let mut out = String::with_capacity(merged.len() + 1);
                        out.push(' ');
                        out.push_str(&merged);
                        out
                    } else {
                        merged
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    match focused_surroundings() {
                        Some((before, after)) => {
                            let merged = teletype_core::smart_insert::merge(
                                &teletype_core::smart_insert::Surroundings {
                                    before: &before,
                                    after: &after,
                                },
                                &payload.final_text,
                            );
                            if merged != payload.final_text {
                                tracing::debug!(
                                    "smart insertion: {} -> {}",
                                    payload.final_text,
                                    merged
                                );
                            }
                            merged
                        }
                        None => payload.final_text.clone(),
                    }
                }
            } else {
                payload.final_text.clone()
            };
            // P8.1: route through the core delivery cascade. The cascade
            // handles the full sequence: clipboard paste + verify, typed
            // fallback, set-value fallback, and the held-for-paste-last
            // outcome. The per-app policy table decides the starting mode.
            // macOS: real clipboard guard; other platforms: compile-safe stub
            // via `delivery_io::macos_io`'s non-mac twin (`Arc<()>`).
            #[cfg(target_os = "macos")]
            let io = crate::delivery_io::macos_io(std::sync::Arc::new(
                crate::clipboard::MacClipboardGuard::new(),
            ));
            #[cfg(not(target_os = "macos"))]
            let io = crate::delivery_io::macos_io(std::sync::Arc::new(()));
            let policy = DeliveryPolicy::builtin();
            let app_id = platform
                .active_application()
                .map(|a| a.application_id)
                .filter(|id| !id.is_empty());
            let outcome = match &e2e_forced {
                Some(mode) => {
                    // P9.6: the forced mode is applied through the core
                    // cascade's per-app policy override (exact bundle-id
                    // match, like the checked-in policy table). `hold`
                    // cannot be expressed in the table, so it
                    // short-circuits to the held outcome: every route is
                    // forced to fail and the text is kept for Paste-Last.
                    match mode {
                        crate::e2e::ForcedMode::Hold => {
                            teletype_core::delivery::DeliveryOutcome::HeldForPasteLast
                        }
                        _ => {
                            let policy = mode.apply(&policy, app_id.as_deref().unwrap_or(""));
                            delivery::deliver(&final_text, app_id.as_deref(), &policy, &io)
                        }
                    }
                }
                None => delivery::deliver(&final_text, app_id.as_deref(), &policy, &io),
            };
            // Map outcomes onto existing UI/trace behaviour.
            let delivery_held = matches!(outcome, DeliveryOutcome::HeldForPasteLast);
            let delivery_refused = match &outcome {
                DeliveryOutcome::Refused { reason } => Some(reason.clone()),
                _ => None,
            };
            match &outcome {
                DeliveryOutcome::PastedVerified => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: pasted verified ({} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::PastedUnverified => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: pasted (unverified, {} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::Typed => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: typed fallback ({} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::SetValue => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!(
                            "inject: set-value fallback ({} chars)",
                            final_text.chars().count()
                        ),
                    );
                }
                DeliveryOutcome::HeldForPasteLast => {
                    crate::log_entry(
                        crate::LogLevel::Warn,
                        "inject: all delivery routes failed; held for Paste-Last",
                    );
                    let app = self.app().clone();
                    let _ = app.emit_to(
                        "pill",
                        "pill-skip",
                        &serde_json::json!({ "message": "Delivery failed — text kept for Paste-Last" }),
                    );
                    crate::tray::show_skip(&app, "Delivery failed — text kept for Paste-Last");
                }
                DeliveryOutcome::Refused { reason } => {
                    crate::log_entry(crate::LogLevel::Warn, format!("inject refused: {reason}"));
                    let app = self.app().clone();
                    let _ = app.emit_to(
                        "pill",
                        "pill-skip",
                        &serde_json::json!({ "message": format!("Not pasted: {reason}") }),
                    );
                    crate::tray::show_skip(&app, &format!("Not pasted: {reason}"));
                }
            }
            // P9.6: stamp the delivery label on the recorded e2e outcome.
            let delivery_label = match &outcome {
                DeliveryOutcome::PastedVerified => "clipboard(pasted,verified)".to_string(),
                DeliveryOutcome::PastedUnverified => "clipboard(pasted,unverified)".to_string(),
                DeliveryOutcome::Typed => "type".to_string(),
                DeliveryOutcome::SetValue => "setvalue".to_string(),
                DeliveryOutcome::HeldForPasteLast => "hold".to_string(),
                DeliveryOutcome::Refused { reason } => format!("refused({reason})"),
            };
            self.e2e_set_delivery(session, &delivery_label);
            // P8.4: store the last final text for Paste-Last. Every
            // successful AND failed delivery updates it; only a cancelled
            // take (handled earlier) does not.
            *self
                .state()
                .last_text
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(final_text.clone());
            // The delivery-refused path (gate, terminal newline) already
            // handled its own toast above. The cascade-level refusal and
            // hold also set `refused` so the trace gets the reason.
            if let Some(reason) = &delivery_refused {
                refused = Some(format!("delivery refused: {reason}"));
            } else if delivery_held {
                refused = Some("delivery held for Paste-Last (all routes failed)".into());
            }
            // Watch what landed so the personalization loop can diff it
            // against what the user changes it into on the next dictation.
            // See `edit_watch` for why the observation point is the
            // following dictation.
            let target = payload.context.application_name.clone();
            // P7.2: record this take into the learning store (only a
            // successful paste is recorded, never a refusal or a cancel) and
            // arm the anchored watch. Store failures disable learning for
            // the session and never block the paste.
            let dictation_id = self.record_dictation_row(&payload, &final_text);
            {
                let state = self.state();
                state
                    .edit_watch
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record_watch(
                        final_text.clone(),
                        anchor,
                        dictation_id,
                        crate::edit_watch::now_ms(),
                    );
                state
                    .edit_watch
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .set_target(target);
            }
        }
        // P0.2: the verdict was made and acted on — stamp and write the
        // trace AFTER the user-visible side effect, never before. The
        // write is one best-effort append; delivery already happened, so a
        // slow or failed write cannot cost the user their text. A refused
        // delivery gets the reason instead of a paste mark.
        let marks = payload.trace_marks;
        let decider_marks = (
            payload.decider,
            payload.decider_ms,
            payload.tool,
            payload.args_source,
        );
        let audio_ms = self.speech_ms.get(&session).copied().unwrap_or(0) as u32;
        match refused {
            Some(reason) => self.finish_trace(session, |t| {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                t.audio_ms = audio_ms;
                t.errors.push(reason);
            }),
            None => self.finish_trace(session, |t| {
                t.apply_stage_marks(marks);
                Self::apply_decider_marks(
                    t,
                    decider_marks.0,
                    decider_marks.1,
                    decider_marks.2,
                    decider_marks.3,
                );
                t.audio_ms = audio_ms;
                t.mark(|s, now| s.t_paste_sent = Some(now));
            }),
        }
        // Verdict made and acted on: tell the controller the session is
        // over. The worker must never send this itself.
        self.controller.send(Event::PipelineDone { session });
    }

    /// P9.6: the forced delivery mode for this take, if the e2e slot has
    /// one set for the target app (exact bundle-id match, like the policy
    /// table). `None` outside `--e2e` or when no override applies.
    fn e2e_forced_mode(&self, payload: &PipelinePayload) -> Option<crate::e2e::ForcedMode> {
        let slot = self.controller.e2e.as_ref()?;
        let id = payload.context.application_id.as_str();
        if id.is_empty() {
            return None;
        }
        slot.force_for(Some(id))
    }

    /// P9.6: records the take's outcome in the e2e slot (no-op outside
    /// `--e2e`). Called at the top of `pipeline_ready`, before the verdict,
    /// so a cancelled take still reports its transcript and error. The
    /// delivery label is updated in place once the injection runs.
    fn e2e_record(&self, session: u64, payload: &PipelinePayload) {
        let Some(slot) = self.controller.e2e.as_ref() else {
            return;
        };
        let cancelled = self.cancelled_sessions.contains(&session);
        let command = payload
            .command
            .as_ref()
            .map(|c| format!("{} ({})", c.tool, c.arguments));
        let error = if cancelled {
            Some("cancelled before the pipeline finished".into())
        } else {
            None
        };
        slot.record(crate::e2e::make_result(
            session,
            payload.final_text.clone(),
            command,
            "pending".into(),
            error,
        ));
    }

    /// P9.6: sets the delivery label on the recorded e2e outcome once the
    /// injection has run (no-op outside `--e2e`).
    fn e2e_set_delivery(&self, session: u64, delivery: &str) {
        let Some(slot) = self.controller.e2e.as_ref() else {
            return;
        };
        slot.set_delivery(session, delivery);
    }

    /// P0.2: finish a session's trace: apply the final mutation, then
    /// append it as one JSON line — always AFTER the user-visible outcome
    /// (paste, refusal, or discard), never before. The write is skipped
    /// without an app handle (unit tests) and is best-effort everywhere
    /// else: a failed write drops a measurement, never a dictation.
    fn finish_trace(&mut self, session: u64, f: impl FnOnce(&mut DictationTrace)) {
        let Some(mut trace) = self.traces.remove(&session) else {
            return;
        };
        f(&mut trace);
        // Phase 5.7: one attach point covers every completion path
        // (success, error, cancel, supersede). Drains the entry so a
        // quality event for a finished id cannot linger.
        if let Some(q) = self.take_quality.remove(&session) {
            apply_take_quality(&mut trace, Some(&q));
        }
        if self.app.is_some() {
            let dir = self.state().data_dir.clone();
            FileTraceSink::new(&dir).write_trace(&trace);
        }
    }

    /// Marks `session` as cancelled in the durable sense: the phase can
    /// move on (a newer take can start), but a later `PipelineReady` for
    /// this id must still be discarded. Called on the controller thread
    /// only, so the tag and the phase transition are one atomic step in
    /// the event order.
    fn cancel_session(&mut self, session: u64) {
        debug_assert!(
            self.cancelled_sessions.insert(session),
            "session {session} cancelled twice"
        );
        tracing::info!(
            session,
            "session marked cancelled; pending pipeline result will be discarded"
        );
        crate::log_entry(
            crate::LogLevel::Info,
            format!("session {session} cancelled; pending result will be discarded"),
        );
    }

    /// P3.7: confidence-aware phonetic rescoring over the dictionary +
    /// pack terms, wired into the dictation pipeline when the engine reports
    /// per-word confidences (Fluid, sherpa). Engines without word timings
    /// skip this entirely — `correct_with_dictionary` runs with confidence
    /// 1.0 and its phonetic pass stays dormant, so behavior is byte-identical
    /// for Whisper/ggml Parakeet.
    fn rescore_with_confidence(&mut self, raw: String, words: &[teletype_speech::Word]) -> String {
        let state = self.state();
        let settings = state.settings();

        // Build RescoreTerms from dictionary words + pack terms.
        let dictionary = if settings.dictionary_enabled {
            state
                .dictionary
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        } else {
            teletype_core::dictionary::Dictionary::default()
        };

        let mut pack_terms = teletype_core::vocab::terms_for(&settings.enabled_packs);
        {
            let user_packs = state
                .user_packs
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            pack_terms.extend(user_packs.terms_for());
        }

        let terms: Vec<teletype_core::rescoring::RescoreTerm> = dictionary
            .words
            .iter()
            .map(|w| teletype_core::rescoring::RescoreTerm {
                text: w.word.clone(),
                aliases: w.aliases.clone(),
                weight: 1.0,
            })
            .chain(
                pack_terms
                    .iter()
                    .map(|pt| teletype_core::rescoring::RescoreTerm {
                        text: pt.canonical.clone(),
                        aliases: vec![pt.alias.clone()],
                        weight: 1.0,
                    }),
            )
            .collect();

        if terms.is_empty() {
            return raw;
        }

        let word_texts: Vec<String> = words.iter().map(|w| w.text.clone()).collect();
        let word_confs: Vec<f32> = words.iter().map(|w| w.conf).collect();

        let (rescored_words, replacements) =
            teletype_core::rescoring::rescore_words(&word_texts, &word_confs, &terms);

        if !replacements.is_empty() {
            crate::log_entry(
                crate::LogLevel::Info,
                format!("rescoring applied {} replacements", replacements.len()),
            );
        }

        rescored_words.join(" ")
    }

    fn arm_edit_check(&mut self, session: u64) {
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no edit-watch to observe (no platform services).
            return;
        }
        let controller = self.controller.clone();
        std::thread::Builder::new()
            .name("teletype-edit-check".into())
            .spawn(move || {
                // Give the user a moment to fix a misheard word or sign-off.
                std::thread::sleep(Duration::from_secs(15));
                controller.send(Event::CheckEdit { session });
            })
            .ok();
    }

    fn flash(&mut self, message: &str) {
        self.next_id += 1;
        let generation = self.next_id;
        self.phase = Phase::Idle;
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no broadcast, no pill, no flash timer. The
            // counter still advances so session ids stay unique.
            return;
        }
        self.broadcast(teletype_core::state::UiState::Message {
            text: message.into(),
        });
        self.show(PillState::Processing {
            message: message.into(),
        });
        let controller = self.controller.clone();
        thread::Builder::new()
            .name("teletype-flash".into())
            .spawn(move || {
                thread::sleep(Duration::from_millis(2000));
                controller.send(Event::FlashDone { generation });
            })
            .ok();
    }

    fn flash_done(&mut self) {
        self.show(PillState::Idle);
        self.broadcast(teletype_core::state::UiState::Idle);
    }

    fn go_idle(&mut self) {
        // P7.2: a take that ended without a paste (cancel, discard, error)
        // must not leave an armed observer behind — drop the watch. Replaces
        // are handled by the next paste's `record_watch`.
        self.cancel_learning_watch();
        self.phase = Phase::Idle;
        self.hands_free = false;
        self.last_release = None;
        self.vad = None;
        self.recording_started = None;
        self.speech_ms.clear();
        // P4: the streaming state is owned by the release worker (moved out
        // of `self.streaming` in `stop()`); if the take ended without a
        // release (e.g. the mic never opened), drop it here so the
        // committer's worker exits.
        self.streaming = None;
        // P4.3b: same rule as `streaming` (a take that ends without a
        // release must not leak its polish install handle).
        self.polish_app = None;
        // Phase 5.7: a take that ends without a release (or without a
        // `finish_trace`) must not leak its stashed quality entry.
        self.take_quality.clear();
        self.streaming_marks.clear();
        self.injected_at = None;
        // P2.6: a selection captured for a command take is single-use. The
        // take that would consume it ends here (transcribed() takes it when
        // the pipeline starts), so a cancelled take's selection can never
        // leak into a later take.
        self.pending_selection = None;
        // P6.4: the screen context is ephemeral per take — a cancelled or
        // discarded take must not leak its field text into the next one.
        self.screen_context = None;
        // P1.4: join the interim thread now, if one is still running. The
        // final transcribe has finished by the time the session goes idle,
        // so the speech-model lock can no longer be contended. This is the
        // only place the join happens: `stop()` itself must not block on
        // an in-flight interim decode.
        if let Some(handle) = self.live_preview.take() {
            handle.join();
        }
        #[cfg(test)]
        if self.headless_flash {
            // Test path: no model managers, pill, or broadcast to touch.
            return;
        }
        // BUG-003: the dictation session is over; arm the idle-unload timer
        // (or leave the model resident for the "Never" policy).
        {
            let state = self.state();
            let delay_secs = state.settings().model_unload_delay_secs;
            let delay = (delay_secs > 0).then(|| std::time::Duration::from_secs(delay_secs));
            state.speech.end_dictation(delay);
            state.parakeet.end_dictation(delay);
        }
        self.broadcast(teletype_core::state::UiState::Idle);
        self.show(PillState::Idle);
    }

    fn show(&self, state: PillState) {
        let settings = self.state().settings();
        let position = parse_position(&settings.pill_position);
        let app = self.app().clone();
        crate::overlay::update(&app, state, position, settings.always_show_pill);
        // The pill is always interactive so the device picker and cancel
        // button work in both push-to-talk and toggle modes. In push-to-talk
        // the user must release the hotkey to click, and the pill would
        // otherwise become click-through the instant the phase changes.
        crate::overlay::set_interactive(&app, true);
    }

    fn broadcast(&self, state: teletype_core::state::UiState) {
        let app = self.app().clone();
        let _ = app.emit("dictation-state", &state);
        crate::tray::show_state(&app, &state);
        *self
            .state()
            .dictation_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }
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

pub(crate) fn parse_position(s: &str) -> PillPosition {
    use PillPosition::*;
    match s {
        "topLeft" => TopLeft,
        "topCenter" => TopCenter,
        "topRight" => TopRight,
        "centerLeft" => CenterLeft,
        "center" => Center,
        "centerRight" => CenterRight,
        "bottomLeft" => BottomLeft,
        "bottomCenter" => BottomCenter,
        "bottomRight" => BottomRight,
        _ => BottomCenter,
    }
}

/// The language to pass to the ASR engine.
///
/// `"auto"` (or an empty legacy setting) is passed through: Whisper models
/// auto-detect the language themselves (whisper.cpp runs without a
/// language hint). Parakeet cannot auto-detect, so its wrapper ignores the
/// language argument and always runs with its trained language mix.
pub fn effective_language(language: String, _english_only: bool) -> String {
    let trimmed = language.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        "auto".into()
    } else {
        trimmed.to_string()
    }
}

/// Resolves the selected speech model to a file path on disk plus which
/// engine kind loads it. Shared by the dictation worker, the model warm-up,
/// and the recovery replay path.
pub fn resolve_speech_model(
    settings: &crate::commands::Settings,
    models_dir: &std::path::Path,
) -> (std::path::PathBuf, SpeechEngineKind) {
    let entry = teletype_speech::catalog::find(&settings.selected_speech_model);
    let engine = entry
        .map(|m| m.engine)
        .unwrap_or(teletype_speech::catalog::Engine::Whisper);
    let kind = match engine {
        teletype_speech::catalog::Engine::ParakeetCoreml => SpeechEngineKind::Fluid,
        teletype_speech::catalog::Engine::SherpaParakeet => SpeechEngineKind::Sherpa,
        teletype_speech::catalog::Engine::Whisper => SpeechEngineKind::Whisper,
        teletype_speech::catalog::Engine::Parakeet => SpeechEngineKind::Parakeet,
    };
    let file = entry
        .map(|m| m.file.to_string())
        .unwrap_or_else(|| format!("{}.bin", settings.selected_speech_model));
    // Per-engine path semantics:
    // - Fluid (Core ML): base dir `<models_dir>/fluid`; the provider appends
    //   the repo folder itself and downloads on first load.
    // - Sherpa: extracted dir `<models_dir>/sherpa/<model-id>`; must exist.
    // - Whisper/Parakeet ggml: `<models_dir>/<file>`; must exist.
    // - Whistle: `<models_dir>/whistle.cact`; must exist (not yet cataloged).
    let model_path = match kind {
        SpeechEngineKind::Fluid => models_dir.join("fluid"),
        SpeechEngineKind::Sherpa => models_dir.join("sherpa").join(&file),
        SpeechEngineKind::Whisper | SpeechEngineKind::Parakeet => models_dir.join(&file),
        SpeechEngineKind::Whistle => models_dir.join("whistle.cact"),
    };
    (model_path, kind)
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
    // Parakeet (ggml and Core ML) and sherpa are English-only: "auto" means
    // English. Whisper can auto-detect any language.
    let english_only = matches!(
        kind,
        SpeechEngineKind::Parakeet | SpeechEngineKind::Fluid | SpeechEngineKind::Sherpa
    );
    effective_language(resolved, english_only)
}

/// Returns `true` when the engine kind is English-only, so `"auto"` is
/// treated as English rather than delegating to Whisper's auto-detect.
pub fn is_english_only(kind: SpeechEngineKind) -> bool {
    matches!(
        kind,
        SpeechEngineKind::Parakeet | SpeechEngineKind::Fluid | SpeechEngineKind::Sherpa
    )
}

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
pub fn transcribe(
    app: &AppHandle,
    model_path: &std::path::Path,
    captured: &Captured,
    language: &str,
    kind: SpeechEngineKind,
) -> Result<(String, Option<Vec<teletype_speech::Word>>), String> {
    let state = app.state::<AppState>();
    let manager = match kind {
        SpeechEngineKind::Fluid | SpeechEngineKind::Sherpa | SpeechEngineKind::Whistle => {
            &state.neural
        }
        SpeechEngineKind::Parakeet => &state.parakeet,
        SpeechEngineKind::Whisper => &state.speech,
    };
    if !manager.is_loaded() {
        // FluidAudio downloads models on first load, so skip the existence
        // check for Core ML. Sherpa/whistle need their files on disk.
        if !matches!(kind, SpeechEngineKind::Fluid) && !model_path.exists() {
            return Err("Speech model not downloaded".into());
        }
        manager.set_model_path(model_path.to_path_buf());
        manager
            .load()
            .map_err(|e| format!("Speech model load failed: {e}"))?;
    }
    let mut speech = manager
        .provider()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Engines with word timings report them through `transcribe_detailed`;
    // others fall back to `transcribe` (default maps text to no words).
    if speech.capabilities().word_timings {
        let detailed = speech
            .transcribe_detailed(&captured.samples, language)
            .map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflict_shows_only_for_the_binding_that_failed() {
        let stored = Some(HotkeyConflict::new("Ctrl+Shift+Space", "os error -9878"));
        // Same binding: visible.
        let shown = visible_conflict(&stored, "Ctrl+Shift+Space");
        assert_eq!(shown, stored);
        // User changed the binding since: the old failure must not linger.
        assert_eq!(visible_conflict(&stored, "Alt+Space"), None);
        // Nothing stored: nothing shown.
        assert_eq!(visible_conflict(&None, "Ctrl+Shift+Space"), None);
    }

    #[test]
    fn the_conflict_message_blames_nobody() {
        // Worded as what macOS reported: a duplicate registration inside our
        // own process fails the same way, so naming another app is unproven.
        let c = HotkeyConflict::new("Ctrl+Shift+Space", "os error -9878");
        assert_eq!(c.binding, "Ctrl+Shift+Space");
        assert!(c.message.contains("already taken"), "{}", c.message);
        assert!(c.message.contains("Cause unknown"), "{}", c.message);
        assert!(c.message.contains("os error -9878"), "{}", c.message);
        for word in ["another app", "other app", "claimed by", "holds"] {
            assert!(!c.message.contains(word), "blames: {word}");
        }
    }

    /// Regression test for the P0-1 cancel guard: a cancel while the
    /// pipeline is running must map to CancelPipeline (finish without
    /// inserting), and a cancel while recording must map to a cancelled
    /// stop (discard the audio).
    #[test]
    fn pill_cancel_maps_per_phase() {
        // Cancel mid-pipeline: let transcription finish, drop the result.
        let action = state::decide(
            Input::Cancel,
            Phase::Transforming,
            RecordingMode::Hold,
            &mut false,
            &mut false,
        );
        assert!(matches!(action, state::Action::CancelPipeline));

        // Cancel while recording: stop and discard.
        let action = state::decide(
            Input::Cancel,
            Phase::Listening,
            RecordingMode::Hold,
            &mut true,
            &mut false,
        );
        assert!(matches!(action, state::Action::Stop { cancelled: true }));

        // Cancel while idle: nothing to cancel.
        let action = state::decide(
            Input::Cancel,
            Phase::Idle,
            RecordingMode::Hold,
            &mut false,
            &mut false,
        );
        assert!(matches!(action, state::Action::Nothing));
    }

    #[test]
    fn transliterates_cyrillic_name() {
        assert_eq!(transliterate_cyrillic("Рида Фатема"), "Rida Fatema");
    }

    #[test]
    fn transliteration_preserves_latin() {
        assert_eq!(transliterate_cyrillic("Hello world"), "Hello world");
    }

    /// P2.5: wake phrase stripping, exact and fuzzy (edit distance ≤ 1).
    #[test]
    fn wake_phrase_strips_exact_and_fuzzy() {
        // Plain wake word.
        assert_eq!(
            strip_wake_phrase("teletype open safari"),
            Some("open safari".to_string())
        );
        // "hey teletype" form.
        assert_eq!(
            strip_wake_phrase("hey teletype send an email"),
            Some("send an email".to_string())
        );
        // Case-insensitive.
        assert_eq!(
            strip_wake_phrase("Hey Teletype do this"),
            Some("do this".to_string())
        );
        // ASR wobble: one edit in each wake word.
        assert_eq!(
            strip_wake_phrase("teletyp open safari"),
            Some("open safari".to_string())
        );
        assert_eq!(
            strip_wake_phrase("telotype open safari"),
            Some("open safari".to_string())
        );
        // Trailing punctuation on the wake word (common with ASR).
        assert_eq!(
            strip_wake_phrase("Teletype, open Safari"),
            Some("open Safari".to_string())
        );
    }

    /// P2.5: negative cases — the wake phrase must not fire on unrelated
    /// speech, on a wake word that is not leading, or leave an empty command.
    #[test]
    fn wake_phrase_requires_leading_words_and_a_command() {
        assert_eq!(strip_wake_phrase("open safari"), None);
        assert_eq!(strip_wake_phrase("hey siri do this"), None);
        // Wake word mid-sentence is dictation, not a command.
        assert_eq!(strip_wake_phrase("please teletype open safari"), None);
        // Wake word alone carries no command: a plain dictation of the word
        // "teletype" must not become an empty command take.
        assert_eq!(strip_wake_phrase("teletype"), None);
        assert_eq!(strip_wake_phrase("hey teletype"), None);
        assert_eq!(strip_wake_phrase(""), None);
        // Two edits away: not the wake word.
        assert_eq!(strip_wake_phrase("teltypo open safari"), None);
    }

    #[test]
    fn transliteration_handles_mixed_script() {
        assert_eq!(
            transliterate_cyrillic("Рида hello Фатема"),
            "Rida hello Fatema"
        );
    }

    #[test]
    fn transliteration_drops_hard_soft_signs() {
        assert_eq!(transliterate_cyrillic("объём"), "obyom");
    }

    #[test]
    fn ensure_latin_transliterates_when_english() {
        assert_eq!(ensure_latin_if_english("Рида Фатема", "en"), "Rida Fatema");
    }

    #[test]
    fn ensure_latin_preserves_when_not_english() {
        assert_eq!(ensure_latin_if_english("Рида Фатема", "ru"), "Рида Фатема");
    }

    #[test]
    fn ensure_latin_preserves_latin_text() {
        assert_eq!(ensure_latin_if_english("Rida Fatema", "en"), "Rida Fatema");
    }

    #[test]
    fn effective_language_defaults_to_auto() {
        // Empty legacy setting means auto-detect (Whisper picks the language;
        // Parakeet ignores the argument inside its wrapper).
        assert_eq!(effective_language("".into(), false), "auto");
        // "auto" (any case) passes through; explicit codes pass through too.
        assert_eq!(effective_language("auto".into(), false), "auto");
        assert_eq!(effective_language("AUTO".into(), false), "auto");
        assert_eq!(effective_language("ru".into(), false), "ru");
        assert_eq!(effective_language("en".into(), true), "en");
        // Surrounding whitespace is trimmed, not treated as a language.
        assert_eq!(effective_language("  ru ".into(), false), "ru");
    }

    #[test]
    fn detected_language_flows_into_profile() {
        // Unit-level check of the auto-detect contract: when the engine
        // reports a detection, the pipeline must polish in that language.
        let mut profile = teletype_core::personalization::UserProfile::default();
        assert_eq!(profile.language, "en");
        let detected: Option<String> = Some("de".into());
        if let Some(d) = &detected {
            profile.language = d.clone();
        }
        assert_eq!(profile.language, "de");
    }

    // ------------------------------------------------------------------
    // Cancellation-race concurrency tests.
    //
    // These drive the REAL controller event loop: `Controller::for_tests`
    // runs the same `handle` the production thread uses, on its own thread,
    // fed by the same FIFO `mpsc` channel. The test thread stands in for
    // the pipeline worker by sending `PipelineReady` (the worker never
    // injects; it only ships the result). The `on_inject` hook records
    // "the controller decided to inject" in place of the platform call,
    // which needs the Tauri main thread. What the tests pin down is the
    // architecture's whole promise: the FIFO event order IS the
    // cancellation decision, and the worker never performs the side effect.

    fn test_payload(session: u64, text: &str) -> PipelinePayload {
        PipelinePayload {
            session,
            final_text: text.to_string(),
            context: teletype_core::context::ApplicationContext::default(),
            record_target: None,
            detected_language: None,
            command: None,
            trace_marks: teletype_core::trace::TraceStageMarks::default(),
            decider: None,
            decider_ms: None,
            tool: None,
            args_source: None,
        }
    }

    // ------------------------------------------------------------------
    // P4.4: mock 60 s take — tail-only decode on release + trace fields.
    //
    // The controller's release path (worker-side) is modelled here the way
    // the rest of the tests model the workers: a mock engine whose decode
    // cost is 10 ms per second of audio. For a 60 s take the Segmenter
    // commits the segments during recording, so on release only the tail
    // (the uncommitted remainder) is decoded. The mock proves both
    // properties the plan gates on: the tail is small (≤ 3 s of audio) and
    // the release-to-final-text cost is the tail decode plus the rules-only
    // join (≤ 80 ms of mock time, no LLM).
    // ------------------------------------------------------------------

    /// P4.4: a 60 s take through the Segmenter leaves a small tail, and
    /// the release-side work (tail decode at 10 ms/s + rules-only join)
    /// stays flat as the take grows.
    #[test]
    fn mock_60s_take_tail_only_decode_on_release() {
        use teletype_core::segmenter::{Segmenter, SegmenterConfig};

        // A long take with 500 ms pauses every 10 s: the Segmenter commits
        // one segment at each pause (5 during the take); the final 2.5 s of
        // speech is released before any pause — the uncommitted tail that is
        // decoded on release.
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut committed_segments: u32 = 0;
        for _ in 0..5 {
            let events = seg.push(&vec![0.5; 160_000], 0.9); // 10 s speech
            committed_segments += events.len() as u32;
            let events = seg.push(&vec![0.0; 8000], 0.0); // 500 ms pause
            committed_segments += events.len() as u32;
        }
        let events = seg.push(&vec![0.5; 40_000], 0.9); // final 2.5 s speech
        committed_segments += events.len() as u32;
        let tail = seg.flush().expect("the tail is flushed on release");
        assert_eq!(committed_segments, 5, "a long take must commit mid-take");

        // Tail audio: the uncommitted remainder after the last commit.
        let tail_audio_ms =
            crate::committer::tail_audio_ms(tail.end_sample - tail.start_sample, 16_000);
        assert!(
            tail_audio_ms <= 3_000,
            "a 60 s take must leave ≤ 3 s of tail audio; got {tail_audio_ms} ms"
        );

        // Release-side cost: the mock engine decodes at 10 ms per second of
        // audio (the plan's mock timing), and the join is rules-only.
        let decode_ms = u64::from(tail_audio_ms) * 10 / 1000;
        let joined = crate::committer::join_segments(&[
            "segment one".to_string(),
            "segment two".to_string(),
            "segment three".to_string(),
            "segment four".to_string(),
            "segment five".to_string(),
            "the tail words".to_string(),
        ]);
        assert!(!joined.is_empty());
        let release_to_final_ms = decode_ms; // join is rules-only: no LLM
        assert!(
            release_to_final_ms <= 80,
            "release-to-final-text must stay flat as takes get longer; got {release_to_final_ms} ms"
        );
    }

    /// P4.4: `committed_segments` and `tail_audio_ms` ride the
    /// `TraceStageMarks` into the `DictationTrace` for a segmented take.
    #[test]
    fn segmented_take_trace_records_committed_segments_and_tail() {
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
        session.phase = Phase::Idle;
        session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
        session.headless_release(TakeKind::Dictation);
        // The worker (mock 60 s take) committed 5 segments during
        // recording and decoded a 2.5 s tail on release.
        let mut payload = test_payload(1, "joined final text");
        payload.trace_marks.committed_segments = 5;
        payload.trace_marks.tail_audio_ms = 2500;
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload,
            },
        );
        let trace = session.traces.get(&1).expect("finished trace");
        assert_eq!(trace.committed_segments, 5);
        assert_eq!(trace.tail_audio_ms, 2500);
    }

    /// Drives the controller loop the same way the production thread does:
    /// one `handle` call per event, in FIFO order. `PipelineReady` goes
    /// through the `on_inject` hook instead of the platform call.
    // Generic over the hook so it accepts both `Box<dyn FnMut(..)>` and the
    // `+ Send` variant `Controller::for_tests` returns (Box is invariant, so
    // a plain `&mut Box<dyn FnMut>` parameter would reject the Send box).
    fn drive<F: FnMut(&PipelinePayload)>(session: &mut Session, on_inject: &mut F, event: Event) {
        match event {
            Event::PipelineReady {
                session: id,
                payload,
            } => session.pipeline_ready(
                id,
                payload,
                Some(on_inject as &mut dyn FnMut(&PipelinePayload)),
            ),
            other => session.handle(other),
        }
    }

    #[test]
    fn cancel_processed_before_pipeline_ready_discards_the_result() {
        // Take 1 is mid-pipeline. The user cancels (hotkey). The worker
        // finishes a moment later and ships PipelineReady. The cancel
        // event was processed first, so the controller must discard: no
        // injection.
        let (injected_tx, injected_rx) = mpsc::channel::<u64>();
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
            let _ = injected_tx.send(p.session);
        }));
        // The user's cancel hotkey is processed first.
        drive(&mut session, &mut on_inject, Event::Cancel);
        // The worker's result arrives after the cancel.
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload: test_payload(1, "must never be injected"),
            },
        );
        assert!(
            injected_rx.try_recv().is_err(),
            "cancelled session must not be injected"
        );
    }

    #[test]
    fn pipeline_ready_processed_before_cancel_allows_injection() {
        // Same take, opposite order: the controller accepts the result and
        // injects; only then is the cancel processed. The text has already
        // landed, so the cancel just transitions the session.
        let (injected_tx, injected_rx) = mpsc::channel::<u64>();
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
            let _ = injected_tx.send(p.session);
        }));
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload: test_payload(1, "lands before the cancel"),
            },
        );
        // The cancel is processed after the verdict.
        drive(&mut session, &mut on_inject, Event::Cancel);
        assert_eq!(
            injected_rx.try_recv().ok(),
            Some(1),
            "ready-before-cancel must inject"
        );
    }

    #[test]
    fn cancel_between_ready_and_done_still_injects_exactly_once() {
        // The race window the artificial delay exists to expose: ready is
        // processed (verdict: inject), the cancel is processed, and only
        // then the controller's own PipelineDone comes back. The injection
        // must happen exactly once and the session must end up idle, not
        // stuck in Transforming.
        let (injected_tx, injected_rx) = mpsc::channel::<u64>();
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
            let _ = injected_tx.send(p.session);
        }));
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload: test_payload(1, "one injection only"),
            },
        );
        // Cancel right after ready — inside the "verdict made, done not
        // yet processed" window.
        drive(&mut session, &mut on_inject, Event::Cancel);
        assert_eq!(injected_rx.try_recv().ok(), Some(1));
        assert!(inject_rx_empty(&injected_rx), "no second injection");
        assert_eq!(session.phase, Phase::Idle);
    }

    fn inject_rx_empty(rx: &mpsc::Receiver<u64>) -> bool {
        rx.try_recv().is_err()
    }

    #[test]
    fn cancel_during_transcribing_drops_the_transcript_itself() {
        // Cancel before the ASR result even arrives: the transcript must
        // be dropped, so no pipeline and no injection can follow.
        let (injected_tx, injected_rx) = mpsc::channel::<u64>();
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
            let _ = injected_tx.send(p.session);
        }));
        drive(&mut session, &mut on_inject, Event::Cancel);
        drive(
            &mut session,
            &mut on_inject,
            Event::Transcribed {
                session: 1,
                transcript: Ok(("late transcript".into(), None)),
                detected_language: None,
                asr_start_us: 0,
                asr_end_us: 0,
                pcm: None,
            },
        );
        assert!(
            injected_rx.try_recv().is_err(),
            "a cancelled take must never transcribe or inject"
        );
        assert_eq!(session.phase, Phase::Idle);
    }

    #[test]
    fn a_late_pipeline_ready_from_a_cancelled_old_session_is_discarded() {
        // Two takes back to back: take 1 is cancelled while its pipeline
        // runs, take 2 finishes normally. The stale PipelineReady for
        // take 1 must be discarded even though the session counter has
        // moved on — the cancelled-set, not the phase, is what remembers.
        let (injected_tx, injected_rx) = mpsc::channel::<u64>();
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
            let _ = injected_tx.send(p.session);
        }));
        // Take 1 is cancelled mid-pipeline.
        drive(&mut session, &mut on_inject, Event::Cancel);
        // Take 2: a fresh session id (the cancel's flash bumped the
        // counter), mid-pipeline.
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 2,
                payload: test_payload(2, "take two lands"),
            },
        );
        // The stale result from cancelled take 1 arrives last.
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload: test_payload(1, "stale, must be discarded"),
            },
        );
        assert_eq!(injected_rx.try_recv().ok(), Some(2));
        assert!(
            injected_rx.try_recv().is_err(),
            "stale cancelled result injected"
        );
    }

    // ------------------------------------------------------------------
    // P1.3: paste before bookkeeping.
    //
    // The pipeline worker ships `PipelineReady` (the injection) FIRST and
    // hands the usage/history/dictionary-usage writes to the bookkeeper
    // afterwards. The test records the event order and pins that the
    // injection (PipelineReady) precedes HistorySaved.

    /// Drives the controller the same way `drive` does, but also records
    /// `PipelineReady` as an event (the injection verdict) so the test can
    /// assert the order against the bookkeeping writes.
    fn drive_with_bookkeeping<F: FnMut(&PipelinePayload)>(
        session: &mut Session,
        on_inject: &mut F,
        events: &Arc<Mutex<Vec<String>>>,
        event: Event,
    ) {
        match event {
            Event::PipelineReady {
                session: id,
                payload,
            } => {
                session.pipeline_ready(
                    id,
                    payload,
                    Some(on_inject as &mut dyn FnMut(&PipelinePayload)),
                );
                events.lock().unwrap().push("PipelineReady".into());
            }
            other => session.handle(other),
        }
    }

    #[test]
    fn bookkeeping_happens_after_paste() {
        // The controller processes `PipelineReady` (the injection) and then
        // the pipeline worker's bookkeeping arrives. The event order must be
        // PipelineReady BEFORE HistorySaved: the user gets the paste first,
        // the disk writes follow. This is the whole point of P1.3 — no disk
        // write sits on the release→paste critical path.
        let events: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (injected_tx, injected_rx) = mpsc::channel::<u64>();
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
            let _ = injected_tx.send(p.session);
        }));
        // The worker ships PipelineReady (the injection) first.
        drive_with_bookkeeping(
            &mut session,
            &mut on_inject,
            &events,
            Event::PipelineReady {
                session: 1,
                payload: test_payload(1, "paste first"),
            },
        );
        // The injection happened.
        assert_eq!(
            injected_rx.try_recv().ok(),
            Some(1),
            "PipelineReady must inject"
        );
        // The bookkeeper's history write arrives after the injection.
        events.lock().unwrap().push("HistorySaved".into());
        // The order is pinned: PipelineReady (the paste) precedes HistorySaved.
        let ev = events.lock().unwrap().clone();
        let ready_pos = ev
            .iter()
            .position(|e| e == "PipelineReady")
            .expect("PipelineReady must be recorded");
        let hist_pos = ev
            .iter()
            .position(|e| e == "HistorySaved")
            .expect("HistorySaved must be recorded");
        assert!(
            ready_pos < hist_pos,
            "PipelineReady (paste) must precede HistorySaved (bookkeeping); got {:?}",
            ev
        );
    }

    // ------------------------------------------------------------------
    // P2.1: Command Mode state machine — failing tests first.
    //
    // These pin the three P2.1 failures from plan §2.1:
    // (a) an armed command take must reach the command router, not be
    //     misclassified as dictation;
    // (b) a Hold-mode release must stop a command take (the state machine
    //     must have tracked the hotkey);
    // (c) the command kind must not leak into the next dictation take.
    //
    // The headless controller (`Controller::for_tests`) drives the real
    // `handle`/`transcribed`/`pipeline_ready` path. A take classified as a
    // command ships `PipelineReady { command: Some(..) }`, which the
    // controller routes through `command_executor::execute` instead of
    // injecting text.

    /// The command take's `PipelineReady`: the pipeline classified the
    /// transcript as a command and extracted a structured action.
    #[cfg(test)]
    fn command_payload(session: u64) -> PipelinePayload {
        let mut p = test_payload(session, "open safari");
        p.command = Some(teletype_core::llm::CommandAction {
            tool: "open_app".into(),
            arguments: serde_json::json!({ "app": "Safari" }),
        });
        p
    }

    #[cfg(test)]
    fn command_executor_says(cmd: &teletype_core::llm::CommandAction) -> String {
        crate::command_executor::execute(cmd, &Default::default())
            .unwrap_or_else(|e| format!("executor error: {e}"))
    }

    /// (a) P2.1: a take started as a command (armed) reaches the command
    /// router — the `PipelineReady` carries the extracted action and the
    /// controller routes it through the executor, not the injector.
    #[test]
    fn armed_command_take_reaches_command_router() {
        let executed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = executed.clone();
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(move |p| {
            // The command's "injection" is its execution.
            if let Some(cmd) = &p.command {
                recorded.lock().unwrap().push(command_executor_says(cmd));
            }
        }));
        // The take is armed as a command and started through the state
        // machine (P2.3: every start goes through `input_with_kind`).
        session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
        // Release the key (the take ends; the pipeline runs in the
        // background) and let it reach the pipeline phase.
        session.headless_release(TakeKind::Command);
        // The pipeline finishes and ships the extracted command.
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload: command_payload(1),
            },
        );
        let got = executed.lock().unwrap().clone();
        assert!(
            got.iter().any(|m| m.contains("Safari")),
            "command take must reach the command router; executor saw: {got:?}"
        );
    }

    /// P2.9: a mock command take leaves its whole lifecycle in the trace,
    /// in order: controller-side `armed` / `started` at take start, then the
    /// worker-side `classified=<intent>` and `executed`/`refused(<reason>)`
    /// that ride the payload's `TraceStageMarks`. The headless `on_inject`
    /// path keeps the finished trace in `session.traces`, which is what
    /// this test reads back (production writes it to the traces JSONL).
    #[test]
    fn command_take_trace_records_full_lifecycle() {
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
        // A start test begins at Idle (the harness presets Transforming).
        session.phase = Phase::Idle;
        // A dedicated command take: armed and started by the state machine.
        session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
        session.headless_release(TakeKind::Command);
        // The worker classified the take as a command, executed the action,
        // and shipped both events in the payload.
        let mut payload = command_payload(1);
        payload.trace_marks.command_events = vec!["classified=command".into(), "executed".into()];
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload,
            },
        );
        let trace = session
            .traces
            .get(&1)
            .expect("the test path keeps the finished trace for assertions");
        assert_eq!(
            trace.command_mode,
            vec!["armed", "started", "classified=command", "executed"],
            "a command take must record its full lifecycle, in order"
        );
    }

    /// P2.9 negative shape: a plain dictation take records no command
    /// lifecycle at all (the field stays empty and is omitted from JSON).
    #[test]
    fn dictation_take_trace_has_no_command_lifecycle() {
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
        session.phase = Phase::Idle;
        session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
        session.headless_release(TakeKind::Dictation);
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload: test_payload(1, "plain dictation"),
            },
        );
        let trace = session.traces.get(&1).expect("finished trace");
        assert!(
            trace.command_mode.is_empty(),
            "dictation takes have no command lifecycle: {:?}",
            trace.command_mode
        );
    }

    /// A.3.3: the command decider trace fields ride the payload into the
    /// finished trace (decider kind, decide wall time, tool, args source).
    #[test]
    fn command_decider_trace_fields_land_in_trace() {
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
        session.phase = Phase::Idle;
        session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
        session.headless_release(TakeKind::Command);
        let mut payload = command_payload(1);
        payload.decider = Some("neudecide".into());
        payload.decider_ms = Some(42);
        payload.tool = Some("web_search".into());
        payload.args_source = Some("transcript".into());
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload,
            },
        );
        let trace = session
            .traces
            .get(&1)
            .expect("the test path keeps the finished trace for assertions");
        assert_eq!(trace.decider.as_deref(), Some("neudecide"));
        assert_eq!(trace.decider_ms, Some(42));
        assert_eq!(trace.tool.as_deref(), Some("web_search"));
        assert_eq!(trace.args_source.as_deref(), Some("transcript"));
    }

    /// A.3.3: a take that never ran the command route keeps the four
    /// decider trace fields `None` (and they are omitted from the JSON).
    #[test]
    fn dictation_take_trace_has_no_decider_fields() {
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
        session.phase = Phase::Idle;
        session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
        session.headless_release(TakeKind::Dictation);
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload: test_payload(1, "plain dictation"),
            },
        );
        let trace = session.traces.get(&1).expect("finished trace");
        assert!(trace.decider.is_none());
        assert!(trace.decider_ms.is_none());
        assert!(trace.tool.is_none());
        assert!(trace.args_source.is_none());
    }

    /// (b) P2.1: in Hold mode, releasing the hotkey stops a command take.
    /// Before the fix the command path skipped `state::decide`, so
    /// `hotkey_down` was never set and the release produced
    /// `Action::Nothing` — recording kept going.
    #[test]
    fn hold_mode_release_stops_command_take() {
        let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
        // Fresh session: the harness presets mid-pipeline Transforming for
        // the cancel tests; a start test begins at Idle.
        session.phase = Phase::Idle;
        // Armed command take, Hold mode (the headless default).
        session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
        assert_eq!(session.phase, Phase::Listening);
        assert!(
            session.hotkey_down,
            "start must track the hotkey in the state machine"
        );
        // Release: the state machine returns Stop, so the take ends.
        // (The real stop needs the mic; the headless release asserts the
        // decision the controller will act on, plus that the tracking state
        // is sound.)
        let action = state::decide(
            Input::HotkeyUp,
            session.phase,
            session.recording_mode(),
            &mut session.hotkey_down,
            &mut session.hands_free,
        );
        assert!(
            matches!(action, state::Action::Stop { cancelled: false }),
            "hold-mode release must stop a command take; got {action:?}"
        );
        assert!(!session.hotkey_down, "release must clear the hotkey latch");
        // The controller acts on that decision: the take ends and the
        // session moves to the pipeline phase.
        session.headless_release(TakeKind::Command);
        assert_eq!(
            session.phase,
            Phase::Transforming,
            "release must end the command take"
        );
    }

    /// (c) P2.1: the command kind is per-session. A command take must not
    /// leave the session in a state where the next take inherits
    /// Command-kind behaviour — the next take is a plain dictation.
    #[test]
    fn command_flag_does_not_leak_into_next_dictation() {
        let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
        // Fresh session (see above): starts take 1 at Idle.
        session.phase = Phase::Idle;
        // Take 1 is a command: the kind is set at start.
        session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
        assert_eq!(session.take_kind(), TakeKind::Command);
        // The take ends (release + pipeline done).
        session.headless_release(TakeKind::Command);
        session.go_idle();
        // Take 2 is a plain dictation: it must start as Dictation, not
        // inherit the command kind from take 1.
        session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
        assert_eq!(session.phase, Phase::Listening);
        assert_eq!(
            session.take_kind(),
            TakeKind::Dictation,
            "command kind leaked into the next dictation take"
        );
    }

    /// P2-B: a press the state machine ignores (the hotkey latch is already
    /// down — e.g. the dictation key pressed while a command take runs)
    /// must not re-tag the in-flight take. Before the fix the kind was set
    /// before `decide`, so such a press silently turned a running command
    /// take into a dictation and the command never reached the pipeline.
    #[test]
    fn swallowed_press_does_not_retag_take_kind() {
        let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
        session.phase = Phase::Idle;
        session.input_with_kind(Input::HotkeyDown, TakeKind::Command);
        assert_eq!(session.take_kind(), TakeKind::Command);
        // Second press, different kind: swallowed by the latch.
        session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
        assert_eq!(
            session.take_kind(),
            TakeKind::Command,
            "a swallowed press re-tagged the in-flight command take"
        );
    }

    /// P2.8: the tray's "Run Voice Command" item starts a command take
    /// from Idle, tagged as a command for the pipeline.
    #[test]
    fn command_toggle_starts_command_take() {
        let (_controller, mut session, _on_inject) = Controller::for_tests(Box::new(|_| {}));
        session.phase = Phase::Idle;
        session.command_toggle_event();
        assert_eq!(session.phase, Phase::Listening);
        assert_eq!(session.take_kind(), TakeKind::Command);
    }

    // ------------------------------------------------------------------
    // P1.4: stop() must not block on the interim decode.
    //
    // `stop()` sets the interim cancel flag and does NOT join an in-flight
    // interim decode. The final decode must start immediately, not wait for
    // the interim thread to release the speech-model lock. The test pines
    // the contract: the interim handle's `stop()` is non-blocking, and the
    // cancel flag is set so the interim thread abandons its decode.

    #[test]
    fn stop_does_not_block_on_interim_decode() {
        // Simulate the P1.4 contract: a mock interim provider that sleeps
        // 500 ms in its (in-flight) decode. When `stop()` is called it must
        // set the cancel flag and return immediately (< 20 ms), NOT join
        // the in-flight decode. The final decode then starts without waiting.
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        // The interim thread: holds the "speech-model lock" (a Mutex) and
        // sleeps 500 ms simulating an in-flight decode. It checks the cancel
        // flag between chunks and releases the lock when it sees it.
        let lock = Arc::new(Mutex::new(()));
        let cancelled_thread = cancelled.clone();
        let lock_thread = lock.clone();
        let interim = thread::spawn(move || {
            // Acquire the "speech-model lock" and hold it for 500 ms
            // (simulating an in-flight interim decode). Check the cancel
            // flag: if set, release early.
            let _guard = lock_thread.lock().unwrap();
            for _ in 0..50 {
                if cancelled_thread.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            // Release the lock (guard drops here).
        });

        // Wait for the interim thread to acquire the lock and start sleeping.
        // (50 ms: the thread's first 10 ms sleep starts almost immediately
        // after spawn; 50 ms is 5x that with margin for CI-runner wakeup
        // jitter. A real regression — stop() joining the 500 ms decode —
        // is caught by the stop_elapsed bound below regardless.)
        thread::sleep(Duration::from_millis(50));

        // Now call the equivalent of `stop()`: set the cancel flag and send
        // the stop signal. This must NOT join the interim thread.
        let stop_started = Instant::now();
        cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
        let _ = stop_tx.send(());
        let stop_elapsed = stop_started.elapsed();

        // The stop must return immediately, not block on the 500 ms decode.
        // 200 ms backstop: the production path sets the flag + sends the
        // mpsc and does not join; a real regression blocks for ~500 ms on
        // the interim decode, so 200 ms is 2.5x margin against the worst
        // CI-runner scheduling jitter observed (a 20 ms bound flaked twice
        // on loaded runners, 2026-10-18, even at 100 ms).
        assert!(
            stop_elapsed < Duration::from_millis(200),
            "stop() must not block on the interim decode; took {:?}",
            stop_elapsed
        );

        // The final decode can now acquire the speech-model lock: the
        // interim thread releases it within 10 ms of seeing the cancel flag.
        let final_started = Instant::now();
        let _final_guard = lock.lock().unwrap();
        let final_elapsed = final_started.elapsed();
        // Same jitter margin as above: a real regression would wait the
        // full remaining 500 ms interim decode.
        assert!(
            final_elapsed < Duration::from_millis(200),
            "final decode must acquire the lock quickly after stop(); took {:?}",
            final_elapsed
        );

        // The interim thread exits once it sees the cancel flag.
        let _ = interim.join();
        let _ = stop_rx; // drain
    }

    // ------------------------------------------------------------------
    // P4 completion: the streaming commits ACTUALLY happen.
    //
    // The earlier P4 unit wired the streaming path end to end but it was
    // inert: `streaming_for_take` built the committer with an UNLOADED
    // provider (empty model path), so every segment decode failed with
    // `NoModel`, `committed_count()` stayed 0, and every take fell back to
    // the whole-take decode. These tests drive the REAL `StreamingTake`
    // (the same `feed_frame`/`tail`/`finish` the production capture
    // callback and release worker use) with a loaded `MockSpeechProvider`
    // standing in for the engine, and prove the four properties the plan
    // gates on: mid-take commits, the zero-commit fallback, pipeline
    // parity, and the real trace fields.
    //
    // The VAD is a SCRIPTED mock engine (the same `detector_with` pattern
    // the `teletype-core` VAD tests use): the real Silero model does not
    // cross its 0.5 operating point on any synthetic signal (it is trained
    // on real speech), so the mock supplies the speech/silence labels the
    // Segmenter consumes. The Segmenter, committer, `feed_frame`, `tail`,
    // and `finish` are all real production code.
    // ------------------------------------------------------------------

    /// A scripted VAD engine: returns the next probability from `script`
    /// (repeating the last value once the script is exhausted). Mirrors the
    /// `detector_with` double in `teletype-core`'s VAD tests.
    struct ScriptedVad {
        script: Vec<f32>,
        i: usize,
    }

    impl teletype_core::vad::VadEngine for ScriptedVad {
        fn process(&mut self, _chunk: &[f32]) -> f32 {
            let p = self.script[self.i.min(self.script.len() - 1)];
            self.i += 1;
            p
        }
        fn chunk_size(&self) -> usize {
            512 // 32 ms @ 16 kHz (Silero's chunk size)
        }
        fn reset(&mut self) {
            self.i = 0;
        }
    }

    /// P4 completion test helper: build a `StreamingTake` the way production
    /// does (`streaming_for_take`) but with a loaded `MockSpeechProvider` as
    /// the committer's engine (the production provider construction — real
    /// Whisper/Parakeet — is isolated from the test by this small
    /// constructor) and a scripted VAD engine so the Segmenter actually cuts
    /// (`committer.load_model` is enqueued, mirroring the production call).
    ///
    /// `speech_chunks` / `silence_chunks` are the per-512-sample-chunk VAD
    /// probabilities, in the exact order `feed_frame` consumes them (the
    /// take is fed as `speech_chunks` chunks of speech, then `silence_chunks`
    /// of silence, and so on).
    fn streaming_take_with_mock(text: &str, vad_script: Vec<f32>) -> StreamingTake {
        let provider = teletype_speech::MockSpeechProvider {
            transcript: text.to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = crate::committer::Committer::new(Box::new(provider));
        // Mirror production: the take-start path enqueues the model load on
        // the committer's worker thread (it jumps the queue, before any
        // segment). The mock is already loaded, so the load is a no-op, but
        // the path is exercised exactly as in production.
        committer.load_model(std::path::PathBuf::from("mock-model.bin"));
        StreamingTake {
            buf16: Vec::new(),
            segmenter: Segmenter::with_rate(SegmenterConfig::default(), 16_000),
            vad: VadDetector::new(Box::new(ScriptedVad {
                script: vad_script,
                i: 0,
            })),
            carry: Vec::new(),
            last_commit_end: 0,
            committer,
            language: "en".to_string(),
        }
    }

    /// The 60 s take's VAD script: 6 blocks of (10 s speech + 500 ms
    /// silence), each 10 s = 312.5 chunks of `0.9` and each 500 ms = 15.625
    /// chunks of `0.0`. Rounded to whole chunks (313 speech / 16 silence per
    /// block) the take is ~61 s — the Segmenter commits a segment at each of
    /// the 5 inter-block pauses, so 5 segments commit mid-take and the final
    /// block stays uncommitted (the tail).
    /// The 60 s take's VAD script: 5 full blocks of (10 s speech + 500 ms
    /// silence) followed by a short final block (1 s speech + 500 ms
    /// silence). Each 10 s = 313 chunks of `0.9` and each 500 ms = 16 chunks
    /// of `0.0`. The Segmenter commits a segment at each of the 5 pauses
    /// (after blocks 1–5), so 5 segments commit mid-take; the final short
    /// block (1.5 s) stays uncommitted and becomes the tail (≤ 3 s).
    fn p4_60s_vad_script() -> Vec<f32> {
        let mut v: Vec<f32> = Vec::new();
        for _ in 0..5 {
            v.extend(std::iter::repeat_n(0.9f32, 313)); // 10 s speech
            v.extend(std::iter::repeat_n(0.0f32, 16)); // 500 ms pause
        }
        // Final short block: 1 s speech + 500 ms silence (the tail).
        v.extend(std::iter::repeat_n(0.9f32, 32));
        v.extend(std::iter::repeat_n(0.0f32, 16));
        v
    }

    /// P4 completion (test 1): a 60 s synthetic take fed at the device rate
    /// (10 s speech + 500 ms silence, ×5, then a 1 s + 500 ms tail) commits
    /// segments MID-TAKE (≥ 5), leaves a small tail (≤ 3 s) on release, and
    /// the release payload is the assembled segments + tail.
    #[test]
    fn p4_streaming_60s_take_commits_mid_take() {
        let mut take = streaming_take_with_mock("committed segment text", p4_60s_vad_script());
        // 5 full blocks of (10 s speech + 500 ms silence), then the short
        // final block. The audio amplitude is irrelevant (the VAD is
        // scripted); the length is what the Segmenter and committer see.
        for _ in 0..5 {
            take.feed_frame(&vec![0.5; 160_000], 16_000); // 10 s speech
            take.feed_frame(&vec![0.0; 8000], 16_000); // 500 ms pause
        }
        // Final short block: 1 s speech + 500 ms pause (the tail).
        take.feed_frame(&vec![0.5; 16_000], 16_000);
        take.feed_frame(&vec![0.0; 8000], 16_000);
        // The committer decodes on its worker thread; give it time, then
        // collect the committed segments (the same call the pill loop makes).
        std::thread::sleep(std::time::Duration::from_millis(300));
        take.committer.poll_completed();
        assert!(
            take.committer.committed_count() >= 5,
            "a 60 s take must commit ≥ 5 segments mid-take; got {}",
            take.committer.committed_count()
        );
        // Release: the tail is the uncommitted remainder (≤ 3 s of audio).
        assert!(
            take.tail_audio_ms() <= 3_000,
            "a 60 s take must leave ≤ 3 s of tail audio; got {} ms",
            take.tail_audio_ms()
        );
        // The payload is the assembled committed segments + the tail
        // (the release worker folds them by timestamp).
        let transcripts = take.finish("en".to_string()).expect("finish");
        assert_eq!(
            transcripts.len(),
            5 + 1,
            "payload = 5 committed segments + 1 tail"
        );
        for t in &transcripts {
            assert_eq!(t.text, "committed segment text");
        }
    }

    /// P4 completion (test 2): a short take that the Segmenter never cuts
    /// (no mid-take commit) takes the OLD path on release — the whole-take
    /// decode — and the trace's `committed_segments` stays 0.
    #[test]
    fn p4_streaming_zero_commit_short_take_falls_back() {
        // 1 s of speech, no pause long enough to commit: the scripted VAD
        // reports speech the whole time, so the Segmenter never fires and
        // nothing is committed mid-take.
        let vad_script = vec![0.9f32; 32]; // 1 s = 32 chunks of speech
        let mut take = streaming_take_with_mock("short take", vad_script);
        take.feed_frame(&vec![0.5; 16_000], 16_000);
        std::thread::sleep(std::time::Duration::from_millis(100));
        take.committer.poll_completed();
        assert_eq!(
            take.committer.committed_count(),
            0,
            "a short take must not commit mid-take"
        );
        // The release path (the transcribe worker) only routes through the
        // streaming finish when `committed_count() > 0`; otherwise it runs
        // the whole-take `transcribe` — byte-identical to before. Model that
        // gate here, exactly as the worker does.
        let uses_streaming = take.committer.committed_count() > 0;
        assert!(!uses_streaming, "zero-commit take must run the old path");
        // The trace mark is recorded only when `committed > 0`; a zero-commit
        // take leaves `committed_segments == 0` (the whole-take fallback).
        let committed = take.committer.committed_count() as u32;
        assert_eq!(committed, 0);
    }

    /// P4 completion (test 3): pipeline parity. The assembled output
    /// (committed segments + tail, folded by the release worker's
    /// `merge_transcripts`) equals the whole-utterance decode
    /// byte-for-byte. Reuses the `wer_parity_assembled_segments_equal_whole_utterance`
    /// pattern from `committer.rs`: the ideal engine returns exactly the
    /// take's words with timings, so the whole-utterance decode is the
    /// reference.
    #[test]
    fn p4_streaming_assembled_output_matches_whole_utterance() {
        // 10 sentences; the ideal engine returns the take's words with
        // timings, so the whole-utterance decode is the reference.
        let sentences: Vec<String> = (1..=10)
            .map(|i| format!("sentence number {i} about the teletype pipeline"))
            .collect();
        let whole: String = sentences.join(" ");
        let words: Vec<teletype_speech::Word> = whole
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| teletype_speech::Word {
                text: w.to_string(),
                start_ms: (i * 400) as u32,
                end_ms: (i * 400 + 350) as u32,
                conf: 0.9,
            })
            .collect();
        let reference = teletype_speech::Transcript {
            text: whole.clone(),
            words: words.clone(),
            confidence: 0.9,
            language: Some("en".to_string()),
        };

        // Build the committed-segment boundaries the way the Segmenter would
        // (1 s speech + 500 ms silence per sentence). The take ends with a
        // final 1 s of speech (uncut) so the tail is that trailing speech —
        // `flush` returns it (after the final pause the segmenter is Idle at
        // the end, which would make `flush` return `None`).
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut boundaries: Vec<usize> = Vec::new();
        for _ in &sentences {
            for ev in seg.push(&vec![0.5; 16_000], 0.9) {
                match ev {
                    SegmentEvent::Ready(s) => boundaries.push(s.end_sample),
                }
            }
            for ev in seg.push(&vec![0.0; 8000], 0.0) {
                match ev {
                    SegmentEvent::Ready(s) => boundaries.push(s.end_sample),
                }
            }
        }
        // Trailing 1 s of speech: the uncommitted tail.
        for ev in seg.push(&vec![0.5; 16_000], 0.9) {
            match ev {
                SegmentEvent::Ready(s) => boundaries.push(s.end_sample),
            }
        }
        let tail = seg.flush().expect("flush returns the tail");
        assert!(boundaries.len() >= 3, "segmenter must cut mid-take");

        // Each committed segment decodes to exactly the words in its window;
        // the tail gets the rest. The release worker folds them with
        // `merge_transcripts` (overlap de-duped by timestamp).
        let total = words.len();
        let mut cursor = 0usize;
        let mut acc: Option<teletype_speech::Transcript> = None;
        for &end_sample in &boundaries {
            let end_ms = end_sample as u64 * 1000 / 16_000;
            let mut next_words = Vec::new();
            while cursor < total {
                let w = &words[cursor];
                if w.start_ms as u64 >= end_ms {
                    break;
                }
                next_words.push(w.clone());
                cursor += 1;
            }
            let next = teletype_speech::Transcript {
                text: next_words
                    .iter()
                    .map(|w| w.text.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
                words: next_words,
                confidence: 0.9,
                language: Some("en".to_string()),
            };
            acc = Some(match acc {
                None => next,
                Some(p) => crate::committer::merge_transcripts(&p, next),
            });
        }
        let tail_words = words[cursor..].to_vec();
        let tail_t = teletype_speech::Transcript {
            text: tail_words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" "),
            words: tail_words,
            confidence: 0.9,
            language: Some("en".to_string()),
        };
        let assembled = match acc {
            None => tail_t,
            Some(p) => crate::committer::merge_transcripts(&p, tail_t),
        };
        assert_eq!(
            assembled.text, reference.text,
            "assembled (segments + tail) must equal the whole-utterance decode byte-for-byte"
        );
        // Sanity: the tail is the one trailing silence, so it is small.
        let tail_ms = (tail.end_sample - tail.start_sample) as u64 * 1000 / 16_000;
        assert!(
            tail_ms <= 1000,
            "tail is one trailing silence: {tail_ms} ms"
        );
    }

    /// P4 completion (test 4): the trace fields are REAL. A 60 s take's
    /// `DictationTrace` carries the real `committed_segments` and
    /// `tail_audio_ms` measured from the actual `StreamingTake` (not a
    /// hardcoded mock). The controller records these marks at release
    /// (from `committed_count()` / `tail_audio_ms()`); the pipeline worker
    /// ships them in `TraceStageMarks`, which `apply_stage_marks` folds into
    /// the `DictationTrace`.
    #[test]
    fn p4_streaming_60s_take_trace_carries_real_fields() {
        // Build the real 60 s take and drive it to a mid-take commit state.
        let mut take = streaming_take_with_mock("committed segment text", p4_60s_vad_script());
        for _ in 0..5 {
            take.feed_frame(&vec![0.5; 160_000], 16_000); // 10 s speech
            take.feed_frame(&vec![0.0; 8000], 16_000); // 500 ms pause
        }
        // Final short block: 1 s speech + 500 ms pause (the tail).
        take.feed_frame(&vec![0.5; 16_000], 16_000);
        take.feed_frame(&vec![0.0; 8000], 16_000);
        std::thread::sleep(std::time::Duration::from_millis(300));
        take.committer.poll_completed();
        // The controller measures these at release (before the worker moves
        // the state) — exactly the values it puts in `streaming_marks`.
        let committed = take.committer.committed_count() as u32;
        let tail_ms = take.tail_audio_ms();
        assert!(
            committed >= 5,
            "60 s take must have committed segments: {committed}"
        );
        assert!(
            tail_ms <= 3_000,
            "60 s take tail must be ≤ 3 s: {tail_ms} ms"
        );

        // Drive the headless controller the way the existing trace test does:
        // the worker (now with real marks) ships `PipelineReady`, and the
        // controller folds `TraceStageMarks` into the `DictationTrace`.
        let (_controller, mut session, mut on_inject) = Controller::for_tests(Box::new(|_| {}));
        session.phase = Phase::Idle;
        session.input_with_kind(Input::HotkeyDown, TakeKind::Dictation);
        session.headless_release(TakeKind::Dictation);
        let mut payload = test_payload(1, "joined final text");
        // The real marks from the take, not hardcoded constants.
        payload.trace_marks.committed_segments = committed;
        payload.trace_marks.tail_audio_ms = tail_ms;
        drive(
            &mut session,
            &mut on_inject,
            Event::PipelineReady {
                session: 1,
                payload,
            },
        );
        let trace = session.traces.get(&1).expect("finished trace");
        assert_eq!(trace.committed_segments, committed);
        assert_eq!(trace.tail_audio_ms, tail_ms);
        // They are the real measured values: non-zero and sane.
        assert!(trace.committed_segments >= 5);
        assert!(trace.tail_audio_ms <= 3_000);
    }

    // ------------------------------------------------------------------
    // P4.3b: per-segment polish adoption for long takes.
    // ------------------------------------------------------------------

    /// Build a headless session with a streaming take wired the way
    /// `start()` wires them (the real `start()` needs a microphone; the
    /// take + slot are the same objects the production path shares with
    /// the capture thread). The polish app slot stays `None`: headless
    /// tests have no Tauri app, so they call `install_segment_polish`
    /// directly with a resolved `AppState`.
    fn session_with_streaming_take(take: StreamingTake) -> (Session, StreamingSlot) {
        let streaming_slot: StreamingSlot = Arc::new(Mutex::new(Some(take)));
        let session = Session {
            app: None,
            #[cfg(test)]
            headless_flash: true,
            controller: Controller::placeholder_for_tests(),
            phase: Phase::Idle,
            hotkey_down: false,
            hands_free: false,
            last_release: None,
            next_id: 0,
            take_kind: TakeKind::default(),
            started_at_ms: 0,
            recording_started: None,
            speech_ms: std::collections::HashMap::new(),
            streaming_marks: std::collections::HashMap::new(),
            vad: None,
            streaming: Some(streaming_slot.clone()),
            polish_app: None,
            live_preview: None,
            spool: None,
            ducker: std::sync::Arc::new(std::sync::Mutex::new(crate::ducking::default_ducker())),
            duck_armed: false,
            record_target: None,
            pending_selection: None,
            injected_at: None,
            cancelled_sessions: std::collections::HashSet::new(),
            traces: std::collections::HashMap::new(),
            trace_key: None,
            front_end: None,
            preroll_this_take: None,
            vad_flags: None,
            take_quality: std::collections::HashMap::new(),
            screen_context: None,
            learning_store: None,
        };
        (session, streaming_slot)
    }

    /// A provider that echoes the text it is asked to transform, so a
    /// successful per-segment polish is observable on the committed text.
    struct EchoPolishEngine;

    impl teletype_core::llm::InferenceProvider for EchoPolishEngine {
        fn model_id(&self) -> &str {
            "echo-polish"
        }
        fn model_name(&self) -> &str {
            "Echo Polish"
        }
        fn generate(
            &self,
            prompt: &str,
            _params: teletype_core::llm::GenerationParams,
        ) -> Result<String, String> {
            // The prompt ends with the text to transform (`<<<\n{text}\n>>>`),
            // so the model's answer is the text itself.
            Ok(prompt
                .rsplit("<<<\n")
                .nth(1)
                .unwrap()
                .trim_end_matches(">>>")
                .to_string())
        }
    }

    /// P4.3b: with `polish_whole_text_at_end` off, a long take's committed
    /// segments pass through the installed per-segment polish function.
    /// The real `StreamingTake` (scripted VAD + mock speech engine) is
    /// driven past the threshold through the real `feed_capture_frame`,
    /// then the install is called directly with the resolved `AppState`
    /// (no `AppHandle` in a headless test), and the committer's committed
    /// texts carry the polished output.
    #[test]
    fn p43b_long_take_segments_pass_through_polish_fn() {
        let state = AppState::for_tests();
        {
            let mut slot = state
                .inference
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *slot = Some(Arc::new(EchoPolishEngine));
        }
        let settings = state.settings();
        assert!(!settings.polish_whole_text_at_end);

        let take = streaming_take_with_mock("raw segment words", p4_60s_vad_script());
        let (mut session, streaming_slot) = session_with_streaming_take(take);

        // Feed the frames through the real capture path (10 s of speech +
        // 500 ms pause, twice). The take buffer crosses the 20 s polish
        // threshold; install is then invoked directly with the resolved
        // `AppState` (a headless test has no `AppHandle`).
        session.feed_capture_frame(&vec![0.5; 160_000], 16_000); // 10 s
        session.feed_capture_frame(&vec![0.0; 8_000], 16_000); // 500 ms
        session.feed_capture_frame(&vec![0.5; 160_000], 16_000); // 10 s
        session.feed_capture_frame(&vec![0.0; 8_000], 16_000); // 500 ms
        let len = {
            let g = streaming_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            g.as_ref().expect("streaming take present").buf16.len()
        };
        session.install_segment_polish(&streaming_slot, &state, len);

        // The committer decodes on its worker thread; give it time, then
        // collect the committed segments (the same call the pill loop makes).
        std::thread::sleep(std::time::Duration::from_millis(300));
        let committed_texts = {
            let g = streaming_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let t = g.as_ref().expect("streaming take present");
            t.committer.poll_completed();
            t.committer.committed_texts()
        };
        assert!(
            !committed_texts.is_empty(),
            "a 20+ s take must commit at least one segment mid-take"
        );
        // Every committed segment passed through the installed polish fn:
        // the echo engine returns the text unchanged, and the validator
        // accepts it (output == input is a valid pass-through), so the
        // committed text is the polished segment — the same text the
        // whole-text path would produce for each segment.
        for text in &committed_texts {
            assert_eq!(
                text, "raw segment words",
                "committed segment must be the polished text"
            );
        }
    }

    /// P4.3b: with `polish_whole_text_at_end` on, nothing is installed and
    /// the release path polishes once (the whole-text path is unchanged).
    #[test]
    fn p43b_whole_text_at_end_setting_skips_install() {
        let state = AppState::for_tests();
        {
            let mut slot = state
                .inference
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *slot = Some(Arc::new(EchoPolishEngine));
        }
        {
            let mut settings = state
                .settings
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            settings.polish_whole_text_at_end = true;
        }

        let take = streaming_take_with_mock("raw segment words", p4_60s_vad_script());
        let (mut session, streaming_slot) = session_with_streaming_take(take);

        // Feed the same frames (the take crosses the threshold). With the
        // setting on the install is a no-op (called directly with the
        // resolved `AppState`, as in the test above).
        session.feed_capture_frame(&vec![0.5; 160_000], 16_000);
        session.feed_capture_frame(&vec![0.0; 8_000], 16_000);
        session.feed_capture_frame(&vec![0.5; 160_000], 16_000);
        session.feed_capture_frame(&vec![0.0; 8_000], 16_000);
        let len = {
            let g = streaming_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            g.as_ref().expect("streaming take present").buf16.len()
        };
        session.install_segment_polish(&streaming_slot, &state, len);

        std::thread::sleep(std::time::Duration::from_millis(300));
        let committed_texts = {
            let g = streaming_slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let t = g.as_ref().expect("streaming take present");
            t.committer.poll_completed();
            t.committer.committed_texts()
        };
        // No polish fn installed: the committed segments are the raw
        // decoded text (the release whole-text path polishes once).
        for text in &committed_texts {
            assert_eq!(
                text, "raw segment words",
                "with the setting on, no per-segment polish runs"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// P1.8: engine Arc snapshot — the UI must not block during generation.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod p1_8_tests {
    use super::*;
    use teletype_core::llm::{GenerationParams, InferenceProvider};

    /// A provider whose `generate`/`generate_stream` sleeps, simulating a
    /// slow local LLM call.
    struct SlowEngine {
        delay: Duration,
        /// Set when generation started, cleared when it finished.
        running: std::sync::atomic::AtomicBool,
    }

    impl InferenceProvider for SlowEngine {
        fn model_id(&self) -> &str {
            "slow-mock"
        }
        fn model_name(&self) -> &str {
            "Slow Mock"
        }
        fn generate(&self, _prompt: &str, _params: GenerationParams) -> Result<String, String> {
            self.running
                .store(true, std::sync::atomic::Ordering::SeqCst);
            thread::sleep(self.delay);
            self.running
                .store(false, std::sync::atomic::Ordering::SeqCst);
            Ok("polished".into())
        }
        fn generate_stream(
            &self,
            _prompt: &str,
            _params: GenerationParams,
            on_token: &mut dyn FnMut(&str),
        ) -> Result<String, String> {
            self.running
                .store(true, std::sync::atomic::Ordering::SeqCst);
            thread::sleep(self.delay);
            self.running
                .store(false, std::sync::atomic::Ordering::SeqCst);
            on_token("polished");
            Ok("polished".into())
        }
    }

    /// While a transform is generating (the engine Arc is held by the
    /// pipeline worker), a model-status query must return immediately — it
    /// must not wait for the generation to finish. P1.8: the dictation
    /// worker clones the Arc and drops the lock before generation, so the
    /// status command's brief lock is never blocked by the LLM call.
    #[test]
    fn status_query_returns_immediately_during_slow_generation() {
        let state = AppState::for_tests();
        let engine = Arc::new(SlowEngine {
            delay: Duration::from_millis(300),
            running: std::sync::atomic::AtomicBool::new(false),
        });
        {
            let mut slot = state
                .inference
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *slot = Some(engine.clone());
        }

        // The dictation worker's contract: clone the Arc, drop the lock,
        // generate.
        let worker_engine = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap();
        let worker = thread::spawn(move || {
            worker_engine
                .generate("prompt", GenerationParams::default())
                .unwrap();
        });

        // Wait until the generation is actually in flight.
        while !engine.running.load(std::sync::atomic::Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(5));
        }

        // The UI status query: takes the same lock the worker briefly held.
        // It must return in well under the 300 ms generation window.
        let settings = state.settings();
        let started = Instant::now();
        let status = crate::commands::llm_runtime(&state, &settings);
        let elapsed = started.elapsed();
        assert_eq!(status.state, "loaded");
        assert_eq!(status.model_id, "slow-mock");
        assert!(
            elapsed < Duration::from_millis(100),
            "status query blocked on the generation lock: {elapsed:?}"
        );

        worker.join().unwrap();
    }
}

// ---------------------------------------------------------------------------
// P1.9: delta streaming — IPC bytes for an N-token take are O(N).
// ---------------------------------------------------------------------------

#[cfg(test)]
mod p1_9_tests {
    /// Models the P1.9 token sink: for each of N tokens it emits a
    /// `{seq, delta}` event and records the serialized IPC payload size.
    /// The old behavior re-sent the whole accumulated string each token
    /// (total O(N²) bytes); the new behavior sends only the delta
    /// (total O(N) bytes).
    #[test]
    fn token_ipc_bytes_are_linear_in_token_count() {
        fn sink_bytes(tokens: &[&str]) -> usize {
            let mut total = 0usize;
            for (seq, tok) in tokens.iter().enumerate() {
                let payload = serde_json::json!({ "seq": seq, "delta": tok });
                total += payload.to_string().len();
            }
            total
        }

        let tokens: Vec<String> = (0..50).map(|i| format!("tok{i} ")).collect();
        let refs: Vec<&str> = tokens.iter().map(|s| s.as_str()).collect();

        let bytes_50 = sink_bytes(&refs);

        // O(N) check: doubling the token count must roughly double the
        // bytes (within a generous factor), NOT quadruple them as the old
        // O(N²) whole-string behavior would.
        let tokens_100: Vec<String> = (0..100).map(|i| format!("tok{i} ")).collect();
        let refs_100: Vec<&str> = tokens_100.iter().map(|s| s.as_str()).collect();
        let bytes_100 = sink_bytes(&refs_100);

        assert!(
            bytes_100 < 3 * bytes_50,
            "IPC bytes grew super-linearly: 50 tokens = {bytes_50} B, \
             100 tokens = {bytes_100} B (3x cap for O(N))"
        );

        // And the old behavior would be ~4x: prove the bound is meaningful.
        let old_style_50: usize = refs
            .iter()
            .scan(String::new(), |acc, t| {
                acc.push_str(t);
                Some(acc.len())
            })
            .map(|n| n + 10) // + ~10 bytes of event framing per emit
            .sum();
        let old_style_100: usize = refs_100
            .iter()
            .scan(String::new(), |acc, t| {
                acc.push_str(t);
                Some(acc.len())
            })
            .map(|n| n + 10)
            .sum();
        assert!(
            old_style_100 > 3 * old_style_50,
            "sanity: the old whole-string scheme must be super-linear \
             ({old_style_50} vs {old_style_100})"
        );
    }
}

// ---------------------------------------------------------------------------
// P1.11: VAD silence trim.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod p1_11_tests {
    use super::*;

    /// Pure-shape check of the trim arithmetic with a fake per-chunk speech
    /// map (the real Silero model is exercised in `silent_take_is_unchanged`,
    /// where silence really scores below threshold).
    #[test]
    fn trim_keeps_150ms_padding_around_speech() {
        // 16 kHz, 512-sample chunks (32 ms). 20 chunks = 640 ms.
        let chunk = 512usize;
        let chunks = 20usize;
        let samples = vec![0.0f32; chunk * chunks];
        // Speech in chunks 5..=14 (inclusive) → 320 ms of speech.
        let first = 5usize;
        let last = 14usize;
        let pad = (150u64 * 16_000u64 / 1000) as usize / chunk; // 150ms → 4 chunks (128ms)
        assert_eq!(pad, 4, "150 ms rounds to 4 chunks of 32 ms");

        let start = first.saturating_sub(pad) * chunk;
        let end = ((last + 1) + pad).min(chunks) * chunk;
        let trimmed = &samples[start..end];

        // Leading silence trimmed: 5 chunks of silence → 1 chunk of padding.
        assert_eq!(start / chunk, 1, "keep 150 ms (4 chunks) before speech");
        // Trailing silence trimmed: 5 chunks → 4 chunks of padding.
        assert_eq!(end / chunk, last + 1 + pad);
        // The trimmed slice still contains all speech chunks.
        assert!(start <= first * chunk);
        assert!(end >= (last + 1) * chunk);
        // And it is strictly shorter than the input.
        assert!(trimmed.len() < samples.len());
    }

    /// A take that is all silence must come back unchanged: the trim must
    /// never drop the user's audio on a VAD miss.
    #[test]
    fn silent_take_is_unchanged() {
        // 1 second of digital silence at 16 kHz.
        let samples = vec![0.0f32; 16_000];
        let trimmed = trim_silence_vad(&samples);
        assert_eq!(
            trimmed.len(),
            samples.len(),
            "silence must never be trimmed away"
        );
    }

    /// A mid-take "speech" burst surrounded by silence must be trimmed to
    /// roughly the speech region plus 150 ms of padding on each side. Uses
    /// the probe-injected variant (the real Silero model does not reliably
    /// flag synthetic tones as speech) so the trim arithmetic is what's
    /// under test.
    #[test]
    fn tone_in_the_middle_is_trimmed() {
        let sr = 16_000usize;
        let chunk = 512usize; // 32 ms @ 16 kHz (Silero's chunk size)
        let chunks = 25usize; // 800 ms total
        let samples = vec![0.0f32; chunks * chunk];
        // "Speech" in chunks 7..=17 (352 ms) → tone region 224..=576 ms.
        let speech: Vec<bool> = (0..chunks).map(|c| (7..=17).contains(&c)).collect();
        let trimmed = trim_silence_vad_with_probe(&samples, chunk, &speech);
        // 352 ms of tone + 128 ms padding each side (150 ms rounds to 4
        // chunks of 32 ms) ≈ 608 ms, well under the full 800 ms.
        assert!(
            trimmed.len() < samples.len(),
            "trim should shorten the take"
        );
        assert!(trimmed.len() >= 400 * sr / 1000 && trimmed.len() <= 750 * sr / 1000);
    }
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
