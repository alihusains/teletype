//! Speech-to-text providers for Teletype.
//!
//! The rest of the application depends only on [`SpeechProvider`]; the
//! whisper.cpp backend is one implementation, not the architecture.

pub mod catalog;
pub mod parakeet;
pub mod parakeet_cpp;
pub mod sherpa;
pub mod whisper;
pub mod whistle;

pub use catalog::{find, SpeechModel, CATALOG};
pub use parakeet::ParakeetProvider;
pub use parakeet_cpp::ParakeetCppProvider;
pub use sherpa::SherpaParakeetProvider;
pub use whistle::WhistleProvider;

use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpeechError {
    #[error("No speech model loaded")]
    NoModel,
    #[error("Model load failed: {0}")]
    ModelLoad(String),
    #[error("Transcription failed: {0}")]
    Transcribe(String),
    #[error("No speech detected")]
    NoSpeech,
    #[error("Cancelled")]
    Cancelled,
}

/// One decoded word with its time span and confidence (P3-B).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Word {
    pub text: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub conf: f32,
}

/// A detailed transcription: text plus per-word timings/confidences and an
/// overall confidence. Engines without detailed support return an empty
/// `words` list through the default [`SpeechProvider::transcribe_detailed`].
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Transcript {
    pub text: String,
    pub words: Vec<Word>,
    pub confidence: f32,
    pub language: Option<String>,
}

/// One custom-vocabulary term for native boosting (the FluidAudio bridge
/// maps these to `CustomVocabularyTerm`). Engines without native boosting
/// ignore them; post-ASR rescoring covers those engines instead.
#[derive(Debug, Clone, PartialEq)]
pub struct BoostTerm {
    pub text: String,
    pub aliases: Vec<String>,
    pub weight: f32,
}

/// What an engine can do beyond the base text contract, so the pipeline can
/// choose native boosting vs post-ASR rescoring without knowing the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModelCapabilities {
    /// The engine boosts custom vocabulary inside the model itself.
    pub native_boost: bool,
    /// `transcribe_detailed` returns real per-word timings/confidences.
    pub word_timings: bool,
}

/// Abstracts a local speech-to-text engine.
pub trait SpeechProvider: Send + 'static {
    /// Loads the model at `path`. No-op if already loaded.
    fn load(&mut self, path: &Path) -> Result<(), SpeechError>;

    /// Unloads the model, freeing memory.
    fn unload(&mut self);

    /// Whether a model is currently loaded.
    fn is_loaded(&self) -> bool;

    /// Transcribes 16 kHz mono f32 samples. `language` is a BCP code or "auto".
    fn transcribe(&mut self, samples: &[f32], language: &str) -> Result<String, SpeechError>;

    /// The language the engine detected on its last run, when auto-detect
    /// was active and the engine reports one. `None` otherwise (no run yet,
    /// locked language, or the engine does not report detection).
    fn detected_language(&self) -> Option<String> {
        None
    }

    /// Transcribes with word timings and confidences (P3-B). Engines that
    /// cannot report them keep the default: the text mapped from
    /// [`SpeechProvider::transcribe`], no words, full confidence.
    fn transcribe_detailed(
        &mut self,
        samples: &[f32],
        language: &str,
    ) -> Result<Transcript, SpeechError> {
        self.transcribe(samples, language).map(|text| Transcript {
            text,
            words: Vec::new(),
            confidence: 1.0,
            language: None,
        })
    }

    /// Sets the custom-vocabulary terms for native boosting. No-op by
    /// default: engines without native boosting are covered by post-ASR
    /// rescoring (P3-G) fed from [`SpeechProvider::transcribe_detailed`].
    fn set_boost_terms(&mut self, _terms: &[BoostTerm]) -> Result<(), SpeechError> {
        Ok(())
    }

    /// What this engine supports beyond text (P3-B). Default: base text only.
    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
}

/// Group per-token timings (seconds) into words.
///
/// Engines that decode BPE/subword tokens (e.g. sherpa-onnx transducers)
/// report one timestamp per token; a token that starts with the
/// sentence-piece boundary marker (`▁`) or a space opens a new word. Word
/// confidence is the mean of its tokens' confidences when the engine
/// reports them, otherwise 1.0.
pub fn group_token_timings(
    tokens: &[String],
    start_seconds: &[f32],
    end_seconds: &[f32],
) -> Vec<Word> {
    let count = tokens.len().min(start_seconds.len()).min(end_seconds.len());
    if count == 0 {
        return Vec::new();
    }

    let mut words: Vec<Word> = Vec::new();
    let mut current = String::new();
    let mut start = 0.0f32;
    let mut end = 0.0f32;

    for i in 0..count {
        let token = &tokens[i];
        let boundary = token.starts_with('\u{2581}') || token.starts_with(' ');
        let piece = token.replace('\u{2581}', " ").trim().to_string();
        if boundary && !current.is_empty() {
            words.push(Word {
                text: std::mem::take(&mut current),
                start_ms: (start * 1000.0).round().max(0.0) as u32,
                end_ms: (end * 1000.0).round().max(0.0) as u32,
                conf: 1.0,
            });
            current.clear();
        }
        if current.is_empty() {
            start = start_seconds[i];
        }
        current.push_str(&piece);
        end = end_seconds[i];
    }
    if !current.is_empty() {
        words.push(Word {
            text: current,
            start_ms: (start * 1000.0).round().max(0.0) as u32,
            end_ms: (end * 1000.0).round().max(0.0) as u32,
            conf: 1.0,
        });
    }
    words
}

