//! Committer: transcribe segments while the user is still talking, so that
//! on release only the tail remains.
//!
//! A worker thread owns a speech engine handle (a second
//! [`SpeechModelManager`] — plan §4.2 "second engine handle") and processes
//! a priority queue of jobs: segments are FIFO, but the final tail jumps
//! the queue. Overlapped segments are de-duplicated by timestamp
//! (words from the later segment whose start is before the previous
//! segment's last word end are dropped).
//!
//! The Committer keeps [`Transcript`]s it has committed internally and
//! exposes them via [`Committer::committed_texts`] and
//! [`Committer::preview_snapshot`] for the pill live-preview.
//!
//! No platform code — pure Rust + `teletype-speech`.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

/// P4.3 (plan §4.3): the user-facing setting "Polish whole text at the end
/// (slower, more consistent)" is a Wave B unit (single-owner rule keeps this
/// task away from `commands.rs` / `Settings`). When `true`, the controller
/// skips the per-segment polish loop and polishes the assembled text once
/// after release, exactly like today's single-pass pipeline.
// Wave B: becomes Settings.polish_whole_text_at_end
pub const POLISH_WHOLE_TEXT_AT_END_DEFAULT: bool = false;

use teletype_inference::manager::SpeechModelManager;
use teletype_speech::{SpeechProvider, Transcript};

/// A transcription job for the worker thread.
enum Job {
    /// Transcribe a segment, processed FIFO. `start_ms` is the segment's
    /// offset in the take: providers return word timings relative to the
    /// audio they were given, and the release fold compares them across
    /// segments, so every job's words are shifted to take-absolute time.
    Segment {
        samples: Vec<f32>,
        language: String,
        start_ms: u32,
        result_tx: mpsc::Sender<Result<Transcript, String>>,
    },
    /// Transcribe the final tail — jumped to the front of the queue.
    Tail {
        samples: Vec<f32>,
        language: String,
        start_ms: u32,
        result_tx: mpsc::Sender<Result<Transcript, String>>,
    },
    /// P4 completion: load the provider's model at `path`. Jumped to the
    /// front of the queue (like `Tail`) so it runs BEFORE any segment that
    /// was submitted while the take started; segments then decode against
    /// the freshly loaded engine. A failed load is harmless: segments that
    /// follow simply fail per-result exactly as an unloaded engine does
    /// today, `committed_count()` stays 0, and the take falls back to the
    /// whole-take decode byte-identically.
    Load { path: std::path::PathBuf },
}

/// Shift a transcript's word timings into take-absolute time.
fn shift_words(transcript: &mut Transcript, start_ms: u32) {
    if start_ms == 0 {
        return;
    }
    for w in &mut transcript.words {
        w.start_ms = w.start_ms.saturating_add(start_ms);
        w.end_ms = w.end_ms.saturating_add(start_ms);
    }
}

/// Merge two transcripts, de-duplicating overlapping words.
///
/// Both transcripts must carry TAKE-ABSOLUTE timings: a provider returns
/// word times relative to the audio it was handed, so the committer worker
/// shifts each job's words by its take offset before this fold compares
/// them. Without the shift, a later segment's early words look older than
/// the previous segment's last word and are dropped as overlap, so the fold
/// silently keeps only the first segment's text.
///
/// For overlapped segments (produced by the [`Segmenter`](crate::...::segmenter::Segmenter)),
/// words from `next` whose `start_ms` is less than `prev`'s last word
/// `end_ms` are dropped, since they overlap the previous segment's tail.
pub fn merge_transcripts(prev: &Transcript, next: Transcript) -> Transcript {
    if prev.words.is_empty() {
        return next;
    }
    if next.words.is_empty() {
        return prev.clone();
    }

    let last_word_end = prev.words.last().unwrap().end_ms;

    let mut words = prev.words.clone();
    let mut text_parts: Vec<String> = prev
        .text
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    for word in &next.words {
        if word.start_ms < last_word_end {
            continue; // skip overlapping word
        }
        words.push(word.clone());
        text_parts.push(word.text.clone());
    }

    Transcript {
        text: text_parts.join(" "),
        words,
        confidence: next.confidence,
        language: next.language.clone(),
    }
}

/// A polish function: takes one committed segment's raw text and returns
/// the polished text. Runs on the committer worker thread (never on the
/// controller). Any error falls back to the raw text: the pipeline never
/// loses the user's words (root convention).
type PolishFn = Box<dyn Fn(&str) -> Result<String, String> + Send>;

/// The Committer: transcribes segments while the user is still talking.
pub struct Committer {
    tx: mpsc::Sender<Job>,
    /// Segments completed so far, in commit order (segment id).
    committed: Mutex<Vec<(usize, Transcript)>>,
    /// Pending segment result receivers, keyed by segment id.
    pending: Mutex<HashMap<usize, mpsc::Receiver<Result<Transcript, String>>>>,
    next_id: Mutex<usize>,
    /// Segments submitted so far this take. Counts the Segmenter's cut
    /// decision, not decodes drained by `poll_completed`. The release
    /// path uses it to decide whether the streaming fold can run at all,
    /// and it must not depend on a poller that may only run after the
    /// decision.
    submitted: Mutex<usize>,
    worker: Option<thread::JoinHandle<()>>,
    /// P4.3: per-segment polish, applied to each segment as it commits
    /// during recording. `None` when the take is shorter than
    /// [`TOTAL_AUDIO_SEGMENTED_POLISH_MS`] or when the user prefers
    /// [`POLISH_WHOLE_TEXT_AT_END_DEFAULT`]-style whole-text polish.
    polish: Mutex<Option<PolishFn>>,
}

/// P4.3 (plan §4.3): takes at least this long get the per-segment polish
/// loop during recording; shorter takes keep the single whole-text polish
/// after release.
pub const TOTAL_AUDIO_SEGMENTED_POLISH_MS: u64 = 20_000;

impl Committer {
    /// Create a new Committer with its own speech engine handle.
    ///
    /// The provider is wrapped in a [`SpeechModelManager`] so load/unload
    /// and the idle-unload timer work the same way as the main dictation
    /// path. For FluidAudio and sherpa-onnx the underlying engine is
    /// process-global, so a second handle is cheap.
    pub fn new(provider: Box<dyn SpeechProvider>) -> Self {
        let manager = SpeechModelManager::new(std::path::PathBuf::new(), provider);
        let manager = Arc::new(manager);
        let (tx, rx) = mpsc::channel::<Job>();
        let worker = thread::Builder::new()
            .name("teletype-committer".into())
            .spawn(move || {
                Self::worker_loop(rx, manager);
            })
            .expect("failed to spawn committer worker");
        Self {
            tx,
            committed: Mutex::new(Vec::new()),
            pending: Mutex::new(HashMap::new()),
            next_id: Mutex::new(0),
            submitted: Mutex::new(0),
            worker: Some(worker),
            polish: Mutex::new(None),
        }
    }

