//! LLM inference providers and model management for Teletype.
//!
//! `teletype-core` defines the [`InferenceProvider`](teletype_core::llm::InferenceProvider)
//! trait; this crate provides the concrete implementations:
//!
//! - [`LlamaProvider`] — llama.cpp GGUF (CPU; Metal on Apple Silicon).
//! - [`MockInferenceProvider`] — deterministic, for tests.
//! - [`ModelManager`] — lifecycle: Unavailable → Downloading → Loading →
//!   Ready → Busy → Unloading → Error. Keeps the selected model warm.

pub mod catalog;
pub mod llama;
pub mod manager;
pub mod mock;

pub use catalog::CATALOG;
pub use llama::LlamaProvider;
pub use manager::{ModelManager, ModelState};
pub use mock::MockInferenceProvider;
