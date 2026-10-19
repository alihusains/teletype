//! Engine handle using ArcSwap for lock-free engine access.
//!
//! This allows dictation to clone the `Arc<dyn InferenceProvider>` and drop
//! the lock immediately, while model swap atomically installs a new `Arc`.
//! The old engine stays alive until all in-flight dictations finish.

use arc_swap::ArcSwap;
use std::sync::Arc;

use teletype_core::llm::InferenceProvider;

/// Lock-free handle to the current inference engine.
///
/// Cloning the returned `Arc` is cheap (atomic refcount increment).
/// Swapping the engine is atomic and lock-free.
pub struct EngineHandle {
    current: ArcSwap<Option<Arc<dyn InferenceProvider>>>,
}

impl EngineHandle {
    /// Creates a new empty engine handle.
    pub fn new() -> Self {
        Self {
            current: ArcSwap::new(Arc::new(None)),
        }
    }

    /// Gets a clone of the current engine, if any.
    ///
    /// Returns `None` if no engine is loaded. The returned `Arc` keeps the
    /// engine alive for the duration of the caller's use.
    pub fn get(&self) -> Option<Arc<dyn InferenceProvider>> {
        self.current.load().as_ref().clone()
    }

    /// Atomically sets the engine.
    ///
    /// The old engine (if any) remains alive until all existing `Arc` clones
    /// are dropped, which happens when in-flight dictations complete.
    pub fn set(&self, engine: Arc<dyn InferenceProvider>) {
        self.current.store(Arc::new(Some(engine)));
    }

    /// Atomically clears the engine.
    ///
    /// The old engine (if any) remains alive until all existing `Arc` clones
    /// are dropped.
    pub fn clear(&self) {
        self.current.store(Arc::new(None));
    }
}

impl Default for EngineHandle {
    fn default() -> Self {
        Self::new()
    }
}
