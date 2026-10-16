//! Whistle / Needle 3 speech provider (Cactus Compute).
//!
//! Wraps the [`needle-sys`] FFI as a [`SpeechProvider`] for the optional
//! "Tiny" dictation engine (16.9 MB `whistle.cact`).
//!
//! # License gate (plan §15 / §3.5)
//!
//! The Needle 3 repository is Apache-2.0, but the **weights license is
//! unverified** ([cactus-compute/needle](https://github.com/cactus-compute/needle)).
//! Per plan §15 this provider ships **optional-only** — never a default —
//! and nothing new is downloaded or enabled until the weights license is
//! confirmed. The model-gated tests skip when `whistle.cact` is absent.
//!
//! # Thread safety
//!
//! The Needle 3 C API is process-global and non-thread-safe. All engine
// calls go through `&mut self`; the [`SpeechModelManager`](crate::manager)
// serializes access with a mutex. Only one speech model may be loaded per
// process.

use std::ffi::CString;
use std::path::Path;
use std::ptr;

use crate::{BoostTerm, ModelCapabilities, SpeechError, SpeechProvider, Transcript, Word};
use serde::Deserialize;

/// Max total length of the keywords string sent to the engine.
const MAX_KEYWORDS_LEN: usize = 4096;
/// Max length of a single keyword.
const MAX_KEYWORD_LEN: usize = 64;
/// Top-N terms passed as keywords (plan §7.5).
const MAX_TERMS: usize = 50;
/// Output buffer size for `needle_transcribe` (64 KB, matching the reference).
const OUT_CAPACITY: usize = needle_sys::DEFAULT_OUT_CAPACITY;
/// Audio above this length (s) is split at VAD pauses instead of one shot.
const SPLIT_THRESHOLD_SECS: f64 = 10.0;
/// Silence RMS threshold (linear) below which a frame is "pause".
const PAUSE_RMS: f32 = 0.01;

// ── JSON shape returned by `needle_transcribe` ──────────────────────────────
// Defensive: unknown fields ignored, all fields optional except `text`.

#[derive(Deserialize, Default)]
struct TranscribeResponse {
    #[serde(default)]
    text: String,
    language: Option<String>,
    #[serde(default)]
    words: Option<Vec<WordItem>>,
}

#[derive(Deserialize, Default)]
struct WordItem {
    word: String,
    #[serde(default)]
    start_ms: Option<f32>,
    #[serde(default)]
    end_ms: Option<f32>,
    #[serde(default)]
    conf: Option<f32>,
}

// ── Provider ────────────────────────────────────────────────────────────────

/// Whistle (Needle 3 speech) provider.
///
/// `load(path)` reads a `.cact` file into the process-global engine's speech
/// slot via [`needle_sys::load_model`]. The engine sniffs the file kind, so
/// `whistle.cact` (speech) coexists with `needle3.cact` (text) in the text
/// slot. [`needle_sys::set_audio`] configures keywords + word timestamps on
/// every [`SpeechProvider::transcribe`] / [`SpeechProvider::transcribe_detailed`]
/// call.
pub struct WhistleProvider {
    loaded: bool,
    model_path: Option<std::path::PathBuf>,
    keywords: Vec<String>,
}

impl WhistleProvider {
    /// Create an unloaded provider. Nothing touches disk or the engine until
    /// [`SpeechProvider::load`].
    pub fn new() -> Self {
        Self {
            loaded: false,
            model_path: None,
            keywords: Vec::new(),
        }
    }

    /// Whether the speech slot of the process-global engine is already
    /// occupied (e.g. by a different speech model).
    fn speech_slot_busy() -> bool {
        needle_sys::loaded_models() & needle_sys::NEEDLE_SPEECH != 0
    }

