//! The inference boundary.
//!
//! Core depends only on [`InferenceProvider`] and [`DecisionProvider`]; the
//! concrete runtimes (llama.cpp GGUF, Needle 3, future remote providers)
//! live in `teletype-inference`. Keeping the traits here means the transform
//! engine, pipeline and tests compile without any model code.

use std::time::Duration;

// ── Decision boundary (Needle 3: classify + embed) ─────────────────────────

/// The intent of a transcript, decided by a fast on-device model.
///
/// Two intents:
/// - `Dictation` — normal speech. Always goes through the LLM polish pass.
/// - `Command` — a system command ("open safari", "set brightness").
///   Skips polish; Needle 3 extracts a structured tool call for execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Intent {
    /// Normal speech to be polished and injected (the common case).
    Dictation,
    /// A command ("open safari", "set a timer"). Skip polish; route to a
    /// command executor.
    Command,
}

impl Intent {
    /// True when the transcript should go through the LLM polish pass.
    /// Only `Dictation` polishes.
    pub fn needs_polish(self) -> bool {
        matches!(self, Self::Dictation)
    }

    /// Short label for tracing / UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Command => "command",
        }
    }
}

/// A structured command extracted from a voice transcript by a tool-calling
/// model (Needle 3). The pipeline stores this in [`crate::pipeline::PipelineResult`]
/// so the desktop app can execute the action (open an app, set brightness,
/// etc.) without any further LLM round-trip.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandAction {
    /// The tool name, e.g. "open_app", "set_brightness", "mute_speaker".
    pub tool: String,
    /// The structured arguments, e.g. `{"app": "safari"}`.
    pub arguments: serde_json::Value,
}

/// A fast on-device model that makes decisions (classify, embed, extract)
/// without generating free text.
///
/// This is the boundary for Needle 3 (and similar small tool-calling
/// models). Unlike [`InferenceProvider`] (which generates text for the
/// polish pass), a `DecisionProvider` answers bounded questions in a
/// single fast pass (~20 ms) and produces structured output or vectors.
///
/// Implementations must be `Send + Sync` (the pipeline may call them from
/// any thread). Thread-safety of the underlying engine is the
/// implementor's concern (e.g. Needle 3 serializes behind a mutex).
pub trait DecisionProvider: Send + Sync {
    /// Stable id of the decision model, e.g. "needle3".
    fn model_id(&self) -> &str;

    /// Classifies the intent of a transcript.
    ///
    /// Must be fast (~20 ms) and must not block for longer than ~100 ms.
    /// On failure, return `Intent::Dictation` (the safe default: run the
    /// polish pass, which is the existing behavior).
    fn classify_intent(&self, text: &str) -> Intent;

    /// Embeds text into a fixed-dimension vector for semantic matching.
    ///
    /// Returns `None` when the provider doesn't support embedding or the
    /// call fails. The pipeline stores the vector for personalization
    /// (similar-utterance matching) but never blocks on it.
    fn embed_text(&self, text: &str) -> Option<Vec<f32>>;

    /// Extracts a structured command from a transcript classified as
    /// `Intent::Command`. Uses the model's tool-calling capability to
    /// produce a typed action (e.g. `open_app("safari")`).
    ///
    /// Returns `None` when the provider doesn't support command extraction
    /// or the call fails. The pipeline falls back to injecting the raw
    /// text (the existing behavior) when this returns `None`.
    fn extract_command(&self, _text: &str) -> Option<CommandAction> {
        None
    }
}

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

/// P6.5: computes the `max_tokens` budget for the generic (non-S1) polish
/// path.
///
/// Formula: `ceil(1.4 × input_tokens) + 24`, floor 128, ceiling 4096.
///
/// The token estimate is `chars / 4` (documented approximation; CJK
/// under-budgets, which is acceptable because the floor of 128 protects
/// short inputs like greetings, and the ceiling of 4096 bounds the worst
/// case). This replaces the old `chars/3 + 100` clamp of 256..2048, which
/// could silently truncate long outputs (G007) or waste context on short
/// ones.
pub fn polish_max_tokens(input: &str) -> u32 {
    const FLOOR: u32 = 128;
    const CEILING: u32 = 4096;
    let input_tokens = input.chars().count() as u32 / 4;
    let budget = input_tokens.saturating_mul(14) / 10 + 24; // ceil(1.4×) + 24
    budget.clamp(FLOOR, CEILING)
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

    /// Stops whatever the provider is running (e.g. kills its child
    /// `llama-server` process) and waits until the resources are released.
    /// Must be non-blocking-safe for the caller: implementations that block
    /// should document that. The default is a no-op, so providers without
    /// external processes need no change.
    ///
    /// Called by `select_model` *before* the replacement provider is spawned,
    /// so at most one heavy process is resident at any point during a model
    /// switch (BUG-007).
    fn stop(&self) {}

    /// Chat-style generation with a separate system message. Used by models
    /// fine-tuned on a fixed system+user split (e.g. S1-mini). The default
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

    /// JSON-constrained chat generation (P2.7).
    ///
    /// Same contract as [`InferenceProvider::generate_with_system`], with
    /// the *shape* of the output additionally constrained to `schema` when
    /// the backend supports grammar-constrained decoding (llama-server's
    /// `json_schema` request field). Providers without that support ignore
    /// the schema and return free text: callers must parse defensively and
    /// treat anything that does not match the schema as a failure, never as
    /// trustworthy output.
    fn generate_json(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
        schema: &serde_json::Value,
    ) -> Result<String, String> {
        let _ = schema;
        self.generate_with_system(system, user, params)
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
    /// no-op, so providers without external processes need no change.
    fn shutdown(&self) {}

    /// True when the provider's backend is still alive. For a subprocess
    /// provider this catches a server that crashed after warm-up; the status
    /// UI must not say "loaded" for a dead process. The default is `true`:
    /// providers without an external process (in-process or remote) have
    /// nothing to die.
    fn is_alive(&self) -> bool {
        true
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

    #[test]
    fn polish_max_tokens_formula() {
        // 4 chars = 1 token → ceil(1.4×1)+24 = 1+24 = 25 → floor 128.
        assert_eq!(polish_max_tokens("abcd"), 128);
        // "hi" = 2 chars = 0 tokens → 0+24 = 24 → floor 128.
        assert_eq!(polish_max_tokens("hi"), 128);
        // 400 chars = 100 tokens → ceil(140)+24 = 164.
        assert_eq!(polish_max_tokens(&"a".repeat(400)), 164);
        // 1600 chars = 400 tokens → ceil(560)+24 = 584.
        assert_eq!(polish_max_tokens(&"a".repeat(1600)), 584);
        // 11504 chars = 2876 tokens → 2876×14/10+24 = 4026+24 = 4050.
        assert_eq!(polish_max_tokens(&"a".repeat(11504)), 4050);
        // 12000 chars = 3000 tokens → ceil(4200)+24 = 4224 → ceiling 4096.
        assert_eq!(polish_max_tokens(&"a".repeat(12000)), 4096);
    }

    #[test]
    fn polish_max_tokens_floor_protects_short_inputs() {
        // A 1-word greeting must not get a max_tokens below 128.
        assert!(polish_max_tokens("hi") >= 128);
        assert!(polish_max_tokens("hello") >= 128);
        assert!(polish_max_tokens("") >= 128);
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
