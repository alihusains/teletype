//! whisper.cpp backend via whisper-rs 0.16 (macOS: Metal + CoreML).
//!
//! P3-E improvements (plan §3.4):
//! - `initial_prompt` = top lexicon terms (from `set_boost_terms`) so the
//!   decoder is biased toward the user's vocabulary.
//! - `no_context = true` between dictations (no cross-session prompt bleed).
//! - `single_segment = true` for audio under 30 s (faster, no re-decode).
//! - `no_timestamps = true` unless word timings are requested.
//! - `n_threads` = performance-core count on Apple Silicon
//!   (`hw.perflevel0.physicalcpu`), not total core count.

use std::path::Path;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::{BoostTerm, SpeechError, SpeechProvider};

/// Audio length (seconds) below which single-segment decoding is used.
const SINGLE_SEGMENT_MAX_SECS: f64 = 30.0;

/// A whisper.cpp model wrapper.
pub struct WhisperProvider {
    context: Option<WhisperContext>,
    state: Option<WhisperState>,
    /// Last detected/used language code (e.g. "en", "ru").
    detected_language: Option<String>,
    /// Top lexicon terms for `initial_prompt` bias (P3-E bullet 1).
    boost_terms: Vec<String>,
}

impl WhisperProvider {
    pub fn new() -> Self {
        Self {
            context: None,
            state: None,
            detected_language: None,
            boost_terms: Vec::new(),
        }
    }

    /// The language detected on the last auto-detect transcription, if any.
    pub fn detected_language(&self) -> Option<String> {
        self.detected_language.clone()
    }
}