    /// P4.3: install the per-segment polish function. The controller calls
    /// this when the take's total audio is ≥
    /// [`TOTAL_AUDIO_SEGMENTED_POLISH_MS`] and whole-text-at-end polish is
    /// off. The fn runs on the worker thread; a failure leaves the segment
    /// as its raw text (never blocks the take).
    pub fn set_polish_fn(&self, f: impl Fn(&str) -> Result<String, String> + Send + 'static) {
        *self.polish.lock().unwrap() = Some(Box::new(f));
    }

    /// P4 completion: enqueue the provider's model load on the worker
    /// thread, at `path`. The load jumps the queue (like `Tail`) so it
    /// runs BEFORE any segment submitted while the take started; those
    /// segments then decode against the freshly loaded engine.
    ///
    /// Background by design: the caller (the take-start path) never blocks
    /// on it. A failed load is harmless — segments then fail per-result
    /// exactly as an unloaded engine does today, `committed_count()` stays
    /// 0, and the take falls back to the whole-take decode byte-identically.
    /// No panic, no poisoned state; `finish()` still works after a failed
    /// load (its tail decodes and fails gracefully, and the take falls
    /// back the same way).
    pub fn load_model(&self, path: std::path::PathBuf) {
        let _ = self.tx.send(Job::Load { path });
    }

    fn worker_loop(
        rx: mpsc::Receiver<Job>,
        manager: Arc<SpeechModelManager<Box<dyn SpeechProvider>>>,
    ) {
        let mut queue: Vec<Job> = Vec::new();

        loop {
            // Drain the channel into the priority queue. `Load` and `Tail`
            // jump to the front (the load must run before any segment that
            // was queued while the take started); segments are FIFO.
            while let Ok(job) = rx.try_recv() {
                match &job {
                    Job::Tail { .. } | Job::Load { .. } => queue.insert(0, job),
                    Job::Segment { .. } => queue.push(job),
                }
            }

            // Process one job if available.
            if !queue.is_empty() {
                let job = queue.remove(0);
                match job {
                    Job::Segment {
                        samples,
                        language,
                        start_ms,
                        result_tx,
                    } => {
                        let mut p = manager.provider().lock().unwrap();
                        let result = p
                            .transcribe_detailed(&samples, &language)
                            .map(|mut t| {
                                shift_words(&mut t, start_ms);
                                t
                            })
                            .map_err(|e| e.to_string());
                        let _ = result_tx.send(result);
                    }
                    Job::Tail {
                        samples,
                        language,
                        start_ms,
                        result_tx,
                    } => {
                        let mut p = manager.provider().lock().unwrap();
                        let result = p
                            .transcribe_detailed(&samples, &language)
                            .map(|mut t| {
                                shift_words(&mut t, start_ms);
                                t
                            })
                            .map_err(|e| e.to_string());
                        let _ = result_tx.send(result);
                    }
                    // Load the engine in place on the worker thread. The
                    // manager's `load()` is a no-op if already loaded, so a
                    // re-take's second load is cheap. A failure is logged
                    // and swallowed: the engine stays unloaded, and the
                    // segments that follow fail per-result exactly as they
                    // would today (graceful fallback, no panic, no poison).
                    Job::Load { path } => {
                        manager.set_model_path(path.clone());
                        match manager.load() {
                            Ok(()) => crate::log_entry(
                                crate::LogLevel::Success,
                                format!("committer engine loaded: {}", path.display()),
                            ),
                            Err(e) => crate::log_entry(
                                crate::LogLevel::Error,
                                format!(
                                    "committer model load failed ({}): {e}; segments fall back to the whole-take decode",
                                    path.display()
                                ),
                            ),
                        }
                    }
                }
            } else {
                // Queue empty: block waiting for the next job.
                match rx.recv() {
                    Ok(job) => match &job {
                        Job::Tail { .. } | Job::Load { .. } => queue.insert(0, job),
                        Job::Segment { .. } => queue.push(job),
                    },
                    Err(_) => break, // channel closed: worker exiting
                }
            }
        }
    }

    /// Submit a segment for transcription. `start_ms` is the segment's
    /// offset in the take (word timings are shifted to take-absolute time).
    /// Returns a segment id.
    ///
    /// The segment is transcribed on the worker thread. Use
    /// [`Committer::poll_completed`] to collect finished results, or
    /// [`Committer::finish`] to wait for everything.
    pub fn submit_segment(&self, samples: Vec<f32>, language: String, start_ms: u32) -> usize {
        let id = {
            let mut n = self.next_id.lock().unwrap();
            let id = *n;
            *n += 1;
            id
        };
        *self.submitted.lock().unwrap() += 1;
        let (result_tx, result_rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, result_rx);
        let _ = self.tx.send(Job::Segment {
            samples,
            language,
            start_ms,
            result_tx,
        });
        id
    }

