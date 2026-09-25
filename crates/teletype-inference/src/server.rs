//! Subprocess `llama-server` inference provider.
//!
//! llama.cpp's ggml cannot be linked in-process alongside whisper.cpp's
//! ggml, so the model runs in a separate `llama-server` process (same
//! approach as EnviousWispr). This provider:
//!
//! 1. Locates the binary (`TELETYPE_LLAMA_SERVER`, next to the app exe,
//!    bundle `Resources/`, then `PATH`).
//! 2. Spawns it on `127.0.0.1:<ephemeral port>` with a random API key.
//! 3. Polls `GET /health` until ready (or the child dies).
//! 4. Serves [`InferenceProvider::generate`] via `POST /v1/chat/completions`.
//! 5. Kills the child on `Drop`.

use std::{
    env,
    fs::File,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use teletype_core::llm::{GenerationParams, InferenceProvider};

/// Model load can take a while for larger GGUFs on first start.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_INTERVAL: Duration = Duration::from_millis(250);
const HEALTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(2);

/// A local model served by a child `llama-server` process.
pub struct ServerProvider {
    model_id: String,
    model_name: String,
    path: PathBuf,
    runtime: Mutex<Option<Runtime>>,
}

struct Runtime {
    base_url: String,
    api_key: String,
    child: Child,
    log_path: PathBuf,
}

impl ServerProvider {
    /// Creates a provider that will spawn `llama-server` for `path` on
    /// [`ServerProvider::warm_up`].
    pub fn new(id: impl Into<String>, name: impl Into<String>, path: &Path) -> Self {
        Self {
            model_id: id.into(),
            model_name: name.into(),
            path: path.to_path_buf(),
            runtime: Mutex::new(None),
        }
    }

    /// Spawns `llama-server` and waits until `/health` returns 200.
    /// No-op if already running.
    pub fn warm_up(&self) -> Result<(), String> {
        let mut guard = self
            .runtime
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        if guard.is_some() {
            return Ok(());
        }
        if !self.path.exists() {
            return Err(format!("Model file not found: {}", self.path.display()));
        }

        let bin = find_llama_server()?;
        let port = pick_port()?;
        let api_key = uuid::Uuid::new_v4().to_string();
        let log_path = env::temp_dir().join(format!(
            "teletype-llama-server-{}-{port}.log",
            self.model_id
        ));
        let log = File::create(&log_path)
            .map_err(|e| format!("create server log {}: {e}", log_path.display()))?;

        let mut child = Command::new(&bin)
            .arg("-m")
            .arg(&self.path)
            .args(["--host", "127.0.0.1", "--port"])
            .arg(port.to_string())
            .arg("--api-key")
            .arg(&api_key)
            .args([
                "-c",
                "4096",
                "-fa",
                "on",
                "-ctk",
                "q8_0",
                "-ctv",
                "q8_0",
                "--jinja",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;

        let base_url = format!("http://127.0.0.1:{port}");
        if let Err(e) = wait_until_ready(&mut child, &base_url, &api_key, &log_path) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }

        tracing::info!(model = %self.model_id, port, "llama-server ready");
        *guard = Some(Runtime {
            base_url,
            api_key,
            child,
            log_path,
        });
        Ok(())
    }

    /// Stops the child process if it is running.
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.runtime.lock() {
            if let Some(rt) = guard.take() {
                stop_child(rt.child, &rt.log_path);
            }
        }
    }

    fn endpoint(&self) -> Result<(String, String), String> {
        let guard = self
            .runtime
            .lock()
            .map_err(|e| format!("lock poisoned: {e}"))?;
        let rt = guard
            .as_ref()
            .ok_or_else(|| "llama-server is not running (call warm_up)".to_string())?;
        Ok((rt.base_url.clone(), rt.api_key.clone()))
    }

    pub fn model_path(&self) -> Option<&Path> {
        Some(&self.path)
    }

    /// True when the subprocess is up and was ready at warm-up time.
    pub fn is_ready(&self) -> bool {
        self.runtime
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }
}

impl Drop for ServerProvider {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl InferenceProvider for ServerProvider {
    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn generate(&self, prompt: &str, params: GenerationParams) -> Result<String, String> {
        let messages = json!([{ "role": "user", "content": prompt }]);
        self.chat(messages, params)
    }

    fn generate_with_system(
        &self,
        system: &str,
        user: &str,
        params: GenerationParams,
    ) -> Result<String, String> {
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": user },
        ]);
        self.chat(messages, params)
    }

    /// The local server is spawned with a 4096-token context, so the engine
    /// can preflight long transcripts against a known window.
    fn context_tokens(&self) -> Option<u32> {
        Some(4096)
    }
}

