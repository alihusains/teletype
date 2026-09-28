//! Voice activity detection for auto-stop.
//!
//! Wraps the pure-Rust Silero VAD engine (`silero-vad-pure`) with the
//! timing policy that decides *when to stop recording*: once speech has been
//! seen and then silence persists for `silence_duration`, the detector
//! reports that the utterance is over.
//!
//! Design contract:
//! - Pure logic + a stateless model call. No I/O, no threads, no platform
//!   code, so it lives in `teletype-core` and runs identically on macOS and
//!   Windows. The Mac uses the Accelerate/NEON fast path the crate ships;
//!   Windows uses the same CPU path. A faster backend can be swapped in later
//!   without touching the timing policy.
//! - The model is created once and reused across recordings (it is cheap to
//!   keep alive; creating it per-take would add a startup cost).
//! - The detector is fed 32 ms chunks (512 samples @ 16 kHz). Callers that
//!   receive audio in larger frames must split it into `chunk_size()`-sized
//!   pieces in arrival order.

use silero_vad_pure::{SampleRate, SileroVad};
use std::time::Duration;

/// Speech probability at or above this is "a person is speaking" for this
/// chunk. Silero's default operating point is 0.5.
const SPEECH_THRESHOLD: f32 = 0.5;

/// How long silence must persist, after speech has been seen, before the
/// detector reports the utterance finished.
const DEFAULT_SILENCE: Duration = Duration::from_millis(800);

/// A VAD engine that turns fixed-size f32 chunks into a speech probability.
///
/// `SileroVad` satisfies this directly; the trait exists so a faster backend
/// (e.g. a CoreML/Neural Engine runner) can be substituted on macOS without
/// changing the timing policy or its tests.
pub trait VadEngine: Send {
    /// Process one chunk of exactly `chunk_size()` samples in [-1, 1] and
    /// return the probability (0..=1) that speech is present.
    fn process(&mut self, chunk: &[f32]) -> f32;
    /// Samples per chunk at the engine's sample rate.
    fn chunk_size(&self) -> usize;
    /// Clear internal state so the next take starts fresh.
    fn reset(&mut self);
}

impl VadEngine for SileroVad {
    fn process(&mut self, chunk: &[f32]) -> f32 {
        // The engine is only ever fed `chunk_size()`-sized slices by the
        // detector, so a size mismatch here is a programming error.
        self.process(chunk).expect("VAD chunk size mismatch")
    }
    fn chunk_size(&self) -> usize {
        SileroVad::chunk_size(self)
    }
    fn reset(&mut self) {
        SileroVad::reset(self)
    }
}

/// One recording's worth of VAD state.
///
/// Feed it 32 ms chunks in arrival order via [`VadDetector::push`]; it reports
/// [`VadEvent::UtteranceComplete`] the first time silence outlasts the
/// configured duration *after* it has seen speech.
pub struct VadDetector {
    engine: Box<dyn VadEngine>,
    /// Silence that must follow speech before we stop.
    silence_duration: Duration,
    /// Samples per chunk (the engine's, at the engine's own rate).
    chunk_samples: usize,
    /// Sample rate the caller feeds frames at. Used to turn "silent chunks"
    /// into real wall-clock time. Defaults to 16 kHz (the engine's native
    /// rate); callers feeding audio at a different device rate MUST call
    /// `set_sample_rate` before the first `push_frame`, otherwise the stop
    /// timing is wrong (a 48 kHz Mac would need ~3x the configured pause).
    sample_rate: u32,
    /// Whether any speech has been seen yet this take.
    speech_seen: bool,
    /// Consecutive silent chunks since the last speech chunk.
    silent_chunks: u64,
    /// True once we have reported completion; `push` becomes a no-op after.
    fired: bool,
    /// Samples from a previous [`push_frame`] that did not fill a whole chunk.
    /// Live capture arrives in device-sized frames that rarely line up with the
    /// engine's 512-sample chunks, so the remainder is carried rather than
    /// dropped (dropping it would lose audio and skew the stop timing).
    pending: Vec<f32>,
}

