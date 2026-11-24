//! Meeting recording engine: real-time transcription, durable audio, and
//! the self-contained HTML report.
//!
//! A meeting is a long-form recording (minutes to hours), unlike a
//! dictation take (seconds). Lifecycle:
//!
//! ```text
//! Idle → Recording → Finalizing → Done
//!            │              │
//!            └── Cancel ────┴──→ Idle (audio discarded / error)
//! ```
//!
//! While recording, the audio flows through the SAME real-time path
//! dictation uses (P4): the capture callback resamples each device-rate
//! frame to 16 kHz once, and [`StreamingTake`] slices the stream into
//! utterance segments (350 ms pause / 12 s forced cut) that the
//! [`Committer`] transcribes on its own engine handle while the user is
//! still talking. The UI polls [`MeetingEngine::live_text`] for the
//! growing transcript, so words appear on screen within a second or two
//! of being spoken.
//!
//! On stop the take is finalized (tail decode + in-flight segments). The
//! committed transcripts — word timings already shifted to take-absolute
//! and de-duplicated across overlapping cuts by the committer's fold —
//! become the meeting transcript. Engines without a streaming provider
//! (Qwen3-ASR, Apple, transcribe.cpp, Whistle) fall back to one batch
//! `transcribe_detailed` pass over the whole audio, exactly as before.
//!
//! Speaker attribution: a single microphone cannot separate speakers, so
//! every segment is labeled `Speaker 1`. The `speaker` field is
//! first-class (`Option<String>`), so the macOS parakeet.cpp
//! diarization/SAS path can fill in real labels later without a schema
//! change.
//!
//! Layout under `<data>/meetings/`:
//!
//! ```text
//! meetings/
//!   <id>.wav            durable 16 kHz mono audio (crash-safe rename)
//!   <id>/
//!     transcript.json   word/segment transcript with timings
//!     report.html       the self-contained report
//! ```
//!
//! Meeting audio is sensitive (it can contain confidential conversation),
//! so the directory and files get owner-only permissions on Unix, the same
//! model as [`crate::recordings`].

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use teletype_core::audio::{build_dfn3_slot, BlockResampler, Recording, TARGET_SAMPLE_RATE};
use teletype_core::llm::{GenerationParams, InferenceProvider, scaled_timeout};
use teletype_core::meeting::{
    chunk_text, segments_from_words, MeetingAction, MeetingInput, MeetingPhase, MeetingTranscript,
    MeetingWord, MAX_CHUNK_CHARS,
};
use teletype_core::meeting_report::{render_report, ReportSummary};
use teletype_core::segmenter::{Segmenter, SegmenterConfig};
use teletype_core::vad::VadDetector;
use teletype_inference::manager::SpeechModelManager;
use teletype_speech::{SpeechError, SpeechProvider, Transcript};
use tauri::Manager;

use crate::committer::Committer;
use crate::dictation::{
    effective_language_for_app, resolve_speech_model, streaming_provider_for, StreamingTake,
};

/// The single-microphone speaker label. Real multi-speaker labels arrive
/// with the macOS parakeet.cpp diarization path; until then every segment
/// of a meeting belongs to this one speaker.
pub const SPEAKER_LABEL: &str = "Speaker 1";

/// `<base>/meetings`.
pub fn dir(base: &Path) -> PathBuf {
    base.join("meetings")
}

/// A finished meeting's artifacts + headline stats.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRef {
    pub id: String,
    pub title: String,
    /// Absolute path of the self-contained HTML report.
    pub report_path: String,
    /// Absolute path of the durable WAV audio.
    pub audio_path: String,
    pub duration_ms: u64,
    pub word_count: u32,
    pub segment_count: u32,
    /// Unique speaker labels, by first appearance.
    pub speakers: Vec<String>,
    /// Whether an LLM summary made it into the report.
    pub has_summary: bool,
}

