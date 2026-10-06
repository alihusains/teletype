//! OpenAI-compatible chat completions provider.
//!
//! One connector covers OpenAI, OpenRouter, Groq, Together, DeepSeek,
//! LM Studio, Ollama (`http://127.0.0.1:11434/v1`), and any custom
//! `/v1/chat/completions` endpoint.

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use teletype_core::llm::{GenerationParams, InferenceProvider};

/// Configuration for an OpenAI-compatible endpoint.
#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    /// Base URL including `/v1` (or equivalent), no trailing `/chat/completions`.
    pub base_url: String,
    /// Bearer token; `None` for local endpoints that need no auth (Ollama).
    pub api_key: Option<String>,
    /// Model id sent in the request body (e.g. `gpt-4o-mini`, `qwen3`).
    pub model: String,
}

impl OpenAiCompatConfig {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into();
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            api_key: api_key
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty()),
            model: model.into(),
        }
    }
}

/// Talks to `{base_url}/chat/completions`.
pub struct OpenAiCompatProvider {
    model_id: String,
    model_name: String,
    config: OpenAiCompatConfig,
}

impl OpenAiCompatProvider {
    pub fn new(id: impl Into<String>, name: impl Into<String>, config: OpenAiCompatConfig) -> Self {
        Self {
            model_id: id.into(),
            model_name: name.into(),
            config,
        }
    }

    pub fn config(&self) -> &OpenAiCompatConfig {
        &self.config
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.config.base_url)
    }

    fn list_models_url(&self) -> String {
        format!("{}/models", self.config.base_url)
    }

    fn build_client(&self, timeout: Duration) -> Result<reqwest::blocking::Client, String> {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| format!("http client: {e}"))
    }

    fn send_chat(
        &self,
        client: &reqwest::blocking::Client,
        body: &Value,
    ) -> Result<reqwest::blocking::Response, String> {
        let mut req = client.post(self.chat_url()).json(body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        req.send().map_err(|e| format!("request failed: {e}"))
    }

    /// Lists model ids from `GET {base}/models` when the endpoint supports it.
    pub fn list_models(&self, timeout: Duration) -> Result<Vec<String>, String> {
        let client = self.build_client(timeout)?;
        let mut req = client.get(self.list_models_url());
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send().map_err(|e| format!("request failed: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().unwrap_or_default();
            let text: String = text.chars().take(300).collect();
            return Err(format!("HTTP {status}: {text}"));
        }
        let value: Value = resp.json().map_err(|e| format!("decode: {e}"))?;
        let mut ids = Vec::new();
        if let Some(arr) = value.as_array() {
            for item in arr {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    ids.push(id.to_string());
                }
            }
        }
        Ok(ids)
    }

    /// One cheap round-trip used by the UI "Test connection" button.
    pub fn test_connection(&self, timeout: Duration) -> Result<String, String> {
        match self.list_models(timeout) {
            Ok(ids) if !ids.is_empty() => {
                // P0-6: check that the configured model is in the list.
                if ids.iter().any(|id| id == &self.config.model) {
                    Ok(format!("{} models available", ids.len()))
                } else {
                    let closest = ids
                        .iter()
                        .min_by_key(|id| {
                            // Simple prefix/substring proximity score.
                            if id.starts_with(&self.config.model)
                                || self.config.model.starts_with(id.as_str())
                            {
                                0
                            } else if id.contains(&self.config.model)
                                || self.config.model.contains(id.as_str())
                            {
                                1
                            } else {
                                2
                            }
                        })
                        .cloned()
                        .unwrap_or_default();
                    Ok(format!(
                        "Connected, but model '{}' was not found. Did you mean '{}'?",
                        self.config.model, closest
                    ))
                }
            }
            Ok(_) => Ok("Connected (no models listed)".into()),
            Err(first) => {
                // Some endpoints (older Ollama) may not expose /models;
                // fall back to a one-token chat call.
                if Self::is_timeout_error(&first) {
                    // The host is unreachable or did not answer within the
                    // window. Do not burn a second full timeout on the
                    // fallback call — report it immediately.
                    return Err(Self::timeout_message());
                }
                let client = self.build_client(timeout)?;
                let mut body = json!({
                    "model": self.config.model,
                    "messages": [{ "role": "user", "content": "ping" }],
                    "max_tokens": 1,
                    "stream": false,
                });
                if !Self::is_reasoning_model(&self.config.model) {
                    body["temperature"] = json!(0.0);
                }
                match self.send_chat(&client, &body) {
                    Ok(resp) => {
                        let status = resp.status();
                        if status.is_success() {
                            Ok("Connected".into())
                        } else {
                            let text = resp.text().unwrap_or_default();
                            let (msg, _retryable) = Self::classify_http_error(status.as_u16(), &text);
                            Err(msg)
                        }
                    }
                    Err(e) => Err(Self::display_request_error(&e)),
                }
            }
        }
    }
    /// True when the request failed because the host did not answer in time
    /// (or could not be reached at all), as opposed to an HTTP-level error.
    fn is_timeout_error(msg: &str) -> bool {
        msg.contains("timed out")
            || msg.contains("timed_out")
            || msg.contains("deadline")
            || msg.contains("dns error")
            || msg.contains("resolve")
            || msg.contains("connection refused")
            || msg.contains("network is unreachable")
            || msg.contains("no route to host")
    }

    fn timeout_message() -> String {
        "Connection timed out. Check the host and base URL, and that the server is reachable from this machine.".into()
    }

    /// Turns a send error into a message that says what actually happened
    /// (timeout vs. refused vs. DNS) instead of a raw error dump.
    fn display_request_error(e: &str) -> String {
        if Self::is_timeout_error(e) {
            Self::timeout_message()
        } else {
            format!("Connection failed: {e}")
        }
    }

    /// Classifies an HTTP error into a user-facing message (P0-5).
    /// Returns `(message, retryable)`.
    fn classify_http_error(status: u16, body: &str) -> (String, bool) {
        match status {
            401 => (
                "API key rejected. Re-enter it in Settings > Models.".into(),
                false,
            ),
            403 => (
                "Access denied. Check your billing or access permissions.".into(),
                false,
            ),
            429 if body.contains("insufficient_quota") => {
                ("Out of credits. Check your provider billing.".into(), false)
            }
            429 => ("Rate limited. Try again in a moment.".into(), true),
            _ => {
                let short: String = body.chars().take(200).collect();
                (
                    format!("HTTP {status}: {short}"),
                    (500..600).contains(&status),
                )
            }
        }
    }

    /// Returns true when the model id is a reasoning model that rejects
    /// `temperature` (P0-4). o1, o3, o4, gpt-5 non-chat variants.
    fn is_reasoning_model(model: &str) -> bool {
        let m = model.to_lowercase();
        m.starts_with("o1")
            || m.starts_with("o3")
            || m.starts_with("o4")
            || (m.starts_with("gpt-5") && !m.contains("instruct"))
    }
}

