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

    /// True when the provider is a local model (e.g. a llama-server child
    /// process). P5.1: the polish gate only applies to local providers, where
    /// the 500 ms – 3 s polish pass is the dominant latency and skipping it
    /// is a net win. Remote providers are left untouched.
    fn is_local(&self) -> bool {
        false
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

    /// Streams generated text, invoking `on_token` for each token as it arrives.
    ///
    /// The default implementation runs the batch `generate` and delivers the whole
    /// result in one callback, so providers that don't support streaming still work.
    /// Providers that can stream (e.g. the local llama-server over SSE) override this
    /// to call `on_token` per token. Returns the full concatenated text on success,
    /// or an error (in which case `on_token` may have been called for partial output).
    fn generate_stream(
        &self,
        prompt: &str,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let out = self.generate(prompt, params)?;
        on_token(&out);
        Ok(out)
    }

    /// Streaming variant of `generate_with_system` — same contract, chat-style.
    fn generate_with_system_stream(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let _ = system;
        self.generate_stream(user, params, on_token)
    }

    /// Releases any resources the provider holds, such as a child
    /// `llama-server` subprocess. Idempotent and non-blocking-safe: callers on
    /// the shutdown path may run it off the async runtime. The default is a
    /// no-op for providers that own no external process.
    fn shutdown(&self) {}
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

    struct MockProvider {
        model_id: &'static str,
        output: String,
    }

    impl InferenceProvider for MockProvider {
        fn model_id(&self) -> &str {
            self.model_id
        }

        fn model_name(&self) -> &str {
            "mock"
        }

        fn generate(&self, _prompt: &str, _params: GenerationParams) -> Result<String, String> {
            Ok(self.output.clone())
        }
    }

    #[test]
    fn default_generate_stream_delivers_batch_output_in_one_callback() {
        let provider = MockProvider {
            model_id: "mock",
            output: "full batch output".to_string(),
        };
        let mut seen = Vec::new();
        let out = provider
            .generate_stream("hi", GenerationParams::default(), &mut |t| {
                seen.push(t.to_string())
            })
            .unwrap();
        assert_eq!(out, "full batch output");
        assert_eq!(
            seen,
            vec!["full batch output"],
            "callback must fire exactly once"
        );
    }

    #[test]
    fn default_generate_with_system_stream_ignores_system_and_uses_batch() {
        let provider = MockProvider {
            model_id: "mock",
            output: "batch result".to_string(),
        };
        let mut seen = Vec::new();
        let out = provider
            .generate_with_system_stream(
                "system prompt",
                "user prompt",
                GenerationParams::default(),
                &mut |t| seen.push(t.to_string()),
            )
            .unwrap();
        assert_eq!(out, "batch result");
        assert_eq!(seen, vec!["batch result"]);
    }
}
