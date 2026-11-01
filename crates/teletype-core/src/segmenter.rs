//! Segmenter: split long audio into commit-able segments while the user
//! is still talking.
//!
//! Input: 16 kHz frames (32 ms hops) plus per-frame Silero VAD
//! probabilities (0..=1). Output: [`Segment`] ranges when speech is
//! followed by `commit_pause_ms` of silence, or when the segment reaches
//! `max_segment_secs` (cut at lowest-energy 20 ms frame within the last
//! 2 s, with 0.4 s overlap into the next segment).
//!
//! Pure logic — no I/O, no threads, no platform code. Sample indices are
//! relative to take start; the caller slices audio accordingly.

use std::collections::VecDeque;

/// One segment ready for transcription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    /// Start sample (inclusive) relative to take start.
    pub start_sample: usize,
    /// End sample (exclusive) — includes any 0.4 s overlap tail.
    pub end_sample: usize,
    /// Where the next segment starts (overlap boundary). Equal to
    /// `end_sample` when no overlap was applied.
    pub next_start_sample: usize,
}

/// Events produced by the segmenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentEvent {
    /// A segment is ready to be transcribed.
    Ready(Segment),
}

/// Configuration for the segmenter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmenterConfig {
    /// Silence duration after speech that triggers a segment commit (ms).
    pub commit_pause_ms: u32,
    /// Maximum segment length before a forced cut (seconds).
    pub max_segment_secs: u32,
}

impl Default for SegmenterConfig {
    fn default() -> Self {
        Self {
            commit_pause_ms: 350,
            max_segment_secs: 12,
        }
    }
}

/// State of the segmenter state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Waiting for speech to begin.
    Idle,
    /// Speech is active; counting silence since last speech frame.
    Speaking,
}

/// Frame energy sample for cut-point selection.
struct FrameEnergy {
    sample_offset: usize,
    rms: f32,
}

/// Splits long audio into commit-able segments.
pub struct Segmenter {
    config: SegmenterConfig,
    sample_rate: u32,
    /// Absolute sample index of the next frame to process.
    next_sample: usize,
    /// Sample index where the current segment began.
    segment_start: usize,
    /// State machine.
    state: State,
    /// Consecutive silent samples since last speech frame.
    silence_samples: usize,
    /// Frame energies for the last 2 seconds (for max-length cut).
    frame_energies: VecDeque<FrameEnergy>,
    /// Pending events.
    events: Vec<SegmentEvent>,
}

impl Segmenter {
    pub fn new(config: SegmenterConfig) -> Self {
        Self::with_rate(config, 16_000)
    }

    pub fn with_rate(config: SegmenterConfig, sample_rate: u32) -> Self {
        Self {
            config,
            sample_rate,
            next_sample: 0,
            segment_start: 0,
            state: State::Idle,
            silence_samples: 0,
            frame_energies: VecDeque::new(),
            events: Vec::new(),
        }
    }

    /// Feed one 32 ms frame of audio with its VAD probability (0..=1).
    /// Returns segment events produced by this frame.
    pub fn push(&mut self, frame: &[f32], vad_prob: f32) -> Vec<SegmentEvent> {
        self.events.clear();
        let frame_start = self.next_sample;
        self.next_sample += frame.len();

        // Track frame energy for max-length cut decisions.
        let rms = if frame.is_empty() {
            0.0
        } else {
            (frame.iter().map(|&s| s * s).sum::<f32>() / frame.len() as f32).sqrt()
        };
        self.frame_energies.push_back(FrameEnergy {
            sample_offset: frame_start,
            rms,
        });
        // Prune energies older than 2 seconds.
        let cutoff = self
            .next_sample
            .saturating_sub(self.sample_rate as usize * 2);
        while let Some(front) = self.frame_energies.front() {
            if front.sample_offset < cutoff {
                self.frame_energies.pop_front();
            } else {
                break;
            }
        }

        let is_speech = vad_prob >= 0.5;

        match self.state {
            State::Idle => {
                if is_speech {
                    self.state = State::Speaking;
                    self.segment_start = frame_start;
                    self.silence_samples = 0;
                }
            }
            State::Speaking => {
                if is_speech {
                    self.silence_samples = 0;
                } else {
                    self.silence_samples += frame.len();
                    let pause_samples = (self.config.commit_pause_ms as f64
                        * self.sample_rate as f64
                        / 1000.0) as usize;
                    if self.silence_samples >= pause_samples {
                        // Commit at the start of the silence. Skip past all
                        // silence samples so the next segment starts fresh.
                        let commit_point = self.next_sample - self.silence_samples;
                        let segment = Segment {
                            start_sample: self.segment_start,
                            end_sample: commit_point,
                            next_start_sample: commit_point,
                        };
                        self.events.push(SegmentEvent::Ready(segment));
                        self.segment_start = self.next_sample;
                        self.silence_samples = 0;
                        self.state = State::Idle;
                    }
                }
            }
        }

        // Check for max-length forced cut.
        let max_samples = (self.config.max_segment_secs as u64 * self.sample_rate as u64) as usize;
        if self.next_sample - self.segment_start >= max_samples {
            let cut_point = self.find_best_cut_point();
            let overlap_samples = (self.sample_rate as f64 * 0.4 / 1000.0) as usize;
            let segment_end = (cut_point + overlap_samples).min(self.next_sample);
            let segment = Segment {
                start_sample: self.segment_start,
                end_sample: segment_end,
                next_start_sample: cut_point,
            };
            self.events.push(SegmentEvent::Ready(segment));
            // Next segment starts at the cut point; samples from cut_point
            // onward are already buffered and belong to it. Keep Speaking so
            // that silence after the cut still counts toward the next pause.
            self.segment_start = cut_point;
            self.silence_samples = 0;
        }

        self.events.clone()
    }