/// What the detector reports to its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadEvent {
    /// Nothing to report yet; keep recording.
    Continue,
    /// Speech was seen, then silence outlasted the duration: stop and transcribe.
    UtteranceComplete,
}

impl VadDetector {
    /// Build a detector around the given engine.
    pub fn new(engine: Box<dyn VadEngine>) -> Self {
        let chunk_samples = engine.chunk_size();
        Self {
            engine,
            silence_duration: DEFAULT_SILENCE,
            chunk_samples,
            sample_rate: 16_000,
            speech_seen: false,
            silent_chunks: 0,
            fired: false,
            pending: Vec::new(),
        }
    }

    /// Convenience constructor for the built-in Silero engine at 16 kHz.
    pub fn silero_16k() -> Result<Self, String> {
        let engine = SileroVad::new(SampleRate::Hz16000)
            .map_err(|e| format!("VAD engine init failed: {e}"))?;
        Ok(Self::new(Box::new(engine)))
    }

    /// Override the silence duration (the "stop after a pause of N ms" slider).
    pub fn set_silence_duration(&mut self, d: Duration) {
        self.silence_duration = d;
    }

    /// Set the sample rate the caller will feed frames at. The engine always
    /// consumes its own fixed-size chunks, but the *duration* of each chunk in
    /// wall-clock time depends on the caller's rate. Call this before the
    /// first `push_frame` when the device rate is not 16 kHz.
    pub fn set_sample_rate(&mut self, hz: u32) {
        if hz > 0 {
            self.sample_rate = hz;
        }
    }

    pub fn silence_duration(&self) -> Duration {
        self.silence_duration
    }

    /// Clear per-take state so the same detector can be reused for the next
    /// recording. The underlying engine state is reset too.
    pub fn reset(&mut self) {
        self.speech_seen = false;
        self.silent_chunks = 0;
        self.fired = false;
        self.pending.clear();
        self.engine.reset();
    }

    /// Feed one 32 ms chunk. Returns the event for this chunk.
    ///
    /// The chunk MUST be exactly `chunk_size()` samples; the engine rejects
    /// any other length. Callers with larger frames must split first (see
    /// [`chunked`]).
    pub fn push(&mut self, chunk: &[f32]) -> VadEvent {
        if self.fired {
            return VadEvent::Continue;
        }
        let is_speech = self.engine.process(chunk) >= SPEECH_THRESHOLD;
        if is_speech {
            self.speech_seen = true;
            self.silent_chunks = 0;
            return VadEvent::Continue;
        }
        // Silent chunk.
        self.silent_chunks += 1;
        // We only stop on silence that *follows* speech. Leading silence
        // (before the user starts talking) must never trigger a stop.
        if self.speech_seen && self.silence_elapsed() >= self.silence_duration {
            self.fired = true;
            VadEvent::UtteranceComplete
        } else {
            VadEvent::Continue
        }
    }

    /// Feed an arbitrarily-sized frame, carrying any trailing partial chunk
    /// over to the next call so no samples are lost across callbacks.
    /// Returns the last event produced (so a caller that processes many chunks
    /// per callback can still react to a completion).
    pub fn push_frame(&mut self, frame: &[f32]) -> VadEvent {
        self.pending.extend_from_slice(frame);
        let mut last = VadEvent::Continue;
        while self.pending.len() >= self.chunk_samples {
            // Drain whole chunks from the front, leaving the remainder carried.
            let chunk: Vec<f32> = self.pending.drain(..self.chunk_samples).collect();
            last = self.push(&chunk);
            if self.fired {
                break;
            }
        }
        last
    }

    fn silence_elapsed(&self) -> Duration {
        // Each chunk is chunk_samples at the caller's rate. Derive wall-clock
        // time from the sample count and the actual feed rate so the stop
        // timing is correct regardless of device sample rate (a 48 kHz Mac
        // yields 3x the chunks per second of a 16 kHz one).
        let sr = self.sample_rate.max(1) as u64;
        Duration::from_micros((self.silent_chunks * (self.chunk_samples as u64) * 1_000_000) / sr)
    }
}

