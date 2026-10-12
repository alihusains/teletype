//! Needle 3 inference provider.
//!
//! Wraps the Cactus Compute Needle 3 engine (via `needle-sys`) as an
//! [`InferenceProvider`]. Unlike the `ServerProvider` (which spawns a
//! `llama-server` subprocess), Needle 3 runs in-process via a static C
//! library. The model is a single 29–35 MB `.cact` file loaded into memory.
//!
//! # Thread safety
//!
//! The Needle 3 C API is process-global and non-thread-safe. This provider
//! serializes all engine calls behind a `Mutex`. The `InferenceProvider`
//! trait is `Send + Sync`, so the mutex is required for the trait bound.
//!
//! # Telemetry
//!
//! The engine sends telemetry by default. This provider sets
//! `NEEDLE_TELEMETRY=0` and `DO_NOT_TRACK=1` before loading the model.

use std::sync::Mutex;

use teletype_core::llm::{GenerationParams, InferenceProvider};

/// A local model served by the in-process Needle 3 engine.
pub struct NeedleProvider {
    model_id: String,
    model_name: String,
    /// Serializes all engine calls (the C API is not thread-safe).
    engine: Mutex<()>,
}

impl NeedleProvider {
    /// Creates a new provider and loads the model.
    ///
    /// `path` is the `.cact` file. The model is loaded into the
    /// process-global engine. Loading is idempotent: if a model is already
    /// loaded, the call is a no-op (the engine cannot unload).
    pub fn new(model_id: &str, model_name: &str, path: &std::path::Path) -> Result<Self, String> {
        // Disable telemetry before the engine initializes.
        std::env::set_var("NEEDLE_TELEMETRY", "0");
        std::env::set_var("DO_NOT_TRACK", "1");

        needle_sys::load_model(path)
            .map_err(|e| format!("needle load failed: {e}"))?;

        Ok(Self {
            model_id: model_id.to_string(),
            model_name: model_name.to_string(),
            engine: Mutex::new(()),
        })
    }

    /// Initializes the engine with a system prompt and tool schemas.
    ///
    /// For the polish use case, `tools_json` is a single "polish" tool that
    /// declares the output shape. The grammar-constrained decode guarantees
    /// the output matches the schema.
    pub fn init(&self, system_prompt: Option<&str>, tools_json: Option<&str>) -> Result<usize, String> {
        let _guard = self
            .engine
            .lock()
            .map_err(|e| format!("needle engine poisoned: {e}"))?;
        needle_sys::init(system_prompt, tools_json)
            .map_err(|e| format!("needle init failed: {e}"))
    }

    /// Runs a text completion and returns the raw JSON response.
    fn complete(&self, input: &str, max_new_tokens: u32) -> Result<String, String> {
        let _guard = self
            .engine
            .lock()
            .map_err(|e| format!("needle engine poisoned: {e}"))?;
        needle_sys::complete_text(input, max_new_tokens)
            .map_err(|e| format!("needle complete failed: {e}"))
    }

    /// Extracts the polished text from a Needle 3 JSON response.
    ///
    /// The response shape for a single "polish" tool call is:
    /// ```json
    /// {
    ///   "type": "call",
    ///   "function_calls": [{"name": "polish", "arguments": {"text": "..."}}],
    ///   "confidence": 0.94,
    ///   ...
    /// }
    /// ```
    /// For a "respond" type (no tool call), the text is in `response`.
    fn extract_text(json_str: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("needle JSON parse: {e}"))?;

        // Tool-call path: the polished text is in function_calls[0].arguments.text.
        if let Some(calls) = v.get("function_calls").and_then(|c| c.as_array()) {
            if let Some(first) = calls.first() {
                if let Some(text) = first
                    .get("arguments")
                    .and_then(|a| a.get("text"))
                    .and_then(|t| t.as_str())
                {
                    return Ok(text.to_string());
                }
            }
        }

        // Respond path: the text is in `response`.
        if let Some(resp) = v.get("response").and_then(|r| r.as_str()) {
            return Ok(resp.to_string());
        }

        // Fallback: if there's a `text` field at the top level.
        if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
            return Ok(text.to_string());
        }

        Err("needle response has no extractable text".to_string())
    }
}