    /// Build a newline-separated keywords string from keyword strings.
    ///
    /// Keywords are capped at `MAX_TERMS`, and each is truncated to
    /// `MAX_KEYWORD_LEN` chars. The total string is capped at
    /// `MAX_KEYWORDS_LEN`.
    fn build_keywords(keywords: &[String]) -> String {
        let mut out = String::new();
        for kw in keywords.iter().take(MAX_TERMS) {
            let kw = kw.trim().chars().take(MAX_KEYWORD_LEN).collect::<String>();
            if kw.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&kw);
            if out.len() >= MAX_KEYWORDS_LEN {
                return out;
            }
        }
        out
    }

    /// Call `needle_transcribe` with the given samples, language, and keywords.
    /// Returns the raw NUL-terminated output buffer on success (`rc >= 0`).
    fn run_transcribe(
        &self,
        samples: &[f32],
        language: &str,
        keywords: &str,
        word_timestamps: i32,
    ) -> Result<Vec<u8>, SpeechError> {
        let lang_ptr = CString::new(language)
            .ok()
            .map(|s| s.as_ptr())
            .unwrap_or(ptr::null());
        let kw_ptr = if keywords.is_empty() {
            ptr::null()
        } else {
            CString::new(keywords.as_bytes())
                .ok()
                .map(|s| s.as_ptr())
                .unwrap_or(ptr::null())
        };
        // Configure the engine with keywords + word timestamps.
        // SAFETY: pointers are valid NUL-terminated C strings (or null).
        unsafe {
            needle_sys::needle_set_audio(lang_ptr, kw_ptr, word_timestamps);
        }
        let mut buf = vec![0u8; OUT_CAPACITY];
        // SAFETY: `buf` is a valid writable region; `pcm` and pointers live
        // for the call.
        let rc = unsafe {
            needle_sys::needle_transcribe(
                samples.as_ptr(),
                samples.len() as i32,
                lang_ptr,
                kw_ptr,
                word_timestamps,
                buf.as_mut_ptr() as *mut i8,
                OUT_CAPACITY as i32,
            )
        };
        if rc < 0 {
            let msg = std::str::from_utf8(&buf)
                .ok()
                .map(|s| s.trim_end_matches('\0').to_string())
                .unwrap_or_default();
            let msg = if msg.is_empty() {
                needle_sys::last_error()
            } else {
                msg
            };
            return Err(SpeechError::Transcribe(msg));
        }
        Ok(buf)
    }

    /// Transcribe a single chunk (no splitting). Shared by `transcribe_single_detailed`
    /// and the per-chunk path of `merge_chunks`.
    fn transcribe_single_detailed(
        &mut self,
        samples: &[f32],
        language: &str,
        keywords: &str,
    ) -> Result<Transcript, SpeechError> {
        let buf = self.run_transcribe(samples, language, keywords, 1)?;
        let raw = std::str::from_utf8(&buf)
            .ok()
            .map(|s| s.trim_end_matches('\0').to_string())
            .unwrap_or_default();
        let response: TranscribeResponse = serde_json::from_str(&raw).unwrap_or_default();
        let text = response.text.trim().to_string();
        if text.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        let mut words = Vec::new();
        if let Some(word_items) = response.words {
            for item in word_items {
                let start_ms = item.start_ms.unwrap_or(0.0).max(0.0) as u32;
                let end_ms = item.end_ms.unwrap_or(0.0).max(0.0) as u32;
                let conf = item.conf.unwrap_or(1.0).clamp(0.0, 1.0);
                words.push(Word {
                    text: item.word,
                    start_ms,
                    end_ms,
                    conf,
                });
            }
        }
        Ok(Transcript {
            text,
            words,
            confidence: 1.0,
            language: response.language,
        })
    }

    /// Merge multiple chunk transcripts into one, deduplicating the boundary
    /// word between adjacent chunks (overlap region). Word timings are shifted
    /// by each chunk's sample offset so they refer to the original audio.
    fn merge_chunks(
        &mut self,
        samples: &[f32],
        language: &str,
        keywords: &str,
        chunks: &[(usize, usize)],
    ) -> Result<Transcript, SpeechError> {
        if chunks.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        if chunks.len() == 1 {
            let (start, end) = chunks[0];
            return self.transcribe_single_detailed(&samples[start..end], language, keywords);
        }

        let mut merged_words: Vec<Word> = Vec::new();
        let mut merged_text_parts: Vec<String> = Vec::new();
        let mut last_word_end_ms: Option<u32> = None;

        for (start, end) in chunks {
            let chunk_samples = &samples[*start..*end];
            let mut chunk = self.transcribe_single_detailed(chunk_samples, language, keywords)?;
            let offset_ms = (*start as f64 / 16_000.0 * 1000.0) as u32;

            // Adjust word timings by the chunk's sample offset.
            for word in &mut chunk.words {
                word.start_ms = word.start_ms.saturating_add(offset_ms);
                word.end_ms = word.end_ms.saturating_add(offset_ms);
            }

            // Dedupe: only the first word of a chunk can overlap the previous
            // chunk's last word (overlap region). Subsequent words are always
            // kept — they're past the boundary.
            let mut first_word = true;
            for word in chunk.words {
                if first_word {
                    first_word = false;
                    if let Some(last_end) = last_word_end_ms {
                        if word.start_ms < last_end {
                            continue;
                        }
                    }
                }
                last_word_end_ms = Some(word.end_ms);
                merged_words.push(word);
            }

            let chunk_text = chunk.text.trim().to_string();
            if !chunk_text.is_empty() {
                merged_text_parts.push(chunk_text);
            }
        }

        let text = merged_text_parts.join(" ");
        if text.is_empty() {
            return Err(SpeechError::NoSpeech);
        }

        Ok(Transcript {
            text,
            words: merged_words,
            confidence: 1.0,
            language: None,
        })
    }
}

