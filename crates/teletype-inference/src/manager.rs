//! Model lifecycle management.
//!
//! Keeps the selected model warm, avoids reloading per request, and exposes
//! a state machine the UI can render.

use std::{
    path::PathBuf,
    sync::{Mutex, RwLock},
    time::Instant,
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

// ---------------------------------------------------------------------------
// Speech-model unload timer (BUG-003)
// ---------------------------------------------------------------------------
//
// The LLM `ModelManager` above predates the desktop app and is still
// orphaned; the speech side is wired, and this trait is what it wires
// through. It is a view over the `teletype-speech::SpeechProvider` surface
// the manager needs (load/unload/is_loaded), which keeps the timer logic
// testable without a real ASR engine. `teletype-speech::SpeechProvider`
// itself implements it.

/// The provider surface the unload timer drives. Implemented by
/// [`teletype_speech::SpeechProvider`] and by test doubles.
pub trait UnloadableProvider: Send + 'static {
    /// Loads the model at `path`. No-op if already loaded.
    fn load(&mut self, path: &std::path::Path) -> Result<(), String>;
    /// Unloads the model, freeing memory.
    fn unload(&mut self);
    /// Whether a model is currently loaded.
    fn is_loaded(&self) -> bool;
}

impl UnloadableProvider for Box<dyn teletype_speech::SpeechProvider> {
    fn load(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.as_mut().load(path).map_err(|e| e.to_string())
    }
    fn unload(&mut self) {
        self.as_mut().unload()
    }
    fn is_loaded(&self) -> bool {
        self.as_ref().is_loaded()
    }
}

/// Drives the speech model's load/unload lifecycle: keeps the model warm
/// while dictation is in flight and unloads it after an idle delay set by
/// the user's `ModelUnloadPolicy` (BUG-003).
///
/// The unload is timer-driven: `end_dictation()` arms a deadline, and
/// `tick()` (called from a background thread, or from tests) fires
/// `unload()` once the deadline passes with no active dictation. Unload
/// never fires while a dictation is in flight, and the next dictation
/// re-loads the model (cold start — the first take after an unload is
/// slower by the model-load time; that is the documented tradeoff).
pub struct SpeechModelManager<P: UnloadableProvider> {
    provider: Mutex<P>,
    model_path: Mutex<PathBuf>,
    /// A dictation session is in flight (recording, transcribing, or
    /// pipeline). While set, the unload timer is a no-op.
    active: Mutex<bool>,
    /// When the armed idle-unload fires, or `None` when unarmed.
    armed_until: Mutex<Option<std::time::Instant>>,
}

impl<P: UnloadableProvider + 'static> SpeechModelManager<P> {
    pub fn new(model_path: PathBuf, provider: P) -> Self {
        Self {
            provider: Mutex::new(provider),
            model_path: Mutex::new(model_path),
            active: Mutex::new(false),
            armed_until: Mutex::new(None),
        }
    }

    pub fn lock<'a, T>(&self, m: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
        m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The inner provider lock, for callers that need the full
    /// [`teletype_speech::SpeechProvider`] surface (transcribe).
    pub fn provider(&self) -> &Mutex<P> {
        &self.provider
    }

    /// Whether a model is currently loaded.
    pub fn is_loaded(&self) -> bool {
        self.lock(&self.provider).is_loaded()
    }

    /// Sets the model path used by [`Self::load`]. The constructor takes a
    /// path, but the desktop app resolves the selected model at warm-up
    /// time, so this updates it before the first load.
    pub fn set_model_path(&self, path: PathBuf) {
        *self.lock(&self.model_path) = path;
    }

    /// Loads the model in place. No-op if already loaded.
    pub fn load(&self) -> Result<(), String> {
        let path = self.lock(&self.model_path).clone();
        let mut provider = self.lock(&self.provider);
        if provider.is_loaded() {
            return Ok(());
        }
        provider.load(&path)
    }

    /// Unloads the model, freeing memory.
    pub fn unload(&self) {
        self.lock(&self.provider).unload();
        *self.lock(&self.armed_until) = None;
    }

    /// Marks a dictation session as in flight: the model is loaded on
    /// demand and any pending idle-unload is cancelled. Must never race
    /// with an unload, which is why `tick()` declines to fire while this
    /// flag is set.
    pub fn begin_dictation(&self) {
        *self.lock(&self.active) = true;
        *self.lock(&self.armed_until) = None;
    }

    /// Marks the dictation session as finished and arms the idle-unload
    /// delay (if any): `None` (policy `Never`) leaves the model resident,
    /// a `Some` deadline is checked by [`Self::tick`].
    pub fn end_dictation(&self, delay: Option<std::time::Duration>) {
        *self.lock(&self.active) = false;
        *self.lock(&self.armed_until) = delay.map(|d| std::time::Instant::now() + d);
    }

    /// One step of the unload timer: if no dictation is active and the
    /// armed delay has elapsed, unloads the model. Safe to call from a
    /// background thread on any cadence; a `Never` policy never arms a
    /// deadline, so this is a no-op for the default setting.
    pub fn tick(&self) {
        if *self.lock(&self.active) {
            return; // never unload mid-dictation
        }
        let due = self.lock(&self.armed_until).map(|t| t <= Instant::now());
        if due == Some(true) {
            self.unload();
        }
    }

    /// Spawns the background thread that calls [`Self::tick`] on a fixed
    /// cadence. The delay granularity is the tick interval (5 s), which is
    /// far below the smallest user-facing policy (immediate / 2 minutes).
    pub fn start_tick_thread(self: &std::sync::Arc<Self>, interval: std::time::Duration) {
        let this = std::sync::Arc::clone(self);
        std::thread::Builder::new()
            .name("teletype-model-unload".into())
            .spawn(move || loop {
                std::thread::sleep(interval);
                this.tick();
            })
            .ok();
    }
}