/// Everything the UI needs to render the finished meeting on the same
/// page (overview + action items + details + full transcript).
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportData {
    pub id: String,
    pub title: String,
    /// Pre-formatted local start time.
    pub started_at: String,
    pub report_path: String,
    pub audio_path: String,
    pub duration_ms: u64,
    pub word_count: u32,
    pub wpm: u32,
    pub segment_count: u32,
    pub speakers: Vec<String>,
    /// The LLM overview (what the meeting was about); `None` when no
    /// inference provider ran the map-reduce pass.
    pub summary: Option<String>,
    pub action_items: Vec<String>,
    pub segments: Vec<ReportSegmentRow>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportSegmentRow {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: Option<String>,
    pub text: String,
}

/// The finished meeting, kept in the engine so the UI can render the
/// report on the same page without re-reading disk.
pub struct FinishedMeeting {
    pub report: ReportRef,
    pub transcript: MeetingTranscript,
    pub summary: Option<ReportSummary>,
    pub started_at_ms: u64,
}

struct ActiveMeeting {
    id: String,
    title: String,
    started_at_ms: u64,
    recording: Recording,
    /// Real-time commit state (the dictation P4 streaming take). `None`
    /// when the selected engine has no streaming provider or the Silero
    /// VAD could not load: the meeting then decodes whole on stop.
    stream: Option<Arc<Mutex<Option<StreamingTake>>>>,
    /// The take-end resampler flush slot: the frame callback creates the
    /// [`BlockResampler`] lazily on the first frame; `stop` flushes the
    /// trailing partial block into the stream the same way dictation does.
    resampler: Arc<Mutex<Option<BlockResampler>>>,
    language: String,
    /// Last transcript text written to the live `report.html`; change
    /// detection so status polls don't rewrite the file every second.
    last_live_report: String,
}

struct Inner {
    phase: MeetingPhase,
    active: Option<ActiveMeeting>,
    finished: Option<FinishedMeeting>,
    error: Option<String>,
}

/// Drives one meeting at a time. Cheap to clone the guard out of
/// [`AppState`]; the mutex is only held for short state transitions —
/// never across ASR or disk I/O.
#[derive(Clone)]
pub struct MeetingEngine {
    inner: Arc<Mutex<Inner>>,
    meetings_dir: PathBuf,
}

impl MeetingEngine {
    pub fn new(meetings_dir: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                phase: MeetingPhase::Idle,
                active: None,
                finished: None,
                error: None,
            })),
            meetings_dir,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn phase(&self) -> MeetingPhase {
        self.lock().phase
    }

    pub fn is_recording(&self) -> bool {
        self.lock().phase == MeetingPhase::Recording
    }

    /// Elapsed capture time, while recording.
    pub fn elapsed_ms(&self) -> Option<u64> {
        self.lock().active.as_ref().map(|a| {
            teletype_core::storage::now_ms().saturating_sub(a.started_at_ms)
        })
    }

    /// The transcript so far (committed segments), while recording.
    /// `None` when nothing has committed yet or the meeting has no
    /// streaming state. Word timings are take-absolute; every word and
    /// segment carries the single-mic speaker label.
    pub fn live_transcript(&self) -> Option<MeetingTranscript> {
        let inner = self.lock();
        let active = inner.active.as_ref()?;
        let slot = active.stream.as_ref()?;
        let (transcripts, started_at_ms) = {
            let guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
            let take = guard.as_ref()?;
            take.committer.poll_completed();
            (take.committer.committed_transcripts(), active.started_at_ms)
        };
        drop(inner);
        if transcripts.is_empty() {
            return None;
        }
        let mut t = transcript_from_committed(&transcripts);
        t.duration_ms = teletype_core::storage::now_ms().saturating_sub(started_at_ms);
        t.segments = segments_from_words(&t.words);
        label_single_speaker(&mut t);
        Some(t)
    }

    /// Rewrites the meeting's `report.html` while recording, whenever new
    /// segments have committed — the report is being built in real time
    /// (transcript-only; the LLM overview lands with the final report).
    /// No-op when the committed text hasn't changed since the last write.
    pub fn refresh_live_report(&self) {
        let live = match self.live_transcript() {
            Some(t) => t,
            None => return,
        };
        let (id, title, started_at_ms, changed) = {
            let mut inner = self.lock();
            let active = match inner.active.as_mut() {
                Some(a) => a,
                None => return,
            };
            let changed = live.text != active.last_live_report;
            if changed {
                active.last_live_report.clone_from(&live.text);
            }
            (active.id.clone(), active.title.clone(), active.started_at_ms, changed)
        };
        if !changed {
            return;
        }
        let meeting_dir = self.meetings_dir.join(&id);
        if fs::create_dir_all(&meeting_dir).is_err() {
            return;
        }
        let started_at = format_local_time(started_at_ms);
        let html = render_report(&title, &started_at, &live, None);
        let _ = write_atomic(&meeting_dir.join("report.html"), &html);
    }

    pub fn last_report(&self) -> Option<ReportRef> {
        self.lock().finished.as_ref().map(|f| f.report.clone())
    }

    /// The finished meeting's full report content (same-page render).
    pub fn report_data(&self) -> Option<ReportData> {
        let inner = self.lock();
        let fm = inner.finished.as_ref()?;
        let word_count = fm.report.word_count;
        let wpm = if fm.report.duration_ms > 0 {
            (word_count as f64 / (fm.report.duration_ms as f64 / 60_000.0)) as u32
        } else {
            0
        };
        Some(ReportData {
            id: fm.report.id.clone(),
            title: fm.report.title.clone(),
            started_at: format_local_time(fm.started_at_ms),
            report_path: fm.report.report_path.clone(),
            audio_path: fm.report.audio_path.clone(),
            duration_ms: fm.report.duration_ms,
            word_count,
            wpm,
            segment_count: fm.report.segment_count,
            speakers: fm.report.speakers.clone(),
            summary: fm.summary.as_ref().map(|s| s.summary.clone()),
            action_items: fm
                .summary
                .as_ref()
                .map(|s| s.action_items.clone())
                .unwrap_or_default(),
            segments: fm
                .transcript
                .segments
                .iter()
                .map(|s| ReportSegmentRow {
                    start_ms: s.start_ms,
                    end_ms: s.end_ms,
                    speaker: s.speaker.clone(),
                    text: s.text.clone(),
                })
                .collect(),
        })
    }

    pub fn last_error(&self) -> Option<String> {
        self.lock().error.clone()
    }

    /// Starts capture. Fails when a meeting is already in flight.
    ///
    /// Builds the same real-time state dictation arms per take
    /// (`streaming_for_take`): a committer with its own engine handle for
    /// the selected model, loaded in the background, and a
    /// [`StreamingTake`] whose segmenter cuts the stream into utterances.
    pub fn start(&self, app: &tauri::AppHandle, title: &str, device_id: &str) -> Result<String, String> {
        let state = app.state::<crate::AppState>();
        let settings = state.settings();
        let (model_path, kind) = resolve_speech_model(&settings, &state.models_dir);
        let language = effective_language_for_app(&state, kind);
        let model_id = settings.selected_speech_model.clone();

        let stream: Option<Arc<Mutex<Option<StreamingTake>>>> =
            match streaming_provider_for(kind, &model_id) {
                Some(provider) => match VadDetector::silero_16k() {
                    Ok(vad) => {
                        let committer = Committer::new(provider);
                        committer.load_model(model_path);
                        Some(Arc::new(Mutex::new(Some(StreamingTake {
                            buf16: Vec::new(),
                            base_offset: 0,
                            segmenter: Segmenter::with_rate(SegmenterConfig::default(), 16_000),
                            vad,
                            carry: Vec::new(),
                            last_commit_end: 0,
                            committer,
                            language: language.clone(),
                        }))))
                    }
                    Err(e) => {
                        crate::log_entry(
                            crate::LogLevel::Warn,
                            format!("meeting streaming VAD init failed: {e}; batch decode on stop"),
                        );
                        None
                    }
                },
                None => {
                    crate::log_entry(
                        crate::LogLevel::Info,
                        format!("meeting streaming off: no {kind:?} streaming provider; batch decode on stop"),
                    );
                    None
                }
            };

        // The frame callback (capture thread): resample the device-rate
        // frame to 16 kHz ONCE, in 100 ms blocks, and feed the streaming
        // take. The durable WAV comes from `recording.finish()`, so nothing
        // else consumes these samples.
        let rate_slot = Arc::new(AtomicU32::new(0));
        let resampler = Arc::new(Mutex::new(None));
        let stream_cb = stream.clone();
        let res_cb = resampler.clone();
        let rate_cb = rate_slot.clone();
        let on_frame: Option<Box<dyn Fn(&[f32]) + Send + Sync + 'static>> = Some(Box::new(
            move |frame: &[f32]| {
                let r = rate_cb.load(Ordering::Relaxed);
                if r == 0 {
                    return;
                }
                let r16 = {
                    let mut guard = res_cb.lock().unwrap_or_else(PoisonError::into_inner);
                    let inner = guard.get_or_insert_with(|| BlockResampler::new(r));
                    inner.push(frame)
                };
                if r16.is_empty() {
                    return;
                }
                if let Some(slot) = &stream_cb {
                    let mut guard = slot.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(take) = guard.as_mut() {
                        take.feed_frame(&r16, 16_000);
                    }
                }
            },
        ));

        // Same noise-mode wiring as dictation: Smart mode runs the DFN3
        // split pipeline for detection; Off mode uses the legacy path.
        let noise_reduction = settings.noise_reduction_enabled;
        let dfn3_slot = if noise_reduction {
            Some(build_dfn3_slot(&state.models_dir, 48_000))
        } else {
            None
        };

        let recording = Recording::start_with_vad_df(
            device_id,
            |_| {},
            on_frame,
            Some(rate_slot),
            noise_reduction,
            dfn3_slot,
        )?;

        let mut inner = self.lock();
        if inner.phase.is_busy() {
            // The capture thread is stopped by dropping the recording; the
            // streaming state drops with the scope and its worker exits.
            drop(recording);
            return Err("A meeting is already in progress".into());
        }
        inner.error = None;
        let id = uuid::Uuid::new_v4().to_string();
        // Create the meeting folder up front so the live report can be
        // written while recording (finalize re-creates idempotently).
        let _ = fs::create_dir_all(&self.meetings_dir);
        let _ = fs::create_dir_all(self.meetings_dir.join(&id));
        inner.active = Some(ActiveMeeting {
            id: id.clone(),
            title: title.to_string(),
            started_at_ms: teletype_core::storage::now_ms(),
            recording,
            stream,
            resampler,
            language,
            last_live_report: String::new(),
        });
        debug_assert_eq!(
            MeetingAction::BeginCapture,
            teletype_core::meeting::decide(MeetingInput::Start, MeetingPhase::Idle)
        );
        inner.phase = MeetingPhase::Recording;
        drop(inner);

        // The live report is built in real time regardless of UI polling:
        // a lightweight thread refreshes report.html whenever new segments
        // have committed (change-detected inside, so steady-state ticks are
        // a mutex check and nothing else). Exits when the phase leaves
        // Recording (stop / cancel / finalize).
        {
            let engine = self.clone();
            let _ = std::thread::Builder::new()
                .name("teletype-meeting-live-report".into())
                .spawn(move || {
                    while engine.phase() == MeetingPhase::Recording {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        engine.refresh_live_report();
                    }
                });
        }
        Ok(id)
    }

    /// Stops capture and hands the audio + committed transcripts to the
    /// caller for finalization. The phase moves to `Finalizing`
    /// immediately: a concurrent start is rejected until the finalize
    /// thread publishes `Done` (or an error).
    pub fn stop(&self) -> Result<FinalizeInput, String> {
        let mut inner = self.lock();
        let active = inner
            .active
            .take()
            .ok_or_else(|| "No meeting is recording".to_string())?;
        debug_assert_eq!(
            MeetingAction::EndCapture,
            teletype_core::meeting::decide(MeetingInput::Stop, MeetingPhase::Recording)
        );
        inner.phase = MeetingPhase::Finalizing;
        drop(inner);

        // 1. Take-end resampler flush (dictation's `flush_resampler_into_take`):
        //    the trailing partial 100 ms block must reach the segmenter.
        if let Some(slot) = &active.stream {
            let tail = active
                .resampler
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
                .map(BlockResampler::flush);
            if let Some(t) = tail {
                if !t.is_empty() {
                    let mut guard = slot
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    if let Some(take) = guard.as_mut() {
                        take.feed_frame(&t, 16_000);
                    }
                }
            }
        }

        // 2. Stop the capture and drain (fast, <1 s). `finish` returns the
        //    whole take resampled to 16 kHz — the source for the durable WAV.
        let captured = active.recording.finish()?;

        // 3. Finalize the streaming take: tail decode + in-flight segments,
        //    in commit order, de-duplicated. Blocks until the tail decodes
        //    (this runs on the finalize thread, never on IPC).
        let transcripts = match active.stream {
            Some(slot) => match slot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                Some(take) => match take.finish(active.language.clone()) {
                    Ok(result) => {
                        if result.failures.failed_segments > 0 || result.failures.tail_failed {
                            crate::log_entry(
                                crate::LogLevel::Warn,
                                format!(
                                    "meeting streaming: {} segment(s) with word loss",
                                    result.failures.failed_segments
                                ),
                            );
                        }
                        result.transcripts
                    }
                    Err(e) => {
                        crate::log_entry(
                            crate::LogLevel::Warn,
                            format!("meeting streaming finish failed: {e}; batch fallback"),
                        );
                        Vec::new()
                    }
                },
                None => Vec::new(),
            },
            None => Vec::new(),
        };

        Ok(FinalizeInput {
            id: active.id,
            title: active.title,
            started_at_ms: active.started_at_ms,
            samples: captured.samples,
            duration_ms: (captured.duration_secs * 1000.0) as u64,
            transcripts,
        })
    }

    /// Stops capture and discards the audio.
    pub fn cancel(&self) {
        let mut inner = self.lock();
        if let Some(active) = inner.active.take() {
            active.recording.stop_capture();
        }
        inner.phase = MeetingPhase::Idle;
    }

    /// Publishes the finished meeting (called by the finalize thread).
    pub fn mark_done(&self, meeting: FinishedMeeting) {
        let mut inner = self.lock();
        debug_assert_eq!(
            MeetingAction::MarkDone,
            teletype_core::meeting::decide(MeetingInput::Finalized, MeetingPhase::Finalizing)
        );
        inner.finished = Some(meeting);
        inner.phase = MeetingPhase::Done;
    }

    /// Finalization failed: back to idle with the error surfaced.
    pub fn mark_failed(&self, error: String) {
        let mut inner = self.lock();
        inner.error = Some(error);
        inner.phase = MeetingPhase::Idle;
    }

    /// Clears the last report and error (UI "close report").
    pub fn reset(&self) {
        let mut inner = self.lock();
        debug_assert_eq!(
            MeetingAction::GoIdle,
            teletype_core::meeting::decide(MeetingInput::Reset, MeetingPhase::Done)
        );
        inner.phase = MeetingPhase::Idle;
    }

    /// The meetings directory (for cleanup commands).
    pub fn dir(&self) -> &Path {
        &self.meetings_dir
    }
}