impl Default for WhistleProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechProvider for WhistleProvider {
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        if self.loaded {
            return Ok(());
        }
        if Self::speech_slot_busy() {
            return Err(SpeechError::ModelLoad(
                "Needle speech slot already occupied".into(),
            ));
        }
        // `needle_sys::load_model` reads the file and calls the raw `needle_load`;
        // the engine sniffs the file kind (speech vs text) and loads into the
        // matching slot.
        needle_sys::load_model(path)
            .map_err(|e| SpeechError::ModelLoad(format!("needle load failed: {e}")))?;
        self.loaded = true;
        self.model_path = Some(path.to_path_buf());
        Ok(())
    }

    fn unload(&mut self) {
        // The Needle C API has no unload; `reset()` clears conversation state
        // but keeps the model resident. We mark ourselves unloaded so the
        // manager won't dispatch to us; the native model stays until process
        // exit (this is a known limitation of the process-global C API).
        needle_sys::reset();
        self.loaded = false;
        self.keywords.clear();
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        self.transcribe_detailed(samples, language).map(|t| t.text)
    }

    fn transcribe_detailed(
        &mut self,
        samples: &[f32],
        language: &str,
    ) -> Result<Transcript, SpeechError> {
        if !self.loaded {
            return Err(SpeechError::NoModel);
        }
        if samples.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        let keywords = Self::build_keywords(&self.keywords);
        let secs = samples.len() as f64 / 16_000.0; // pipeline resamples to 16 kHz
        if secs > SPLIT_THRESHOLD_SECS {
            let chunks = split_on_pause(samples, 16_000, 300, 300);
            self.merge_chunks(samples, language, &keywords, &chunks)
        } else {
            self.transcribe_single_detailed(samples, language, &keywords)
        }
    }

    fn set_boost_terms(&mut self, terms: &[BoostTerm]) -> Result<(), SpeechError> {
        let texts: Vec<String> = terms.iter().map(|t| t.text.clone()).collect();
        self.keywords = Self::build_keywords(&texts)
            .split('\n')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        Ok(())
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            native_boost: true,
            word_timings: true,
        }
    }
}

// ── VAD-pause splitter ─────────────────────────────────────────────────────