#[cfg(test)]
mod unload_timer_tests {
    use super::*;

    /// Minimal provider double for the timer tests: tracks loaded state
    /// like the real providers (`parakeet.rs` frees its session, `whisper`
    /// drops its context).
    struct MockProvider {
        loaded: bool,
        path: std::path::PathBuf,
    }

    impl UnloadableProvider for MockProvider {
        fn load(&mut self, path: &std::path::Path) -> Result<(), String> {
            self.path = path.to_path_buf();
            self.loaded = true;
            Ok(())
        }
        fn unload(&mut self) {
            self.loaded = false;
        }
        fn is_loaded(&self) -> bool {
            self.loaded
        }
    }

    fn manager() -> SpeechModelManager<MockProvider> {
        SpeechModelManager::new(
            std::path::PathBuf::from("mock-model.bin"),
            MockProvider {
                loaded: false,
                path: std::path::PathBuf::new(),
            },
        )
    }

    #[test]
    fn unload_frees_the_model() {
        let mut provider: Box<dyn teletype_speech::SpeechProvider> =
            Box::new(teletype_speech::MockSpeechProvider {
                transcript: String::new(),
                ..Default::default()
            });
        provider.load(std::path::Path::new("mock.bin")).unwrap();
        assert!(provider.is_loaded());
        provider.unload();
        assert!(!provider.is_loaded());
    }

    #[test]
    fn unload_does_not_fire_during_active_dictation() {
        let manager = manager();
        manager.load().unwrap();
        manager.begin_dictation();
        // Arm an immediate deadline, then start a new dictation before the
        // timer can fire: the active flag must hold the unload back.
        manager.end_dictation(Some(std::time::Duration::ZERO));
        manager.begin_dictation();
        manager.tick(); // should be a no-op while dictation is active
        assert!(manager.is_loaded(), "model must not unload mid-dictation");
    }

    #[test]
    fn unload_fires_after_policy_duration() {
        let manager = manager();
        manager.load().unwrap();
        manager.end_dictation(Some(std::time::Duration::from_millis(1)));
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.tick();
        assert!(
            !manager.is_loaded(),
            "model must unload after the policy duration"
        );
    }

    #[test]
    fn never_policy_never_unloads() {
        let manager = manager();
        manager.load().unwrap();
        manager.end_dictation(None); // policy: Never
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.tick();
        assert!(
            manager.is_loaded(),
            "Never policy must keep the model resident"
        );
    }

    #[test]
    fn next_dictation_after_unload_reloads() {
        let manager = manager();
        manager.load().unwrap();
        manager.unload();
        assert!(!manager.is_loaded());
        manager.begin_dictation();
        manager.load().unwrap();
        assert!(
            manager.is_loaded(),
            "model must be re-loaded on next dictation"
        );
    }

    /// End-to-end for BUG-003: the app arms the deadline via
    /// `end_dictation(Some(delay))` on session end and a background thread
    /// ticks the manager (see `teletype-desktop` `lib.rs`). This test proves
    /// that exact sequence unloads after the delay, and that `None` (the
    /// "Never" policy) keeps the model resident. Uses the `MockProvider`
    /// double; the `Box<dyn SpeechProvider>` adapter is exercised by the
    /// desktop app's own managers and by `unload_frees_the_model`.
    #[test]
    fn end_dictation_arms_unload_that_tick_fires() {
        let manager = manager();
        manager.load().unwrap();
        assert!(manager.is_loaded());

        // The desktop app's exact call site: arm a short deadline when the
        // session ends.
        manager.end_dictation(Some(std::time::Duration::from_millis(1)));
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.tick();
        assert!(!manager.is_loaded(), "armed delay must unload the model");

        // "Never" policy (delay_secs == 0 -> None) keeps the model resident.
        manager.load().unwrap();
        manager.end_dictation(None);
        std::thread::sleep(std::time::Duration::from_millis(10));
        manager.tick();
        assert!(manager.is_loaded(), "Never policy must keep the model resident");
    }
}