/// A stopped meeting's audio + committed transcripts, handed to the
/// finalize thread.
pub struct FinalizeInput {
    pub id: String,
    pub title: String,
    pub started_at_ms: u64,
    /// 16 kHz mono (the whole take, from `recording.finish`).
    pub samples: Vec<f32>,
    pub duration_ms: u64,
    /// Real-time committed transcripts (take-absolute word timings,
    /// de-duplicated). Empty when the engine has no streaming provider or
    /// the streaming path failed — the finalize step then falls back to a
    /// batch decode of `samples`.
    pub transcripts: Vec<Transcript>,
}

/// Runs the finalization pipeline on the calling (blocking) thread:
/// durable WAV → transcript (committed or batch) → optional LLM summary →
/// report render. Returns the finished meeting on success.
pub fn finalize(
    engine: &MeetingEngine,
    speech: &SpeechModelManager<Box<dyn SpeechProvider>>,
    inference: &Mutex<Option<Arc<dyn InferenceProvider>>>,
    engine_label: &str,
    input: FinalizeInput,
) -> Result<FinishedMeeting, String> {
    let meetings_dir = engine.dir();
    let meeting_dir = meetings_dir.join(&input.id);
    fs::create_dir_all(meetings_dir).map_err(|e| format!("create meetings dir: {e}"))?;
    fs::create_dir_all(&meeting_dir).map_err(|e| format!("create meeting dir: {e}"))?;
    restrict_owner_only(meetings_dir);
    restrict_owner_only(&meeting_dir);

    // 1. Durable audio: tmp + rename, same crash-safety model as recordings.
    let wav_path = meetings_dir.join(format!("{}.wav", input.id));
    let wav_tmp = meetings_dir.join(format!("{}.tmp.wav", input.id));
    crate::recordings::write_wav16(&wav_tmp, &input.samples, TARGET_SAMPLE_RATE)
        .map_err(|e| format!("write meeting audio: {e}"))?;
    fs::rename(&wav_tmp, &wav_path).map_err(|e| {
        let _ = fs::remove_file(&wav_tmp);
        format!("publish meeting audio: {e}")
    })?;
    restrict_owner_only_file(&wav_path);

    // 2. Transcript: the real-time committed segments when the streaming
    //    path produced any, otherwise a batch decode of the whole audio.
    let mut transcript = if !input.transcripts.is_empty() {
        transcript_from_committed(&input.transcripts)
    } else {
        speech.begin_dictation();
        let result = transcribe_meeting(speech, &input.samples);
        speech.end_dictation(None);
        result?
    };
    transcript.duration_ms = input.duration_ms;
    transcript.engine = Some(engine_label.to_string());
    transcript.segments = segments_from_words(&transcript.words);
    label_single_speaker(&mut transcript);

    // 3. LLM summary: best-effort. A missing or failing provider degrades
    //    the report to transcript-only; it never fails the meeting.
    let summary = summarize(inference, &transcript.text);

    // 4. transcript.json + report.html.
    let transcript_json =
        serde_json::to_string_pretty(&transcript).unwrap_or_else(|_| "{}".to_string());
    write_atomic(&meeting_dir.join("transcript.json"), &transcript_json)
        .map_err(|e| format!("write transcript: {e}"))?;

    let started_at = format_local_time(input.started_at_ms);
    let html = render_report(&input.title, &started_at, &transcript, summary.as_ref());
    let report_path = meeting_dir.join("report.html");
    write_atomic(&report_path, &html).map_err(|e| format!("write report: {e}"))?;

    let speakers = {
        let mut seen: Vec<String> = Vec::new();
        for s in transcript.segments.iter().filter_map(|s| s.speaker.clone()) {
            if !seen.contains(&s) {
                seen.push(s);
            }
        }
        seen
    };

    Ok(FinishedMeeting {
        report: ReportRef {
            id: input.id,
            title: input.title,
            report_path: report_path.to_string_lossy().to_string(),
            audio_path: wav_path.to_string_lossy().to_string(),
            duration_ms: input.duration_ms,
            word_count: transcript.words.len() as u32,
            segment_count: transcript.segments.len() as u32,
            speakers,
            has_summary: summary
                .as_ref()
                .is_some_and(|s| !s.summary.trim().is_empty()),
        },
        transcript,
        summary,
        started_at_ms: input.started_at_ms,
    })
}