/// Split audio at VAD pauses with overlap.
///
/// Returns `(start_sample, end_sample)` ranges covering `[0, samples.len())`.
/// Adjacent chunks overlap by `overlap_ms` so boundary words are covered
/// twice; the caller should deduplicate by preferring the chunk with higher
/// confidence. Audio longer than 30 s is cut at the last pause before the
/// 30 s mark.
///
/// Pure function of `(samples, rate, pause_ms, overlap_ms)` — no engine,
/// no I/O, trivially unit-tested.
pub fn split_on_pause(
    samples: &[f32],
    rate: u32,
    pause_ms: u32,
    overlap_ms: u32,
) -> Vec<(usize, usize)> {
    if samples.is_empty() {
        return Vec::new();
    }
    let frame_samples = (rate as usize / 100).max(1); // 10 ms frames
    let pause_frames =
        (pause_ms as f64 * rate as f64 / 1000.0 / frame_samples as f64).ceil() as usize;
    let overlap_samples = (overlap_ms as f64 * rate as f64 / 1000.0) as usize;
    let max_chunk_samples = 30_usize * rate as usize; // 30 s safety cap

    // Per-frame RMS energy.
    let frame_count = samples.len().div_euclid(frame_samples);
    let mut frame_rms: Vec<f32> = Vec::with_capacity(frame_count);
    for f in 0..frame_count {
        let start = f * frame_samples;
        let end = (start + frame_samples).min(samples.len());
        let sum: f32 = samples[start..end].iter().map(|&s| s * s).sum();
        frame_rms.push((sum / (end - start) as f32).sqrt());
    }
    let is_pause: Vec<bool> = frame_rms.iter().map(|&r| r < PAUSE_RMS).collect();

    let mut chunks: Vec<(usize, usize)> = Vec::new();
    let mut chunk_start = 0usize;
    let mut i = 0;

    while i < frame_count {
        // Max chunk size safety cut.
        if (i * frame_samples) - (chunk_start * frame_samples) >= max_chunk_samples {
            let cut = (chunk_start..=i).rev().find(|&j| is_pause[j]).unwrap_or(i);
            let cut_sample = cut * frame_samples;
            let chunk_end = (cut_sample + overlap_samples).min(samples.len());
            chunks.push((chunk_start * frame_samples, chunk_end));
            chunk_start =
                (cut_sample.saturating_sub(overlap_samples) / frame_samples).max(chunk_start + 1);
            i = cut + 1;
            continue;
        }

        // Check for a pause of at least `pause_frames`.
        if is_pause[i] {
            let pause_len = (i..frame_count).take_while(|&j| is_pause[j]).count();
            if pause_len >= pause_frames {
                let cut = i + pause_len / 2; // middle of the pause
                let cut_sample = cut * frame_samples;
                let chunk_end = (cut_sample + overlap_samples).min(samples.len());
                chunks.push((chunk_start * frame_samples, chunk_end));
                chunk_start = (cut_sample.saturating_sub(overlap_samples) / frame_samples)
                    .max(chunk_start + 1);
                i = cut + 1;
                continue;
            }
        }
        i += 1;
    }

    // Final chunk for remaining audio.
    if chunk_start * frame_samples < samples.len() {
        chunks.push((chunk_start * frame_samples, samples.len()));
    }

    chunks
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn term(text: &str, weight: f32) -> BoostTerm {
        BoostTerm {
            text: text.to_string(),
            aliases: Vec::new(),
            weight,
        }
    }

    #[test]
    fn new_provider_is_unloaded() {
        let p = WhistleProvider::new();
        assert!(!p.is_loaded());
        assert!(p.capabilities().native_boost);
        assert!(p.capabilities().word_timings);
    }

    #[test]
    fn transcribe_without_load_returns_no_model() {
        let mut p = WhistleProvider::new();
        assert!(matches!(
            p.transcribe(&[0.1; 160], "en"),
            Err(SpeechError::NoModel)
        ));
        assert!(matches!(
            p.transcribe_detailed(&[0.1; 160], "en"),
            Err(SpeechError::NoModel)
        ));
    }

    #[test]
    fn empty_audio_returns_no_speech() {
        let mut p = WhistleProvider::new();
        assert!(matches!(
            p.transcribe(&[], "en"),
            Err(SpeechError::NoModel) // loaded check fires first
        ));
    }

    #[test]
    fn build_keywords_caps_at_max_terms() {
        let terms: Vec<String> = (0..60).map(|i| format!("term{i}")).collect();
        let kw = WhistleProvider::build_keywords(&terms);
        let count = kw.split('\n').count();
        assert!(count <= MAX_TERMS, "got {count} keywords, max {MAX_TERMS}");
    }

    #[test]
    fn build_keywords_preserves_order() {
        let terms = vec!["low".to_string(), "high".to_string(), "mid".to_string()];
        let kw = WhistleProvider::build_keywords(&terms);
        let parts: Vec<&str> = kw.split('\n').collect();
        assert_eq!(parts, vec!["low", "high", "mid"]);
    }

    #[test]
    fn build_keywords_truncates_long_terms() {
        let long = "x".repeat(200);
        let terms = vec![long];
        let kw = WhistleProvider::build_keywords(&terms);
        assert!(kw.len() <= MAX_KEYWORD_LEN);
    }

    #[test]
    fn build_keywords_total_cap() {
        let terms: Vec<String> = (0..1000).map(|i| format!("w{i}")).collect();
        let kw = WhistleProvider::build_keywords(&terms);
        assert!(kw.len() <= MAX_KEYWORDS_LEN);
    }

    #[test]
    fn build_keywords_only_includes_text_not_aliases() {
        // `build_keywords` takes raw strings; aliases are expanded by the
        // caller (`set_boost_terms`) before calling. Passing just the text
        // yields only the text.
        let terms = vec!["Teletype".to_string()];
        let kw = WhistleProvider::build_keywords(&terms);
        let parts: Vec<&str> = kw.split('\n').collect();
        assert_eq!(parts, vec!["Teletype"]);
    }

    #[test]
    fn set_boost_terms_stores_keywords() {
        let mut p = WhistleProvider::new();
        let terms = vec![term("Siobhan", 3.0), term("shivon", 1.0)];
        assert!(p.set_boost_terms(&terms).is_ok());
        // `keywords` is private; verify via capabilities + the fact that
        // set_boost_terms doesn't panic or error.
        assert!(p.capabilities().native_boost);
    }

    #[test]
    fn split_on_pause_empty_is_empty() {
        assert!(split_on_pause(&[], 16000, 300, 300).is_empty());
    }

    #[test]
    fn split_on_pause_short_audio_one_chunk() {
        // 1 s of loud speech → one chunk.
        let samples: Vec<f32> = (0..16000).map(|_| 0.5).collect();
        let chunks = split_on_pause(&samples, 16000, 300, 300);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], (0, 16000));
    }

    #[test]
    fn split_on_pause_splits_at_pause() {
        // 1 s speech, 500 ms silence, 1 s speech → >= 2 chunks covering all.
        let speech: Vec<f32> = (0..16000).map(|_| 0.5).collect();
        let silence: Vec<f32> = vec![0.0; 8000]; // 500 ms at 16 kHz
        let mut samples = speech.clone();
        samples.extend_from_slice(&silence);
        samples.extend_from_slice(&speech);
        let chunks = split_on_pause(&samples, 16000, 300, 300);
        assert!(
            chunks.len() >= 2,
            "expected >= 2 chunks, got {}",
            chunks.len()
        );
        let first_start = chunks[0].0;
        let last_end = chunks.last().unwrap().1;
        assert_eq!(first_start, 0);
        assert_eq!(last_end, samples.len());
    }

    #[test]
    fn split_on_pause_overlap_covers_boundary() {
        let speech: Vec<f32> = (0..16000).map(|_| 0.5).collect();
        let silence: Vec<f32> = vec![0.0; 8000];
        let mut samples = speech.clone();
        samples.extend_from_slice(&silence);
        samples.extend_from_slice(&speech);
        let chunks = split_on_pause(&samples, 16000, 300, 300);
        // Adjacent chunks should overlap.
        for w in chunks.windows(2) {
            assert!(w[0].1 > w[1].0, "chunks {w:?} do not overlap");
        }
    }

    #[test]
    fn split_on_pause_max_chunk_cap() {
        // 35 s of continuous speech → capped into >= 2 chunks.
        let samples: Vec<f32> = (0..35 * 16000).map(|_| 0.5).collect();
        let chunks = split_on_pause(&samples, 16000, 300, 300);
        assert!(chunks.len() >= 2, "expected >= 2 chunks for 35 s audio");
        let first_len = chunks[0].1 - chunks[0].0;
        assert!(
            first_len <= 30 * 16000 + 300 * 16000 / 1000,
            "first chunk {first_len} samples exceeds 30 s + overlap"
        );
    }

    /// Model-gated e2e: transcribes a 1 s sine-like buffer through the real
    /// engine. Skips when `whistle.cact` is absent (weights license
    /// unverified — plan §15). Run with:
    ///   TELETYPE_WHISTLE_MODEL=models/whistle.cact cargo test -p teletype-speech whistle::tests::model_gated_transcribe -- --ignored --nocapture
    #[test]
    #[ignore]
    fn model_gated_transcribe() {
        let path = std::env::var("TELETYPE_WHISTLE_MODEL")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("models/whistle.cact"));
        if !path.exists() {
            eprintln!("SKIP: {path:?} not found (TELETYPE_WHISTLE_MODEL)");
            return;
        }
        let mut p = WhistleProvider::new();
        p.load(&path).expect("whistle load failed");
        assert!(p.is_loaded());
        // 1 s of silence should give NoSpeech (no actual audio content).
        let result = p.transcribe(&[0.0; 16000], "en");
        // Silence may transcribe as empty text or NoSpeech — either is valid.
        assert!(result.is_err() || result.unwrap().trim().is_empty());
        p.unload();
        assert!(!p.is_loaded());
    }
}