/// Yield in-order slices of at most `size` items. A trailing partial slice is
/// included; the detector's `push` expects full `chunk_size()` chunks, so
/// callers feeding the engine must only pass complete chunks (drop the final
/// partial, which is a few samples at the end of a take and irrelevant to
/// stop timing).
pub fn chunked(data: &[f32], size: usize) -> impl Iterator<Item = &[f32]> {
    assert!(size > 0, "chunk size must be non-zero");
    data.chunks(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake engine with a scriptable speech/silence pattern, so the timing
    /// policy is testable without the real model.
    struct Scripted {
        /// One entry per chunk: true = speech, false = silence.
        script: std::vec::IntoIter<bool>,
    }

    impl VadEngine for Scripted {
        fn process(&mut self, _chunk: &[f32]) -> f32 {
            match self.script.next() {
                Some(true) => 0.9,
                Some(false) => 0.01,
                // Script exhausted: treat as silence.
                None => 0.01,
            }
        }
        fn chunk_size(&self) -> usize {
            512
        }
        fn reset(&mut self) {}
    }

    // `to_vec()` is required: `Scripted` holds an owned `IntoIter<bool>`, so a
    // borrow of `script` cannot be stored. Clippy reads it as a needless copy.
    #[allow(clippy::unnecessary_to_owned)]
    fn detector_with(script: &[bool], silence_ms: u64) -> VadDetector {
        let mut d = VadDetector::new(Box::new(Scripted {
            // The field is an owned `IntoIter<bool>`, so a borrow cannot be
            // stored. Clippy suggests dropping the `to_vec`, which does not
            // compile here.
            script: script.to_vec().into_iter(),
        }));
        d.set_silence_duration(Duration::from_millis(silence_ms));
        d
    }

    fn feed(d: &mut VadDetector, n_chunks: usize) -> VadEvent {
        let mut last = VadEvent::Continue;
        for _ in 0..n_chunks {
            last = d.push(&[0.0; 512]);
            if d.fired {
                break;
            }
        }
        last
    }

    #[test]
    fn leading_silence_never_stops() {
        // 40 silent chunks before any speech: must keep recording.
        let mut d = detector_with(&[false; 40], 800);
        assert_eq!(feed(&mut d, 40), VadEvent::Continue);
        assert!(!d.fired);
    }

    #[test]
    fn stops_after_silence_following_speech() {
        // 10 speech chunks, then silence. 800 ms = 25 chunks of silence.
        let mut script = vec![true; 10];
        script.extend(std::iter::repeat_n(false, 40));
        let mut d = detector_with(&script, 800);
        // Feed the 10 speech chunks: no stop.
        assert_eq!(feed(&mut d, 10), VadEvent::Continue);
        // Then feed silence; it should fire at exactly 25 silent chunks.
        let mut fired_at: Option<usize> = None;
        for i in 0..40 {
            if d.push(&[0.0; 512]) == VadEvent::UtteranceComplete {
                fired_at = Some(i);
                break;
            }
        }
        assert_eq!(
            fired_at,
            Some(24),
            "800ms / 32ms = 25 chunks -> 0-based index 24"
        );
    }

    #[test]
    fn short_pause_between_words_does_not_stop() {
        // Speech, a 5-chunk (160 ms) pause, more speech, then a long silence.
        let mut script = vec![true; 5];
        script.extend(std::iter::repeat_n(false, 5)); // 160 ms pause
        script.extend(std::iter::repeat_n(true, 5));
        script.extend(std::iter::repeat_n(false, 40));
        let mut d = detector_with(&script, 800);
        let mut last = VadEvent::Continue;
        for _ in 0..script.len() {
            last = d.push(&[0.0; 512]);
            if d.fired {
                break;
            }
        }
        // The 160 ms mid-speech pause must NOT have fired; only the final
        // silence does. So it fires near the end, not in the middle.
        assert!(d.fired);
        assert_eq!(last, VadEvent::UtteranceComplete);
    }

    #[test]
    fn reset_rearms_the_detector() {
        let mut script = vec![true; 10];
        script.extend(std::iter::repeat_n(false, 40));
        let mut d = detector_with(&script, 800);
        feed(&mut d, 50);
        assert!(d.fired);
        d.reset();
        assert!(!d.fired);
        // After reset, leading silence again does not stop.
        assert_eq!(d.push(&[0.0; 512]), VadEvent::Continue);
    }

    #[test]
    fn push_frame_splits_and_fires() {
        // One big frame = 10 speech chunks + 30 silence chunks, fed as a
        // single frame (20480 samples = 40 full chunks, no partial).
        let mut script = vec![true; 10];
        script.extend(std::iter::repeat_n(false, 30));
        let mut d = detector_with(&script, 800);
        let frame = vec![0.0; 40 * 512];
        assert_eq!(d.push_frame(&frame), VadEvent::UtteranceComplete);
    }

    #[test]
    fn push_frame_carries_partial_chunks_across_calls() {
        // Frames that are not a multiple of 512 must neither panic nor drop
        // samples: the remainder carries to the next call.
        let mut script = vec![true; 3];
        script.extend(std::iter::repeat_n(false, 40));
        let mut d = detector_with(&script, 800);
        // 1000 = 512 + 488: one chunk now, 488 carried.
        assert_eq!(d.push_frame(&vec![0.0; 1000]), VadEvent::Continue);
        assert_eq!(d.pending.len(), 488);
        // 1000 more joins the carry: 488 + 1000 = 1488 = 2 chunks + 464.
        assert_eq!(d.push_frame(&vec![0.0; 1000]), VadEvent::Continue);
        assert_eq!(d.pending.len(), 464);
        // 488 more: 464 + 488 = 952 = 1 chunk + 440 carried.
        assert_eq!(d.push_frame(&vec![0.0; 488]), VadEvent::Continue);
        assert_eq!(d.pending.len(), 440);
        // Nothing was dropped: 2488 samples in = 4 chunks out + 440 carried.
        assert_eq!(4 * 512 + d.pending.len(), 1000 + 1000 + 488);
        // 440 + 100 = 540 fills one more chunk, leaving 28 carried.
        d.push_frame(&vec![0.0; 100]);
        assert_eq!(d.pending.len(), 28);
        // Reset drops the carry so the next take starts clean.
        d.reset();
        assert_eq!(d.pending.len(), 0);
    }

    #[test]
    fn push_frame_carried_samples_are_processed_in_order() {
        // Feed speech in misaligned frames; the detector must still see all
        // 3 speech chunks and then fire on the following silence.
        let mut script = vec![true; 3];
        script.extend(std::iter::repeat_n(false, 40));
        let mut d = detector_with(&script, 800);
        // 3 chunks of speech = 1536 samples, split as 500 + 500 + 536.
        assert_eq!(d.push_frame(&vec![0.0; 500]), VadEvent::Continue);
        assert_eq!(d.push_frame(&vec![0.0; 500]), VadEvent::Continue);
        assert_eq!(d.push_frame(&vec![0.0; 536]), VadEvent::Continue);
        assert!(
            d.speech_seen,
            "all three speech chunks should have been seen"
        );
        // Now silence; fire at 25 silent chunks (800 ms).
        let mut fired_at: Option<usize> = None;
        for i in 0..40 {
            if d.push_frame(&vec![0.0; 512]) == VadEvent::UtteranceComplete {
                fired_at = Some(i);
                break;
            }
        }
        assert_eq!(fired_at, Some(24));
    }

    #[test]
    fn chunked_yields_full_chunks_plus_trailing_partial() {
        let data: Vec<f32> = (0..10 * 512 + 100).map(|i| i as f32).collect();
        let chunks: Vec<_> = chunked(&data, 512).collect();
        // 10 full chunks + one 100-sample trailing partial.
        assert_eq!(chunks.len(), 11);
        assert!(chunks[..10].iter().all(|c| c.len() == 512));
        assert_eq!(chunks[10].len(), 100);
    }

    #[test]
    fn silence_timing_uses_actual_feed_rate_not_16k() {
        // Regression: the detector once hardcoded 16 kHz when converting
        // silent chunks to wall-clock time. On a 48 kHz device the same
        // 800 ms config would need ~2.4 s of real silence to fire. With the
        // rate set correctly, 800 ms of *real* silence at 48 kHz must fire.
        //
        // 48 kHz, 512-sample chunks -> 10.67 ms per chunk. 800 ms / 10.67 ms
        // ~= 75 silent chunks. (At the old 16 kHz assumption it would have
        // taken 25 chunks = only 266 ms of real time, i.e. fired early; the
        // user-visible symptom was the opposite because the device feeds 3x
        // the chunks per second, so 25 chunks arrived in ~266 ms of capture
        // but the timer thought they were 800 ms.)
        let mut script = vec![true; 5];
        script.extend(std::iter::repeat_n(false, 200));
        let mut d = detector_with(&script, 800);
        d.set_sample_rate(48_000);
        // Feed 5 speech chunks.
        for _ in 0..5 {
            assert_eq!(d.push(&[0.0; 512]), VadEvent::Continue);
        }
        // Now silence. Count how many 48 kHz chunks until it fires.
        let mut fired_at: Option<usize> = None;
        for i in 0..200 {
            if d.push(&[0.0; 512]) == VadEvent::UtteranceComplete {
                fired_at = Some(i);
                break;
            }
        }
        let at = fired_at.expect("should fire on 48 kHz silence");
        // 800 ms at 48 kHz = 3840 samples = 3840/512 = 7.5 chunks. But each
        // chunk is 10.67 ms, so 800 ms is 800/10.67 ~= 75 chunks. The detector
        // fires when silence_elapsed (silent_chunks * chunk_ms) >= 800 ms,
        // i.e. at silent_chunks = ceil(800 / 10.67) = 75 (0-based index 74).
        // At a 16 kHz assumption the same 800 ms would be 25 chunks and fire
        // at index 24 -> far too early.
        assert!(
            (73..=76).contains(&at),
            "fired at chunk {at}; expected ~74 at 48 kHz (would be 24 at 16 kHz)"
        );
    }

    #[test]
    fn real_engine_runs_and_is_in_range() {
        // Smoke test the actual Silero engine: it must init and return
        // probabilities in range for both a tone (speech-like) and silence.
        let mut d = VadDetector::silero_16k().expect("silero init");
        let chunk = d.engine.chunk_size();
        let tone: Vec<f32> = (0..chunk)
            .map(|i| (i as f32 * (2.0 * std::f32::consts::PI * 300.0 / 16000.0)).sin() * 0.25)
            .collect();
        let silent = vec![0.0004; chunk];
        let p_speech = d.engine.process(&tone);
        let p_silence = d.engine.process(&silent);
        assert!((0.0..=1.0).contains(&p_speech));
        assert!((0.0..=1.0).contains(&p_silence));
        // A 300 Hz tone at 0.25 amplitude should score higher than near-silence.
        assert!(
            p_speech > p_silence,
            "tone {p_speech} should exceed silence {p_silence}"
        );
    }

    #[test]
    fn real_engine_stays_under_10ms_per_chunk() {
        // Roadmap P2.2 gate: VAD must be << 10 ms per frame so auto-stop adds
        // no perceptible latency. One 32 ms chunk of a 300 Hz tone, averaged
        // over 100 chunks (first run includes any lazy state).
        let mut d = VadDetector::silero_16k().expect("silero init");
        let chunk = d.engine.chunk_size();
        let tone: Vec<f32> = (0..chunk)
            .map(|i| (i as f32 * (2.0 * std::f32::consts::PI * 300.0 / 16000.0)).sin() * 0.25)
            .collect();
        let started = std::time::Instant::now();
        for _ in 0..100 {
            d.engine.process(&tone);
        }
        let per_chunk_ms = started.elapsed().as_micros() as f64 / 100.0 / 1000.0;
        assert!(
            per_chunk_ms < 10.0,
            "VAD took {per_chunk_ms:.2} ms per 32 ms chunk; budget is 10 ms"
        );
    }
}
