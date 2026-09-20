//! GGUF inference provider.
//!
//! The real llama.cpp runtime links a second copy of the ggml library, which
//! collides at link time with whisper.cpp's ggml (the speech engine). Until
//! both engines are built against one shared ggml, this provider rejects
//! model loads with a clear error instead of shipping a broken binary.
//!
//! The provider keeps the same public surface (`LlamaProvider::new`,
//! `warm_up`, `model_path`, `InferenceProvider`) so the UI, model catalog,
//! and download flow stay intact; selecting a model reports the
//! unavailable state instead of crashing.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use teletype_core::llm::{GenerationParams, InferenceProvider};

/// A GGUF-backed inference provider (see module docs for the runtime status).
pub struct LlamaProvider {
    model_id: String,
    model_name: String,
    path: PathBuf,
    inner: Mutex<Option<()>>,
}

impl LlamaProvider {
    /// Creates a provider that will load `path` on first use.
    pub fn new(id: impl Into<String>, name: impl Into<String>, path: &Path) -> Self {
        Self {
            model_id: id.into(),
            model_name: name.into(),
            path: path.to_path_buf(),
            inner: Mutex::new(None),
        }
    }

    /// Eagerly loads the model so the first transform is fast.
    pub fn warm_up(&self) -> Result<(), String> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        if guard.is_none() {
            *guard = Some(load(&self.path)?);
        }
        Ok(())
    }

    pub fn model_path(&self) -> Option<&Path> {
        Some(&self.path)
    }


}

fn load(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Model file not found: {}", path.display()));
    }
    Err(
        "The local GGUF model runtime is not linked in this build (it conflicts \
         with the speech engine's math library). Transforms fall back to the \
         deterministic pipeline."
            .into(),
    )
}

impl InferenceProvider for LlamaProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn generate(&self, _prompt: &str, _params: GenerationParams) -> Result<String, String> {
        // The model can never be loaded in this build, so this surfaces the
        // same error as warm_up().
        Err("The local GGUF model runtime is not linked in this build.".into())
    }
}
