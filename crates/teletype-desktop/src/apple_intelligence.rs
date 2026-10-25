//! Apple Intelligence system-model provider (roadmap P1.4).
//!
//! macOS 26+ and Apple Silicon only. The weights live inside the OS, so there
//! is nothing to download or delete; "installed" means
//! `SystemLanguageModel::is_available()`. The model runs in-process (no child
//! process, no warm-up), so `stop`/`shutdown` are no-ops and `is_local` is
//! `true` (it is a local model, so the polish-skip gate applies).
//!
//! The whole module is `#[cfg(target_os = "macos")]` (see `lib.rs`); the
//! `foundation-models` dep is target-gated to macOS, so none of this is
//! compiled for the windows-latest CI job.

use teletype_core::llm::{GenerationParams, InferenceProvider};

/// Stable id used across settings, the model list, and the select command.
pub const MODEL_ID: &str = "apple-intelligence";

/// Display name for the UI.
pub const MODEL_NAME: &str = "Apple Intelligence";

/// True only when the OS actually has the model: macOS 26+, Apple Silicon, and
/// Apple Intelligence enabled in System Settings. The `foundation-models`
/// crate stubs its FFI on older SDKs, so this returns `false` (not a crash) on
/// a pre-26 or Intel Mac. Cheap; safe to call on every list/select.
pub fn apple_intelligence_available() -> bool {
    foundation_models::SystemLanguageModel::is_available()
}

/// The system model as a polish/transform engine. No warm-up, no child
/// process: `LanguageModelSession::new` is the whole "load".
pub struct AppleIntelligenceProvider {
    session: foundation_models::LanguageModelSession,
}

impl AppleIntelligenceProvider {
    /// Builds a ready provider, or an error when the session cannot be created
    /// (the model is not actually available at this moment).
    pub fn new() -> Result<Self, String> {
        let session = foundation_models::LanguageModelSession::new().map_err(|e| e.to_string())?;
        Ok(Self { session })
    }
}

impl InferenceProvider for AppleIntelligenceProvider {
    fn model_id(&self) -> &str {
        MODEL_ID
    }

    fn model_name(&self) -> &str {
        MODEL_NAME
    }

    fn generate(&self, prompt: &str, _params: GenerationParams) -> Result<String, String> {
        // The system model has no stable temperature/max_tokens surface exposed
        // by this crate's `respond`; the prompt is the contract, matching how
        // the transform engine calls a one-shot generation.
        self.session.respond(prompt).map_err(|e| e.to_string())
    }

    fn context_tokens(&self) -> Option<u32> {
        // The OS does not publish a stable context window for the system model,
        // so the caller skips the context preflight.
        None
    }

    fn is_local(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_gate_does_not_panic() {
        // The gate must run everywhere on macOS; on a host without the model it
        // is simply `false`. We assert only that the call returns (no panic),
        // not its value, so the test holds on Intel / pre-26 CI hosts.
        let _ = apple_intelligence_available();
    }

    #[test]
    fn identity_is_stable_and_local() {
        // Requires a real session (model present). Skip when unavailable so the
        // test is not platform-fragile.
        if !apple_intelligence_available() {
            return;
        }
        let p = AppleIntelligenceProvider::new().expect("session on an available host");
        assert_eq!(p.model_id(), MODEL_ID);
        assert_eq!(p.model_name(), MODEL_NAME);
        assert!(p.is_local());
        assert_eq!(p.context_tokens(), None);
    }
}