    /// Find the lowest-energy 20 ms frame within the last 2 seconds.
    fn find_best_cut_point(&self) -> usize {
        let frame_20ms = (self.sample_rate as f64 * 0.02) as usize;
        if frame_20ms == 0 {
            return self.segment_start;
        }

        let search_start = self.segment_start;
        let search_end = self.next_sample.saturating_sub(frame_20ms);

        if search_end <= search_start {
            return self.segment_start;
        }

        let mut best_offset = search_start;
        let mut best_energy = f32::MAX;

        for fe in &self.frame_energies {
            if fe.sample_offset < search_start {
                continue;
            }
            if fe.sample_offset + frame_20ms > self.next_sample {
                break;
            }
            if fe.rms < best_energy {
                best_energy = fe.rms;
                best_offset = fe.sample_offset;
            }
        }

        best_offset
    }

    /// Flush any remaining audio as a final segment.
    pub fn flush(&mut self) -> Option<Segment> {
        if self.next_sample <= self.segment_start {
            return None;
        }
        let segment = Segment {
            start_sample: self.segment_start,
            end_sample: self.next_sample,
            next_start_sample: self.next_sample,
        };
        self.segment_start = self.next_sample;
        self.silence_samples = 0;
        self.state = State::Idle;
        self.frame_energies.clear();
        Some(segment)
    }

    /// Number of samples in the current (uncommitted) segment.
    pub fn current_segment_samples(&self) -> usize {
        self.next_sample - self.segment_start
    }

    /// Drain pending events.
    pub fn pending_events(&mut self) -> Vec<SegmentEvent> {
        let events = self.events.clone();
        self.events.clear();
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn silence(len: usize) -> Vec<f32> {
        vec![0.0; len]
    }

    fn speech(len: usize) -> Vec<f32> {
        vec![0.5; len]
    }

    #[test]
    fn pause_after_speech_emits_segment() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        // 1 s speech, then 500 ms silence (> 350 ms threshold at 16 kHz).
        let events = seg.push(&speech(16000), 0.9);
        assert!(events.is_empty(), "no segment during speech");
        let events = seg.push(&silence(8000), 0.0); // 500 ms silence
        assert_eq!(events.len(), 1);
        match events[0] {
            SegmentEvent::Ready(s) => {
                assert_eq!(s.start_sample, 0);
                assert_eq!(s.end_sample, 16000); // cut at silence start
                assert_eq!(s.next_start_sample, 16000);
            }
        }
    }