impl InferenceProvider for NeedleProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        // Enforce the wall-clock budget: Needle 3 is fast (~100–500 ms for
        // typical transcripts), but a pathological input could exceed the
        // budget. We check after the call rather than interrupting mid-call
        // because the C API has no cancellation mechanism.
        let started = std::time::Instant::now();
        let max_tokens = params.max_tokens.max(256);
        let raw = self.complete(prompt, max_tokens)?;
        let elapsed = started.elapsed();
        if elapsed > params.timeout {
            tracing::warn!(
                elapsed_ms = elapsed.as_millis(),
                budget_ms = params.timeout.as_millis(),
                "needle completion exceeded timeout budget"
            );
        }
        Self::extract_text(&raw)
    }

    fn generate_with_system(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
    ) -> Result<String, String> {
        // Needle 3's system prompt is set at init time, not per-call. For the
        // polish use case, the system prompt is a fixed instruction. We
        // re-init with the system prompt on each call (cheap: it just
        // re-tokenizes the static prefix).
        self.init(Some(system), None)
            .map_err(|e| format!("needle re-init failed: {e}"))?;
        self.generate(user, params)
    }

    fn context_tokens(&self) -> Option<u32> {
        // Needle 3 has an 8192-token context window.
        Some(8192)
    }

    fn is_local(&self) -> bool {
        true
    }

    fn stop(&self) {
        // The Needle 3 engine cannot be unloaded (process-global). Reset the
        // conversation state so the next init starts clean.
        let _guard = self.engine.lock();
        if let Ok(guard) = _guard {
            drop(guard);
            needle_sys::reset();
        }
    }

    fn shutdown(&self) {
        // Same as stop: the engine is process-global and cannot be unloaded.
        // Reset the conversation state.
        if let Ok(guard) = self.engine.lock() {
            drop(guard);
            needle_sys::reset();
        }
    }

    fn is_alive(&self) -> bool {
        // The engine is in-process; it's alive as long as the model is loaded.
        needle_sys::loaded_models() & needle_sys::NEEDLE_TEXT != 0
    }
}

/// The tool schema for the polish use case.
///
/// Declares a single "polish" tool with a `text` argument. The
/// grammar-constrained decode guarantees the output is a valid JSON object
/// with a `text` field — no preambles, no markdown fences, no off-topic text.
pub const POLISH_TOOLS_JSON: &str = r#"[{"name":"polish","description":"Clean up a voice transcript: fix punctuation, capitalization, filler words, and grammar. Return the cleaned text.","parameters":{"type":"object","properties":{"text":{"type":"string","description":"The cleaned transcript text"}},"required":["text"]}}]"#;

/// The system prompt for the polish use case.
///
/// Needle 3's system prompt carries "facts, never instructions" per the C
/// API docs. The polish instruction lives in the tool description.
pub const POLISH_SYSTEM_PROMPT: &str = "You are a transcript polisher. Clean up voice transcripts: fix punctuation, capitalization, remove filler words (um, uh, like, you know), and correct grammar. Preserve the user's meaning and word choices. Do not add or remove content.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_from_tool_call() {
        let json = r#"{
            "type": "call",
            "success": true,
            "function_calls": [{"name": "polish", "arguments": {"text": "Hello, world."}}],
            "confidence": 0.95
        }"#;
        let text = NeedleProvider::extract_text(json).unwrap();
        assert_eq!(text, "Hello, world.");
    }

    #[test]
    fn extract_text_from_response() {
        let json = r#"{
            "type": "respond",
            "response": "Cleaned text here."
        }"#;
        let text = NeedleProvider::extract_text(json).unwrap();
        assert_eq!(text, "Cleaned text here.");
    }

    #[test]
    fn extract_text_from_top_level_text() {
        let json = r#"{"text": "Fallback text"}"#;
        let text = NeedleProvider::extract_text(json).unwrap();
        assert_eq!(text, "Fallback text");
    }

    #[test]
    fn extract_text_empty_calls() {
        // Empty function_calls = refusal; no text to extract.
        let json = r#"{
            "type": "call",
            "function_calls": [],
            "confidence": 0.1
        }"#;
        assert!(NeedleProvider::extract_text(json).is_err());
    }

    #[test]
    fn polish_tools_json_is_valid() {
        let v: serde_json::Value = serde_json::from_str(POLISH_TOOLS_JSON).unwrap();
        assert!(v.is_array());
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["name"], "polish");
    }
}