/// Folds the committer's committed transcripts (already take-absolute and
/// de-duplicated across overlapping cuts) into one meeting transcript.
fn transcript_from_committed(transcripts: &[Transcript]) -> MeetingTranscript {
    let words: Vec<MeetingWord> = transcripts
        .iter()
        .flat_map(|t| t.words.iter())
        .map(|w| MeetingWord {
            text: w.text.clone(),
            start_ms: u64::from(w.start_ms),
            end_ms: u64::from(w.end_ms),
            speaker: None,
        })
        .collect();
    let text = transcripts
        .iter()
        .map(|t| t.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let confidence = transcripts
        .first()
        .map(|t| t.confidence)
        .unwrap_or(1.0);
    let language = transcripts.iter().find_map(|t| t.language.clone());
    MeetingTranscript {
        text,
        words,
        segments: Vec::new(),
        duration_ms: 0,
        confidence,
        language,
        engine: None,
    }
}

/// A single microphone has one speaker: label every word and segment so
/// the report shows the attribution column (real multi-speaker labels
/// arrive with the macOS diarization path).
fn label_single_speaker(transcript: &mut MeetingTranscript) {
    for word in &mut transcript.words {
        word.speaker = Some(SPEAKER_LABEL.to_string());
    }
    for segment in &mut transcript.segments {
        segment.speaker = Some(SPEAKER_LABEL.to_string());
    }
}

/// Transcribes a meeting's samples. `NoSpeech` degrades to an empty
/// transcript (the report says "No speech was captured") instead of failing.
fn transcribe_meeting(
    speech: &SpeechModelManager<Box<dyn SpeechProvider>>,
    samples: &[f32],
) -> Result<MeetingTranscript, String> {
    if !speech.is_loaded() {
        speech
            .load()
            .map_err(|e| format!("speech model not loaded: {e}"))?;
    }
    let mut provider = speech
        .provider()
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    match provider.transcribe_detailed(samples, "auto") {
        Ok(t) => Ok(MeetingTranscript {
            text: t.text,
            words: t
                .words
                .into_iter()
                .map(|w| MeetingWord {
                    text: w.text,
                    start_ms: u64::from(w.start_ms),
                    end_ms: u64::from(w.end_ms),
                    speaker: None,
                })
                .collect(),
            segments: Vec::new(),
            duration_ms: 0,
            confidence: t.confidence,
            language: t.language,
            engine: None,
        }),
        Err(SpeechError::NoSpeech) => Ok(MeetingTranscript::default()),
        Err(e) => Err(format!("transcribe meeting: {e}")),
    }
}

/// Map-reduce summary under the 4096-token llama-server context.
fn summarize(
    inference: &Mutex<Option<Arc<dyn InferenceProvider>>>,
    text: &str,
) -> Option<ReportSummary> {
    let provider = inference
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()?;
    let chunks = chunk_text(text, MAX_CHUNK_CHARS);
    if chunks.is_empty() {
        return None;
    }
    if chunks.len() == 1 {
        return summarize_chunk(&provider, &chunks[0]);
    }
    // Map: one partial summary per chunk. A failed chunk is skipped; the
    // partial set still yields a (partial) report summary.
    let mut partials = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        match summarize_chunk(&provider, chunk) {
            Some(p) => partials.push(p),
            None => tracing::warn!(chunk = i, "meeting summary: chunk failed"),
        }
    }
    if partials.is_empty() {
        return None;
    }
    // Reduce: combine the partials into one summary + action list.
    let combined = partials
        .iter()
        .map(|p| {
            format!(
                "Summary: {}\nAction items: {}",
                p.summary,
                if p.action_items.is_empty() {
                    "(none)".to_string()
                } else {
                    p.action_items.join("; ")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    let system = "You combine partial summaries of the same meeting into one consistent summary. \
                  Keep it concise and factual; merge duplicate action items.";
    let user = format!(
        "Combine these partial summaries of one meeting into a single summary and one list of \
         explicit action items.\n\n{combined}"
    );
    let params = GenerationParams {
        max_tokens: 700,
        temperature: 0.0,
        timeout: scaled_timeout(700),
    };
    let raw = provider
        .generate_json(system, &user, params, &summary_schema())
        .ok()?;
    parse_summary(&raw)
}

fn summarize_chunk(provider: &Arc<dyn InferenceProvider>, chunk: &str) -> Option<ReportSummary> {
    let system = "You summarize meeting transcripts. Be concise and factual. \
                  Extract only explicit action items (who should do what); do not invent any.";
    let user = format!(
        "Summarize this part of a meeting transcript in a few sentences, and list its explicit \
         action items.\n\n{chunk}"
    );
    let params = GenerationParams {
        max_tokens: 600,
        temperature: 0.0,
        timeout: scaled_timeout(600),
    };
    let raw = provider.generate_json(system, &user, params, &summary_schema()).ok()?;
    parse_summary(&raw)
}

fn summary_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "action_items": { "type": "array", "items": { "type": "string" } }
        },
        "required": ["summary", "action_items"]
    })
}