    #[test]
    fn short_pause_does_not_split() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        // 1 s speech, then 300 ms silence (< 350 ms threshold).
        seg.push(&speech(16000), 0.9);
        let events = seg.push(&silence(4800), 0.0); // 300 ms
        assert!(events.is_empty(), "300 ms pause should not split");
    }

    #[test]
    fn max_length_cut_with_overlap() {
        let mut seg = Segmenter::new(SegmenterConfig {
            commit_pause_ms: 350,
            max_segment_secs: 2, // 2 seconds for fast test
        });
        // First second: normal speech
        let events = seg.push(&speech(16000), 0.9);
        assert!(events.is_empty(), "no cut at 1 s");
        // Second second: very low energy (good cut point) — triggers cut at 2 s
        let events = seg.push(&vec![0.001; 16000], 0.9);
        assert!(!events.is_empty(), "max length should trigger cut at 2 s");
        match &events[0] {
            SegmentEvent::Ready(s) => {
                assert_eq!(s.start_sample, 0);
                assert!(s.next_start_sample > 0, "cut point should be > 0");
                assert!(
                    s.end_sample > s.next_start_sample,
                    "overlap should extend past cut point"
                );
                assert!(s.end_sample <= 32000, "end should not exceed 2 s + overlap");
            }
        }
    }

    #[test]
    fn flush_returns_remaining() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        seg.push(&speech(8000), 0.9); // 500 ms
        let segment = seg.flush().expect("flush should return segment");
        assert_eq!(segment.start_sample, 0);
        assert_eq!(segment.end_sample, 8000);
        assert_eq!(segment.next_start_sample, 8000);
    }

    #[test]
    fn flush_empty_returns_none() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        assert!(seg.flush().is_none());
    }

    #[test]
    fn multiple_segments_in_order() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        let mut all_events = Vec::new();
        // Three utterances separated by long pauses.
        for _ in 0..3 {
            all_events.extend(seg.push(&speech(16000), 0.9));
            all_events.extend(seg.push(&silence(8000), 0.0));
        }
        assert_eq!(all_events.len(), 3);
        // Segments should be non-overlapping and in order.
        for (i, ev) in all_events.iter().enumerate() {
            match ev {
                SegmentEvent::Ready(s) => {
                    assert_eq!(s.start_sample, i * 24000, "segment {i} start");
                    assert_eq!(s.end_sample, i * 24000 + 16000, "segment {i} end");
                }
            }
        }
    }

    #[test]
    fn current_segment_samples_tracks_buffer() {
        let mut seg = Segmenter::new(SegmenterConfig::default());
        assert_eq!(seg.current_segment_samples(), 0);
        seg.push(&speech(8000), 0.9);
        assert_eq!(seg.current_segment_samples(), 8000);
        seg.push(&silence(8000), 0.0); // triggers commit at 350ms silence
        assert_eq!(seg.current_segment_samples(), 0);
    }

    /// Feeds 400 frames of 20 ms each (8 s total), each frame quieter than
    /// the one before, so the newest eligible frame is always the
    /// lowest-energy cut point. This is the case a "cut at the newest frame"
    /// fallback livelocks on: it would re-emit a 6-sample segment every
    /// frame and never advance the segment start.
    fn quieting_frames(frames: usize) -> Vec<Vec<f32>> {
        (0..frames)
            .map(|i| vec![1.0 / (i as f32 + 1.0); 320])
            .collect()
    }

    #[test]
    fn forced_cut_advances_when_newest_frame_is_quietest() {
        let mut seg = Segmenter::new(SegmenterConfig {
            commit_pause_ms: 350,
            max_segment_secs: 1,
        });
        let mut prev_start: Option<usize> = None;
        let mut ready = 0;
        for frame in quieting_frames(400) {
            for ev in seg.push(&frame, 0.9) {
                match ev {
                    SegmentEvent::Ready(s) => {
                        ready += 1;
                        let len = s.end_sample - s.start_sample;
                        assert!(
                            len >= 320,
                            "a committed segment must be at least one frame long; got {len} samples"
                        );
                        if let Some(p) = prev_start {
                            assert!(
                                s.start_sample > p,
                                "every cut must advance the segment start; {p} -> {}",
                                s.start_sample
                            );
                        }
                        prev_start = Some(s.start_sample);
                    }
                }
            }
        }
        assert!(
            ready >= 3,
            "a 1 s max segment over 8 s of speech must cut repeatedly; got {ready}"
        );
    }

    #[test]
    fn forced_cut_covers_large_frame_feed() {
        // One frame larger than max_segment_secs: no 20 ms frame exists
        // inside the segment, so the cut falls back to the segment start
        // (find_best_cut_point's initialisation). The deleted guard also
        // fired here and livelocked: it re-emitted a 6-sample segment every
        // frame and never advanced the start. With the guard gone, the cut
        // still advances the start over multiple frames (the second frame
        // has 20 ms frames in the segment, so the cut picks one and the
        // start finally moves), so the feed is covered.
        let mut seg = Segmenter::new(SegmenterConfig {
            commit_pause_ms: 350,
            max_segment_secs: 1,
        });
        let mut first_start: Option<usize> = None;
        let mut last_start: Option<usize> = None;
        let mut ready = 0;
        // Feed 4 oversized frames (8 s total, 4x the 1 s max).
        for _ in 0..4 {
            for SegmentEvent::Ready(s) in seg.push(&vec![0.5f32; 32_000], 0.9) {
                ready += 1;
                if first_start.is_none() {
                    first_start = Some(s.start_sample);
                }
                last_start = Some(s.start_sample);
            }
        }
        assert!(
            ready >= 2,
            "oversized frames must still cut repeatedly; got {ready}"
        );
        // The start must have advanced: the deleted guard kept it stuck at
        // 0 forever (351 cuts, 350 non-advancing starts on the probe).
        assert!(
            last_start.unwrap() > first_start.unwrap(),
            "the segment start must advance over multiple frames; first {:?} last {:?}",
            first_start,
            last_start
        );
        // The tail must hold audio (the feed is covered, not lost).
        let tail = seg.flush().expect("the tail must flush");
        assert!(
            tail.end_sample - tail.start_sample > 0,
            "the flushed tail must hold audio"
        );
    }

    #[test]
    fn forced_cut_covers_most_of_the_take() {
        let mut seg = Segmenter::new(SegmenterConfig {
            commit_pause_ms: 350,
            max_segment_secs: 1,
        });
        let mut committed = 0usize;
        for frame in quieting_frames(400) {
            for SegmentEvent::Ready(s) in seg.push(&frame, 0.9) {
                committed += s.end_sample - s.start_sample;
            }
        }
        let total = 400 * 320;
        // The uncommitted tail is at most one max segment (1 s = 16 000
        // samples); everything else must be covered by a commit.
        assert!(
            committed > total - 16_000,
            "the cuts must cover the take; committed {committed} of {total} samples"
        );
    }
}
