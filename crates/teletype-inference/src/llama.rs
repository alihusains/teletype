//! llama.cpp GGUF provider via the `llama_cpp` crate (v0.3).
//!
//! On Apple Silicon, llama.cpp uses Metal automatically; elsewhere CPU.
//! The model is held in a `Mutex` so the provider is `Send + Sync`.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use teletype_core::llm::{GenerationParams, InferenceProvider};

/// A llama.cpp-backed inference provider.
pub struct LlamaProvider {
    model_id: String,
    model_name: String,
    path: PathBuf,
    inner: Mutex<Option<Session>>,
}

struct Session {
    model: llama_cpp::LlamaModel,
    session: llama_cpp::LlamaSession,
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

    fn with_session<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        if guard.is_none() {
            *guard = Some(load(&self.path)?);
        }
        let session = guard.as_mut().ok_or("model not loaded")?;
        Ok(f(session))
    }
}

fn load(path: &Path) -> Result<Session, String> {
    if !path.exists() {
        return Err(format!("Model file not found: {}", path.display()));
    }
    let model = llama_cpp::LlamaModel::load_from_file(path, llama_cpp::LlamaParams::default())
        .map_err(|e| format!("Couldn't load model: {e:?}"))?;
    let threads = std::cmp::max(
        2,
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4),
    ) as u32;
    let session_params = llama_cpp::SessionParams {
        n_ctx: 1024,
        n_batch: 512,
        n_threads: threads,
        n_threads_batch: 4,
        ..Default::default()
    };
    let session = model
        .create_session(session_params)
        .map_err(|e| format!("Couldn't create session: {e:?}"))?;
    Ok(Session { model, session })
}

impl InferenceProvider for LlamaProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        let deadline = Instant::now() + params.timeout;
        let result = self.with_session(|session| {
            let full_prompt = format!(
                "System: You are a text transformation engine. Return only the transformed text.\nUser: {prompt}\nAssistant:"
            );

            session
                .session
                .advance_context(full_prompt.as_bytes())
                .map_err(|e| format!("context advance failed: {e:?}"))?;

            let mut handle = session
                .session
                .start_completing()
                .map_err(|e| format!("completion start failed: {e:?}"))?;

            let mut output = String::new();
            let mut last_check = Instant::now();
            let mut token_count = 0;

            while token_count < params.max_tokens as usize {
                if last_check.elapsed() > Duration::from_millis(250) {
                    if Instant::now() > deadline {
                        return Err("Generation timed out".into());
                    }
                    last_check = Instant::now();
                }

                match handle.next_token() {
                    Some(token) => {
                        output.push_str(&session.model.token_to_piece(token));
                        token_count += 1;
                    }
                    None => break,
                }
            }

            let text = output.trim().to_string();
            if text.is_empty() {
                Err("Model produced no text".into())
            } else {
                Ok(text)
            }
        });
        result?
    }
}