impl Default for WhisperProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Performance-core count on Apple Silicon via `hw.perflevel0.physicalcpu`.
/// Falls back to total parallelism when the sysctl is unavailable.
fn perf_core_count() -> usize {
    #[cfg(target_os = "macos")]
    {
        // hw.perflevel0.physicalcpu is the number of performance cores.
        // We read it via a small C FFI call to avoid adding a sysctl dep.
        extern "C" {
            #[cfg(target_os = "macos")]
            fn sysctlbyname(
                name: *const u8,
                oldp: *mut std::ffi::c_void,
                oldlenp: *mut u32,
                newp: *mut std::ffi::c_void,
                newlen: u32,
            ) -> i32;
        }
        let name = b"hw.perflevel0.physicalcpu\0";
        let mut value: i32 = 0;
        let mut len = std::mem::size_of::<i32>() as u32;
        // SAFETY: `name` is a NUL-terminated valid C string; `value` and
        // `len` are valid, mutable, and large enough for an i32 result.
        unsafe {
            if sysctlbyname(
                name.as_ptr(),
                &mut value as *mut i32 as *mut std::ffi::c_void,
                &mut len,
                std::ptr::null_mut(),
                0,
            ) == 0
                && value > 0
            {
                return value as usize;
            }
        }
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

impl SpeechProvider for WhisperProvider {
    fn load(&mut self, path: &Path) -> Result<(), SpeechError> {
        if self.is_loaded() {
            return Ok(());
        }
        let params = WhisperContextParameters::default();
        let context = WhisperContext::new_with_params(path, params)
            .map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
        let state = context
            .create_state()
            .map_err(|e| SpeechError::ModelLoad(e.to_string()))?;
        self.context = Some(context);
        self.state = Some(state);
        Ok(())
    }

    fn unload(&mut self) {
        self.state = None;
        self.context = None;
        self.detected_language = None;
        self.boost_terms.clear();
    }

    fn is_loaded(&self) -> bool {
        self.state.is_some()
    }

    fn set_boost_terms(&mut self, terms: &[BoostTerm]) -> Result<(), SpeechError> {
        // Store the top 30 terms (by weight, descending) for initial_prompt.
        let mut sorted: Vec<&BoostTerm> = terms.iter().collect();
        sorted.sort_by(|a, b| {
            b.weight
                .partial_cmp(&a.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.boost_terms = sorted.iter().take(30).map(|t| t.text.clone()).collect();
        Ok(())
    }

    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError> {
        let state = self.state.as_mut().ok_or(SpeechError::NoModel)?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        if language != "auto" && !language.is_empty() {
            params.set_language(Some(language));
        }

        // P3-E: performance-core count on Apple Silicon, not total cores.
        params.set_n_threads(perf_core_count().max(2) as i32);

        // P3-E: no_context prevents cross-dictation prompt bleed.
        params.set_no_context(true);

        // P3-E: single_segment for short audio (under 30 s) is faster.
        let audio_secs = samples.len() as f64 / 16_000.0;
        params.set_single_segment(audio_secs < SINGLE_SEGMENT_MAX_SECS);

        // P3-E: no_timestamps unless word timings are needed.
        // The base `transcribe` doesn't need them; `transcribe_detailed`
        // would set `no_timestamps = false` (P3-B trait default handles
        // engines without detailed support).
        params.set_no_timestamps(true);

        // P3-E: initial_prompt bias from lexicon terms.
        if !self.boost_terms.is_empty() {
            let prompt = self.boost_terms.join(", ");
            params.set_initial_prompt(&prompt);
        }

        state
            .full(params, samples)
            .map_err(|e| SpeechError::Transcribe(e.to_string()))?;

        // Remember what the engine detected so the UI can show a chip and
        // the user can lock the language. Only meaningful for auto-detect
        // runs; a locked language always yields the same code.
        self.detected_language = if language == "auto" || language.is_empty() {
            let id = state.full_lang_id_from_state();
            if id > 0 {
                whisper_rs::get_lang_str(id).map(str::to_string)
            } else {
                None
            }
        } else {
            Some(language.to_string())
        };

        let n_segments = state.full_n_segments();
        if n_segments == 0 {
            return Err(SpeechError::NoSpeech);
        }

        let mut text = String::new();
        for seg in state.as_iter() {
            if let Ok(s) = seg.to_str() {
                text.push_str(s);
            }
        }

        let text = text.trim().to_string();
        if text.is_empty() {
            Err(SpeechError::NoSpeech)
        } else {
            Ok(text)
        }
    }

    fn detected_language(&self) -> Option<String> {
        self.detected_language.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perf_core_count_is_positive() {
        assert!(perf_core_count() > 0);
    }

    #[test]
    fn boost_terms_are_stored_by_weight() {
        let mut provider = WhisperProvider::new();
        provider
            .set_boost_terms(&[
                BoostTerm {
                    text: "low".into(),
                    aliases: vec![],
                    weight: 1.0,
                },
                BoostTerm {
                    text: "high".into(),
                    aliases: vec![],
                    weight: 5.0,
                },
                BoostTerm {
                    text: "mid".into(),
                    aliases: vec![],
                    weight: 3.0,
                },
            ])
            .unwrap();
        assert_eq!(provider.boost_terms[0], "high");
        assert_eq!(provider.boost_terms[1], "mid");
        assert_eq!(provider.boost_terms[2], "low");
    }

    #[test]
    fn boost_terms_capped_at_30() {
        let mut provider = WhisperProvider::new();
        let terms: Vec<BoostTerm> = (0..50)
            .map(|i| BoostTerm {
                text: format!("term{i}"),
                aliases: vec![],
                weight: i as f32,
            })
            .collect();
        provider.set_boost_terms(&terms).unwrap();
        assert_eq!(provider.boost_terms.len(), 30);
        // Highest-weight terms survive.
        assert_eq!(provider.boost_terms[0], "term49");
    }

    #[test]
    fn unload_clears_boost_terms() {
        let mut provider = WhisperProvider::new();
        provider
            .set_boost_terms(&[BoostTerm {
                text: "x".into(),
                aliases: vec![],
                weight: 1.0,
            }])
            .unwrap();
        provider.unload();
        assert!(provider.boost_terms.is_empty());
    }

    #[test]
    fn transcribe_without_load_is_no_model() {
        let mut provider = WhisperProvider::new();
        let err = provider.transcribe(&[0.0; 16000], "en").unwrap_err();
        assert!(matches!(err, SpeechError::NoModel));
    }
}
