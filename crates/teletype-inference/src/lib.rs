//! LLM inference providers and model management for Teletype.
//!
//! `teletype-core` defines the [`InferenceProvider`](teletype_core::llm::InferenceProvider)
//! trait; this crate provides the concrete implementations:
//!
//! - [`LlamaProvider`] — GGUF model slot (the llama.cpp runtime is not
//!   linked in this build; see `llama.rs` for why).
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
