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

use teletype_inference::manager::SpeechModelManager;
use teletype_speech::{SpeechProvider, Transcript, Word};

/// A transcription job for the worker thread.
enum Job {
    /// Transcribe a segment — processed FIFO.
    Segment {
        id: usize,
        samples: Vec<f32>,
        language: String,
        result_tx: mpsc::Sender<Result<Transcript, String>>,
    },
    /// Transcribe the final tail — jumped to the front of the queue.
    Tail {
        samples: Vec<f32>,
        language: String,
        result_tx: mpsc::Sender<Result<Transcript, String>>,
    },
}

/// Merge two transcripts, de-duplicating overlapping words.
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

/// The Committer: transcribes segments while the user is still talking.
pub struct Committer {
    tx: mpsc::Sender<Job>,
    /// Segments completed so far, in commit order.
    committed: Mutex<Vec<Transcript>>,
    /// Pending segment result receivers, keyed by segment id.
    pending: Mutex<HashMap<usize, mpsc::Receiver<Result<Transcript, String>>>>,
    next_id: Mutex<usize>,
    worker: Option<thread::JoinHandle<()>>,
}

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
            worker: Some(worker),
        }
    }

    fn worker_loop(
        rx: mpsc::Receiver<Job>,
        manager: Arc<SpeechModelManager<Box<dyn SpeechProvider>>>,
    ) {
        let mut queue: Vec<Job> = Vec::new();

        loop {
            // Drain the channel into the priority queue.
            while let Ok(job) = rx.try_recv() {
                match &job {
                    Job::Tail { .. } => queue.insert(0, job), // tail jumps the queue
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
                        result_tx,
                        ..
                    } => {
                        let mut p = manager.provider().lock().unwrap();
                        let result = p
                            .transcribe_detailed(&samples, &language)
                            .map_err(|e| e.to_string());
                        let _ = result_tx.send(result);
                    }
                    Job::Tail {
                        samples,
                        language,
                        result_tx,
                        ..
                    } => {
                        let mut p = manager.provider().lock().unwrap();
                        let result = p
                            .transcribe_detailed(&samples, &language)
                            .map_err(|e| e.to_string());
                        let _ = result_tx.send(result);
                    }
                }
            } else {
                // Queue empty: block waiting for the next job.
                match rx.recv() {
                    Ok(job) => match &job {
                        Job::Tail { .. } => queue.insert(0, job),
                        Job::Segment { .. } => queue.push(job),
                    },
                    Err(_) => break, // channel closed: worker exiting
                }
            }
        }
    }

    /// Submit a segment for transcription. Returns a segment id.
    ///
    /// The segment is transcribed on the worker thread. Use
    /// [`Committer::poll_completed`] to collect finished results, or
    /// [`Committer::finish`] to wait for everything.
    pub fn submit_segment(&self, samples: Vec<f32>, language: String) -> usize {
        let id = {
            let mut n = self.next_id.lock().unwrap();
            let id = *n;
            *n += 1;
            id
        };
        let (result_tx, result_rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, result_rx);
        let _ = self.tx.send(Job::Segment {
            id,
            samples,
            language,
            result_tx,
        });
        id
    }

    /// Check for newly completed segment transcriptions and add them to
    /// the committed list. Call this periodically (e.g. each interim loop
    /// tick) so the pill preview stays fresh.
    pub fn poll_completed(&self) {
        let mut pending = self.pending.lock().unwrap();
        let mut completed: Vec<(usize, Transcript)> = Vec::new();
        for (id, rx) in pending.iter_mut() {
            if let Ok(result) = rx.try_recv() {
                match result {
                    Ok(transcript) => completed.push((*id, transcript)),
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
            committed.push(transcript);
        }
    }

    /// Transcribed text of all committed segments, joined with spaces.
    pub fn committed_texts(&self) -> Vec<String> {
        self.committed
            .lock()
            .unwrap()
            .iter()
            .map(|t| t.text.clone())
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
            .map(|t| t.text.clone())
            .collect::<Vec<_>>()
            .join(" ");
        (committed_text, String::new())
    }

    /// Wait for the tail transcription and all previously submitted
    /// segments, then return all transcripts in commit order followed by
    /// the tail. After this call, the Committer is reset for the next take.
    pub fn finish(&self, samples: Vec<f32>, language: String) -> Result<Vec<Transcript>, String> {
        // Collect any segments that completed while we were waiting.
        self.poll_completed();

        // Send the tail job — it jumps the queue ahead of any remaining
        // segments.
        let (result_tx, result_rx) = mpsc::channel();
        self.tx
            .send(Job::Tail {
                samples,
                language,
                result_tx,
            })
            .map_err(|e| e.to_string())?;

        // Wait for the tail. Since it jumped the queue, all segments
        // submitted before this call have already been processed by the
        // worker (single-threaded, FIFO with tail-at-front).
        let tail = result_rx.recv().map_err(|e| e.to_string())??;

        // Collect any remaining segment results.
        self.poll_completed();

        // Build the final list: committed segments in order, then the tail.
        let mut all: Vec<Transcript> = {
            let committed = self.committed.lock().unwrap();
            committed.clone()
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

        Ok(all)
    }

    /// Number of segments committed so far this take.
    pub fn committed_count(&self) -> usize {
        self.committed.lock().unwrap().len()
    }
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
    use std::time::Duration;
    use teletype_speech::MockSpeechProvider;

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
        let mut provider = MockSpeechProvider::default();
        provider.transcript = "hello world".to_string();
        provider.loaded = true;
        let committer = Committer::new(Box::new(provider));

        let id = committer.submit_segment(vec![0.1; 1600], "en".to_string());
        assert_eq!(id, 0);

        // Not completed yet (worker needs a moment)
        committer.poll_completed();
        assert_eq!(committer.committed_count(), 0);

        // Give the worker thread time to process
        std::thread::sleep(Duration::from_millis(50));
        committer.poll_completed();
        assert_eq!(committer.committed_count(), 1);
        assert_eq!(committer.committed_texts(), vec!["hello world"]);
    }

    #[test]
    fn finish_returns_tail_after_segments() {
        let mut provider = MockSpeechProvider::default();
        provider.transcript = "segment text".to_string();
        provider.loaded = true;
        let committer = Committer::new(Box::new(provider));

        // Submit a segment
        committer.submit_segment(vec![0.1; 1600], "en".to_string());
        std::thread::sleep(Duration::from_millis(50));
        committer.poll_completed();
        assert_eq!(committer.committed_count(), 1);

        // Finish with tail
        let result = committer.finish(vec![0.2; 800], "en".to_string());
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
        let mut provider = MockSpeechProvider::default();
        provider.transcript = "committed text".to_string();
        provider.loaded = true;
        let committer = Committer::new(Box::new(provider));

        // Initially empty
        let (ct, vt) = committer.preview_snapshot();
        assert_eq!(ct, "");
        assert_eq!(vt, "");

        // After a segment completes
        committer.submit_segment(vec![0.1; 1600], "en".to_string());
        std::thread::sleep(Duration::from_millis(50));
        committer.poll_completed();
        let (ct, vt) = committer.preview_snapshot();
        assert_eq!(ct, "committed text");
        assert_eq!(vt, "");
    }
}
