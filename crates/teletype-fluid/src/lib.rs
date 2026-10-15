//! FluidAudio (Parakeet Core ML / Neural Engine) speech provider — macOS 14+
//! Apple Silicon only.
//!
//! Plan §3.1: an in-process Swift static library (FluidAudio, pinned
//! `exact: 0.17.7` in `swift/Package.swift`) called from Rust through a C ABI.
//! No CLI sidecar runs on the hot path.
//!
//! ## Platform split
//!
//! - **macOS on Apple Silicon**: [`FluidParakeet`] wraps the Swift bridge
//!   (`src/bridge.rs`, `swift/Sources/TeletypeFluid/Bridge.swift`). Loading
//!   downloads the Core ML models on first run into
//!   `<models dir>/<repo folder>/`; transcription runs on the Neural Engine.
//! - **Everything else** (Windows, Linux, Intel macOS): the crate compiles a
//!   stub provider that reports the platform requirement from
//!   [`SpeechProvider::load`]. The ggml Parakeet engine remains the fallback
//!   there — this crate never silently pretends to work.
//!
//! All bridge calls block the calling thread, and the first `load` may
//! download a model: run them from a worker thread, never from the UI thread.

mod format;

#[cfg(teletype_fluid_swift)]
mod bridge;
#[cfg(teletype_fluid_swift)]
pub use bridge::FluidParakeet;

#[cfg(not(teletype_fluid_swift))]
mod unsupported;
#[cfg(not(teletype_fluid_swift))]
pub use unsupported::FluidParakeet;

/// Parakeet checkpoint family requested from FluidAudio.
///
/// FluidAudio also ships internal variants Teletype does not expose (redux,
/// phonon2); these three are the ones the plan picks between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidVersion {
    /// Parakeet Ultra (moondream's full-precision post-training) — the plan's
    /// macOS 14+ Apple Silicon default.
    Ultra,
    /// Parakeet TDT 0.6B v2 — the reference engine on every platform.
    V2,
    /// Parakeet TDT 0.6B v3 (multilingual).
    V3,
}

impl FluidVersion {
    /// The bridge's wire name (`ttf_init` takes this string).
    pub fn as_str(self) -> &'static str {
        match self {
            FluidVersion::Ultra => "ultra",
            FluidVersion::V2 => "v2",
            FluidVersion::V3 => "v3",
        }
    }
}

impl std::fmt::Display for FluidVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// True when this build links the FluidAudio Swift bridge (macOS Apple
/// Silicon builds only). Callers can use it to pick the Neural Engine engine
/// at runtime without repeating the cfg.
pub const fn bridge_built() -> bool {
    cfg!(teletype_fluid_swift)
}

#[cfg(test)]
mod tests {
    use super::*;
    use teletype_speech::SpeechProvider;

    #[test]
    fn version_wire_names_match_the_bridge_contract() {
        // Bridge.swift parses exactly these three strings.
        assert_eq!(FluidVersion::Ultra.as_str(), "ultra");
        assert_eq!(FluidVersion::V2.as_str(), "v2");
        assert_eq!(FluidVersion::V3.as_str(), "v3");
        assert_eq!(FluidVersion::Ultra.to_string(), "ultra");
    }

    #[test]
    fn provider_reports_no_detected_language() {
        let provider = FluidParakeet::new(FluidVersion::Ultra);
        assert_eq!(provider.detected_language(), None);
    }

    /// The unsafe calls only run when the bridge is linked; without it the
    /// stub errors before touching anything (and `load` is never called
    /// here because it would download models).
    #[test]
    fn transcribe_without_models_is_a_no_model_error() {
        let mut provider = FluidParakeet::new(FluidVersion::V2);
        let error = provider
            .transcribe(&[0.0; 160], "en")
            .expect_err("unloaded provider must refuse");
        assert!(matches!(error, teletype_speech::SpeechError::NoModel));
    }
}
