//! Non-macOS stub for the whisper.cpp backend.
//!
//! `whisper-rs` is only wired into this workspace for the macOS target. On
//! other targets this stub keeps the crate compiling and reports a clear
//! error if the provider is actually used. The native Windows port (W2/W3)
//! re-enables the real backend and drops this stub.

use std::path::Path;

use crate::{SpeechError, SpeechProvider};

/// A placeholder whisper provider that is never expected to run off-macOS.
pub struct WhisperProvider;

impl WhisperProvider {
    pub fn new() -> Self {
        Self
    }

    /// The language detected on the last auto-detect transcription, if any.
    pub fn detected_language(&self) -> Option<String> {
        None
    }
}

impl Default for WhisperProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeechProvider for WhisperProvider {
    fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
        Err(SpeechError::ModelLoad(
            "the whisper.cpp backend is not built for this platform".into(),
        ))
    }

    fn unload(&mut self) {}

    fn is_loaded(&self) -> bool {
        false
    }

    fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
        Err(SpeechError::NoModel)
    }

    fn detected_language(&self) -> Option<String> {
        None
    }
}
