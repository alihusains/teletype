//! Stub provider for builds without the Apple speech Swift bridge: Windows,
//! Linux, Intel macOS, and macOS builds without the Swift toolchain
//! (`TELETYPE_APPLE_DISABLE_SWIFT=1`). The other engines remain the fallback
//! on those platforms; this provider reports the requirement and never
//! loads anything, so callers can probe availability without cfg-gating
//! their own code.

use std::path::Path;

use teletype_speech::{ModelCapabilities, SpeechError, SpeechProvider, Transcript};

use crate::AppleModule;

/// Unavailable-engine placeholder with the same constructor surface as the
/// real provider. [`SpeechProvider::load`] always fails with the platform
/// requirement; construction itself succeeds so callers can branch on
/// [`SpeechProvider::capabilities`] / `is_loaded` uniformly.
#[derive(Debug)]
pub struct AppleSpeech {
    module: AppleModule,
}

impl AppleSpeech {
    /// Create an unloaded provider for `module` (never loads on any platform
    /// this stub is compiled for).
    pub fn new(module: AppleModule) -> Self {
        Self { module }
    }

    /// The requested transcriber module.
    pub fn module(&self) -> AppleModule {
        self.module
    }

    /// Always false: this build has no bridge.
    pub fn is_ready(&self) -> bool {
        false
    }
}

impl SpeechProvider for AppleSpeech {
    fn load(&mut self, _path: &Path) -> Result<(), SpeechError> {
        Err(SpeechError::ModelLoad(format!(
            "Apple on-device speech ({}) needs macOS 26+ and the TeletypeApple \
             Swift bridge; this build has no bridge",
            self.module
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

    fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities::default()
    }
}

/// The installed-locale asset status the app reports to the UI. Mirrors the
/// real bridge's enum so callers compile on every target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetStatus {
    /// The locale is in `SpeechTranscriber.installedLocales`: ready now.
    Installed,
    /// The locale is supported but not installed: the first `load` downloads
    /// it (bounded by [`INSTALL_TIMEOUT_SECS`]).
    Supported,
    /// The locale is supported by neither module: `load` fails.
    Unsupported,
}

/// The Swift-side install timeout, mirrored so the Rust side can document the
/// worst case to the user ("up to 10 minutes").
pub const INSTALL_TIMEOUT_SECS: u64 = 600;

/// No bridge, so no locale is ever installed: always `Unsupported`.
pub fn locale_status(_module: AppleModule, _locale: &str) -> AssetStatus {
    AssetStatus::Unsupported
}

/// No bridge, so there is nothing to install.
pub fn install_assets(_locale: &str, _progress: &mut dyn FnMut(f64)) -> Result<(), SpeechError> {
    Err(SpeechError::ModelLoad(
        "Apple on-device speech needs the TeletypeApple Swift bridge, which this build does not include".into(),
    ))
}

/// No bridge, so no locale list exists.
pub fn supported_locales() -> Result<Vec<String>, SpeechError> {
    Err(SpeechError::ModelLoad(
        "Apple on-device speech needs the TeletypeApple Swift bridge, which this build does not include".into(),
    ))
}
