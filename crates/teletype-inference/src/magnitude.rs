//! Magnitude inference provider — OpenAI-compatible API on localhost:10100.
//!
//! Magnitude runs an OpenAI-compatible server at `http://127.0.0.1:10100/inference/v1`
//! when the Desktop app is running. This provider talks to it directly.

use std::time::Duration;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use thiserror::Error;

use teletype_core::llm::{GenerationParams, InferenceProvider};

/// Configuration for connecting to Magnitude.
#[derive(Debug, Clone)]
pub struct MagnitudeConfig {
    /// Base URL for the Magnitude inference API.
    /// Default: `http://127.0.0.1:10100/inference/v1`
    pub base_url: String,

    /// Optional API key. Not required for local connections.
    pub api_key: Option<String>,

    /// The model ID to use (must match a model downloaded in Magnitude).
    pub model: String,

    /// Human-readable name for the UI.
    pub model_name: String,

    /// Request timeout.
    pub timeout: Duration,
}

impl Default for MagnitudeConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:10100/inference/v1".into(),
            api_key: None,
            model: "magnitude-model".into(),
            model_name: "Magnitude Model".into(),
            timeout: Duration::from_secs(30),
        }
    }
}

/// Errors from the Magnitude provider.
#[derive(Debug, Error)]
pub enum MagnitudeError {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("HTTP {status}: {body}")]
    HttpError { status: u16, body: String },

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Model not loaded: {0}")]
    ModelNotLoaded(String),

    #[error("Stream error: {0}")]
    StreamError(String),
}

/// Magnitude provider using the OpenAI-compatible chat completions API.
#[derive(Debug, Clone)]
pub struct MagnitudeProvider {
    model_id: String,
    model_name: String,
    config: MagnitudeConfig,
    client: Client,
}

impl MagnitudeProvider {
    /// Creates a new Magnitude provider with the given config.
    pub fn new(config: MagnitudeConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .expect("reqwest client build failed");
        Self {
            model_id: config.model.clone(),
            model_name: config.model_name.clone(),
            config,
            client,
        }
    }

    /// Returns the chat completions endpoint URL.
    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.config.base_url.trim_end_matches('/'))
    }

    /// Returns the models list endpoint URL.
    fn models_url(&self) -> String {
        format!("{}/models", self.config.base_url.trim_end_matches('/'))
    }

    /// Builds a request with optional auth.
    fn build_request(&self, body: Value) -> reqwest::blocking::RequestBuilder {
        let mut req = self.client.post(self.chat_url()).json(&body);
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        req
    }

    /// Performs a non-streaming chat completion.
    fn chat_completion(
        &self,
        messages: Value,
        params: GenerationParams,
    ) -> Result<String, MagnitudeError> {
        let body = json!({
            "model": self.config.model,
            "messages": messages,
            "temperature": params.temperature,
            "max_tokens": params.max_tokens,
            "stream": false,
        });

        let resp = self.build_request(body).send()?;
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(MagnitudeError::HttpError {
                status: status.as_u16(),
                body: body.chars().take(500).collect(),
            });
        }

        let value: Value = resp.json()?;
        let content = value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| MagnitudeError::InvalidResponse("missing choices[0].message.content".into()))?;

        Ok(content.to_string())
    }

    /// Performs a streaming chat completion.
    fn chat_completion_stream(
        &self,
        messages: Value,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, MagnitudeError> {
        let body = json!({
            "model": self.config.model,
            "messages": messages,
            "temperature": params.temperature,
            "max_tokens": params.max_tokens,
            "stream": true,
        });

        let resp = self.build_request(body).send()?;
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(MagnitudeError::HttpError {
                status: status.as_u16(),
                body: body.chars().take(500).collect(),
            });
        }

        let mut full_text = String::new();

        // Read the SSE stream line by line
        for line in resp.text()?.lines() {
            if line.starts_with("data: ") {
                let data = &line[6..];
                if data == "[DONE]" {
                    break;
                }
                if let Ok(chunk) = serde_json::from_str::<Value>(data) {
                    if let Some(delta) = chunk
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("delta"))
                        .and_then(|d| d.get("content"))
                        .and_then(|c| c.as_str())
                    {
                        if !delta.is_empty() {
                            on_token(delta);
                            full_text.push_str(delta);
                        }
                    }
                }
            }
        }

        Ok(full_text)
    }

    /// Lists available models from Magnitude (for health check).
    pub fn list_models(&self) -> Result<Vec<String>, MagnitudeError> {
        let mut req = self.client.get(self.models_url());
        if let Some(key) = &self.config.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req.send()?;
        let status = resp.status();

        if !status.is_success() {
            let body = resp.text().unwrap_or_default();
            return Err(MagnitudeError::HttpError {
                status: status.as_u16(),
                body: body.chars().take(300).collect(),
            });
        }

        #[derive(Deserialize)]
        struct ModelsResponse {
            data: Vec<ModelInfo>,
        }
        #[derive(Deserialize)]
        struct ModelInfo {
            id: String,
        }

        let value: ModelsResponse = resp.json()?;
        Ok(value.data.into_iter().map(|m| m.id).collect())
    }

    /// Tests the connection to Magnitude.
    pub fn test_connection(&self) -> Result<String, MagnitudeError> {
        match self.list_models() {
            Ok(ids) if !ids.is_empty() => {
                if ids.iter().any(|id| id == &self.config.model) {
                    Ok(format!("Connected. Model '{}' available.", self.config.model))
                } else {
                    Err(MagnitudeError::ModelNotLoaded(format!(
                        "Model '{}' not found. Available: {}",
                        self.config.model,
                        ids.join(", ")
                    )))
                }
            }
            Ok(_) => Err(MagnitudeError::ModelNotLoaded("No models loaded in Magnitude".into())),
            Err(e) => Err(e),
        }
    }
}

impl InferenceProvider for MagnitudeProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        let messages = json!([{ "role": "user", "content": prompt }]);
        self.chat_completion(messages, params).map_err(|e| e.to_string())
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
        self.chat_completion(messages, params).map_err(|e| e.to_string())
    }

    fn generate_stream(
        &self,
        prompt: &str,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let messages = json!([{ "role": "user", "content": prompt }]);
        self.chat_completion_stream(messages, params, on_token)
            .map_err(|e| e.to_string())
    }

    fn generate_with_system_stream(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);
        self.chat_completion_stream(messages, params, on_token)
            .map_err(|e| e.to_string())
    }

    fn is_local(&self) -> bool {
        // Magnitude runs locally, so the polish gate applies
        true
    }

    fn context_tokens(&self) -> Option<u32> {
        // Magnitude configures context per model; unknown here
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnitude_config_defaults() {
        let config = MagnitudeConfig::default();
        assert_eq!(config.base_url, "http://127.0.0.1:10100/inference/v1");
        assert_eq!(config.timeout, Duration::from_secs(30));
    }

    #[test]
    fn magnitude_provider_construction() {
        let config = MagnitudeConfig::default();
        let provider = MagnitudeProvider::new(config);
        assert_eq!(provider.model_id(), "magnitude-model");
        assert_eq!(provider.model_name(), "Magnitude Model");
    }
}