    /// Check for newly completed segment transcriptions and add them to
    /// the committed list. Call this periodically (e.g. each interim loop
    /// tick) so the pill preview stays fresh.
    ///
    /// P4.3: when a polish function is installed, each freshly committed
    /// segment is polished here (on the caller's thread — the pill loop
    /// tick in production, the test thread in tests). A polish failure
    /// falls back to that segment's raw text: the pipeline never loses the
    /// user's words and never blocks the take.
    pub fn poll_completed(&self) {
        let mut pending = self.pending.lock().unwrap();
        let mut completed: Vec<(usize, Transcript)> = Vec::new();
        for (id, rx) in pending.iter_mut() {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(transcript) => {
                        completed.push((*id, self.polish_segment(*id, transcript)));
                    }
                    Err(e) => {
                        tracing::warn!("committer segment {id} failed: {e}");
                    }
                }
            }
        }
        drop(pending);
        let mut committed = self.committed.lock().unwrap();
        for (id, transcript) in completed {
            self.pending.lock().unwrap().remove(&id);
            committed.push((id, transcript));
        }
        committed.sort_by_key(|(id, _)| *id);
    }

    /// P4.3: apply the installed per-segment polish to one transcript. A
    /// failure (or no polish fn) keeps the raw text: the pipeline never
    /// loses the user's words.
    fn polish_segment(&self, id: usize, mut transcript: Transcript) -> Transcript {
        if let Some(polish) = self.polish.lock().unwrap().as_ref() {
            match polish(&transcript.text) {
                Ok(polished) => transcript.text = polished,
                Err(e) => {
                    tracing::warn!("committer segment {id} polish failed, keeping raw text: {e}");
                }
            }
        }
        transcript
    }

    /// Block until every submitted segment has produced a result, adding
    /// each to `committed` in arrival order (the caller sorts by id).
    ///
    /// Required at release: `Job::Tail` jumps the queue, so segments that
    /// were still queued behind it decode AFTER the tail. Returning once
    /// the tail is in without waiting for them would drop their words.
    /// The wait is bounded by the remaining decode time, which the take
    /// must pay anyway to keep every word.
    fn wait_pending(&self) {
        loop {
            let next = {
                let mut pending = self.pending.lock().unwrap();
                match pending.keys().next().copied() {
                    Some(id) => pending.remove(&id).map(|rx| (id, rx)),
                    None => None,
                }
            };
            let Some((id, rx)) = next else { break };
            match rx.recv() {
                Ok(Ok(transcript)) => {
                    let transcript = self.polish_segment(id, transcript);
                    self.committed.lock().unwrap().push((id, transcript));
                }
                Ok(Err(e)) => tracing::warn!("committer segment {id} failed: {e}"),
                Err(_) => {
                    // The worker exited (channel closed): no more results
                    // can arrive for the remaining ids either.
                    self.pending.lock().unwrap().clear();
                    break;
                }
            }
        }
        self.committed.lock().unwrap().sort_by_key(|(id, _)| *id);
    }

    /// Transcribed text of all committed segments, joined with spaces.
    /// With P4.3 per-segment polish active this is the polished text.
    pub fn committed_texts(&self) -> Vec<String> {
        self.committed
            .lock()
            .unwrap()
            .iter()
            .map(|(_, t)| t.text.clone())
            .collect()
    }

    /// Snapshot for the pill live-preview: `(committed_text, volatile_text)`.
    ///
    /// `committed_text` is the joined text of all committed segments (never
    /// changes once committed). `volatile_text` is empty here — the caller
    /// (dictation worker) is responsible for the cheap interim decode of the
    /// uncommitted tail.
    pub fn preview_snapshot(&self) -> (String, String) {
        let committed = self.committed.lock().unwrap();
        let committed_text = committed
            .iter()
            .map(|(_, t)| t.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        (committed_text, String::new())
    }

    /// Wait for the tail transcription and all previously submitted
    /// segments, then return all transcripts in commit order followed by
    /// the tail. After this call, the Committer is reset for the next take.
    ///
    /// `tail_start_ms` is the tail's offset in the take (see
    /// [`Committer::submit_segment`]).
    pub fn finish(
        &self,
        samples: Vec<f32>,
        language: String,
        tail_start_ms: u32,
    ) -> Result<Vec<Transcript>, String> {
        // Collect any segments that completed while we were waiting.
        self.poll_completed();

        // Send the tail job — it jumps the queue ahead of any remaining
        // segments.
        let (result_tx, result_rx) = mpsc::channel();
        self.tx
            .send(Job::Tail {
                samples,
                language,
                start_ms: tail_start_ms,
                result_tx,
            })
            .map_err(|e| e.to_string())?;

        // Wait for the tail.
        let tail = result_rx.recv().map_err(|e| e.to_string())??;

        // The queue jump means tail-first, not everything-first: segments
        // still queued behind the tail decode after it. Wait for them, or
        // their words are lost from the take.
        self.wait_pending();

        // Build the final list: committed segments in id order, then the
        // tail.
        let mut all: Vec<Transcript> = {
            let committed = self.committed.lock().unwrap();
            committed.iter().map(|(_, t)| t.clone()).collect()
        };
        all.push(tail);

        // Reset for the next take.
        {
            let mut committed = self.committed.lock().unwrap();
            committed.clear();
        }
        {
            let mut pending = self.pending.lock().unwrap();
            pending.clear();
        }
        let mut n = self.next_id.lock().unwrap();
        *n = 0;
        drop(n);
        *self.submitted.lock().unwrap() = 0;

        Ok(all)
    }

    /// Number of segments committed so far this take.
    pub fn committed_count(&self) -> usize {
        self.committed.lock().unwrap().len()
    }

    /// Number of segments *submitted* so far this take.
    ///
    /// This is the Segmenter's cut decision, so it is already ≥ 1 by the
    /// time the release path runs, unlike [`Committer::committed_count`],
    /// which only counts decodes that `poll_completed` has drained and is
    /// therefore still 0 at release when nothing polled mid-take.
    pub fn submitted_count(&self) -> usize {
        *self.submitted.lock().unwrap()
    }
}

/// P4.3 (plan §4.3) release-time "join" pass, rules only — no LLM:
/// capitalize boundaries between committed segments and continue lists —
/// a segment that starts with its own `a.`/`2)` marker keeps it (its label
/// letter is capitalized as the boundary); a plain-word start inside a live
/// list gets the next marker (`a.` → `b.`, `1.` → `2.`); every word that
/// directly follows a list marker starts capitalized.
///
/// Pure and deterministic: `join_segments(&["first sentence", "second sentence"])`
/// == `"First sentence Second sentence"`.
pub fn join_segments(segments: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    // Continuation marker for the current list context (e.g. `"b."`), set
    // whenever a list marker was present in the previous output.
    let mut carry: Option<String> = None;

    for raw in segments {
        let seg = raw.trim();
        if seg.is_empty() {
            continue;
        }
        let mut tokens: Vec<String> = seg.split_whitespace().map(str::to_string).collect();

        // A plain-word start inside a live list continues the enumeration.
        let starts_with_own_marker = parse_list_marker(&tokens[0]).is_some();
        let inserted = !starts_with_own_marker && carry.is_some();
        if inserted {
            tokens.insert(0, carry.clone().unwrap_or_default());
        }

        // Capitalize after markers; capitalize the first token as the
        // boundary unless it is an inserted continuation marker.
        let mut last_marker: Option<ListMarker> = None;
        let mut after_marker = false;
        for (i, tok) in tokens.iter_mut().enumerate() {
            if let Some(m) = parse_list_marker(tok) {
                if i == 0 && !inserted {
                    *tok = capitalize_first_alpha(tok);
                }
                last_marker = Some(m);
                after_marker = true;
                continue;
            }
            if i == 0 || after_marker {
                *tok = capitalize_first_alpha(tok);
            }
            after_marker = false;
        }

        carry = last_marker.and_then(|m| m.next());
        out.push(tokens.join(" "));
    }

    out.join(" ")
}

/// A parsed list marker: `a.` / `A.` / `2)` / `12.`.
struct ListMarker {
    label: String,
    sep: char,
}

impl ListMarker {
    /// The next marker in the same style: `a.` → `b.`, `2)` → `3)`.
    /// No continuation past `z`/`Z` or beyond two-digit labels.
    fn next(&self) -> Option<String> {
        if self.label.chars().all(|c| c.is_ascii_digit()) {
            let n: u64 = self.label.parse().ok()?;
            return Some(format!("{}{}", n + 1, self.sep));
        }
        let c = self.label.chars().next()?;
        if self.label.len() == 1 && c.is_ascii_alphabetic() && c != 'z' && c != 'Z' {
            let next = (c as u8 + 1) as char;
            return Some(format!("{next}{}", self.sep));
        }
        None
    }
}