/// A provider that returns a fixed transcript — for tests.
#[derive(Debug, Clone, Default)]
pub struct MockSpeechProvider {
    pub transcript: String,
    pub loaded: bool,
    /// The language the mock claims it detected (test-only).
    pub detected_language: Option<String>,
}

impl SpeechProvider for MockSpeechProvider {
    fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
        self.loaded = true;
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }

    fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
        if !self.loaded {
            return Err(SpeechError::NoModel);
        }
        if self.transcript.is_empty() {
            return Err(SpeechError::NoSpeech);
        }
        Ok(self.transcript.clone())
    }

    fn detected_language(&self) -> Option<String> {
        self.detected_language.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded_mock(text: &str) -> MockSpeechProvider {
        MockSpeechProvider {
            transcript: text.into(),
            loaded: true,
            detected_language: None,
        }
    }

    #[test]
    fn group_token_timings_groups_bpe_tokens_into_words() {
        let tokens = vec!["▁Hello".into(), "▁world".into(), ".".into()];
        let starts = vec![0.0, 0.3, 0.55];
        let ends = vec![0.25, 0.5, 0.6];
        let words = group_token_timings(&tokens, &starts, &ends);
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].text, "Hello");
        assert_eq!(words[0].start_ms, 0);
        assert_eq!(words[0].end_ms, 250);
        assert_eq!(words[1].text, "world.");
        assert_eq!(words[1].start_ms, 300);
        assert_eq!(words[1].end_ms, 600);
    }

    #[test]
    fn group_token_timings_empty_inputs_yield_empty_words() {
        assert!(group_token_timings(&[], &[], &[]).is_empty());
        // Uneven lengths are truncated to the shortest side, not a panic.
        let words = group_token_timings(&["▁hi".into()], &[0.0], &[]);
        assert!(words.is_empty());
    }

    #[test]
    fn default_detailed_maps_text_without_words() {
        let mut provider = loaded_mock("hello world");
        let t = provider
            .transcribe_detailed(&[], "en")
            .expect("default detailed");
        assert_eq!(t.text, "hello world");
        assert!(t.words.is_empty());
        assert_eq!(t.confidence, 1.0);
        assert_eq!(t.language, None);
    }

    #[test]
    fn default_boost_and_capabilities_are_no_ops() {
        let mut provider = loaded_mock("x");
        assert!(provider
            .set_boost_terms(&[BoostTerm {
                text: "Siobhan".into(),
                aliases: vec!["shivon".into()],
                weight: 1.0,
            }])
            .is_ok());
        let caps = provider.capabilities();
        assert!(!caps.native_boost);
        assert!(!caps.word_timings);
    }

    /// A provider that overrides the whole extended surface, to prove the
    /// defaults are not the only reachable path and stay object-safe.
    struct DetailedMock;

    impl SpeechProvider for DetailedMock {
        fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
            Ok(())
        }

        fn unload(&mut self) {}

        fn is_loaded(&self) -> bool {
            true
        }

        fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
            Ok("plain".into())
        }

        fn transcribe_detailed(
            &mut self,
            _samples: &[f32],
            _language: &str,
        ) -> Result<Transcript, SpeechError> {
            Ok(Transcript {
                text: "hello".into(),
                words: vec![Word {
                    text: "hello".into(),
                    start_ms: 0,
                    end_ms: 300,
                    conf: 0.97,
                }],
                confidence: 0.97,
                language: Some("en".into()),
            })
        }

        fn set_boost_terms(&mut self, terms: &[BoostTerm]) -> Result<(), SpeechError> {
            if terms.is_empty() {
                return Err(SpeechError::Transcribe("empty boost list".into()));
            }
            Ok(())
        }

        fn capabilities(&self) -> ModelCapabilities {
            ModelCapabilities {
                native_boost: true,
                word_timings: true,
            }
        }
    }

    #[test]
    fn overrides_are_used_through_a_dyn_provider() {
        let mut provider: Box<dyn SpeechProvider> = Box::new(DetailedMock);
        let t = provider.transcribe_detailed(&[], "auto").expect("detailed");
        assert_eq!(t.words.len(), 1);
        assert_eq!(t.words[0].end_ms, 300);
        assert_eq!(t.language.as_deref(), Some("en"));
        assert!(provider.capabilities().native_boost);
        assert!(provider.capabilities().word_timings);
        assert!(provider
            .set_boost_terms(&[BoostTerm {
                text: "Pod".into(),
                aliases: vec![],
                weight: 1.0,
            }])
            .is_ok());
        assert!(provider.set_boost_terms(&[]).is_err());
    }

    /// The real providers keep compiling against the extended trait and
    /// answer the new methods with the defaults.
    #[test]
    fn real_providers_answer_the_extended_trait_with_defaults() {
        let mut whisper: Box<dyn SpeechProvider> = Box::new(whisper::WhisperProvider::new());
        assert!(!whisper.capabilities().word_timings);
        assert!(whisper.set_boost_terms(&[]).is_ok());

        let mut parakeet: Box<dyn SpeechProvider> = Box::new(parakeet::ParakeetProvider::new());
        assert!(!parakeet.capabilities().native_boost);
        assert!(parakeet.set_boost_terms(&[]).is_ok());
    }
}
