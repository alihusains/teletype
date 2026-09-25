//! LLM inference providers and model management for Teletype.
//!
//! `teletype-core` defines the [`InferenceProvider`](teletype_core::llm::InferenceProvider)
//! trait; this crate provides the concrete implementations:
//!
//! - [`ServerProvider`] — GGUF via a child `llama-server` process.
//! - [`OpenAiCompatProvider`] — any OpenAI-compatible `/v1/chat/completions`
//!   endpoint (OpenAI, Ollama, OpenRouter, custom…).
//! - [`MockInferenceProvider`] — deterministic, for tests.
//! - [`ModelManager`] — lifecycle: Unavailable → Downloading → Loading →
//!   Ready → Busy → Unloading → Error. Keeps the selected model warm.

pub mod catalog;
pub mod download;
pub mod manager;
pub mod mock;
pub mod openai_compat;
pub mod server;

pub use catalog::CATALOG;
pub use download::{download_entry, download_entry_with_progress, DownloadProgress, ProgressFn};
pub use manager::{ModelManager, ModelState};
pub use mock::MockInferenceProvider;
pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};
pub use server::ServerProvider;