/// A list marker token: a 1–2 character label (all digits, or a single
/// letter) followed by `.` or `)` — "a.", "A.", "2)", "12.".
fn parse_list_marker(token: &str) -> Option<ListMarker> {
    let b = token.as_bytes();
    if !(2..=3).contains(&b.len()) {
        return None;
    }
    let sep = *b.last()?;
    if sep != b'.' && sep != b')' {
        return None;
    }
    let label = &token[..token.len() - 1];
    if label.is_empty() || !label.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let all_digit = label.chars().all(|c| c.is_ascii_digit());
    let single_alpha = label.len() == 1 && label.chars().all(|c| c.is_ascii_alphabetic());
    if !all_digit && !single_alpha {
        return None; // mixed / two-letter labels have no continuation style
    }
    Some(ListMarker {
        label: label.to_string(),
        sep: sep as char,
    })
}

/// Uppercase the first alphabetic character of `s` (digits/punctuation are
/// left alone; unicode uppercase expansions like `ß` → `SS` are preserved).
fn capitalize_first_alpha(s: &str) -> String {
    let mut done = false;
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if !done && c.is_alphabetic() {
            done = true;
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The tail audio in ms the Segmenter's uncommitted remainder holds — i.e.
/// what the controller must decode after release. Exposed for the trace's
/// `tail_audio_ms`.
pub fn tail_audio_ms(uncommitted_samples: usize, sample_rate: u32) -> u32 {
    if sample_rate == 0 {
        return 0;
    }
    (uncommitted_samples as u64 * 1000 / sample_rate as u64) as u32
}

impl Drop for Committer {
    fn drop(&mut self) {
        // Close the sender side of the channel so the worker's `rx.recv()`
        // returns `Err` and the worker loop exits, then join it.
        let (_new_tx, _new_rx) = mpsc::channel();
        let old_tx = std::mem::replace(&mut self.tx, _new_tx);
        drop(old_tx);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    use teletype_speech::{MockSpeechProvider, Word};

    /// True on a headless CI runner (GitHub Actions sets `CI=true`). Same
    /// predicate as `teletype_core`'s `autotext::placeholders::is_headless`.
    /// Used here only to choose the release-latency bound: CI's OS
    /// thread-scheduling jitter must not flake the timing assertion.
    fn is_ci_runner() -> bool {
        matches!(std::env::var("CI"), Ok(v) if !v.is_empty() && v != "false")
    }

    #[test]
    fn merge_transcripts_dedupe_overlap() {
        let prev = Transcript {
            text: "hello world".to_string(),
            words: vec![
                Word {
                    text: "hello".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    conf: 0.9,
                },
                Word {
                    text: "world".to_string(),
                    start_ms: 500,
                    end_ms: 1000,
                    conf: 0.9,
                },
            ],
            confidence: 0.9,
            language: Some("en".to_string()),
        };
        let next = Transcript {
            text: "world of teletype".to_string(),
            words: vec![
                Word {
                    text: "world".to_string(),
                    start_ms: 800,
                    end_ms: 1300,
                    conf: 0.9,
                },
                Word {
                    text: "of".to_string(),
                    start_ms: 1300,
                    end_ms: 1600,
                    conf: 0.9,
                },
                Word {
                    text: "teletype".to_string(),
                    start_ms: 1600,
                    end_ms: 2400,
                    conf: 0.9,
                },
            ],
            confidence: 0.9,
            language: Some("en".to_string()),
        };

        let merged = merge_transcripts(&prev, next);
        assert_eq!(merged.text, "hello world of teletype");
        assert_eq!(merged.words.len(), 4);
        assert_eq!(merged.words[0].text, "hello");
        assert_eq!(merged.words[1].text, "world");
        assert_eq!(merged.words[1].start_ms, 500);
        assert_eq!(merged.words[2].text, "of");
        assert_eq!(merged.words[3].text, "teletype");
    }

    #[test]
    fn merge_transcripts_no_overlap() {
        let prev = Transcript {
            text: "hello".to_string(),
            words: vec![Word {
                text: "hello".to_string(),
                start_ms: 0,
                end_ms: 500,
                conf: 0.9,
            }],
            confidence: 0.9,
            language: None,
        };
        let next = Transcript {
            text: "world".to_string(),
            words: vec![Word {
                text: "world".to_string(),
                start_ms: 1000,
                end_ms: 1500,
                conf: 0.9,
            }],
            confidence: 0.9,
            language: None,
        };

        let merged = merge_transcripts(&prev, next);
        assert_eq!(merged.text, "hello world");
        assert_eq!(merged.words.len(), 2);
    }

    #[test]
    fn merge_transcripts_empty_prev() {
        let prev = Transcript {
            text: String::new(),
            words: vec![],
            confidence: 0.0,
            language: None,
        };
        let next = Transcript {
            text: "hello world".to_string(),
            words: vec![
                Word {
                    text: "hello".to_string(),
                    start_ms: 0,
                    end_ms: 500,
                    conf: 0.9,
                },
                Word {
                    text: "world".to_string(),
                    start_ms: 500,
                    end_ms: 1000,
                    conf: 0.9,
                },
            ],
            confidence: 0.9,
            language: None,
        };

        let merged = merge_transcripts(&prev, next);
        assert_eq!(merged.text, "hello world");
        assert_eq!(merged.words.len(), 2);
    }

    #[test]
    fn submit_and_poll_completes_segment() {
        let provider = MockSpeechProvider {
            transcript: "hello world".to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = Committer::new(Box::new(provider));

        let id = committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        assert_eq!(id, 0);

        // Poll until the worker completes the segment. A bounded wait, not a
        // sleep: on a fast machine the worker can finish before the first
        // poll, so the contract under test is "polling completes the
        // segment", never "the first poll necessarily sees an incomplete
        // store". The deadline keeps a hung worker failing, not looping.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            committer.poll_completed();
            if committer.committed_count() == 1 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "segment not committed within 2 s"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(committer.committed_texts(), vec!["hello world"]);
    }

    #[test]
    fn finish_returns_tail_after_segments() {
        let provider = MockSpeechProvider {
            transcript: "segment text".to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = Committer::new(Box::new(provider));

        // Submit a segment
        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        std::thread::sleep(Duration::from_millis(50));
        committer.poll_completed();
        assert_eq!(committer.committed_count(), 1);

        // Finish with tail
        let result = committer.finish(vec![0.2; 800], "en".to_string(), 100);
        assert!(result.is_ok());
        let transcripts = result.unwrap();
        assert_eq!(transcripts.len(), 2);
        assert_eq!(transcripts[0].text, "segment text");
        assert_eq!(transcripts[1].text, "segment text");

        // Committed should be reset
        assert_eq!(committer.committed_count(), 0);
    }

    #[test]
    fn preview_snapshot() {
        let provider = MockSpeechProvider {
            transcript: "committed text".to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = Committer::new(Box::new(provider));

        // Initially empty
        let (ct, vt) = committer.preview_snapshot();
        assert_eq!(ct, "");
        assert_eq!(vt, "");

        // After a segment completes
        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        std::thread::sleep(Duration::from_millis(50));
        committer.poll_completed();
        let (ct, vt) = committer.preview_snapshot();
        assert_eq!(ct, "committed text");
        assert_eq!(vt, "");
    }

    // ------------------------------------------------------------------
    // P4.3: per-segment polish during recording.
    // ------------------------------------------------------------------

    /// P4.3: with a polish fn installed, each committed segment is
    /// polished as it lands; `committed_texts` exposes the polished text.
    #[test]
    fn per_segment_polish_applies_to_committed_segments() {
        let provider = MockSpeechProvider {
            transcript: "this is raw text".to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = Committer::new(Box::new(provider));
        committer.set_polish_fn(|text| Ok(format!("[polished {text}]")));

        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        std::thread::sleep(Duration::from_millis(50));
        committer.poll_completed();
        assert_eq!(
            committer.committed_texts(),
            vec!["[polished this is raw text]"],
            "committed text must be the polished segment"
        );
    }

    /// P4.3 failure behavior: a per-segment polish failure falls back to
    /// that segment's raw text and never blocks the take.
    #[test]
    fn per_segment_polish_failure_keeps_raw_text() {
        let provider = MockSpeechProvider {
            transcript: "keep me raw".to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = Committer::new(Box::new(provider));
        committer.set_polish_fn(|_| Err("polish down".to_string()));

        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        std::thread::sleep(Duration::from_millis(50));
        committer.poll_completed();
        assert_eq!(committer.committed_texts(), vec!["keep me raw"]);
    }

    // ------------------------------------------------------------------
    // P4.3: the rules-only join pass (release-time, no LLM).
    // ------------------------------------------------------------------

    #[test]
    fn join_segments_capitalizes_boundaries() {
        let joined = join_segments(&[
            "first sentence done".to_string(),
            "second sentence starts here".to_string(),
            "and a third".to_string(),
        ]);
        assert_eq!(
            joined,
            "First sentence done Second sentence starts here And a third"
        );
    }

    #[test]
    fn join_segments_continues_lists() {
        let joined = join_segments(&["todo a. first item".to_string(), "second item".to_string()]);
        assert_eq!(joined, "Todo a. First item b. Second item");

        let joined = join_segments(&["step 1. boot".to_string(), "step two".to_string()]);
        assert_eq!(joined, "Step 1. Boot 2. Step two");
    }

    #[test]
    fn join_segments_keeps_literal_marker_start() {
        // A segment that already starts with its own marker is not doubled.
        let joined = join_segments(&["a. first item".to_string(), "2) already marked".to_string()]);
        assert_eq!(joined, "A. First item 2) Already marked");
    }

    #[test]
    fn join_segments_empty_and_blank() {
        assert_eq!(join_segments(&[]), "");
        assert_eq!(
            join_segments(&["".to_string(), "  ".to_string(), "x".to_string()]),
            "X"
        );
    }

    // ------------------------------------------------------------------
    // P4.4: WER parity unit gate.
    //
    // On a synthetic corpus (10 sentences fed as alternating speech/
    // silence so the Segmenter cuts mid-take), the assembled
    // committed-segments output equals the whole-utterance decode output
    // byte-for-byte. A transcript whose words are exactly the take's
    // words — the idealized whole-utterance decode — is the reference.
    // ------------------------------------------------------------------

    fn transcript_from_words(text: &str) -> Transcript {
        let words: Vec<Word> = text
            .split_whitespace()
            .enumerate()
            .map(|(i, w)| Word {
                text: w.to_string(),
                start_ms: (i * 400) as u32,
                end_ms: (i * 400 + 350) as u32,
                conf: 0.9,
            })
            .collect();
        Transcript {
            text: text.to_string(),
            words,
            confidence: 0.9,
            language: Some("en".to_string()),
        }
    }

    #[test]
    fn wer_parity_assembled_segments_equal_whole_utterance() {
        // 10 sentences; the ideal engine returns exactly the take's words
        // with timings, so the whole-utterance decode is the reference.
        let sentences: Vec<String> = (1..=10)
            .map(|i| format!("sentence number {i} about the teletype pipeline"))
            .collect();
        let whole: String = sentences.join(" ");
        // The ideal engine's words: each sentence's words sit inside that
        // sentence's 1 s speech window (50 ms in, 130 ms apart, 120 ms long),
        // so a commit window (which ends at each speech end) never splits a
        // word.
        let mut reference_words: Vec<Word> = Vec::new();
        for (i, sentence) in sentences.iter().enumerate() {
            for (k, w) in sentence.split_whitespace().enumerate() {
                let start_ms = (i * 1500 + 50 + k * 130) as u32;
                reference_words.push(Word {
                    text: w.to_string(),
                    start_ms,
                    end_ms: start_ms + 120,
                    conf: 0.9,
                });
            }
        }
        let reference = Transcript {
            text: whole.clone(),
            words: reference_words,
            confidence: 0.9,
            language: Some("en".to_string()),
        };

        // Feed the take to the Segmenter as 1 s speech + 500 ms silence
        // per sentence; the committed segments cover the take minus the
        // final 500 ms of trailing silence (the tail, flushed on release).
        let mut seg = teletype_core::segmenter::Segmenter::new(
            teletype_core::segmenter::SegmenterConfig::default(),
        );
        let mut boundaries: Vec<usize> = Vec::new();
        for _ in &sentences {
            for s in seg.push(&vec![0.5; 16000], 0.9) {
                /* 1 s speech */
                match s {
                    teletype_core::segmenter::SegmentEvent::Ready(s) => {
                        boundaries.push(s.end_sample)
                    }
                }
            }
            for s in seg.push(&vec![0.0; 8000], 0.0) {
                /* 500 ms silence */
                match s {
                    teletype_core::segmenter::SegmentEvent::Ready(s) => {
                        boundaries.push(s.end_sample)
                    }
                }
            }
        }
        let tail = seg.flush(); // None when the take ended exactly at a commit
        assert!(boundaries.len() >= 3, "segmenter must cut mid-take");

        // The ideal engine transcribes each committed segment as exactly
        // the words that fell in its window (word i lives in [i*400ms,
        // i*400+350ms)); the tail gets the remaining words. Merge across
        // boundaries must reproduce the whole-utterance text byte-for-byte.
        let total_words = reference.words.len();
        let mut cursor = 0usize;
        let mut prev: Option<Transcript> = None;
        for &end_sample in &boundaries {
            let end_ms = end_sample as u64 * 1000 / 16_000;
            let mut next_words = Vec::new();
            while cursor < total_words {
                let w = &reference.words[cursor];
                if w.start_ms as u64 >= end_ms {
                    break;
                }
                next_words.push(w.clone());
                cursor += 1;
            }
            let next = Transcript {
                text: next_words
                    .iter()
                    .map(|w| w.text.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
                words: next_words,
                confidence: 0.9,
                language: Some("en".to_string()),
            };
            prev = Some(match prev {
                None => next,
                Some(p) => merge_transcripts(&p, next),
            });
        }
        // The tail: the words the commit windows did not consume (empty when
        // the segmenter consumed the whole take — a take ending inside a
        // pause has nothing left to decode).
        let tail_words: Vec<Word> = if tail.is_some() {
            reference.words[cursor..].to_vec()
        } else {
            Vec::new()
        };
        let tail_transcript = Transcript {
            text: tail_words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" "),
            words: tail_words,
            confidence: 0.9,
            language: Some("en".to_string()),
        };
        let assembled = match prev {
            None => tail_transcript,
            Some(p) => merge_transcripts(&p, tail_transcript),
        };
        assert_eq!(
            assembled.text, reference.text,
            "assembled committed-segments output must equal the whole-utterance decode byte-for-byte"
        );
        // Tail must be small relative to the take (the point of P4.3).
        let tail_ms = tail
            .as_ref()
            .map(|t| ((t.end_sample - t.start_sample) as u64 * 1000 / 16_000) as u32)
            .unwrap_or(0);
        assert!(
            tail_ms <= 1000,
            "tail is one trailing silence: {tail_ms} ms"
        );
    }

    /// U6: the parity guard above, extended to the forced-cut shape.
    ///
    /// The original test's take is cut only at 500 ms pauses — no segment
    /// ever reaches `max_segment_secs`, so it never exercises the
    /// lowest-energy 20 ms cut point with its 0.4 s overlap. This one feeds
    /// a continuous 30 s take (speech the whole time, no pause) so the
    /// Segmenter must cut twice at the 12 s mark, and proves the release
    /// fold still reproduces the whole-utterance decode byte-for-byte when
    /// segment boundaries are forced cuts: the overlap words belong to the
    /// LATER segment (the earlier segment's words stop at the cut point), so
    /// the merge keeps exactly one copy of each word.
    #[test]
    fn wer_parity_continuous_take_forced_cuts_equal_whole_utterance() {
        // 30 s of speech, no silence: the Segmenter force-cuts at 12 s and
        // 24 s (lowest-energy 20 ms frame + 0.4 s overlap), leaving the
        // final 6 s as the tail. 400 words, 75 ms apart, 60 ms long —
        // dense enough that every 20 ms cut window and overlap contains
        // words, so the overlap de-duplication is actually exercised.
        let words_per_sec = 400u32 / 30;
        assert_eq!(words_per_sec, 13); // 390 words
        let word_interval_ms = 1000u32 / words_per_sec; // 77 ms
        let reference_words: Vec<Word> = (0..words_per_sec * 30)
            .map(|i| {
                let start_ms = i * word_interval_ms;
                Word {
                    text: format!("w{i}"),
                    start_ms,
                    end_ms: start_ms + 60,
                    conf: 0.9,
                }
            })
            .collect();
        let reference = Transcript {
            text: reference_words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" "),
            words: reference_words,
            confidence: 0.9,
            language: Some("en".to_string()),
        };

        // Drive the real Segmenter with continuous speech (VAD 0.9 the
        // whole time): the forced cuts are the only cuts.
        let mut seg = teletype_core::segmenter::Segmenter::new(
            teletype_core::segmenter::SegmenterConfig::default(),
        );
        let mut cuts: Vec<teletype_core::segmenter::Segment> = Vec::new();
        for _ in 0..30 {
            for ev in seg.push(&vec![0.5; 16_000], 0.9) {
                let teletype_core::segmenter::SegmentEvent::Ready(s) = ev;
                cuts.push(s);
            }
        }
        // 30 s of continuous speech must force at least two 12 s cuts.
        assert!(
            cuts.len() >= 2,
            "a 30 s continuous take must force ≥ 2 cuts; got {}",
            cuts.len()
        );
        let tail = seg.flush().expect("flush returns the tail");

        // The ideal engine transcribes each window as exactly the words it
        // contains: a committed segment gets the words up to its cut point
        // (`next_start_sample` — the overlap belongs to the NEXT segment,
        // whose words the fold keeps), the tail gets the rest.
        let total = reference.words.len();
        let mut cursor = 0usize;
        let mut prev: Option<Transcript> = None;
        for c in &cuts {
            let cut_ms = c.next_start_sample as u64 * 1000 / 16_000;
            let mut next_words = Vec::new();
            while cursor < total && (reference.words[cursor].start_ms as u64) < cut_ms {
                next_words.push(reference.words[cursor].clone());
                cursor += 1;
            }
            let next = Transcript {
                text: next_words
                    .iter()
                    .map(|w| w.text.clone())
                    .collect::<Vec<_>>()
                    .join(" "),
                words: next_words,
                confidence: 0.9,
                language: Some("en".to_string()),
            };
            prev = Some(match prev {
                None => next,
                Some(p) => merge_transcripts(&p, next),
            });
        }
        let tail_transcript = Transcript {
            text: reference.words[cursor..]
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" "),
            words: reference.words[cursor..].to_vec(),
            confidence: 0.9,
            language: Some("en".to_string()),
        };
        let assembled = match prev {
            None => tail_transcript,
            Some(p) => merge_transcripts(&p, tail_transcript),
        };
        assert_eq!(
            assembled.text, reference.text,
            "forced-cut segments folded by timestamp must equal the whole-utterance decode byte-for-byte"
        );
        // The tail is the remainder after the last forced cut: one cut
        // period minus the overlap, ≤ 12 s (the point of the force cut).
        let tail_ms = tail_audio_ms(tail.end_sample - tail.start_sample, 16_000);
        assert!(
            tail_ms <= 12_000,
            "the tail after the last forced cut must be ≤ 12 s; got {tail_ms} ms"
        );
    }

    // ------------------------------------------------------------------
    // Gate 4.4 (automated evidence, plan section 4.4): tail bound and
    // release-to-text latency on a mock engine that decodes at 10 ms per
    // second of audio. A 60 s take driven through the real Segmenter +
    // live committer worker must (a) end with tail_audio_ms <= 3000 and
    // (b) return the finished, joined text within 80 ms of release:
    // every segment decode already completed on the worker, so the
    // release path only decodes the short tail and joins.
    // ------------------------------------------------------------------

    /// `MockSpeechProvider` with a decode cost of 10 ms per second of
    /// audio (the plan's 10 ms/s keep-up engine budget).
    struct SlowMock(MockSpeechProvider);

    impl SpeechProvider for SlowMock {
        fn load(&mut self, path: &std::path::Path) -> Result<(), teletype_speech::SpeechError> {
            self.0.load(path)
        }
        fn unload(&mut self) {
            self.0.unload();
        }
        fn is_loaded(&self) -> bool {
            self.0.is_loaded()
        }
        fn transcribe(
            &mut self,
            samples: &[f32],
            language: &str,
        ) -> Result<String, teletype_speech::SpeechError> {
            let cost = (samples.len() as f64 / 16_000.0 * 10.0) as u64;
            std::thread::sleep(Duration::from_millis(cost.max(1)));
            self.0.transcribe(samples, language)
        }
        fn detected_language(&self) -> Option<String> {
            self.0.detected_language()
        }
    }

    #[test]
    fn gate44_sixty_s_take_tail_and_release_latency() {
        let provider = MockSpeechProvider {
            transcript: "segment words here".to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = Committer::new(Box::new(SlowMock(provider)));

        // 40 x (1 s speech + 500 ms silence) = 60 s of audio through the
        // real Segmenter; each Ready window is submitted as the live
        // committer submits it.
        let mut seg = teletype_core::segmenter::Segmenter::new(
            teletype_core::segmenter::SegmenterConfig::default(),
        );
        let mut buf: Vec<f32> = Vec::new();
        let mut boundaries: Vec<usize> = Vec::new();
        for _ in 0..40 {
            let speech = vec![0.5f32; 16_000];
            let silence = vec![0.0f32; 8_000];
            for ev in seg.push(&speech, 0.9) {
                match ev {
                    teletype_core::segmenter::SegmentEvent::Ready(s) => {
                        boundaries.push(s.end_sample)
                    }
                }
            }
            buf.extend_from_slice(&speech);
            for ev in seg.push(&silence, 0.0) {
                match ev {
                    teletype_core::segmenter::SegmentEvent::Ready(s) => {
                        boundaries.push(s.end_sample)
                    }
                }
            }
            buf.extend_from_slice(&silence);
        }
        let tail = seg.flush();
        assert!(
            boundaries.len() >= 10,
            "segmenter must cut mid-take, got {} boundaries",
            boundaries.len()
        );

        // Submit the windows in order (no overlap; the merge handles the
        // adjacency).
        let mut prev = 0usize;
        let mut submitted = 0usize;
        for &end in &boundaries {
            committer.submit_segment(
                buf[prev..end].to_vec(),
                "en".to_string(),
                (prev as u64 * 1000 / 16_000) as u32,
            );
            prev = end;
            submitted += 1;
        }
        // The 10 ms/s worker keeps up; poll until every window committed.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            committer.poll_completed();
            if committer.committed_count() >= submitted {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not keep up (committed {} of {})",
                committer.committed_count(),
                submitted
            );
            std::thread::sleep(Duration::from_millis(10));
        }

        // (a) tail bound: the flush tail is the trailing silence only.
        let tail_ms = tail
            .as_ref()
            .map(|t| ((t.end_sample - t.start_sample) as u64 * 1000 / 16_000) as u32)
            .unwrap_or(0);
        assert!(tail_ms <= 3_000, "tail_audio_ms = {tail_ms}");

        // (b) release -> finished text: only the short tail decode + the
        // rules-only join may run; no waiting on pending segment decodes.
        let tail_audio: Vec<f32> = tail
            .as_ref()
            .map(|t| buf[..t.end_sample][t.start_sample..].to_vec())
            .unwrap_or_default();
        let t0 = std::time::Instant::now();
        let transcripts = committer
            .finish(
                tail_audio,
                "en".to_string(),
                (tail.as_ref().map(|t| t.start_sample).unwrap_or(0) as u64 * 1000 / 16_000) as u32,
            )
            .expect("finish the take");
        let final_text = join_segments(
            &transcripts
                .iter()
                .map(|t| t.text.clone())
                .collect::<Vec<_>>(),
        );
        let release_to_text = t0.elapsed();
        assert!(!final_text.is_empty(), "take must yield text");

        // release -> finished text must be fast: by release time every
        // committed-segment decode has already finished on the worker, so
        // the release path only decodes the short tail (bounded above by
        // tail_audio_ms <= 3000 -> <= 30 ms on this 10 ms/s mock) and runs
        // the rules-only join. The 80 ms plan bound protects against a
        // regression that breaks the tail queue-jump or makes finish() block
        // on / re-decode committed segments.
        //
        // That guard is environment-sensitive: finish() round-trips to the
        // committer worker thread over mpsc, and on a loaded CI runner
        // (macos-latest under parallel test load) waking that sleeper thread
        // adds hundreds of ms of pure OS scheduling jitter unrelated to the
        // code under test (observed 677 ms on run 39091372939). Locally that
        // jitter is single-digit ms, so the strict 80 ms plan bound holds.
        //
        // On CI we relax the bound to a 3 s catastrophic backstop rather than
        // waive it: it still catches a stalled/deadlocked release or
        // sec-scale disproportionate work on the release path (and stays well
        // under the 30 s poll deadline), while the strict 80 ms guard keeps
        // running on every local/dev run and the functional assertions above
        // plus the poll deadline independently catch structural regressions
        // on CI. Bump this if CI jitter grows.
        let release_bound = if is_ci_runner() {
            Duration::from_secs(3)
        } else {
            Duration::from_millis(80)
        };
        assert!(
            release_to_text <= release_bound,
            "release -> text took {release_to_text:?} (bound {release_bound:?})"
        );
    }

    // ------------------------------------------------------------------
    // P4.4: overlap de-dupe property test (plan §4.4).
    // No word is duplicated or dropped at segment joins.
    // ------------------------------------------------------------------

    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::proptest;

        proptest!(
            #[test]
            fn overlap_merge_never_dups_or_drops(
                n_words in 1usize..=40,
                n_segments in 2usize..=8,
            ) {
                // Segment boundaries as sample offsets; 16 kHz, so 100 ms
                // = 1600 samples. Boundaries must lie strictly between
                // word spans: word i ends at i*400+350 ms; the next word
                // starts at (i+1)*400 ms — the 50 ms gap is safe.
                let all_ms = (n_words * 400) as u32;
                let mut bounds_ms: Vec<u32> = (0..n_segments)
                    .map(|i| {
                        // Pick a boundary in the (i - 1)-th inter-word gap.
                        let gap_index = (i * n_words) / n_segments;
                        let gap_index = gap_index.min(n_words.saturating_sub(1));
                        gap_index as u32 * 400 + 375 // mid-gap, between words
                    })
                    .collect();
                bounds_ms.sort_unstable();
                bounds_ms.dedup();
                // The release tail always covers the remainder of the take.
                bounds_ms.push(all_ms);
                proptest::prop_assume!(bounds_ms.len() >= 2);

                let full = transcript_from_words(
                    &(0..n_words)
                        .map(|i| format!("w{i}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                let mut prev: Option<Transcript> = None;
                let mut prev_bound: u32 = 0;
                for &b in &bounds_ms {
                    let words: Vec<Word> = full
                        .words
                        .iter()
                        .filter(|w| w.start_ms >= prev_bound && w.end_ms <= b)
                        .cloned()
                        .collect();
                    let next = Transcript {
                        text: words
                            .iter()
                            .map(|w| w.text.clone())
                            .collect::<Vec<_>>()
                            .join(" "),
                        words,
                        confidence: 0.9,
                        language: None,
                    };
                    prev = Some(match prev {
                        None => next,
                        Some(p) => merge_transcripts(&p, next),
                    });
                    prev_bound = b;
                }
                let merged = prev.expect("at least two segments");
                // No word dropped and none duplicated: the merged word
                // texts are exactly the original sequence.
                let got: Vec<String> = merged.words.iter().map(|w| w.text.clone()).collect();
                let want: Vec<String> = (0..n_words).map(|i| format!("w{i}")).collect();
                assert_eq!(
                    got, want,
                    "overlap merge must preserve every word exactly once"
                );
            }
        );
    }

    #[test]
    fn tail_audio_ms_converts_samples() {
        assert_eq!(tail_audio_ms(16_000, 16_000), 1000);
        assert_eq!(tail_audio_ms(48_000, 16_000), 3000);
        assert_eq!(tail_audio_ms(0, 16_000), 0);
        assert_eq!(tail_audio_ms(16_000, 0), 0);
    }

    // ------------------------------------------------------------------
    // Release-path regressions: the tail jumps the queue, so `finish`
    // must still return every submitted segment, in segment-id order.
    // ------------------------------------------------------------------

    /// A provider whose text identifies the segment by its sample count and
    /// whose decode cost scales with the audio, so segments complete in a
    /// different order from the one they were submitted in.
    struct LenMock;

    impl SpeechProvider for LenMock {
        fn load(&mut self, _path: &std::path::Path) -> Result<(), teletype_speech::SpeechError> {
            Ok(())
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn transcribe(
            &mut self,
            samples: &[f32],
            _language: &str,
        ) -> Result<String, teletype_speech::SpeechError> {
            std::thread::sleep(Duration::from_millis((samples.len() / 160) as u64));
            Ok(format!("len{}", samples.len()))
        }
    }

    /// `finish` right after submitting: the tail decodes ahead of segments
    /// still queued behind it, and those segments must still land, in
    /// submission order, before the tail.
    #[test]
    fn finish_never_drops_segments_queued_behind_the_tail() {
        let committer = Committer::new(Box::new(LenMock));
        committer.submit_segment(vec![0.1; 4800], "en".to_string(), 0); // slowest
        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);

        // No poll and no sleep: release immediately, exactly like a user
        // who stops talking the moment the last segment was cut.
        let out = committer
            .finish(vec![0.1; 800], "en".to_string(), 300)
            .expect("finish");
        let texts: Vec<String> = out.into_iter().map(|t| t.text).collect();
        assert_eq!(
            texts,
            vec!["len4800", "len1600", "len1600", "len800"],
            "every submitted segment must survive the tail queue-jump, in order"
        );
    }

    /// The release gate reads *submitted* segments, so it opens at release
    /// even though nothing polled mid-take.
    #[test]
    fn submitted_count_is_visible_without_any_poll() {
        let provider = MockSpeechProvider {
            transcript: "x".to_string(),
            loaded: true,
            ..Default::default()
        };
        let committer = Committer::new(Box::new(provider));
        assert_eq!(committer.submitted_count(), 0);
        assert_eq!(committer.committed_count(), 0);
        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        committer.submit_segment(vec![0.1; 1600], "en".to_string(), 0);
        assert_eq!(committer.submitted_count(), 2);
        assert_eq!(
            committer.committed_count(),
            0,
            "decodes are not drained without a poll"
        );
        let _ = committer.finish(vec![0.1; 800], "en".to_string(), 500);
        assert_eq!(committer.submitted_count(), 0, "reset for the next take");
    }

    /// A provider that reports word timings RELATIVE to the audio it was
    /// given (one word per 3200 samples = 200 ms), exactly like every real
    /// engine.
    struct RelativeWordsMock;

    impl SpeechProvider for RelativeWordsMock {
        fn load(&mut self, _path: &std::path::Path) -> Result<(), teletype_speech::SpeechError> {
            Ok(())
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
        fn transcribe(
            &mut self,
            samples: &[f32],
            language: &str,
        ) -> Result<String, teletype_speech::SpeechError> {
            Ok(self.transcribe_detailed(samples, language)?.text)
        }
        fn transcribe_detailed(
            &mut self,
            samples: &[f32],
            _language: &str,
        ) -> Result<Transcript, teletype_speech::SpeechError> {
            let words: Vec<Word> = samples
                .chunks(3200)
                .enumerate()
                .map(|(i, _)| Word {
                    text: format!("w{i}"),
                    start_ms: (i * 200) as u32,
                    end_ms: (i * 200 + 150) as u32,
                    conf: 0.9,
                })
                .collect();
            let text = words
                .iter()
                .map(|w| w.text.clone())
                .collect::<Vec<_>>()
                .join(" ");
            Ok(Transcript {
                text,
                words,
                confidence: 0.9,
                language: Some("en".to_string()),
            })
        }
    }

    /// Regression: the release fold compares word timings across segments,
    /// but engines return them relative to each segment. The committer must
    /// shift each job by its take offset, or `merge_transcripts` treats a
    /// later segment's words as overlap and drops the take's tail.
    #[test]
    fn fold_keeps_every_segment_when_word_timings_are_segment_relative() {
        let committer = Committer::new(Box::new(RelativeWordsMock));
        // Three 1 s segments at 0/1000/2000 ms and a 400 ms tail at 3000 ms,
        // exactly the offsets the streaming take computes.
        committer.submit_segment(vec![0.1; 16_000], "en".to_string(), 0);
        committer.submit_segment(vec![0.1; 16_000], "en".to_string(), 1_000);
        committer.submit_segment(vec![0.1; 16_000], "en".to_string(), 2_000);
        let out = committer
            .finish(vec![0.1; 6_400], "en".to_string(), 3_000)
            .expect("finish");

        let starts: Vec<u32> = out
            .iter()
            .map(|t| t.words.first().map(|w| w.start_ms).unwrap_or(0))
            .collect();
        assert_eq!(
            starts,
            vec![0, 1_000, 2_000, 3_000],
            "each transcript's words must be shifted to take-absolute time"
        );

        // The fold the release path runs: no word may be dropped.
        let folded = out
            .iter()
            .cloned()
            .reduce(|prev, next| merge_transcripts(&prev, next))
            .expect("take has text");
        assert_eq!(
            folded.words.len(),
            5 + 5 + 5 + 2,
            "every segment and the tail must survive the fold"
        );
    }
}