impl InferenceProvider for OpenAiCompatProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        let messages = json!([{ "role": "user", "content": prompt }]);
        self.chat_completion(messages, params)
    }

    fn generate_with_system(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
    ) -> Result<String, String> {
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);
        self.chat_completion(messages, params)
    }
}

impl OpenAiCompatProvider {
    fn chat_completion(&self, messages: Value, params: GenerationParams) -> Result<String, String> {
        let client = self.build_client(params.timeout)?;
        // P0-4: reasoning models (o1/o3/o4/gpt-5) reject temperature.
        let mut body = json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": params.max_tokens,
            "stream": false,
        });
        if !Self::is_reasoning_model(&self.config.model) {
            body["temperature"] = json!(params.temperature);
        }

        let deadline = Instant::now() + params.timeout;
        let mut last_err = String::from("request failed");
        // One retry on 5xx / network error, within the generation budget.
        for attempt in 0..2 {
            if Instant::now() >= deadline {
                break;
            }
            match self.send_chat(&client, &body) {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let value: Value = resp.json().map_err(|e| format!("decode: {e}"))?;
                        // P0-7: truncated output is still better than raw
                        // input; return the partial content with a warning.
                        if value["choices"][0]["finish_reason"].as_str() == Some("length") {
                            let partial = value["choices"][0]["message"]["content"]
                                .as_str()
                                .map(|s| s.trim().to_string())
                                .unwrap_or_default();
                            if !partial.is_empty() {
                                tracing::warn!(
                                    model = %self.config.model,
                                    "polish output truncated at max_tokens; using partial text"
                                );
                                return Ok(partial);
                            }
                            return Err("stopped at max_tokens (truncated output)".into());
                        }
                        return value["choices"][0]["message"]["content"]
                            .as_str()
                            .map(|s| s.trim().to_string())
                            .ok_or_else(|| "response missing message content".to_string());
                    }
                    // P0-5: classify errors into user-facing messages.
                    let text = resp.text().unwrap_or_default();
                    let (msg, retryable) = Self::classify_http_error(status.as_u16(), &text);
                    last_err = msg;
                    if !retryable || attempt == 1 {
                        return Err(last_err);
                    }
                }
                Err(e) => {
                    last_err = e;
                    if attempt == 1 {
                        return Err(last_err);
                    }
                }
            }
        }
        Err(last_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_url_trims_trailing_slash() {
        let cfg = OpenAiCompatConfig::new("http://127.0.0.1:11434/v1/", None, "qwen3");
        assert_eq!(cfg.base_url, "http://127.0.0.1:11434/v1");
        let p = OpenAiCompatProvider::new("ollama", "Ollama", cfg);
        assert_eq!(p.config().base_url, "http://127.0.0.1:11434/v1");
    }

    #[test]
    fn empty_api_key_becomes_none() {
        let cfg = OpenAiCompatConfig::new(
            "https://api.openai.com/v1",
            Some("  ".into()),
            "gpt-4o-mini",
        );
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn generate_without_server_errors_fast() {
        let cfg = OpenAiCompatConfig::new("http://127.0.0.1:1", None, "test");
        let p = OpenAiCompatProvider::new("t", "t", cfg);
        let params = GenerationParams {
            timeout: Duration::from_millis(500),
            ..Default::default()
        };
        let err = p.generate("hi", params).unwrap_err();
        assert!(
            err.contains("request failed") || err.contains("HTTP"),
            "got: {err}"
        );
    }

    #[test]
    fn classify_http_error_401() {
        let (msg, retryable) = OpenAiCompatProvider::classify_http_error(401, "Unauthorized");
        assert!(msg.contains("API key rejected"), "got: {msg}");
        assert!(!retryable);
    }

    #[test]
    fn classify_http_error_403() {
        let (msg, retryable) = OpenAiCompatProvider::classify_http_error(403, "Forbidden");
        assert!(msg.contains("Access denied"), "got: {msg}");
        assert!(!retryable);
    }

    #[test]
    fn classify_http_error_429_quota() {
        let body = r#"{"error":{"code":"insufficient_quota"}}"#;
        let (msg, retryable) = OpenAiCompatProvider::classify_http_error(429, body);
        assert!(msg.contains("Out of credits"), "got: {msg}");
        assert!(!retryable);
    }

    #[test]
    fn classify_http_error_429_rate_limit() {
        let (msg, retryable) = OpenAiCompatProvider::classify_http_error(429, "rate limited");
        assert!(msg.contains("Rate limited"), "got: {msg}");
        assert!(retryable);
    }

    #[test]
    fn classify_http_error_500() {
        let (msg, retryable) =
            OpenAiCompatProvider::classify_http_error(500, "Internal Server Error");
        assert!(msg.contains("HTTP 500"), "got: {msg}");
        assert!(retryable);
    }

    #[test]
    fn is_reasoning_model_detects_o_series() {
        assert!(OpenAiCompatProvider::is_reasoning_model("o1"));
        assert!(OpenAiCompatProvider::is_reasoning_model("o1-mini"));
        assert!(OpenAiCompatProvider::is_reasoning_model("o3"));
        assert!(OpenAiCompatProvider::is_reasoning_model("o3-mini"));
        assert!(OpenAiCompatProvider::is_reasoning_model("o4-mini"));
        assert!(OpenAiCompatProvider::is_reasoning_model("gpt-5"));
        assert!(!OpenAiCompatProvider::is_reasoning_model("gpt-4o"));
        assert!(!OpenAiCompatProvider::is_reasoning_model("gpt-4o-mini"));
        assert!(!OpenAiCompatProvider::is_reasoning_model("qwen3"));
    }

    #[test]
    fn is_reasoning_model_allows_gpt5_instruct() {
        // gpt-5-instruct-style models may accept temperature.
        assert!(!OpenAiCompatProvider::is_reasoning_model("gpt-5-instruct"));
    }
}
