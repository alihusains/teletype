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

/// A model that can turn a prompt into text.
pub trait InferenceProvider: Send + Sync {
    /// Stable id of the loaded model, e.g. "fast" or a path.
    fn model_id(&self) -> &str;

    /// Human-readable name for the UI.
    fn model_name(&self) -> &str;

    /// Generates text for `prompt`. Must not block for longer than
    /// `params.timeout` when avoidable.
    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String>;
}
