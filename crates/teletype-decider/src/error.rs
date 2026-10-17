//! Errors for the NeuDecide runtime.

use thiserror::Error;

/// Everything that can go wrong in the decider crate.
#[derive(Debug, Error)]
pub enum DeciderError {
    /// The `onnx` feature is off: no ONNX runtime is linked, so the model
    /// cannot be loaded or run. The serializer/parser are unaffected.
    #[error("the ONNX runtime is not enabled (build with the `onnx` feature)")]
    RuntimeNotEnabled,

    /// A model file (config.json, tokenizer, graph) is missing or unreadable.
    #[error("model file: {0}")]
    Io(#[from] std::io::Error),

    /// A model file is not valid JSON.
    #[error("model file is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),

    /// The model files are inconsistent (missing key in config.json, unknown
    /// variant, ...).
    #[error("model config: {0}")]
    Config(String),

    /// The model's output could not be parsed as a list of tool calls.
    #[error("could not parse tool calls from model output: {0}")]
    Parse(String),

    /// A token id outside the vocabulary range (decode only; the Python
    /// binding raises `IndexError` in this case).
    #[error("token id {id} out of range (vocabulary size {vocab})")]
    InvalidTokenId { id: u32, vocab: u32 },
}

/// Crate-local Result alias.
pub type Result<T> = std::result::Result<T, DeciderError>;
