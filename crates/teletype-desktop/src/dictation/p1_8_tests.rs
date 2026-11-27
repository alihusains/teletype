use super::*;
use teletype_core::llm::{GenerationParams, InferenceProvider};

/// A provider whose `generate`/`generate_stream` sleeps, simulating a
/// slow local LLM call.
struct SlowEngine {
    delay: Duration,
    /// Set when generation started, cleared when it finished.
    running: std::sync::atomic::AtomicBool,
}

impl InferenceProvider for SlowEngine {
    fn model_id(&self) -> &str {
        "slow-mock"
    }
    fn model_name(&self) -> &str {
        "Slow Mock"
    }
    fn generate(&self, _prompt: &str, _params: GenerationParams) -> Result<String, String> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        thread::sleep(self.delay);
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok("polished".into())
    }
    fn generate_stream(
        &self,
        _prompt: &str,
        _params: GenerationParams,
        on_token: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);
        thread::sleep(self.delay);
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
        on_token("polished");
        Ok("polished".into())
    }
}

/// While a transform is generating (the engine Arc is held by the
/// pipeline worker), a model-status query must return immediately — it
/// must not wait for the generation to finish. P1.8: the dictation
/// worker clones the Arc and drops the lock before generation, so the
/// status command's brief lock is never blocked by the LLM call.
#[test]
fn status_query_returns_immediately_during_slow_generation() {
    let state = AppState::for_tests();
    let engine = Arc::new(SlowEngine {
        delay: Duration::from_millis(300),
        running: std::sync::atomic::AtomicBool::new(false),
    });
    {
        let mut slot = state
            .inference
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(engine.clone());
    }

    // The dictation worker's contract: clone the Arc, drop the lock,
    // generate.
    let worker_engine = state
        .inference
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap();
    let worker = thread::spawn(move || {
        worker_engine
            .generate("prompt", GenerationParams::default())
            .unwrap();
    });

    // Wait until the generation is actually in flight.
    while !engine.running.load(std::sync::atomic::Ordering::SeqCst) {
        thread::sleep(Duration::from_millis(5));
    }

    // The UI status query: takes the same lock the worker briefly held.
    // It must return in well under the 300 ms generation window.
    let settings = state.settings();
    let started = Instant::now();
    let status = crate::commands::models::llm_runtime(&state, &settings);
    let elapsed = started.elapsed();
    assert_eq!(status.state, "loaded");
    assert_eq!(status.model_id, "slow-mock");
    assert!(
        elapsed < Duration::from_millis(100),
        "status query blocked on the generation lock: {elapsed:?}"
    );

    worker.join().unwrap();
}
