//! Stub provider for builds without the FluidAudio Swift bridge: Windows,
//! Linux, and Intel macOS. The plan keeps the ggml Parakeet engine as the
//! fallback on those platforms — this provider reports the requirement and
//! never loads anything, so callers can probe availability without
//! cfg-gating their own code.

use std::path::Path;

use teletype_speech::{BoostTerm, ModelCapabilities, SpeechError, SpeechProvider, Transcript};

use crate::FluidVersion;

/// Unavailable-engine placeholder with the same constructor surface as the
/// real provider. [`SpeechProvider::load`] always fails with the platform
/// requirement; construction itself succeeds so callers can branch on
/// [`SpeechProvider::capabilities`] / `is_loaded` uniformly.
#[derive(Debug)]
pub struct FluidParakeet {
    version: FluidVersion,
}

impl FluidParakeet {
    /// Create an unloaded provider for `version` (never loads on any
    /// platform this stub is compiled for).
    pub fn new(version: FluidVersion) -> Self {
        Self { version }
    }

    /// The requested checkpoint family.
    pub fn version(&self) -> FluidVersion {
        self.version
    }

    /// Always false: this build has no bridge.
    pub fn is_ready(&self) -> bool {
        false
    }

    /// Always empty: no transcriptions happen here.
    pub fn last_detected_terms(&self) -> &[String] {
        &[]
    }

    /// Always empty: no transcriptions happen here.
    pub fn last_applied_terms(&self) -> &[String] {
        &[]
    }
}

impl SpeechProvider for FluidParakeet {
    fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
        Err(SpeechError::ModelLoad(format!(
            "Parakeet {} Core ML via FluidAudio needs macOS 14+ on Apple Silicon; \
             this build has no FluidAudio bridge",
            self.version
        )))
    }

    fn unload(&mut self) {}

    fn is_loaded(&self) -> bool {
        false
    }

    fn transcribe(&mut self, _samples: &[f32], _language: &str) -> Result<String, SpeechError> {
        Err(SpeechError::NoModel)
    }

    fn transcribe_detailed(
        &mut self,
        _samples: &[f32],
        _language: &str,
    ) -> Result<Transcript, SpeechError> {
        Err(SpeechError::NoModel)
    }

    fn set_boost_terms(&mut self, _terms: &[BoostTerm]) -> Result<(), SpeechError> {
        Err(SpeechError::NoModel)
    }

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
}