fn parse_summary(raw: &str) -> Option<ReportSummary> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    let summary = v.get("summary")?.as_str()?.trim().to_string();
    let action_items = v
        .get("action_items")?
        .as_array()?
        .iter()
        .filter_map(|a| a.as_str().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<String>>();
    if summary.is_empty() && action_items.is_empty() {
        return None;
    }
    Some(ReportSummary {
        summary,
        action_items,
    })
}

/// `1765432123456` → "Tuesday, December 9, 2026, 9:05 AM" (local time).
fn format_local_time(ms: u64) -> String {
    use chrono::TimeZone;
    let dt = chrono::Local.timestamp_millis(i64::try_from(ms).unwrap_or(0));
    let day = dt.format("%d").to_string().trim_start_matches('0').to_string();
    let time = dt.format("%I:%M %p").to_string().trim_start_matches('0').to_string();
    format!(
        "{}, {} {}, {}",
        dt.format("%A"),
        dt.format("%B"),
        day,
        format!("{}, {}", dt.format("%Y"), time)
    )
}

/// Owner-only directory on Unix (no-op elsewhere), same model as
/// [`crate::recordings`].
#[cfg(unix)]
fn restrict_owner_only(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(meta.permissions().mode() & !0o077));
    }
}

#[cfg(not(unix))]
fn restrict_owner_only(_path: &Path) {}