impl ServerProvider {
    fn chat(&self, messages: Value, params: GenerationParams) -> Result<String, String> {
        let (base_url, api_key) = self.endpoint()?;

        let client = reqwest::blocking::Client::builder()
            .timeout(params.timeout)
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let body = json!({
            "model": "local",
            "messages": messages,
            "max_tokens": params.max_tokens,
            "temperature": params.temperature,
            "stream": false,
        });

        let deadline = Instant::now() + params.timeout;
        let mut last_err = String::from("request failed");
        // One retry on 5xx / network error, within the generation budget
        // (same pattern as the OpenAI-compatible provider).
        for attempt in 0..2 {
            if Instant::now() >= deadline {
                break;
            }
            match self.send_chat(&client, &base_url, &api_key, &body) {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        let value: Value = resp
                            .json()
                            .map_err(|e| format!("llama-server response decode: {e}"))?;
                        // finish_reason == "length" is a partial rewrite; reject it.
                        if value["choices"][0]["finish_reason"].as_str() == Some("length") {
                            return Err(
                                "llama-server stopped at max_tokens (truncated output)".into(),
                            );
                        }
                        return value["choices"][0]["message"]["content"]
                            .as_str()
                            .map(|s| s.trim().to_string())
                            .ok_or_else(|| "llama-server response missing message content".to_string());
                    }
                    let retryable = status.is_server_error() || status.as_u16() == 429;
                    let text = resp.text().unwrap_or_default();
                    let text: String = text.chars().take(400).collect();
                    last_err = format!("llama-server HTTP {status}: {text}");
                    if !retryable || attempt == 1 {
                        return Err(last_err);
                    }
                }
                Err(e) => {
                    last_err = e;
                    if attempt == 1 {
                        return Err(last_err);
                    }
                }
            }
        }
        Err(last_err)
    }

    fn send_chat(
        &self,
        client: &reqwest::blocking::Client,
        base_url: &str,
        api_key: &str,
        body: &Value,
    ) -> Result<reqwest::blocking::Response, String> {
        client
            .post(format!("{base_url}/v1/chat/completions"))
            .bearer_auth(api_key)
            .json(body)
            .send()
            .map_err(|e| format!("llama-server request failed: {e}"))
    }
}

fn stop_child(mut child: Child, log_path: &Path) {
    let _ = child.kill();
    let _ = child.wait();
    tracing::info!(log = %log_path.display(), "llama-server stopped");
}

fn wait_until_ready(
    child: &mut Child,
    base_url: &str,
    api_key: &str,
    log_path: &Path,
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(HEALTH_REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Err(format!(
                    "llama-server exited early with {status} (log: {})",
                    log_path.display()
                ));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("wait on llama-server: {e}")),
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "llama-server not ready after {}s (log: {})",
                READY_TIMEOUT.as_secs(),
                log_path.display()
            ));
        }

        let ready = client
            .get(format!("{base_url}/health"))
            .bearer_auth(api_key)
            .send()
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ready {
            return Ok(());
        }
        thread::sleep(HEALTH_INTERVAL);
    }
}

