//! Meeting recording: session state machine, transcript model, and
//! context-window chunking.
//!
//! A meeting is a long-form recording (minutes to hours), unlike a dictation
//! take (seconds). The desktop app drives the state machine from the meeting
//! engine (`teletype-desktop::meeting`); this module keeps the decision logic
//! and the data model platform-independent so both are testable without a
//! microphone.
//!
//! Speaker attribution is a first-class field (`speaker: Option<String>`) on
//! every word and segment: the baseline engine leaves it `None` (single
//! speaker), and the parakeet.cpp diarization path (macOS) fills it in
//! without any schema change.

use serde::{Deserialize, Serialize};

/// Phases of a meeting session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum MeetingPhase {
    #[default]
    Idle,
    Recording,
    /// Capture stopped; transcription and report generation are running.
    Finalizing,
    /// The report is on disk.
    Done,
}

impl MeetingPhase {
    pub fn is_busy(self) -> bool {
        matches!(self, MeetingPhase::Recording | MeetingPhase::Finalizing)
    }
}

/// User inputs to the meeting session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingInput {
    Start,
    Stop,
    /// Stop and discard: no transcription, no report.
    Cancel,
    /// Finalization (transcription + report) finished.
    Finalized,
    /// Return to idle after the report has been seen.
    Reset,
}

/// What the engine should do in response to an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingAction {
    Nothing,
    BeginCapture,
    /// Stop capture and run finalization (transcription, report).
    EndCapture,
    /// Stop capture and discard the audio.
    Discard,
    /// Publish the finished report.
    MarkDone,
    GoIdle,
}

/// Decides what `input` does in `phase`. Pure, like the dictation machine in
/// [`crate::state`].
pub fn decide(input: MeetingInput, phase: MeetingPhase) -> MeetingAction {
    match (input, phase) {
        (MeetingInput::Start, MeetingPhase::Idle) => MeetingAction::BeginCapture,
        (MeetingInput::Stop, MeetingPhase::Recording) => MeetingAction::EndCapture,
        (MeetingInput::Cancel, MeetingPhase::Recording) => MeetingAction::Discard,
        (MeetingInput::Finalized, MeetingPhase::Finalizing) => MeetingAction::MarkDone,
        (MeetingInput::Reset, MeetingPhase::Done) => MeetingAction::GoIdle,
        _ => MeetingAction::Nothing,
    }
}

/// One spoken word with its timing. `speaker` is `None` until diarization
/// runs (the single-speaker baseline).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingWord {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// A run of words spoken without a long pause — the report's line unit and
/// the diarization turn boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// The transcript of a finished meeting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MeetingTranscript {
    pub text: String,
    pub words: Vec<MeetingWord>,
    pub segments: Vec<MeetingSegment>,
    pub duration_ms: u64,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The engine that produced it (report provenance).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<String>,
}

/// A pause longer than this starts a new segment.
pub const SEGMENT_GAP_MS: u64 = 1_000;

/// Groups words into segments on pauses longer than [`SEGMENT_GAP_MS`].
/// Words without timings (engines without word detail) form one segment.
pub fn segments_from_words(words: &[MeetingWord]) -> Vec<MeetingSegment> {
    if words.is_empty() {
        return Vec::new();
    }
    let has_timings = words.iter().all(|w| w.end_ms > w.start_ms || w.start_ms > 0);
    if !has_timings {
        let text = words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if text.trim().is_empty() {
            return Vec::new();
        }
        return vec![MeetingSegment {
            start_ms: 0,
            end_ms: 0,
            text,
            speaker: words[0].speaker.clone(),
        }];
    }
    let mut segments = Vec::new();
    let mut current: Vec<&MeetingWord> = Vec::new();
    let push = |segments: &mut Vec<MeetingSegment>, current: &mut Vec<&MeetingWord>| {
        if current.is_empty() {
            return;
        }
        let text = current
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        segments.push(MeetingSegment {
            start_ms: current.first().copied().map(|w| w.start_ms).unwrap_or(0),
            end_ms: current.last().copied().map(|w| w.end_ms).unwrap_or(0),
            text,
            speaker: current.first().copied().and_then(|w| w.speaker.clone()),
        });
        current.clear();
    };
    for word in words {
        if let Some(prev) = current.last() {
            let gap = word.start_ms.saturating_sub(prev.end_ms);
            if gap > SEGMENT_GAP_MS {
                push(&mut segments, &mut current);
            }
        }
        current.push(word);
    }
    push(&mut segments, &mut current);
    segments
}

/// Character budget per map-reduce chunk. The local llama-server runs with a
/// 4096-token context; at ~2.7 chars/token (English) that is ~11k chars,
/// kept conservative so prompt + schema + summary output still fit.
pub const MAX_CHUNK_CHARS: usize = 11_000;

/// Splits `text` into chunks of at most `max_chars`, breaking only on word
/// boundaries. Empty input yields no chunks.
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return if text.trim().is_empty() { Vec::new() } else { vec![text.trim().to_string()] };
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in text.split_inclusive(' ') {
        if word.len() > max_chars {
            // A single word longer than the budget: flush the pending buffer,
            // then give the word its own char-safe chunks.
            if !current.trim().is_empty() {
                chunks.push(current.trim_end().to_string());
                current.clear();
            }
            let chars: Vec<char> = word.chars().collect();
            for piece in chars.chunks(max_chars) {
                let s: String = piece.iter().collect();
                if !s.trim().is_empty() {
                    chunks.push(s.trim_end().to_string());
                }
            }
            continue;
        }
        if current.len() + word.len() > max_chars && !current.trim().is_empty() {
            chunks.push(current.trim_end().to_string());
            current.clear();
        }
        current.push_str(word);
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim_end().to_string());
    }
    chunks
}

