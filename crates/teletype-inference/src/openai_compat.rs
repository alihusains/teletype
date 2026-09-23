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
        let value: Value = resp
            .json()
            .map_err(|e| format!("decode: {e}"))?;
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
            Ok(ids) if !ids.is_empty() => Ok(format!("{} models available", ids.len())),
            Ok(_) => Ok("Connected (no models listed)".into()),
            Err(e) => {
                // Some endpoints (older Ollama) may not expose /models;
                // fall back to a one-token chat call.
                let _ = e;
                let client = self.build_client(timeout)?;
                let body = json!({
                    "model": self.config.model,
                    "messages": [{ "role": "user", "content": "ping" }],
                    "max_tokens": 1,
                    "temperature": 0.0,
                    "stream": false,
                });
                let resp = self.send_chat(&client, &body)?;
                let status = resp.status();
                if status.is_success() {
                    Ok("Connected".into())
                } else {
                    let text = resp.text().unwrap_or_default();
                    let text: String = text.chars().take(300).collect();
                    Err(format!("HTTP {status}: {text}"))
                }
            }
        }
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
        let body = json!({
            "model": self.config.model,
            "messages": messages,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "stream": false,
        });

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
                        let value: Value = resp
                            .json()
                            .map_err(|e| format!("decode: {e}"))?;
                        if value["choices"][0]["finish_reason"].as_str() == Some("length") {
                            return Err("stopped at max_tokens (truncated output)".into());
                        }
                        return value["choices"][0]["message"]["content"]
                            .as_str()
                            .map(|s| s.trim().to_string())
                            .ok_or_else(|| "response missing message content".to_string());
                    }
                    let retryable = status.is_server_error() || status.as_u16() == 429;
                    let text = resp.text().unwrap_or_default();
                    let text: String = text.chars().take(400).collect();
                    last_err = format!("HTTP {status}: {text}");
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
        assert_eq!(
            p.config().base_url,
            "http://127.0.0.1:11434/v1"
        );
    }

    #[test]
    fn empty_api_key_becomes_none() {
        let cfg = OpenAiCompatConfig::new("https://api.openai.com/v1", Some("  ".into()), "gpt-4o-mini");
        assert!(cfg.api_key.is_none());
    }

    #[test]
    fn generate_without_server_errors_fast() {
        let cfg = OpenAiCompatConfig::new(
            "http://127.0.0.1:1",
            None,
            "test",
        );
        let p = OpenAiCompatProvider::new("t", "t", cfg);
        let params = GenerationParams {
            timeout: Duration::from_millis(500),
            ..Default::default()
        };
        let err = p.generate("hi", params).unwrap_err();
        assert!(err.contains("request failed") || err.contains("HTTP"), "got: {err}");
    }
}
