//! Speech-to-text providers for Teletype.
//!
//! The rest of the application depends only on [`SpeechProvider`]; the
//! whisper.cpp backend is one implementation, not the architecture.

pub mod catalog;
pub mod parakeet;
pub mod whisper;

pub use catalog::{find, SpeechModel, CATALOG};
pub use parakeet::ParakeetProvider;

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
