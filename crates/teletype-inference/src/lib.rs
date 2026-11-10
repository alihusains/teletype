//! LLM inference providers and model management for Teletype.
//!
//! `teletype-core` defines the [`InferenceProvider`](teletype_core::llm::InferenceProvider)
//! trait; this crate provides the concrete implementations:
//!
//! - [`ServerProvider`] — GGUF via a child `llama-server` process.
//! - [`OpenAiCompatProvider`] — any OpenAI-compatible `/v1/chat/completions`
//!   endpoint (OpenAI, Ollama, OpenRouter, custom…).

// Production code must not panic: the lints deny `unwrap`, `expect` and
// `panic!`/`unreachable!`/`todo!` outside tests. Test code may keep them.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
//! - [`MagnitudeProvider`] — Magnitude OpenAI-compatible API on localhost:10100.
//! - [`MockInferenceProvider`] — deterministic, for tests.
//! - [`ModelManager`] — lifecycle: Unavailable → Downloading → Loading →
//!   Ready → Busy → Unloading → Error. Keeps the selected model warm.
//! - [`EngineHandle`] — lock-free `ArcSwap` handle for hot engine swapping.

pub mod catalog;
pub mod child_restart;
pub mod command_llm;
pub mod download;
pub mod engine;
pub mod magnitude;
pub mod manager;
pub mod mock;
pub mod needle;
pub mod openai_compat;
pub mod qwen3_asr;
pub mod server;
pub mod tinydecide;

/// Creation flags for spawning helper processes (llama-server) so they do
/// not flash a console window on Windows. Returns 0 on other platforms.
pub fn no_window_creation_flags() -> u32 {
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW (0x08000000): the child does not create a console.
        // FFI to avoid a windows-sys dependency in this crate.
        0x0800_0000
    }
    #[cfg(not(windows))]
    {
        0
    }
}

pub use catalog::CATALOG;
pub use command_llm::{parse_command_json, LlmCommandExtractor, ProviderLookup};
pub use download::{
    download_entry, download_entry_with_cancel, download_entry_with_progress, DownloadProgress,
    ProgressFn,
};
pub use engine::EngineHandle;
pub use magnitude::{MagnitudeConfig, MagnitudeProvider};
pub use manager::{ModelManager, ModelState};
pub use mock::MockInferenceProvider;
pub use needle::{NeedleProvider, CLASSIFY_TOOLS_JSON, EXTRACT_SYSTEM_PROMPT, EXTRACT_TOOLS_JSON};
pub use openai_compat::{OpenAiCompatConfig, OpenAiCompatProvider};
pub use qwen3_asr::Qwen3AsrProvider;
pub use server::{find_llama_server_bin, reap_orphan, ServerProvider};
pub use tinydecide::TinyDecideProvider;

pub mod combined;
pub use combined::{CombinedDecisionProvider, CommandExtractor};
