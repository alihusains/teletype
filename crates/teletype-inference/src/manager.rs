//! Model lifecycle management.
//!
//! Keeps the selected model warm, avoids reloading per request, and exposes
//! a state machine the UI can render.

use std::{
    path::PathBuf,
    sync::{Mutex, RwLock},
};

use teletype_core::llm::InferenceProvider;

/// The lifecycle state of a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelState {
    /// Model file not present on disk.
    Unavailable,
    /// Download in progress.
    Downloading,
    /// File present, loading into memory.
    Loading,
    /// Loaded and ready to generate.
    Ready,
    /// Currently generating.
    Busy,
    /// Being unloaded (memory pressure or user action).
    Unloading,
    /// Load or generation failed.
    Error,
}

/// Manages one model's lifecycle.
///
/// The actual provider is created by the caller (desktop app) and passed in,
/// so the manager stays decoupled from the concrete runtime.
pub struct ModelManager {
    state: RwLock<ModelState>,
    provider: Mutex<Option<Box<dyn InferenceProvider>>>,
    pub model_path: PathBuf,
}

impl ModelManager {
    pub fn new(model_path: PathBuf) -> Self {
        // The model is not loaded until explicitly loaded, so the initial state
        // is Unavailable regardless of whether the file exists yet.
        let initial = ModelState::Unavailable;
        Self {
            state: RwLock::new(initial),
            provider: Mutex::new(None),
            model_path,
        }
    }

    pub fn state(&self) -> ModelState {
        *self
            .state
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_state(&self, state: ModelState) {
        *self
            .state
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = state;
    }

    /// Loads the model into memory. No-op if already loaded.
    pub fn load(&self) -> Result<(), String> {
        let mut provider = self
            .provider
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if provider.is_some() {
            self.set_state(ModelState::Ready);
            return Ok(());
        }
        self.set_state(ModelState::Loading);
        let id = self
            .model_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "model".into());
        let server = crate::server::ServerProvider::new(id, "Local model", &self.model_path);
        match server.warm_up() {
            Ok(()) => {
                *provider = Some(Box::new(server));
                self.set_state(ModelState::Ready);
                Ok(())
            }
            Err(e) => {
                self.set_state(ModelState::Error);
                Err(e)
            }
        }
    }

    /// Unloads the model, freeing memory.
    pub fn unload(&self) {
        self.set_state(ModelState::Unloading);
        let mut provider = self
            .provider
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *provider = None;
        self.set_state(ModelState::Unavailable);
    }

    /// Returns a reference to the loaded provider, if ready.
    pub fn provider(
        &self,
    ) -> Option<std::sync::MutexGuard<'_, Option<Box<dyn InferenceProvider>>>> {
        let guard = self
            .provider
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (guard.is_some()).then_some(guard)
    }
}
