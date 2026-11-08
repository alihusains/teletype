//! Data-driven model catalog.
//!
//! The speech, LLM, and command catalogs are JSON files parsed at runtime,
//! owned by this crate. The Rust side keeps the schema, the engine registry,
//! the validator, and the loader; adding a model is a data edit.

pub mod engine;
pub mod gate;
pub mod loader;
pub mod schema;
pub mod update;
pub mod validate;

pub use engine::Engine;
pub use gate::{command_mode_available, CommandModeStatus};
pub use schema::{Artifact, Capability, Catalog, CommandEntry, LlmEntry, Purpose, SpeechEntry};
pub use validate::{validate_catalog, validate_command, validate_llm, validate_speech};

/// Errors the catalog can surface.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("unknown engine name: {0}")]
    UnknownEngine(String),
    #[error("invalid JSON: {0}")]
    InvalidJson(String),
    #[error("catalog validation failed: {0}")]
    Validation(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("network error: {0}")]
    Network(String),
    #[error("sha256 mismatch: {0}")]
    HashMismatch(String),
}