/// Resolve the `llama-server` binary.
///
/// Order: `TELETYPE_LLAMA_SERVER` → next to the current exe →
/// macOS bundle `../Resources/llama-server` → `PATH`.
fn find_llama_server() -> Result<PathBuf, String> {
    if let Ok(p) = env::var("TELETYPE_LLAMA_SERVER") {
        let p = PathBuf::from(p);
        if is_runnable_binary(&p) {
            return Ok(p);
        }
        return Err(format!(
            "TELETYPE_LLAMA_SERVER points at a missing, empty, or \
             non-executable file: {}",
            p.display()
        ));
    }

    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                dir.join("llama-server"),
                dir.join("llama-server.exe"),
                dir.join("..").join("Resources").join("llama-server"),
                dir
                    .join("..")
                    .join("Resources")
                    .join("llama-server.exe"),
            ];
            for c in candidates {
                if is_runnable_binary(&c) {
                    return Ok(c);
                }
            }
        }
    }

    if let Some(p) = find_in_path("llama-server").or_else(|| find_in_path("llama-server.exe")) {
        return Ok(p);
    }

    Err(
        "llama-server binary not found. Set TELETYPE_LLAMA_SERVER or run \
         scripts/build-llama-server.sh"
            .to_string(),
    )
}

/// A candidate only counts if it could actually be executed.
///
/// The bundle declares `resources.binaries/llama-server`, and `cargo tauri
/// build` requires that path to exist, so a 0-byte placeholder often sits
/// next to the dev exe. Spawning it fails with `EACCES` (os error 13), which
/// surfaces to the user as a baffling "Permission denied" instead of the
/// actionable "run scripts/build-llama-server.sh". Skipping such a stub here
/// lets lookup fall through to PATH and the clear not-found error.
fn is_runnable_binary(p: &Path) -> bool {
    match std::fs::metadata(p) {
        Ok(md) => md.is_file() && md.len() > 0 && has_exec_bit(&md),
        Err(_) => false,
    }
}

#[cfg(unix)]
fn has_exec_bit(md: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    md.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn has_exec_bit(_md: &std::fs::Metadata) -> bool {
    true
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|c| is_runnable_binary(c))
}

fn pick_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind ephemeral port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    // Drop the listener so llama-server can bind it (small race, acceptable
    // for a single local process).
    drop(listener);
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_llama_server_error_mentions_env_override() {
        // When the binary is absent from PATH/exe dir, the error must tell
        // the user how to point at one. We only assert the message shape when
        // lookup fails; a successful lookup (dev machine with the binary) is
        // also fine.
        match find_llama_server() {
            Ok(p) => assert!(!p.as_os_str().is_empty()),
            Err(e) => assert!(e.contains("TELETYPE_LLAMA_SERVER")),
        }
    }

    #[test]
    fn pick_port_is_localhost_ephemeral() {
        let port = pick_port().unwrap();
        assert!(port > 0);
        // Port must be bindable again after pick_port drops the listener.
        // Retry a few times to absorb TIME_WAIT from concurrent test threads.
        let mut ok = false;
        for _ in 0..5 {
            if TcpListener::bind(("127.0.0.1", port)).is_ok() {
                ok = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(ok, "port {port} not bindable after 5 retries");
    }

    #[test]
    fn placeholder_stubs_are_not_runnable() {
        let dir = std::env::temp_dir().join(format!("teletype-llama-stub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty-llama-server");
        std::fs::write(&empty, b"").unwrap();
        assert!(
            !is_runnable_binary(&empty),
            "0-byte placeholder must be skipped"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let non_exec = dir.join("non-exec-llama-server");
            std::fs::write(&non_exec, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&non_exec, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(
                !is_runnable_binary(&non_exec),
                "non-executable file must be skipped"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn generate_without_warm_up_errors() {
        let p = ServerProvider::new("t", "t", Path::new("/nonexistent.gguf"));
        let err = p
            .generate("hi", GenerationParams::default())
            .unwrap_err();
        assert!(err.contains("not running"), "got: {err}");
    }

    #[test]
    fn warm_up_missing_model_errors() {
        let p = ServerProvider::new("t", "t", Path::new("/nonexistent.gguf"));
        let err = p.warm_up().unwrap_err();
        assert!(err.contains("not found") || err.contains("llama-server"), "got: {err}");
    }
}