/// `95_000` → "1:35"; `3_723_000` → "1:02:03".
pub fn format_clock(ms: u64) -> String {
    let total_secs = ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    if mins < 60 {
        format!("{}:{:02}", mins, secs)
    } else {
        format!("{}:{:02}:{:02}", mins / 60, mins % 60, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start_ms: u64, end_ms: u64) -> MeetingWord {
        MeetingWord {
            text: text.into(),
            start_ms,
            end_ms,
            speaker: None,
        }
    }

    #[test]
    fn state_machine_happy_path() {
        assert_eq!(
            decide(MeetingInput::Start, MeetingPhase::Idle),
            MeetingAction::BeginCapture
        );
        assert_eq!(
            decide(MeetingInput::Stop, MeetingPhase::Recording),
            MeetingAction::EndCapture
        );
        assert_eq!(
            decide(MeetingInput::Finalized, MeetingPhase::Finalizing),
            MeetingAction::MarkDone
        );
        assert_eq!(
            decide(MeetingInput::Reset, MeetingPhase::Done),
            MeetingAction::GoIdle
        );
    }

    #[test]
    fn state_machine_rejects_invalid_transitions() {
        assert_eq!(
            decide(MeetingInput::Start, MeetingPhase::Recording),
            MeetingAction::Nothing
        );
        assert_eq!(
            decide(MeetingInput::Stop, MeetingPhase::Idle),
            MeetingAction::Nothing
        );
        assert_eq!(
            decide(MeetingInput::Start, MeetingPhase::Finalizing),
            MeetingAction::Nothing
        );
        assert_eq!(
            decide(MeetingInput::Cancel, MeetingPhase::Done),
            MeetingAction::Nothing
        );
    }

    #[test]
    fn cancel_discards_a_recording() {
        assert_eq!(
            decide(MeetingInput::Cancel, MeetingPhase::Recording),
            MeetingAction::Discard
        );
    }

    #[test]
    fn segments_split_on_long_gaps() {
        let words = vec![
            word("hello", 0, 400),
            word("world", 450, 900),
            // 1.5 s gap -> new segment
            word("second", 2_400, 2_900),
            word("part", 2_950, 3_300),
        ];
        let segments = segments_from_words(&words);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].text, "hello world");
        assert_eq!(segments[0].start_ms, 0);
        assert_eq!(segments[0].end_ms, 900);
        assert_eq!(segments[1].text, "second part");
        assert_eq!(segments[1].start_ms, 2_400);
        assert_eq!(segments[1].end_ms, 3_300);
    }

    #[test]
    fn segments_keep_speakers_when_present() {
        let mut a = word("hi", 0, 300);
        a.speaker = Some("Alice".into());
        let mut b = word("hello", 1_500, 1_900);
        b.speaker = Some("Bob".into());
        let segments = segments_from_words(&[a, b]);
        assert_eq!(segments[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(segments[1].speaker.as_deref(), Some("Bob"));
    }

    #[test]
    fn untimed_words_form_one_segment() {
        let words = vec![word("a", 0, 0), word("b", 0, 0)];
        let segments = segments_from_words(&words);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "a b");
    }

    #[test]
    fn empty_words_yield_no_segments() {
        assert!(segments_from_words(&[]).is_empty());
    }

    #[test]
    fn chunk_text_respects_word_boundaries() {
        let text = "one two three four five six";
        let chunks = chunk_text(text, 11);
        assert!(chunks.iter().all(|c| c.len() <= 11));
        assert_eq!(chunks.join(" "), text);
    }

    #[test]
    fn chunk_text_handles_long_words_and_empty() {
        assert!(chunk_text("", 100).is_empty());
        let long = "x".repeat(250);
        let chunks = chunk_text(&long, 100);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), 250);
    }

    #[test]
    fn chunk_text_default_budget_splits_a_meeting() {
        // ~30 minutes of speech at ~15 words/minute, ~6 chars/word.
        let text = (0..2_700).map(|i| format!("w{i} ")).collect::<String>();
        let chunks = chunk_text(&text, MAX_CHUNK_CHARS);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.len() <= MAX_CHUNK_CHARS));
    }

    #[test]
    fn format_clock_shapes() {
        assert_eq!(format_clock(0), "0:00");
        assert_eq!(format_clock(95_000), "1:35");
        assert_eq!(format_clock(59_999), "0:59");
        assert_eq!(format_clock(3_723_000), "1:02:03");
    }

    #[test]
    fn transcript_roundtrips_through_json() {
        let t = MeetingTranscript {
            text: "hello world".into(),
            words: vec![word("hello", 0, 400)],
            segments: vec![MeetingSegment {
                start_ms: 0,
                end_ms: 400,
                text: "hello world".into(),
                speaker: Some("Alice".into()),
            }],
            duration_ms: 400,
            confidence: 1.0,
            language: Some("en".into()),
            engine: Some("sherpa".into()),
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: MeetingTranscript = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
        // A speaker-less word serializes without the field (skip_serializing_if).
        let w = word("hi", 0, 100);
        let wjson = serde_json::to_string(&w).unwrap();
        assert!(!wjson.contains("speaker"));
    }
}