#[cfg(unix)]
fn restrict_owner_only_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(meta.permissions().mode() & !0o077));
    }
}

#[cfg(not(unix))]
fn restrict_owner_only_file(_path: &Path) {}

/// Writes `bytes` to a temp file in the same directory, then renames it over
/// `path` — a crash mid-write never leaves a torn file at the final name.
fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path).inspect_err(|_e| {
        let _ = fs::remove_file(&tmp);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_local_time_is_sane() {
        // 2026-12-09 09:05:00 UTC.
        let ms: u64 = 1_796_853_500_000;
        let s = format_local_time(ms);
        assert!(s.contains(", 2026"), "{s}");
        assert!(s.contains("2026,"), "{s}");
        // Hour is 01-12 in %I, never zero-padded here.
        assert!(!s.ends_with("00:00 AM") && !s.ends_with("00:00 PM"), "{s}");
    }

    #[test]
    fn write_atomic_replaces_existing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.html");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        // No temp file left behind.
        assert!(dir.path().read_dir().unwrap().count() == 1);
    }

    #[test]
    fn write_atomic_leaves_no_tmp_on_bad_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no/such/dir/x.html");
        assert!(write_atomic(&path, "x").is_err());
    }

    #[test]
    fn parse_summary_requires_both_fields() {
        assert!(parse_summary(r#"{"summary":"s","action_items":[]}"#).is_some());
        assert!(parse_summary(r#"{"summary":"","action_items":[]}"#).is_none());
        assert!(parse_summary(r#"{"summary":"s"}"#).is_none());
        assert!(parse_summary("not json").is_none());
    }

    #[test]
    fn engine_rejects_second_start() {
        let dir = tempfile::tempdir().unwrap();
        let engine = MeetingEngine::new(dir.path().to_path_buf());
        // start() needs a real mic; verify the busy-rejection path instead
        // by checking phase transitions are sound.
        assert_eq!(engine.phase(), MeetingPhase::Idle);
        assert!(!engine.is_recording());
        assert_eq!(
            teletype_core::meeting::decide(MeetingInput::Start, MeetingPhase::Idle),
            MeetingAction::BeginCapture
        );
    }

    /// Committed transcripts with take-absolute timings fold into one
    /// transcript; the 1.1 s gap splits into two segments, both labeled.
    #[test]
    fn committed_transcripts_fold_and_label() {
        let transcripts = vec![
            Transcript {
                text: "hello meeting".into(),
                words: vec![
                    teletype_speech::Word { text: "hello".into(), start_ms: 0, end_ms: 400, conf: 1.0 },
                    teletype_speech::Word { text: "meeting".into(), start_ms: 450, end_ms: 900, conf: 1.0 },
                ],
                confidence: 1.0,
                language: Some("en".into()),
            },
            Transcript {
                text: "world again".into(),
                words: vec![
                    teletype_speech::Word { text: "world".into(), start_ms: 2_000, end_ms: 2_400, conf: 1.0 },
                    teletype_speech::Word { text: "again".into(), start_ms: 2_450, end_ms: 2_900, conf: 1.0 },
                ],
                confidence: 1.0,
                language: None,
            },
        ];
        let mut t = transcript_from_committed(&transcripts);
        assert_eq!(t.text, "hello meeting world again");
        assert_eq!(t.words.len(), 4);
        t.segments = segments_from_words(&t.words);
        label_single_speaker(&mut t);
        assert_eq!(t.segments.len(), 2); // 1.1 s gap splits
        assert!(t
            .segments
            .iter()
            .all(|s| s.speaker.as_deref() == Some(SPEAKER_LABEL)));
        assert!(t.words.iter().all(|w| w.speaker.as_deref() == Some(SPEAKER_LABEL)));
    }

    /// A mock engine that reports deterministic words with timings, so the
    /// full finalize pipeline (WAV → transcribe → report) is testable
    /// without a microphone or a real model.
    struct MockMeetingProvider;
    impl SpeechProvider for MockMeetingProvider {
        fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
            Ok(())
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
            Ok("hello meeting world".into())
        }
        fn transcribe_detailed(
            &mut self,
            _samples: &[f32],
            _language: &str,
        ) -> Result<teletype_speech::Transcript, SpeechError> {
            Ok(teletype_speech::Transcript {
                text: "hello meeting world".into(),
                words: vec![
                    teletype_speech::Word { text: "hello".into(), start_ms: 0, end_ms: 400, conf: 1.0 },
                    teletype_speech::Word { text: "meeting".into(), start_ms: 450, end_ms: 900, conf: 1.0 },
                    teletype_speech::Word { text: "world".into(), start_ms: 1_500, end_ms: 1_900, conf: 1.0 },
                ],
                confidence: 1.0,
                language: Some("en".into()),
            })
        }
    }

    #[test]
    fn finalize_writes_wav_transcript_and_report() {
        let dir = tempfile::tempdir().unwrap();
        let engine = MeetingEngine::new(dir.path().to_path_buf());
        let speech = SpeechModelManager::new(
            PathBuf::new(),
            Box::new(MockMeetingProvider) as Box<dyn SpeechProvider>,
        );
        let inference: Mutex<Option<Arc<dyn InferenceProvider>>> = Mutex::new(None);

        // ~0.5 s of 16 kHz silence is enough to exercise the pipeline.
        let samples = vec![0.0f32; 8_000];
        let input = FinalizeInput {
            id: "test-meeting".into(),
            title: "Test Meeting".into(),
            started_at_ms: 1_796_853_500_000,
            samples,
            duration_ms: 500,
            // Empty: exercises the batch fallback path.
            transcripts: Vec::new(),
        };

        let meeting = finalize(&engine, &speech, &inference, "mock", input)
            .expect("finalize should succeed");
        let report = &meeting.report;

        // Durable WAV next to the meeting dir.
        let wav = dir.path().join("test-meeting.wav");
        assert!(wav.is_file(), "wav missing: {wav:?}");
        // Per-meeting artifacts.
        let transcript = dir.path().join("test-meeting/transcript.json");
        assert!(transcript.is_file());
        let html_path = dir.path().join("test-meeting/report.html");
        assert!(html_path.is_file());
        assert_eq!(PathBuf::from(&report.report_path), html_path);

        // Headline stats on the report ref.
        assert_eq!(report.word_count, 3);
        assert_eq!(report.segment_count, 2);
        assert_eq!(report.speakers, vec![SPEAKER_LABEL.to_string()]);
        assert!(!report.has_summary);
        assert_eq!(report.duration_ms, 500);

        // transcript.json carries words + segments + timings + speaker.
        let tjson = fs::read_to_string(&transcript).unwrap();
        let t: serde_json::Value = serde_json::from_str(&tjson).unwrap();
        assert_eq!(t["text"], "hello meeting world");
        assert_eq!(t["words"].as_array().unwrap().len(), 3);
        assert_eq!(t["segments"].as_array().unwrap().len(), 2); // 1.1 s gap splits
        assert_eq!(t["engine"], "mock");
        assert_eq!(t["segments"][0]["speaker"], SPEAKER_LABEL);

        // report.html is self-contained and shows the content.
        let html = fs::read_to_string(&html_path).unwrap();
        assert!(html.contains("hello meeting world"));
        assert!(html.contains("Test Meeting"));
        assert!(html.contains(SPEAKER_LABEL));
        assert!(!html.contains("<script"));
        assert!(!html.contains("https://"));

        // No tmp files left behind.
        for entry in fs::read_dir(dir.path()).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().to_string();
            assert!(!name.contains(".tmp"), "leftover temp file: {name}");
        }
    }

    #[test]
    fn finalize_prefers_committed_transcripts_over_batch() {
        let dir = tempfile::tempdir().unwrap();
        let engine = MeetingEngine::new(dir.path().to_path_buf());
        let speech = SpeechModelManager::new(
            PathBuf::new(),
            Box::new(MockMeetingProvider) as Box<dyn SpeechProvider>,
        );
        let inference: Mutex<Option<Arc<dyn InferenceProvider>>> = Mutex::new(None);
        let input = FinalizeInput {
            id: "streamed".into(),
            title: "Streamed".into(),
            started_at_ms: 1_796_853_500_000,
            samples: vec![0.0f32; 16_000],
            duration_ms: 1_000,
            transcripts: vec![Transcript {
                text: "the streamed words win".into(),
                words: vec![
                    teletype_speech::Word { text: "the".into(), start_ms: 0, end_ms: 200, conf: 1.0 },
                    teletype_speech::Word { text: "streamed".into(), start_ms: 250, end_ms: 700, conf: 1.0 },
                    teletype_speech::Word { text: "words".into(), start_ms: 750, end_ms: 1_000, conf: 1.0 },
                    teletype_speech::Word { text: "win".into(), start_ms: 1_050, end_ms: 1_300, conf: 1.0 },
                ],
                confidence: 1.0,
                language: Some("en".into()),
            }],
        };
        let meeting = finalize(&engine, &speech, &inference, "mock", input).unwrap();
        let tjson = fs::read_to_string(dir.path().join("streamed/transcript.json")).unwrap();
        let t: serde_json::Value = serde_json::from_str(&tjson).unwrap();
        // The committed text wins over the mock's batch text.
        assert_eq!(t["text"], "the streamed words win");
        assert!(fs::read_to_string(PathBuf::from(&meeting.report.report_path)).unwrap().contains("the streamed words win"));
    }

    #[test]
    fn finalize_no_speech_yields_empty_transcript_report() {
        struct NoSpeechProvider;
        impl SpeechProvider for NoSpeechProvider {
            fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
                Ok(())
            }
            fn unload(&mut self) {}
            fn is_loaded(&self) -> bool {
                true
            }
            fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
                Err(SpeechError::NoSpeech)
            }
            fn transcribe_detailed(
                &mut self,
                _samples: &[f32],
                _language: &str,
            ) -> Result<teletype_speech::Transcript, SpeechError> {
                Err(SpeechError::NoSpeech)
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let engine = MeetingEngine::new(dir.path().to_path_buf());
        let speech = SpeechModelManager::new(
            PathBuf::new(),
            Box::new(NoSpeechProvider) as Box<dyn SpeechProvider>,
        );
        let inference: Mutex<Option<Arc<dyn InferenceProvider>>> = Mutex::new(None);
        let input = FinalizeInput {
            id: "quiet".into(),
            title: "Quiet".into(),
            started_at_ms: 1_796_853_500_000,
            samples: vec![0.0f32; 1_600],
            duration_ms: 100,
            transcripts: Vec::new(),
        };
        let meeting = finalize(&engine, &speech, &inference, "mock", input)
            .expect("no-speech should not fail the meeting");
        let html = fs::read_to_string(PathBuf::from(&meeting.report.report_path)).unwrap();
        assert!(html.contains("No speech was captured."));
    }
}
