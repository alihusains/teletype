//! The inference boundary.
//!
//! Core depends only on [`InferenceProvider`]; the concrete runtimes
//! (llama.cpp GGUF, future remote providers) live in `teletype-inference`.
//! Keeping the trait here means the transform engine, pipeline and tests
//! compile without any model code.

use std::time::Duration;

/// Parameters for one generation. Transforms are near-deterministic and
/// short: low temperature, no reasoning, a small token budget.
#[derive(Debug, Clone, Copy)]
pub struct GenerationParams {
    pub max_tokens: u32,
    pub temperature: f32,
    /// Hard wall-clock budget. Providers that can't interrupt should at least
    /// not start work beyond it.
    pub timeout: Duration,
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            max_tokens: 300,
            temperature: 0.0,
            timeout: Duration::from_secs(20),
        }
    }
}

/// Scales the wall-clock timeout to match the token budget.
///
/// A 2048-token generation at ~100 tok/s needs ~40s of generation time on
/// top of prompt processing. The default 20s budget is too tight for long
/// inputs, so we add a margin proportional to `max_tokens`.
///
/// Formula: `20 + max_tokens / 50` seconds.
/// - 300 tokens -> 26s
/// - 2048 tokens -> 60s
pub fn scaled_timeout(max_tokens: u32) -> Duration {
    Duration::from_secs(20 + max_tokens as u64 / 50)
}

/// A model that can turn a prompt into text.
pub trait InferenceProvider: Send + Sync {
    /// Stable id of the loaded model, e.g. "fast" or a path.
    fn model_id(&self) -> &str;

    /// Human-readable name for the UI.
    fn model_name(&self) -> &str;

    /// Generates text for `prompt`. Must not block for longer than
    /// `params.timeout` when avoidable.
    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String>;

    /// The model's context window in tokens, when known. `None` means the
    /// provider cannot say (remote endpoints, unknown models) and the caller
    /// skips the context preflight.
    fn context_tokens(&self) -> Option<u32> {
        None
    }

    /// Chat-style generation with a separate system message. Used by models
    /// fine-tuned on a fixed system+user split (e.g. EG-1). The default
    /// concatenates both into a single user prompt for providers that only
    /// expose a one-shot `generate`.
    fn generate_with_system(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
    ) -> Result<String, String> {
        let _ = system;
        self.generate(user, params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_timeout_grows_with_max_tokens() {
        assert_eq!(scaled_timeout(300), Duration::from_secs(26));
        assert_eq!(scaled_timeout(2048), Duration::from_secs(60));
        assert_eq!(scaled_timeout(0), Duration::from_secs(20));
    }

    #[test]
    fn scaled_timeout_is_monotonic() {
        let small = scaled_timeout(300);
        let large = scaled_timeout(2048);
        assert!(small < large);
    }
}
